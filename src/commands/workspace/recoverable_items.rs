use std::fmt::Write;

use anyhow::Result;

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::enrich_forbidden;
use crate::output;

fn list_path(workspace: &str, item_type: Option<&str>, recoverable_by_me: Option<bool>) -> String {
    let mut path = format!("/workspaces/{workspace}/recoverableItems");
    let mut separator = '?';

    if let Some(item_type) = item_type {
        let _ = write!(path, "{separator}type={}", urlencoding::encode(item_type));
        separator = '&';
    }
    if let Some(recoverable_by_me) = recoverable_by_me {
        let _ = write!(path, "{separator}recoverableByMe={recoverable_by_me}");
    }

    path
}

pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_type: Option<&str>,
    recoverable_by_me: Option<bool>,
) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    let response = client
        .get_list(
            &list_path(workspace, item_type, recoverable_by_me),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|error| enrich_forbidden(error, "workspace list-recoverable-items", "Viewer"))?;

    let display_items;
    let items = if output::has_tags(&response.items) {
        display_items = output::enrich_with_tags_display(&response.items);
        &display_items
    } else {
        &response.items
    };

    output::render_list_with_token(
        cli,
        items,
        &[
            "displayName",
            "id",
            "type",
            "parentItemId",
            "retentionExpirationDateTime",
            "_tagsDisplay",
        ],
        &[
            "NAME",
            "ID",
            "TYPE",
            "PARENT ITEM ID",
            "RETENTION EXPIRES",
            "TAGS",
        ],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn recover(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(item_id, "--item-id")?;
    let preview = serde_json::json!({
        "workspaceId": workspace,
        "itemId": item_id,
        "recoversDescendants": true,
    });
    if output::dry_run_guard(cli, "workspace recover-item", &preview) {
        return Ok(());
    }

    let response = client
        .post(
            &format!("/workspaces/{workspace}/recoverableItems/{item_id}/recover"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|error| enrich_forbidden(error, "workspace recover-item", "Contributor"))?;

    if let Some(items) = response.get("value").and_then(serde_json::Value::as_array) {
        output::render_list(
            cli,
            items,
            &["displayName", "id", "type", "workspaceId"],
            &["NAME", "ID", "TYPE", "WORKSPACE ID"],
            "id",
        );
    } else {
        output::render_object(cli, &response, "id");
    }
    Ok(())
}

pub(super) async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(item_id, "--item-id")?;
    let preview = serde_json::json!({
        "workspaceId": workspace,
        "itemId": item_id,
        "permanent": true,
    });
    if output::dry_run_guard(cli, "workspace delete-recoverable-item", &preview) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/recoverableItems/{item_id}"
        ))
        .await
        .map_err(|error| {
            enrich_forbidden(error, "workspace delete-recoverable-item", "Contributor")
        })?;
    output::render_object(
        cli,
        &serde_json::json!({
            "workspaceId": workspace,
            "id": item_id,
            "status": "deleted",
            "permanent": true,
        }),
        "status",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::list_path;

    #[test]
    fn list_path_has_no_query_when_filters_are_absent() {
        assert_eq!(
            list_path("workspace-id", None, None),
            "/workspaces/workspace-id/recoverableItems"
        );
    }

    #[test]
    fn list_path_serializes_all_query_parameters() {
        assert_eq!(
            list_path("workspace-id", Some("SQL Endpoint"), Some(true)),
            "/workspaces/workspace-id/recoverableItems?type=SQL%20Endpoint&recoverableByMe=true"
        );
    }

    #[test]
    fn list_path_serializes_false_recoverable_filter() {
        assert_eq!(
            list_path("workspace-id", None, Some(false)),
            "/workspaces/workspace-id/recoverableItems?recoverableByMe=false"
        );
    }
}
