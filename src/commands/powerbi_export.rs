//! Shared Power BI `exportToFile` action for Power BI reports and paginated reports.
//!
//! Implements the asynchronous export-to-file flow documented at
//! <https://learn.microsoft.com/rest/api/power-bi/reports/export-to-file-in-group>:
//! `POST .../reports/{id}/ExportTo` (202 + job id) → poll
//! `GET .../reports/{id}/exports/{jobId}` until `Succeeded` →
//! `GET .../reports/{id}/exports/{jobId}/file` (download bytes).

use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use tokio::time::sleep;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// Formats supported for both Power BI and paginated reports.
const COMMON_FORMATS: &[&str] = &["PDF", "PPTX"];
/// Formats supported only for Power BI reports.
const POWERBI_ONLY_FORMATS: &[&str] = &["PNG"];
/// Formats supported only for paginated reports.
const PAGINATED_ONLY_FORMATS: &[&str] = &[
    "IMAGE",
    "XLSX",
    "DOCX",
    "CSV",
    "XML",
    "MHTML",
    "ACCESSIBLEPDF",
];

/// Poll interval while waiting for an export job to finish.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Which kind of report is being exported (governs the valid format set and the
/// request-body configuration key).
#[derive(Clone, Copy)]
pub enum ReportKind {
    PowerBi,
    Paginated,
}

impl ReportKind {
    const fn is_paginated(self) -> bool {
        matches!(self, Self::Paginated)
    }

    const fn command_prefix(self) -> &'static str {
        match self {
            Self::PowerBi => "report export",
            Self::Paginated => "paginated-report export",
        }
    }
}

/// Validate and normalize a requested file format for the given report kind.
///
/// Returns the upper-cased format on success, or an error that enumerates the
/// valid values (agent-native error). Pure — unit-tested.
pub fn validate_format(format: &str, kind: ReportKind) -> Result<String> {
    let normalized = format.trim().to_ascii_uppercase();
    let allowed: Vec<&str> = COMMON_FORMATS
        .iter()
        .chain(if kind.is_paginated() {
            PAGINATED_ONLY_FORMATS
        } else {
            POWERBI_ONLY_FORMATS
        })
        .copied()
        .collect();
    if allowed.contains(&normalized.as_str()) {
        return Ok(normalized);
    }
    Err(FabioError::with_hint(
        ErrorCode::InvalidInput,
        format!("Unsupported export format '{format}' for this report type"),
        format!("Valid formats: {}", allowed.join(", ")),
    )
    .into())
}

/// Parse a `name=value` parameter argument into a `(name, value)` pair. Pure.
pub fn parse_parameter(arg: &str) -> Result<(String, String)> {
    match arg.split_once('=') {
        Some((name, value)) if !name.is_empty() => Ok((name.to_string(), value.to_string())),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid parameter '{arg}'"),
            "Parameters must be name=value, e.g. --parameter Year=2026".to_string(),
        )
        .into()),
    }
}

/// Build the `ExportTo` request body. Pure — unit-tested.
pub fn build_export_body(format: &str, params: &[(String, String)], kind: ReportKind) -> Value {
    let mut body = serde_json::json!({ "format": format });
    if kind.is_paginated() && !params.is_empty() {
        let values: Vec<Value> = params
            .iter()
            .map(|(name, value)| serde_json::json!({ "name": name, "value": value }))
            .collect();
        body["paginatedReportConfiguration"] = serde_json::json!({ "parameterValues": values });
    }
    body
}

/// Execute the full export-to-file flow: trigger the job, poll to completion,
/// download the file, and write it to `out`.
#[allow(clippy::too_many_arguments)]
pub async fn export(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: &str,
    params: &[String],
    out: &str,
    timeout_secs: u64,
    kind: ReportKind,
) -> Result<()> {
    let fmt = validate_format(format, kind)?;
    let parsed: Vec<(String, String)> = params
        .iter()
        .map(|p| parse_parameter(p))
        .collect::<Result<Vec<_>>>()?;
    let body = build_export_body(&fmt, &parsed, kind);

    if output::dry_run_guard(
        cli,
        kind.command_prefix(),
        &serde_json::json!({ "workspace": workspace, "id": id, "format": fmt, "out": out }),
    ) {
        return Ok(());
    }

    // 1. Trigger the export job (202 + job id).
    let started = client
        .post_powerbi(&format!("/groups/{workspace}/reports/{id}/ExportTo"), &body)
        .await?;
    let job_id = started
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "ExportTo response did not include a job id".to_string(),
            )
        })?
        .to_string();

    // 2. Poll until the job reaches a terminal state.
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let status_path = format!("/groups/{workspace}/reports/{id}/exports/{job_id}");
    let extension = loop {
        let status = client.get_powerbi(&status_path).await?;
        let state = status
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("Undefined");
        match state {
            "Succeeded" => {
                break status
                    .get("resourceFileExtension")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
            "Failed" => {
                return Err(FabioError::with_hint(
                    ErrorCode::ApiError,
                    format!("Export job {job_id} failed"),
                    "Check that the report renders in the portal and that any required parameters were supplied via --parameter.".to_string(),
                )
                .into());
            }
            _ => {
                if std::time::Instant::now() >= deadline {
                    return Err(FabioError::with_hint(
                        ErrorCode::Timeout,
                        format!("Export job {job_id} did not complete within {timeout_secs}s (last status: {state})"),
                        "Increase --timeout for large reports.".to_string(),
                    )
                    .into());
                }
                sleep(POLL_INTERVAL).await;
            }
        }
    };

    // 3. Download the exported file and write it to disk.
    let bytes = client
        .get_powerbi_bytes(&format!(
            "/groups/{workspace}/reports/{id}/exports/{job_id}/file"
        ))
        .await?;
    std::fs::write(out, &bytes).map_err(|e| {
        FabioError::new(
            ErrorCode::InvalidInput,
            format!("Failed to write '{out}': {e}"),
        )
    })?;

    let result = serde_json::json!({
        "id": job_id,
        "reportId": id,
        "format": fmt,
        "status": "Succeeded",
        "file": out,
        "bytes": bytes.len(),
        "fileExtension": extension,
    });
    output::render_object(cli, &result, "file");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_format_accepts_common_for_both() {
        assert_eq!(validate_format("pdf", ReportKind::PowerBi).unwrap(), "PDF");
        assert_eq!(
            validate_format("PPTX", ReportKind::Paginated).unwrap(),
            "PPTX"
        );
    }

    #[test]
    fn validate_format_png_only_powerbi() {
        assert_eq!(validate_format("png", ReportKind::PowerBi).unwrap(), "PNG");
        assert!(validate_format("PNG", ReportKind::Paginated).is_err());
    }

    #[test]
    fn validate_format_paginated_only_rejected_for_powerbi() {
        for f in ["XLSX", "DOCX", "CSV", "IMAGE", "MHTML"] {
            assert!(
                validate_format(f, ReportKind::Paginated).is_ok(),
                "{f} should be valid for paginated"
            );
            assert!(
                validate_format(f, ReportKind::PowerBi).is_err(),
                "{f} should be invalid for power bi"
            );
        }
    }

    #[test]
    fn validate_format_unknown_errors_with_enumeration() {
        let err = validate_format("txt", ReportKind::Paginated).unwrap_err();
        assert!(err.to_string().contains("Unsupported export format"));
    }

    #[test]
    fn parse_parameter_splits_on_first_equals() {
        assert_eq!(
            parse_parameter("Year=2026").unwrap(),
            ("Year".to_string(), "2026".to_string())
        );
        // Value may itself contain '='.
        assert_eq!(
            parse_parameter("Filter=a=b").unwrap(),
            ("Filter".to_string(), "a=b".to_string())
        );
    }

    #[test]
    fn parse_parameter_rejects_missing_name() {
        assert!(parse_parameter("=value").is_err());
        assert!(parse_parameter("noequals").is_err());
    }

    #[test]
    fn build_export_body_minimal_for_powerbi() {
        let body = build_export_body("PDF", &[], ReportKind::PowerBi);
        assert_eq!(body, serde_json::json!({ "format": "PDF" }));
    }

    #[test]
    fn build_export_body_paginated_includes_parameters() {
        let params = vec![("Year".to_string(), "2026".to_string())];
        let body = build_export_body("PDF", &params, ReportKind::Paginated);
        assert_eq!(
            body["paginatedReportConfiguration"]["parameterValues"][0]["name"],
            "Year"
        );
        assert_eq!(
            body["paginatedReportConfiguration"]["parameterValues"][0]["value"],
            "2026"
        );
    }

    #[test]
    fn build_export_body_powerbi_ignores_parameters() {
        // Power BI reports don't take paginated parameterValues.
        let params = vec![("Year".to_string(), "2026".to_string())];
        let body = build_export_body("PDF", &params, ReportKind::PowerBi);
        assert!(body.get("paginatedReportConfiguration").is_none());
    }
}
