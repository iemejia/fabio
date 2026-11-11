use std::path::Path;

use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

pub(super) struct QueryParams<'a> {
    pub workspace: &'a str,
    pub id: &'a str,
    pub page_size: Option<u16>,
    pub continuation_token: Option<&'a str>,
    pub search: Option<&'a str>,
    pub start_submitted_date_time: Option<&'a str>,
    pub end_submitted_date_time: Option<&'a str>,
    pub channels: &'a [String],
    pub products: &'a [String],
    pub platforms: &'a [String],
    pub feedback_types: &'a [String],
    pub sort_by: Option<&'a str>,
    pub sort_order: Option<&'a str>,
}

fn validate_feedback_id(feedback_id: &str) -> Result<()> {
    if !feedback_id.is_empty()
        && feedback_id.len() <= 200
        && !feedback_id.chars().any(char::is_control)
    {
        return Ok(());
    }
    Err(FabioError::invalid_input(
        "Feedback ID must be a non-empty opaque value of at most 200 characters without control characters",
    )
    .into())
}

fn validate_values(name: &str, values: &[String]) -> Result<()> {
    if values.len() > 100
        || values.iter().any(|value| {
            value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control)
        })
    {
        return Err(FabioError::invalid_input(format!(
            "--{name} accepts at most 100 non-blank values of at most 256 characters without control characters"
        ))
        .into());
    }
    Ok(())
}

fn query_body(params: &QueryParams<'_>) -> Result<serde_json::Value> {
    if let Some(search) = params.search
        && (search.len() > 200 || search.chars().any(char::is_control))
    {
        return Err(FabioError::invalid_input(
            "--search must be at most 200 characters and cannot contain control characters",
        )
        .into());
    }
    for (name, values) in [
        ("channels", params.channels),
        ("products", params.products),
        ("platforms", params.platforms),
        ("feedback-types", params.feedback_types),
    ] {
        validate_values(name, values)?;
    }
    let has_query = params.search.is_some()
        || params.start_submitted_date_time.is_some()
        || params.end_submitted_date_time.is_some()
        || !params.channels.is_empty()
        || !params.products.is_empty()
        || !params.platforms.is_empty()
        || !params.feedback_types.is_empty()
        || params.sort_by.is_some()
        || params.sort_order.is_some();
    if params.continuation_token.is_some() && (params.page_size.is_some() || has_query) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--continuation-token cannot be combined with --page-size or query criteria",
            "Pass only the continuation token exactly as returned by the preceding page.",
        )
        .into());
    }
    let mut body = serde_json::json!({});
    if let Some(value) = params.search {
        body["search"] = serde_json::Value::from(value);
    }
    if let Some(value) = params.start_submitted_date_time {
        body["startSubmittedDateTime"] = serde_json::Value::from(value);
    }
    if let Some(value) = params.end_submitted_date_time {
        body["endSubmittedDateTime"] = serde_json::Value::from(value);
    }
    let mut filter = serde_json::json!({});
    for (key, values) in [
        ("channels", params.channels),
        ("products", params.products),
        ("platforms", params.platforms),
        ("feedbackTypes", params.feedback_types),
    ] {
        if !values.is_empty() {
            filter[key] = serde_json::json!(values);
        }
    }
    if filter.as_object().is_some_and(|value| !value.is_empty()) {
        body["filter"] = filter;
    }
    if let Some(value) = params.sort_by {
        body["sortBy"] = serde_json::Value::from(value);
    }
    if let Some(value) = params.sort_order {
        body["sortOrder"] = serde_json::Value::from(value);
    }
    Ok(body)
}

pub(super) async fn query(cli: &Cli, client: &FabricClient, params: QueryParams<'_>) -> Result<()> {
    let body = query_body(&params)?;
    let mut path = format!(
        "/workspaces/{}/dataAgents/{}/feedback/search?beta=true",
        params.workspace, params.id
    );
    if let Some(value) = params.continuation_token {
        path.push_str("&continuationToken=");
        path.push_str(&urlencoding::encode(value));
    } else if let Some(value) = params.page_size {
        path.push_str("&pageSize=");
        path.push_str(&value.to_string());
    }
    let data = if params.continuation_token.is_some() {
        client.post_readonly_empty(&path).await
    } else {
        client.post_readonly(&path, &body).await
    }
    .map_err(|e| enrich_forbidden(e, "data-agent query-feedback", "Contributor"))?;
    let items = data
        .get("value")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let token = data
        .get("continuationToken")
        .and_then(serde_json::Value::as_str);
    output::render_list_with_token(
        cli,
        &items,
        &[
            "id",
            "submittedDateTime",
            "feedbackType",
            "comment",
            "hasAttachments",
        ],
        &["ID", "SUBMITTED", "TYPE", "COMMENT", "ATTACHMENTS"],
        "id",
        token,
    );
    Ok(())
}

pub(super) async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    feedback_id: &str,
) -> Result<()> {
    validate_feedback_id(feedback_id)?;
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/dataAgents/{id}/feedback/{}?beta=true",
            urlencoding::encode(feedback_id)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent show-feedback", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn download_attachments(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    feedback_id: &str,
    output_file: &str,
    language: Option<&str>,
    allow_download_without_sensitivity_label: bool,
) -> Result<()> {
    if !allow_download_without_sensitivity_label {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Downloading attachments requires acknowledging that sensitivity labels are not preserved",
            "After explicit approval, add --allow-download-without-sensitivity-label.",
        )
        .into());
    }
    validate_feedback_id(feedback_id)?;
    if let Some(value) = language
        && (value.is_empty()
            || value.len() > 35
            || value.chars().any(char::is_whitespace)
            || value.chars().any(char::is_control))
    {
        return Err(FabioError::invalid_input(
            "--language must be a non-empty BCP 47 tag of at most 35 characters",
        )
        .into());
    }
    let mut path = format!(
        "/workspaces/{workspace}/dataAgents/{id}/feedback/{}/attachments?beta=true&allowDownloadWithoutSensitivityLabel=true",
        urlencoding::encode(feedback_id)
    );
    if let Some(value) = language {
        path.push_str("&language=");
        path.push_str(&urlencoding::encode(value));
    }
    let preview = serde_json::json!({
        "feedbackId": feedback_id,
        "outputFile": output_file,
        "language": language.unwrap_or("en"),
    });
    if output::dry_run_guard(cli, "data-agent download-feedback-attachments", &preview) {
        return Ok(());
    }
    let bytes = client.get_bytes(&path).await.map_err(|e| {
        enrich_forbidden(e, "data-agent download-feedback-attachments", "Contributor")
    })?;
    std::fs::write(Path::new(output_file), &bytes).map_err(|error| {
        FabioError::invalid_input(format!("Cannot write output file {output_file}: {error}"))
    })?;
    output::render_object(
        cli,
        &serde_json::json!({
            "feedbackId": feedback_id,
            "outputFile": output_file,
            "bytes": bytes.len(),
        }),
        "outputFile",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{QueryParams, query_body, validate_feedback_id};

    fn params<'a>() -> QueryParams<'a> {
        QueryParams {
            workspace: "w",
            id: "a",
            page_size: None,
            continuation_token: None,
            search: None,
            start_submitted_date_time: None,
            end_submitted_date_time: None,
            channels: &[],
            products: &[],
            platforms: &[],
            feedback_types: &[],
            sort_by: None,
            sort_order: None,
        }
    }

    #[test]
    fn continuation_token_rejects_new_query_options() {
        let mut value = params();
        value.continuation_token = Some("opaque");
        value.page_size = Some(50);
        assert!(query_body(&value).is_err());
    }

    #[test]
    fn feedback_id_is_opaque_but_bounded() {
        assert!(validate_feedback_id("fb_2026-07-15:001").is_ok());
        assert!(validate_feedback_id("").is_err());
        assert!(validate_feedback_id(&"x".repeat(201)).is_err());
    }
}
