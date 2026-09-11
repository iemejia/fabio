//! Git sync operations: status, commit, pull, and tracked-item listing.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

pub(super) async fn status(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    include_files_details: bool,
) -> Result<()> {
    let data = client
        .get_with_lro(&build_status_url(workspace, include_files_details))
        .await?;

    if include_files_details {
        output::render_object(cli, &data, "workspaceHead");
        return Ok(());
    }

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

fn build_status_url(workspace: &str, include_files_details: bool) -> String {
    let suffix = if include_files_details {
        "?includeFilesDetails=true"
    } else {
        ""
    };
    format!("/workspaces/{workspace}/git/status{suffix}")
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ItemWithFileSelection {
    #[serde(skip_serializing_if = "Option::is_none")]
    object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logical_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_files: Option<Vec<String>>,
}

fn parse_items_with_file_selection(raw: &str) -> Result<Vec<ItemWithFileSelection>> {
    let items: Vec<ItemWithFileSelection> = serde_json::from_str(raw).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --items-with-file-selection JSON: {e}"),
            r#"Example: --items-with-file-selection '[{"objectId":"<ID>","selectedFiles":["metadata.json"]}]'"#,
        )
    })?;
    if items.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--items-with-file-selection must contain at least one item".to_string(),
            "Provide an item identifier and selectedFiles, or use --commit-all.",
        )
        .into());
    }
    for item in &items {
        if item.object_id.as_deref().is_none_or(str::is_empty)
            && item.logical_id.as_deref().is_none_or(str::is_empty)
        {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Each file selection must include a non-empty objectId or logicalId".to_string(),
                r#"Example: {"objectId":"<ID>","selectedFiles":["metadata.json"]}"#,
            )
            .into());
        }
        if let Some(files) = &item.selected_files {
            for path in files {
                validate_selected_file_path(path)?;
            }
        }
    }
    Ok(items)
}

fn validate_selected_file_path(path: &str) -> Result<()> {
    let invalid_segment = path.split('/').any(|segment| {
        segment.trim().is_empty()
            || segment.ends_with('.')
            || segment.chars().next_back().is_some_and(char::is_whitespace)
    });
    if path.starts_with('/') || path.contains('\\') || path.contains(['*', '?']) || invalid_segment
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid selected file path '{path}'"),
            "Use a full path relative to the item root with forward slashes, no leading slash, wildcards, empty segments, trailing dots, or trailing whitespace.",
        )
        .into());
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
    items_with_file_selection: Option<&str>,
    workspace_head: Option<&str>,
    wait: bool,
    timeout: u64,
) -> Result<()> {
    if !all && items.is_none() && items_with_file_selection.is_none() {
        bail!(
            "Specify --commit-all, --items for selective commit, or \
             --items-with-file-selection for file-level selective commit"
        );
    }

    let file_selections = items_with_file_selection
        .map(parse_items_with_file_selection)
        .transpose()?;
    let mode = if all {
        "All"
    } else if file_selections.is_some() {
        "FileLevelSelective"
    } else {
        "Selective"
    };
    let mut preview = serde_json::json!({
        "workspace": workspace,
        "mode": mode,
        "workspaceHead": workspace_head.unwrap_or("<auto-fetched>"),
    });
    if let Some(msg) = message {
        preview["comment"] = Value::from(msg);
    }
    if let Some(item_ids) = items {
        preview["items"] = Value::Array(
            item_ids
                .iter()
                .map(|id| serde_json::json!({"objectId": id}))
                .collect(),
        );
    }
    if let Some(selections) = &file_selections {
        preview["itemsWithFileSelection"] = serde_json::to_value(selections)?;
    }
    if output::dry_run_guard(cli, "git commit", &preview) {
        return Ok(());
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
    if let Some(selections) = file_selections {
        body["itemsWithFileSelection"] = serde_json::to_value(selections)?;
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
        .await
        .map_err(|e| enrich_pull_conflict(e, allow_override))?;

    output::render_object(cli, &data, "status");
    Ok(())
}

/// Enrich a `git pull` (updateFromGit) conflict error with a teaching hint.
///
/// When the workspace has changes that conflict with incoming remote changes,
/// the API rejects the pull. If the caller did NOT already pass
/// `--allow-override`, surface a hint pointing at it. Because `--allow-override`
/// is a registered safety-bypass flag (it discards conflicting workspace changes
/// and is irreversible), naming it in the hint makes the agent safety notice
/// fire for detected AI agents.
fn enrich_pull_conflict(err: anyhow::Error, allow_override: bool) -> anyhow::Error {
    if allow_override || !is_git_conflict(&err.to_string()) {
        return err;
    }
    FabioError::with_hint(
        ErrorCode::ApiError,
        format!("Git pull failed due to a conflict: {err}"),
        "The workspace has changes that conflict with the incoming remote changes. \
         Resolve them (commit or discard workspace changes), or re-run with \
         --allow-override to overwrite the conflicting workspace items with the \
         remote version. --allow-override is irreversible: it discards the \
         conflicting workspace changes.",
    )
    .into()
}

/// Heuristic: does an updateFromGit error indicate a merge conflict?
fn is_git_conflict(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("conflict") || lower.contains("override")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent;

    #[test]
    fn detects_conflict_messages() {
        assert!(is_git_conflict("UpdateFromGitConflict: items conflict"));
        assert!(is_git_conflict("The pull would OVERRIDE workspace changes"));
        assert!(!is_git_conflict("Workspace not connected to Git"));
        assert!(!is_git_conflict("Could not determine workspaceHead"));
    }

    #[test]
    fn conflict_error_is_enriched_only_without_override() {
        let base = || anyhow::anyhow!("updateFromGit failed: Conflict detected");

        // Already passed --allow-override: no enrichment (return as-is).
        let passthrough = enrich_pull_conflict(base(), true);
        assert!(!passthrough.to_string().contains("--allow-override"));

        // Conflict without override: hint names --allow-override so the
        // agent safety notice fires.
        let enriched = enrich_pull_conflict(base(), false);
        let fe = enriched
            .downcast_ref::<FabioError>()
            .expect("enriched to FabioError");
        let hint = fe.hint.as_deref().expect("hint present");
        assert!(hint.contains("--allow-override"));
        assert!(agent::hint_suggests_dangerous_flag(hint));
    }

    #[test]
    fn non_conflict_error_is_not_enriched() {
        let err = anyhow::anyhow!("Workspace not connected to Git");
        let out = enrich_pull_conflict(err, false);
        assert!(out.downcast_ref::<FabioError>().is_none());
    }

    #[test]
    fn status_url_includes_file_details_only_when_requested() {
        assert_eq!(
            build_status_url("ws-1", false),
            "/workspaces/ws-1/git/status"
        );
        assert_eq!(
            build_status_url("ws-1", true),
            "/workspaces/ws-1/git/status?includeFilesDetails=true"
        );
    }

    #[test]
    fn parses_and_serializes_file_level_selections() {
        let items = parse_items_with_file_selection(
            r#"[{"objectId":"item-1","selectedFiles":["metadata.json","config/settings.json"]},{"logicalId":"item-2","selectedFiles":[]}]"#,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(items).unwrap(),
            serde_json::json!([
                {
                    "objectId": "item-1",
                    "selectedFiles": ["metadata.json", "config/settings.json"]
                },
                {"logicalId": "item-2", "selectedFiles": []}
            ])
        );
    }

    #[test]
    fn file_level_selection_rejects_invalid_paths_and_identifiers() {
        for path in [
            "/metadata.json",
            r"config\settings.json",
            "config//settings.json",
            "config/../settings.json",
            "config/*.json",
            "metadata.json ",
            "metadata.json\t",
        ] {
            let raw = format!(
                r#"[{{"objectId":"item-1","selectedFiles":[{}]}}]"#,
                serde_json::to_string(path).unwrap()
            );
            assert!(parse_items_with_file_selection(&raw).is_err(), "{path}");
        }
        assert!(
            parse_items_with_file_selection(r#"[{"selectedFiles":["metadata.json"]}]"#).is_err()
        );
        assert!(parse_items_with_file_selection("[]").is_err());
    }
}
