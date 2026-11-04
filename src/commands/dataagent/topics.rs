use std::ffi::OsStr;
use std::path::Path;

use anyhow::Result;

use super::{resolve_datasource_id_at_stage, stage_prefix};
use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

fn validate_topic_file_name(file_name: &str) -> Result<()> {
    if file_name.is_empty()
        || Path::new(file_name).extension() != Some(OsStr::new("md"))
        || file_name.contains('/')
        || file_name.contains('\\')
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Topic file name must be a basename ending in .md",
            "Example: --file-name company-policies.md",
        )
        .into());
    }
    Ok(())
}

fn validate_if_match(if_match: Option<&str>) -> Result<()> {
    if let Some(value) = if_match
        && !(value.len() >= 2 && value.starts_with('"') && value.ends_with('"'))
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--if-match must be an ETag enclosed in double quotes",
            r#"Example: --if-match '"etag-value"'"#,
        )
        .into());
    }
    Ok(())
}

fn topic_base(workspace: &str, agent_id: &str, datasource_id: &str, stage: &str) -> String {
    let prefix = stage_prefix(stage);
    format!(
        "/workspaces/{workspace}/dataAgents/{agent_id}{prefix}/datasources/{datasource_id}/topics"
    )
}

pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    agent_id: &str,
    datasource: &str,
    stage: &str,
) -> Result<()> {
    let datasource_id =
        resolve_datasource_id_at_stage(client, workspace, agent_id, datasource, stage).await?;
    let response = client
        .get_list(
            &topic_base(workspace, agent_id, &datasource_id, stage),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["fileName"],
        &["FILE NAME"],
        "fileName",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn download(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    agent_id: &str,
    datasource: &str,
    file_name: &str,
    stage: &str,
) -> Result<()> {
    validate_topic_file_name(file_name)?;
    let datasource_id =
        resolve_datasource_id_at_stage(client, workspace, agent_id, datasource, stage).await?;
    let path = format!(
        "{}/content?fileName={}",
        topic_base(workspace, agent_id, &datasource_id, stage),
        urlencoding::encode(file_name)
    );
    let response = client.get_text_response(&path).await?;
    let data = serde_json::json!({
        "fileName": file_name,
        "content": response.content,
        "etag": response.etag,
    });
    output::render_object(cli, &data, "fileName");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn upload(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    agent_id: &str,
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
                "Example: --file topic.md, or --content '# Topic'",
            )
            .into());
        }
    };
    if output::dry_run_guard(
        cli,
        "data-agent upload-topic",
        &serde_json::json!({
            "workspace": workspace,
            "id": agent_id,
            "datasource": datasource,
            "fileName": file_name,
            "contentLength": markdown.len(),
            "ifMatch": if_match,
        }),
    ) {
        return Ok(());
    }

    let datasource_id =
        resolve_datasource_id_at_stage(client, workspace, agent_id, datasource, "staging").await?;
    let path = format!(
        "{}?fileName={}",
        topic_base(workspace, agent_id, &datasource_id, "staging"),
        urlencoding::encode(file_name)
    );
    let response = client
        .put_raw_with_if_match(&path, &markdown, "text/markdown", if_match)
        .await?;
    let data = serde_json::json!({
        "status": "uploaded",
        "fileName": file_name,
        "etag": response.get("etag"),
    });
    output::render_object(cli, &data, "status");
    Ok(())
}

pub(super) async fn format(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    agent_id: &str,
    datasource: &str,
    file_name: &str,
) -> Result<()> {
    validate_topic_file_name(file_name)?;
    let datasource_id =
        resolve_datasource_id_at_stage(client, workspace, agent_id, datasource, "staging").await?;
    let path = format!(
        "{}/format?fileName={}",
        topic_base(workspace, agent_id, &datasource_id, "staging"),
        urlencoding::encode(file_name)
    );
    let response = client
        .post_empty_readonly_text_with_accept(&path, "text/markdown")
        .await?;
    let data = serde_json::json!({
        "fileName": file_name,
        "content": response.content,
        "etag": response.etag,
        "applied": false,
    });
    output::render_object(cli, &data, "fileName");
    Ok(())
}

pub(super) async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    agent_id: &str,
    datasource: &str,
    file_name: &str,
    if_match: Option<&str>,
) -> Result<()> {
    validate_topic_file_name(file_name)?;
    validate_if_match(if_match)?;
    if output::dry_run_guard(
        cli,
        "data-agent delete-topic",
        &serde_json::json!({
            "workspace": workspace,
            "id": agent_id,
            "datasource": datasource,
            "fileName": file_name,
            "ifMatch": if_match,
        }),
    ) {
        return Ok(());
    }
    let datasource_id =
        resolve_datasource_id_at_stage(client, workspace, agent_id, datasource, "staging").await?;
    let path = format!(
        "{}?fileName={}",
        topic_base(workspace, agent_id, &datasource_id, "staging"),
        urlencoding::encode(file_name)
    );
    client.delete_with_if_match(&path, if_match).await?;
    let data = serde_json::json!({"status": "deleted", "fileName": file_name});
    output::render_object(cli, &data, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_if_match, validate_topic_file_name};

    #[test]
    fn topic_file_name_requires_safe_markdown_basename() {
        assert!(validate_topic_file_name("company-policies.md").is_ok());
        for invalid in [
            "",
            "topic.txt",
            "../topic.md",
            "folder/topic.md",
            r"folder\topic.md",
        ] {
            assert!(validate_topic_file_name(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn if_match_requires_quoted_etag() {
        assert!(validate_if_match(None).is_ok());
        assert!(validate_if_match(Some("\"etag-1\"")).is_ok());
        assert!(validate_if_match(Some("etag-1")).is_err());
        assert!(validate_if_match(Some("\"etag-1")).is_err());
    }
}
