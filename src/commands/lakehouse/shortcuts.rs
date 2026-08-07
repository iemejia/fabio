use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::shortcut_target::{ShortcutTargetFlags, build_shortcut_target};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

// ─── Shortcuts ───────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn create_shortcut(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    path: &str,
    target_type: &str,
    target: Option<&str>,
    flags: &ShortcutTargetFlags<'_>,
    transform: &ShortcutTransformFlags<'_>,
    conflict_policy: Option<&str>,
) -> Result<()> {
    let (discriminator, target_body) = build_shortcut_target(target_type, target, flags)?;

    let mut body = serde_json::json!({
        "name": name,
        "path": path,
        "target": {
            discriminator: target_body
        }
    });
    // Optional data transformation (CSV → Delta table). No-op when not requested.
    if let Some(t) = build_transformation(transform)? {
        body["transform"] = t;
    }

    let url = conflict_policy.map_or_else(
        || format!("/workspaces/{workspace}/items/{id}/shortcuts"),
        |policy| {
            format!("/workspaces/{workspace}/items/{id}/shortcuts?shortcutConflictPolicy={policy}")
        },
    );

    let data = client.post(&url, &body, false).await?;
    output::render_object(cli, &data, "name");
    Ok(())
}

/// Typed flags for a shortcut data transformation.
///
/// A transformation converts structured source files referenced by the shortcut
/// into a queryable Delta table (Fabric Spark keeps it in sync). Only `csvToDelta`
/// is exposed by the Fabric REST API today; Parquet/JSON/Excel and the AI-powered
/// transforms are portal-only. `--transform-json` is a raw escape hatch for any
/// future transform shape.
#[derive(Default)]
pub(super) struct ShortcutTransformFlags<'a> {
    pub transform_type: Option<&'a str>,
    pub transform_json: Option<&'a str>,
    pub csv_delimiter: Option<&'a str>,
    pub csv_no_header: bool,
    pub csv_keep_error_files: bool,
    pub include_subfolders: bool,
}

/// Normalize a transform type to the Fabric discriminator, or explain why it is
/// not (yet) reachable via the public REST API. Pure for unit testing.
fn normalize_transform_type(input: &str) -> Result<&'static str> {
    let key: String = input
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .collect();
    match key.as_str() {
        "csvtodelta" | "csv" | "csv2delta" => Ok("csvToDelta"),
        "parquettodelta" | "parquet" | "jsontodelta" | "json" | "exceltodelta" | "excel"
        | "xlsx" | "ai" | "summarization" | "translation" | "sentiment" | "pii" => {
            Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Transform '{input}' is not available via the Fabric REST API."),
                "Only 'csvToDelta' is exposed by the shortcuts REST API. Parquet/JSON/Excel and \
                 AI-powered (summarization/translation/sentiment/PII/name-recognition) transforms \
                 are currently portal-only."
                    .to_string(),
            )
            .into())
        }
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Unknown transform type '{input}'."),
            "The only supported transform is 'csvToDelta' (aliases: csv). Or pass a raw transform \
             object with --transform-json."
                .to_string(),
        )
        .into()),
    }
}

/// Build the optional `transform` object for a shortcut create request:
/// `{type: "csvToDelta", includeSubfolders, properties: {delimiter,
/// useFirstRowAsHeader, skipFilesWithErrors}}`. Returns `None` when no transform
/// was requested. `--transform-json` (raw) overrides the typed flags. Pure.
fn build_transformation(f: &ShortcutTransformFlags) -> Result<Option<Value>> {
    if let Some(json) = f.transform_json {
        let v: Value = serde_json::from_str(json)
            .map_err(|e| FabioError::invalid_input(format!("Invalid --transform-json: {e}")))?;
        return Ok(Some(v));
    }
    let Some(t) = f.transform_type else {
        return Ok(None);
    };
    normalize_transform_type(t)?; // currently only csvToDelta succeeds
    Ok(Some(serde_json::json!({
        "type": "csvToDelta",
        "includeSubfolders": f.include_subfolders,
        "properties": {
            "delimiter": f.csv_delimiter.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(","),
            "useFirstRowAsHeader": !f.csv_no_header,
            "skipFilesWithErrors": !f.csv_keep_error_files,
        }
    })))
}

/// List shortcuts within an item, optionally under a parent path.
///
/// By default, DW-managed shortcuts (internal OneLake→OneLake references that
/// Warehouse/SQL endpoints create under `Tables/…`, which can number in the
/// thousands) are hidden; pass `include_managed` to show them.
pub(super) async fn list_shortcuts(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    parent_path: Option<&str>,
    include_managed: bool,
) -> Result<()> {
    let mut path = format!("/workspaces/{workspace}/items/{id}/shortcuts");
    if let Some(pp) = parent_path.filter(|s| !s.is_empty()) {
        use std::fmt::Write as _;
        let _ = write!(path, "?parentPath={}", urlencoding::encode(pp));
    }

    let resp = client.get_list(&path, "value", true, None).await?;
    let items: Vec<Value> = if include_managed {
        resp.items
    } else {
        resp.items
            .into_iter()
            .filter(|s| !is_managed_shortcut(s))
            .collect()
    };

    output::render_list_with_token(
        cli,
        &items,
        &["name", "path"],
        &["NAME", "PATH"],
        "name",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

/// Heuristic for a DW-managed shortcut: an internal OneLake→OneLake reference
/// living under a `Tables/…` path (created by Warehouse/SQL endpoints). Pure.
fn is_managed_shortcut(shortcut: &Value) -> bool {
    let path = shortcut.get("path").and_then(Value::as_str).unwrap_or("");
    let under_tables = path == "Tables" || path.starts_with("Tables/");
    if !under_tables {
        return false;
    }
    let target = shortcut.get("target");
    let is_onelake_type = target
        .and_then(|t| t.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|t| t.eq_ignore_ascii_case("oneLake"));
    let has_onelake_key = target.and_then(|t| t.get("oneLake")).is_some();
    is_onelake_type || has_onelake_key
}

pub(super) async fn get_shortcut(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    path: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/items/{id}/shortcuts/{path}/{name}"
        ))
        .await?;
    output::render_object(cli, &data, "name");
    Ok(())
}

pub(super) async fn delete_shortcut(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    path: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "lakehouse delete-shortcut",
        &serde_json::json!({ "workspace": workspace, "id": id, "name": name, "path": path }),
    ) {
        return Ok(());
    }
    client
        .delete(&format!(
            "/workspaces/{workspace}/items/{id}/shortcuts/{path}/{name}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse delete-shortcut", "Contributor"))?;

    let obj = serde_json::json!({
        "name": name,
        "path": path,
        "status": "deleted"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn bulk_create_shortcuts(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    conflict_policy: Option<&str>,
) -> Result<()> {
    let input = read_shortcut_json_input(file, content)?;

    // Wrap in the API envelope if user provided a raw array
    let body = if input.is_array() {
        serde_json::json!({ "createShortcutRequests": input })
    } else {
        input
    };

    if output::dry_run_guard(cli, "lakehouse bulk-create-shortcuts", &body) {
        return Ok(());
    }

    let mut url = format!("/workspaces/{workspace}/items/{id}/shortcuts/bulkCreate");
    if let Some(policy) = conflict_policy {
        use std::fmt::Write;
        let _ = write!(url, "?shortcutConflictPolicy={policy}");
    }

    let data = client.post(&url, &body, true).await?;
    output::render_object(cli, &data, "value");
    Ok(())
}

fn read_shortcut_json_input(file: Option<&str>, content: Option<&str>) -> Result<Value> {
    if let Some(c) = content {
        serde_json::from_str(c).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in --content: {e}"),
                "Provide a valid JSON array of shortcut requests.".to_string(),
            )
            .into()
        })
    } else if let Some(f) = file {
        let data = std::fs::read_to_string(f).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read file '{f}': {e}"),
                "Provide a valid file path.".to_string(),
            )
        })?;
        serde_json::from_str(&data).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in file '{f}': {e}"),
                "Provide a valid JSON array of shortcut requests.".to_string(),
            )
            .into()
        })
    } else {
        Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            "Example: fabio lakehouse bulk-create-shortcuts --workspace <WS> --id <ID> --file shortcuts.json".to_string(),
        )
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_none_when_not_requested() {
        let f = ShortcutTransformFlags::default();
        assert!(build_transformation(&f).unwrap().is_none());
    }

    #[test]
    fn transform_csv_builds_expected_shape() {
        let f = ShortcutTransformFlags {
            transform_type: Some("csv"),
            ..Default::default()
        };
        let t = build_transformation(&f).unwrap().unwrap();
        assert_eq!(t["type"], "csvToDelta");
        assert_eq!(t["includeSubfolders"], false);
        assert_eq!(t["properties"]["delimiter"], ",");
        assert_eq!(t["properties"]["useFirstRowAsHeader"], true);
        assert_eq!(t["properties"]["skipFilesWithErrors"], true);
    }

    #[test]
    fn transform_csv_honors_flags() {
        let f = ShortcutTransformFlags {
            transform_type: Some("csvToDelta"),
            csv_delimiter: Some(";"),
            csv_no_header: true,
            csv_keep_error_files: true,
            include_subfolders: true,
            transform_json: None,
        };
        let t = build_transformation(&f).unwrap().unwrap();
        assert_eq!(t["includeSubfolders"], true);
        assert_eq!(t["properties"]["delimiter"], ";");
        assert_eq!(t["properties"]["useFirstRowAsHeader"], false);
        assert_eq!(t["properties"]["skipFilesWithErrors"], false);
    }

    #[test]
    fn transform_json_escape_hatch_overrides() {
        let f = ShortcutTransformFlags {
            transform_type: Some("csv"),
            transform_json: Some(r#"{"type":"customTransform","x":1}"#),
            ..Default::default()
        };
        let t = build_transformation(&f).unwrap().unwrap();
        assert_eq!(t["type"], "customTransform");
        assert_eq!(t["x"], 1);
    }

    #[test]
    fn transform_rejects_portal_only_types_with_hint() {
        for ty in ["parquet", "json", "excel", "xlsx", "summarization"] {
            let f = ShortcutTransformFlags {
                transform_type: Some(ty),
                ..Default::default()
            };
            let err = build_transformation(&f).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("not available"), "{ty}: {msg}");
        }
    }

    #[test]
    fn transform_rejects_unknown_type() {
        let f = ShortcutTransformFlags {
            transform_type: Some("bogus"),
            ..Default::default()
        };
        assert!(build_transformation(&f).is_err());
    }

    #[test]
    fn is_managed_shortcut_detects_dw_tables_onelake() {
        let managed = serde_json::json!({
            "name": "dbo.orders", "path": "Tables/dbo",
            "target": {"type": "oneLake", "oneLake": {"workspaceId": "w"}}
        });
        assert!(is_managed_shortcut(&managed));

        let user_files = serde_json::json!({
            "name": "ext", "path": "Files",
            "target": {"type": "adlsGen2", "adlsGen2": {"location": "x"}}
        });
        assert!(!is_managed_shortcut(&user_files));

        let user_table = serde_json::json!({
            "name": "ext", "path": "Tables",
            "target": {"type": "amazonS3"}
        });
        assert!(!is_managed_shortcut(&user_table));
    }
}
