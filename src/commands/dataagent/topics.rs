use std::ffi::OsStr;
use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use super::{resolve_datasource_id_at_stage, stage_prefix};
use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

fn validate_topic_file_name(file_name: &str) -> Result<()> {
    let path = Path::new(file_name);
    if file_name.contains('/')
        || file_name.contains('\\')
        || path.file_name().and_then(|name| name.to_str()) != Some(file_name)
        || path.extension() != Some(OsStr::new("md"))
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Topic --file-name must be a file name ending in .md",
            "Example: --file-name company-policies.md",
        )
        .into());
    }
    Ok(())
}

fn validate_if_match(if_match: Option<&str>) -> Result<()> {
    if let Some(etag) = if_match
        && !(etag.len() >= 2 && etag.starts_with('"') && etag.ends_with('"'))
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Topic --if-match must be a quoted ETag",
            "Example: --if-match '\"topic-etag\"'",
        )
        .into());
    }
    Ok(())
}

async fn topic_path(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    stage: &str,
) -> Result<String> {
    let datasource_id =
        resolve_datasource_id_at_stage(client, workspace, id, datasource, stage).await?;
    let prefix = stage_prefix(stage);
    Ok(format!(
        "/workspaces/{workspace}/dataAgents/{id}{prefix}/datasources/{datasource_id}/topics"
    ))
}

pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    stage: &str,
) -> Result<()> {
    let path = topic_path(client, workspace, id, datasource, stage).await?;
    let response = client
        .get_list(&path, "value", cli.all, cli.continuation_token.as_deref())
        .await?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["fileName"],
        &["FILE"],
        "fileName",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn download(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    stage: &str,
    file_name: &str,
) -> Result<()> {
    validate_topic_file_name(file_name)?;
    let path = topic_path(client, workspace, id, datasource, stage).await?;
    let data = client
        .get_text_with_metadata(&format!(
            "{path}/content?fileName={}",
            urlencoding::encode(file_name)
        ))
        .await?;
    output::render_object(cli, &data, "content");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn upload(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    file_name: &str,
    file: Option<&str>,
    content: Option<&str>,
    if_match: Option<&str>,
) -> Result<()> {
    validate_topic_file_name(file_name)?;
    validate_if_match(if_match)?;
    let markdown = match (file, content) {
        (Some(path), None) => std::fs::read_to_string(path).map_err(|error| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read topic file '{path}': {error}"),
                "Provide a readable UTF-8 Markdown file.",
            )
        })?,
        (None, Some(value)) => value.to_string(),
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Provide exactly one of --file or --content",
                "Example: --file topic.md",
            )
            .into());
        }
    };
    if output::dry_run_guard(
        cli,
        "data-agent upload-datasource-topic",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
            "fileName": file_name,
            "contentLength": markdown.len(),
            "ifMatch": if_match
        }),
    ) {
        return Ok(());
    }

    let path = topic_path(client, workspace, id, datasource, "staging").await?;
    let response = client
        .put_raw_with_if_match(
            &format!("{path}?fileName={}", urlencoding::encode(file_name)),
            &markdown,
            "text/markdown",
            if_match,
        )
        .await?;
    let result = serde_json::json!({
        "status": "uploaded",
        "fileName": file_name,
        "etag": response.get("etag").cloned().unwrap_or(Value::Null)
    });
    output::render_object(cli, &result, "status");
    Ok(())
}

pub(super) async fn format(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    file_name: &str,
) -> Result<()> {
    validate_topic_file_name(file_name)?;
    let path = topic_path(client, workspace, id, datasource, "staging").await?;
    let data = client
        .post_fabric_text_readonly_empty_with_metadata(
            &format!("{path}/format?fileName={}", urlencoding::encode(file_name)),
            "text/markdown",
        )
        .await?;
    output::render_object(
        cli,
        &serde_json::json!({
            "fileName": file_name,
            "content": data["content"],
            "etag": data["etag"]
        }),
        "content",
    );
    Ok(())
}

pub(super) async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    file_name: &str,
    if_match: Option<&str>,
) -> Result<()> {
    validate_topic_file_name(file_name)?;
    validate_if_match(if_match)?;
    if output::dry_run_guard(
        cli,
        "data-agent delete-datasource-topic",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
            "fileName": file_name,
            "ifMatch": if_match
        }),
    ) {
        return Ok(());
    }
    let path = topic_path(client, workspace, id, datasource, "staging").await?;
    client
        .delete_with_if_match(
            &format!("{path}?fileName={}", urlencoding::encode(file_name)),
            if_match,
        )
        .await?;
    output::render_object(
        cli,
        &serde_json::json!({"status": "deleted", "fileName": file_name}),
        "status",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_file_name_requires_markdown_basename() {
        assert!(validate_topic_file_name("company-policies.md").is_ok());
        for invalid in [
            "topic.txt",
            "../topic.md",
            "folder/topic.md",
            r"folder\topic.md",
            "",
        ] {
            assert!(validate_topic_file_name(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn if_match_requires_quoted_etag() {
        assert!(validate_if_match(None).is_ok());
        assert!(validate_if_match(Some("\"topic-etag\"")).is_ok());
        assert!(validate_if_match(Some("topic-etag")).is_err());
    }
}
