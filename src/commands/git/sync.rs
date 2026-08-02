//! Git sync operations: status, commit, pull, and tracked-item listing.

use anyhow::{Result, bail};
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

pub(super) async fn status(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let data = client
        .get_with_lro(&format!("/workspaces/{workspace}/git/status"))
        .await?;

    let changes = data
        .get("changes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if changes.is_empty() {
        output::render_object(cli, &data, "status");
    } else {
        output::render_list(
            cli,
            &changes,
            &[
                "itemMetadata.displayName",
                "itemMetadata.itemType",
                "workspaceChange",
                "remoteChange",
                "conflictType",
            ],
            &["NAME", "TYPE", "WORKSPACE", "REMOTE", "CONFLICT"],
            "itemMetadata.displayName",
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn commit(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    message: Option<&str>,
    all: bool,
    items: Option<&[String]>,
    workspace_head: Option<&str>,
    wait: bool,
    timeout: u64,
) -> Result<()> {
    if !all && items.is_none() {
        bail!("Specify --all to commit all changes, or --items for selective commit");
    }

    // Auto-fetch workspace head if not provided
    let head = if let Some(h) = workspace_head {
        h.to_string()
    } else {
        let status = client
            .get_with_lro(&format!("/workspaces/{workspace}/git/status"))
            .await?;
        status
            .get("workspaceHead")
            .and_then(Value::as_str)
            .ok_or_else(|| FabioError::with_hint(
                ErrorCode::ApiError,
                "Could not determine workspaceHead from status",
                "Ensure the workspace is connected to Git and initialized: fabio git connection show --workspace <WS>",
            ))?
            .to_string()
    };

    let mode = if all { "All" } else { "Selective" };
    let mut body = serde_json::json!({
        "mode": mode,
        "workspaceHead": head,
    });

    if let Some(msg) = message {
        body["comment"] = Value::from(msg);
    }

    if let Some(item_ids) = items {
        let item_objs: Vec<Value> = item_ids
            .iter()
            .map(|id| serde_json::json!({"objectId": id}))
            .collect();
        body["items"] = Value::Array(item_objs);
    }

    let data = client
        .post_with_timeout(
            &format!("/workspaces/{workspace}/git/commitToGit"),
            &body,
            wait,
            timeout,
        )
        .await?;

    output::render_object(cli, &data, "status");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn pull(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    conflict_resolution: Option<&str>,
    allow_override: bool,
    workspace_head: Option<&str>,
    remote_commit_hash: Option<&str>,
    wait: bool,
    timeout: u64,
) -> Result<()> {
    // Auto-fetch hashes from status if not provided
    let (head, remote_hash) = if let (Some(h), Some(r)) = (workspace_head, remote_commit_hash) {
        (h.to_string(), r.to_string())
    } else {
        let status = client
            .get_with_lro(&format!("/workspaces/{workspace}/git/status"))
            .await?;
        let h = workspace_head
            .map(String::from)
            .or_else(|| {
                status
                    .get("workspaceHead")
                    .and_then(Value::as_str)
                    .map(String::from)
            })
            .ok_or_else(|| FabioError::with_hint(
                ErrorCode::ApiError,
                "Could not determine workspaceHead from status",
                "Ensure the workspace is connected to Git and initialized: fabio git connection show --workspace <WS>",
            ))?;
        let r = remote_commit_hash
            .map(String::from)
            .or_else(|| {
                status
                    .get("remoteCommitHash")
                    .and_then(Value::as_str)
                    .map(String::from)
            })
            .ok_or_else(|| FabioError::with_hint(
                ErrorCode::ApiError,
                "Could not determine remoteCommitHash from status",
                "Ensure there are remote commits to pull. Check remote branch status with: fabio git status --workspace <WS>",
            ))?;
        (h, r)
    };

    let mut body = serde_json::json!({
        "remoteCommitHash": remote_hash,
        "workspaceHead": head,
    });

    if let Some(policy) = conflict_resolution {
        let api_policy = match policy {
            "prefer-remote" => "PreferRemote",
            "prefer-workspace" => "PreferWorkspace",
            _ => policy,
        };
        body["conflictResolution"] = serde_json::json!({
            "conflictResolutionType": "Workspace",
            "conflictResolutionPolicy": api_policy,
        });
    }

    if allow_override {
        body["options"] = serde_json::json!({
            "allowOverrideItems": true,
        });
    }

    let data = client
        .post_with_timeout(
            &format!("/workspaces/{workspace}/git/updateFromGit"),
            &body,
            wait,
            timeout,
        )
        .await?;

    output::render_object(cli, &data, "status");
    Ok(())
}

/// Show items tracked by Git integration in a workspace.
///
/// Fetches git status and lists ALL items with their sync state:
/// - tracked: items in git with no pending changes
/// - added/modified/deleted: items with uncommitted workspace changes
/// - remote changes: incoming changes from the remote branch
///
/// This helps agents understand what Fabric Git tracks (item definitions only,
/// NOT table data, uploaded files, or `OneLake` runtime data).
#[allow(clippy::too_many_lines)]
pub(super) async fn show_tracked(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    // Get connection info to verify workspace is connected
    let connection = client
        .get(&format!("/workspaces/{workspace}/git/connection"))
        .await?;

    let state = connection
        .get("gitConnectionState")
        .and_then(Value::as_str)
        .unwrap_or("NotConnected");

    if state == "NotConnected" || state == "NotInitialized" {
        let hint = if state == "NotConnected" {
            "Connect first with: fabio git connect --workspace <ID> --provider <github|azure-devops> ...\n\
             For GitHub, you also need --connection-id. Find it with: fabio connection list"
                .to_string()
        } else {
            "Workspace is connected but not initialized. Run: fabio git init --workspace <ID> --strategy prefer-workspace --wait"
                .to_string()
        };
        return Err(FabioError::with_hint(
            ErrorCode::ApiError,
            format!("Workspace Git state: {state}. Cannot show tracked items."),
            hint,
        )
        .into());
    }

    let provider = connection
        .get("gitProviderDetails")
        .and_then(|d| d.get("repositoryName"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let branch = connection
        .get("gitProviderDetails")
        .and_then(|d| d.get("branchName"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    // Get git status (LRO-aware)
    let status_data = client
        .get_with_lro(&format!("/workspaces/{workspace}/git/status"))
        .await?;

    let workspace_head = status_data
        .get("workspaceHead")
        .and_then(Value::as_str)
        .unwrap_or("(none)");

    let remote_head = status_data
        .get("remoteCommitHash")
        .and_then(Value::as_str)
        .unwrap_or("(none)");

    let changes = status_data
        .get("changes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // Build tracked items list: each item gets a status label
    let mut tracked_items: Vec<Value> = Vec::new();

    for change in &changes {
        let display_name = change
            .pointer("/itemMetadata/displayName")
            .and_then(Value::as_str)
            .unwrap_or("(unknown)");
        let item_type = change
            .pointer("/itemMetadata/itemType")
            .and_then(Value::as_str)
            .unwrap_or("(unknown)");
        let object_id = change
            .pointer("/itemMetadata/itemIdentifier/objectId")
            .and_then(Value::as_str)
            .unwrap_or("");
        let workspace_change = change
            .get("workspaceChange")
            .and_then(Value::as_str)
            .unwrap_or("None");
        let remote_change = change.get("remoteChange").and_then(Value::as_str);
        let conflict_type = change
            .get("conflictType")
            .and_then(Value::as_str)
            .unwrap_or("None");

        let status = match workspace_change {
            "Added" => "uncommitted (new)",
            "Modified" => "uncommitted (modified)",
            "Deleted" => "uncommitted (deleted)",
            _ => {
                if remote_change.is_some_and(|r| r != "None") {
                    "incoming remote change"
                } else if conflict_type != "None" {
                    "conflict"
                } else {
                    "tracked"
                }
            }
        };

        tracked_items.push(serde_json::json!({
            "displayName": display_name,
            "itemType": item_type,
            "objectId": object_id,
            "status": status,
            "workspaceChange": workspace_change,
            "remoteChange": remote_change.unwrap_or("None"),
            "conflict": conflict_type,
        }));
    }

    // If no changes, workspace is fully synced
    if tracked_items.is_empty() {
        let result = serde_json::json!({
            "repository": provider,
            "branch": branch,
            "workspaceHead": workspace_head,
            "remoteHead": remote_head,
            "status": "clean",
            "message": "All items are synced. No pending changes.",
            "items": [],
            "note": "Fabric Git tracks item definitions only (notebooks, lakehouses, pipelines). Table data, uploaded files, and OneLake runtime data are NOT tracked."
        });
        output::render_object(cli, &result, "status");
    } else {
        let result = serde_json::json!({
            "repository": provider,
            "branch": branch,
            "workspaceHead": workspace_head,
            "remoteHead": remote_head,
            "totalChanges": tracked_items.len(),
            "items": tracked_items,
            "note": "Fabric Git tracks item definitions only (notebooks, lakehouses, pipelines). Table data, uploaded files, and OneLake runtime data are NOT tracked."
        });

        // Render as table for human readability
        output::render_list(
            cli,
            result["items"].as_array().unwrap(),
            &[
                "displayName",
                "itemType",
                "status",
                "workspaceChange",
                "remoteChange",
            ],
            &["NAME", "TYPE", "STATUS", "WORKSPACE", "REMOTE"],
            "displayName",
        );
    }

    Ok(())
}
