use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

use super::resolve_datasource_id;

fn validate_file_name(file_name: &str) -> Result<()> {
    if std::path::Path::new(file_name)
        .extension()
        .is_some_and(|extension| extension == "md")
        && file_name.len() > 3
    {
        return Ok(());
    }
    Err(FabioError::with_hint(
        ErrorCode::InvalidInput,
        "Topic file name must end with .md",
        "Use a Markdown file name such as --file-name company-policies.md.",
    )
    .into())
}

fn path(workspace: &str, id: &str, datasource_id: &str, stage: &str) -> String {
    let prefix = if stage.eq_ignore_ascii_case("published") {
        ""
    } else {
        "/staging"
    };
    format!("/workspaces/{workspace}/dataAgents/{id}{prefix}/datasources/{datasource_id}/topics")
}

pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    stage: &str,
) -> Result<()> {
    let datasource_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let response = client
        .get_list(
            &path(workspace, id, &datasource_id, stage),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent list-topics", "Viewer"))?;
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
    id: &str,
    datasource: &str,
    file_name: &str,
    stage: &str,
) -> Result<()> {
    validate_file_name(file_name)?;
    let datasource_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let content = client
        .get_text(&format!(
            "{}/content?fileName={}",
            path(workspace, id, &datasource_id, stage),
            urlencoding::encode(file_name)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent download-topic", "Viewer"))?;
    output::render_object(
        cli,
        &serde_json::json!({"fileName": file_name, "content": content}),
        "content",
    );
    Ok(())
}

fn read_content(file: Option<&str>, content: Option<&str>) -> Result<String> {
    match (file, content) {
        (Some(path), None) => std::fs::read_to_string(path).map_err(|error| {
            FabioError::invalid_input(format!("Cannot read {path}: {error}")).into()
        }),
        (None, Some(value)) => Ok(value.to_owned()),
        (None, None) => std::io::read_to_string(std::io::stdin()).map_err(|error| {
            FabioError::invalid_input(format!("Cannot read stdin: {error}")).into()
        }),
        (Some(_), Some(_)) => unreachable!("clap prevents conflicting content inputs"),
    }
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
    validate_file_name(file_name)?;
    if let Some(etag) = if_match
        && !(etag.starts_with('"') && etag.ends_with('"'))
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--if-match must be a quoted ETag",
            r#"Pass the ETag exactly as returned, including quotes: --if-match '"etag-value"'."#,
        )
        .into());
    }
    let content = read_content(file, content)?;
    let preview = serde_json::json!({"fileName": file_name, "contentLength": content.len()});
    if output::dry_run_guard(cli, "data-agent upload-topic", &preview) {
        return Ok(());
    }
    let datasource_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let data = client
        .put_raw_with_if_match(
            &format!(
                "{}?fileName={}",
                path(workspace, id, &datasource_id, "staging"),
                urlencoding::encode(file_name)
            ),
            &content,
            if_match,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent upload-topic", "Contributor"))?;
    output::render_object(
        cli,
        &serde_json::json!({"fileName": file_name, "status": "uploaded", "response": data}),
        "status",
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
) -> Result<()> {
    validate_file_name(file_name)?;
    if output::dry_run_guard(
        cli,
        "data-agent delete-topic",
        &serde_json::json!({"datasource": datasource, "fileName": file_name}),
    ) {
        return Ok(());
    }
    let datasource_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    client
        .delete(&format!(
            "{}?fileName={}",
            path(workspace, id, &datasource_id, "staging"),
            urlencoding::encode(file_name)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent delete-topic", "Contributor"))?;
    output::render_object(
        cli,
        &serde_json::json!({"fileName": file_name, "status": "deleted"}),
        "status",
    );
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
    validate_file_name(file_name)?;
    let datasource_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let content = client
        .post_readonly_text(&format!(
            "{}/format?fileName={}",
            path(workspace, id, &datasource_id, "staging"),
            urlencoding::encode(file_name)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent format-topic", "Viewer"))?;
    output::render_object(
        cli,
        &serde_json::json!({"fileName": file_name, "content": content}),
        "content",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{path, validate_file_name};

    #[test]
    fn topic_paths_distinguish_stages() {
        assert!(path("w", "a", "d", "staging").contains("/staging/datasources/"));
        assert!(!path("w", "a", "d", "published").contains("/staging/"));
    }

    #[test]
    fn topic_file_name_requires_markdown() {
        assert!(validate_file_name("topic.md").is_ok());
        assert!(validate_file_name("topic.txt").is_err());
    }
}
