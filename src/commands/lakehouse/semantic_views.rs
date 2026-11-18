use std::fmt::Write;

use anyhow::Result;
use serde_json::json;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

fn list_path(workspace: &str, lakehouse: &str, max_results: Option<usize>) -> String {
    let mut path =
        format!("/workspaces/{workspace}/lakehouses/{lakehouse}/semanticViews?beta=true");
    if let Some(max_results) = max_results {
        let _ = write!(path, "&maxResults={}", max_results.clamp(1, 100));
    }
    path
}

fn publish_path(workspace: &str, lakehouse: &str, name: &str) -> String {
    format!(
        "/workspaces/{workspace}/lakehouses/{lakehouse}/semanticViews/{}/publish?beta=true",
        urlencoding::encode(name)
    )
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Semantic view name cannot be empty",
            "Provide --name with the authored semantic view name.",
        )
        .into());
    }
    Ok(())
}

pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: &str,
) -> Result<()> {
    let response = client
        .get_list(
            &list_path(workspace, lakehouse, cli.limit),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|error| enrich_forbidden(error, "lakehouse list-semantic-views", "Viewer"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["name"],
        &["NAME"],
        "name",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn publish(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: &str,
    name: &str,
) -> Result<()> {
    validate_name(name)?;
    let preview =
        json!({"workspaceId": workspace, "lakehouseId": lakehouse, "semanticViewName": name});
    if output::dry_run_guard(cli, "lakehouse publish-semantic-view", &preview) {
        return Ok(());
    }

    let data = client
        .post_empty(&publish_path(workspace, lakehouse, name), true)
        .await
        .map_err(|error| {
            enrich_forbidden(error, "lakehouse publish-semantic-view", "Contributor")
        })?;
    output::render_object(cli, &data, "semanticModel.itemId");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_path_requires_beta_and_bounds_page_size() {
        assert_eq!(
            list_path("workspace", "lakehouse", Some(500)),
            "/workspaces/workspace/lakehouses/lakehouse/semanticViews?beta=true&maxResults=100"
        );
    }

    #[test]
    fn publish_path_encodes_semantic_view_name() {
        assert_eq!(
            publish_path("workspace", "lakehouse", "sales overview"),
            "/workspaces/workspace/lakehouses/lakehouse/semanticViews/sales%20overview/publish?beta=true"
        );
    }

    #[test]
    fn empty_semantic_view_name_is_rejected() {
        assert!(validate_name("").is_err());
        assert!(validate_name("sales_overview").is_ok());
    }
}
