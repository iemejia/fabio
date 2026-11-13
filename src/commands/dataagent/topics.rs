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
    if file_name.is_empty()
        || Path::new(file_name).extension() != Some(OsStr::new("md"))
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.chars().any(char::is_control)
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
        && (!(value.len() >= 2 && value.starts_with('"') && value.ends_with('"'))
            || value.chars().any(char::is_control))
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

fn validate_etag_snapshot(requested: &str, current: Option<&str>) -> Result<()> {
    if current != Some(requested) {
        return Err(FabioError::with_hint(ErrorCode::Conflict,
            "The topic ETag is stale or its current version cannot be verified; no mutation was sent",
            "Download and review the current topic before requesting another conditional mutation.")
            .set_retriable(Some(false)).into());
    }
    Ok(())
}

async fn check_etag_snapshot(
    client: &FabricClient,
    base: &str,
    file_name: &str,
    requested: Option<&str>,
) -> Result<()> {
    if let Some(requested) = requested {
        let current = client
            .get_text_response(&format!(
                "{base}/content?fileName={}",
                urlencoding::encode(file_name)
            ))
            .await?;
        validate_etag_snapshot(requested, current.etag.as_deref())?;
    }
    Ok(())
}

fn attach_precondition_notice(data: &mut Value, requested: Option<&str>) {
    if requested.is_some() {
        data["preconditionCheck"] = Value::from("client_snapshot");
        data["warnings"] = serde_json::json!([
            "The client ETag check is a snapshot, not an atomic compare-and-swap. Service-side If-Match enforcement is unverified; avoid concurrent writers."
        ]);
    }
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
    crate::client::validate_uuid(workspace, "--workspace")?;
    crate::client::validate_uuid(agent_id, "--id")?;
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
        (None, None) => crate::commands::query_input::resolve_query_input(
            None,
            "topic Markdown",
            "--content",
            "--file topic.md",
        )?,
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
            "preconditionCheck": "not_checked",
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
    if !cli.readonly {
        check_etag_snapshot(
            client,
            &topic_base(workspace, agent_id, &datasource_id, "staging"),
            file_name,
            if_match,
        )
        .await?;
    }
    let response = client
        .put_raw_with_if_match(&path, &markdown, "text/markdown", if_match)
        .await?;
    let mut data = serde_json::json!({
        "status": "uploaded",
        "fileName": file_name,
        "etag": response.get("etag"),
    });
    attach_precondition_notice(&mut data, if_match);
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
    crate::client::validate_uuid(workspace, "--workspace")?;
    crate::client::validate_uuid(agent_id, "--id")?;
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
            "preconditionCheck": "not_checked",
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
    if !cli.readonly {
        check_etag_snapshot(
            client,
            &topic_base(workspace, agent_id, &datasource_id, "staging"),
            file_name,
            if_match,
        )
        .await?;
    }
    client.delete_with_if_match(&path, if_match).await?;
    let mut data = serde_json::json!({"status": "deleted", "fileName": file_name});
    attach_precondition_notice(&mut data, if_match);
    output::render_object(cli, &data, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn snapshot_preconditions_fail_closed_without_claiming_atomicity() {
        use super::{attach_precondition_notice, validate_etag_snapshot};
        assert!(validate_etag_snapshot("\"current\"", Some("\"current\"")).is_ok());
        assert!(validate_etag_snapshot("\"stale\"", Some("\"current\"")).is_err());
        assert!(validate_etag_snapshot("\"current\"", None).is_err());
        let mut data = serde_json::json!({});
        attach_precondition_notice(&mut data, Some("\"current\""));
        assert_eq!(data["preconditionCheck"], "client_snapshot");
        assert!(
            data["warnings"][0]
                .as_str()
                .unwrap()
                .contains("not an atomic")
        );
    }
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
