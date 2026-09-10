//! Git sync operations: status, commit, pull, and tracked-item listing.

use anyhow::{Result, bail};
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

pub(super) async fn status(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    include_file_details: bool,
) -> Result<()> {
    let data = client
        .get_with_lro(&status_path(workspace, include_file_details))
        .await?;

    let changes = data
        .get("changes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if changes.is_empty() {
        output::render_object(cli, &data, "status");
    } else {
        let mut fields = vec![
            "itemMetadata.displayName",
            "itemMetadata.itemType",
            "workspaceChange",
            "remoteChange",
            "conflictType",
        ];
        let mut headers = vec!["NAME", "TYPE", "WORKSPACE", "REMOTE", "CONFLICT"];
        if include_file_details {
            fields.push("fileChanges");
            headers.push("FILE CHANGES");
        }
        output::render_list(cli, &changes, &fields, &headers, "itemMetadata.displayName");
    }
    Ok(())
}

fn status_path(workspace: &str, include_file_details: bool) -> String {
    if include_file_details {
        format!("/workspaces/{workspace}/git/status?includeFilesDetails=true")
    } else {
        format!("/workspaces/{workspace}/git/status")
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn commit(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    message: Option<&str>,
    all: bool,
    items: Option<&[String]>,
    file_selection: &[String],
    workspace_head: Option<&str>,
    wait: bool,
    timeout: u64,
) -> Result<()> {
    if !all && items.is_none() && file_selection.is_empty() {
        bail!(
            "Specify --all to commit all changes, --items for selective commit, or --file-selection for file-level selective commit"
        );
    }

    let items_with_file_selection = parse_file_selections(file_selection)?;
    let mode = if all {
        "All"
    } else if items.is_some() {
        "Selective"
    } else {
        "FileLevelSelective"
    };

    let preview = build_commit_body(
        mode,
        workspace_head.unwrap_or("<auto-fetched from git status>"),
        message,
        items,
        &items_with_file_selection,
    );
    if output::dry_run_guard(cli, "git commit", &preview) {
        return Ok(());
    }

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

    let body = build_commit_body(mode, &head, message, items, &items_with_file_selection);

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

fn build_commit_body(
    mode: &str,
    workspace_head: &str,
    message: Option<&str>,
    items: Option<&[String]>,
    items_with_file_selection: &[Value],
) -> Value {
    let mut body = serde_json::json!({
        "mode": mode,
        "workspaceHead": workspace_head,
    });
    if let Some(message) = message {
        body["comment"] = Value::from(message);
    }
    if let Some(item_ids) = items {
        body["items"] = Value::Array(
            item_ids
                .iter()
                .map(|id| serde_json::json!({"objectId": id}))
                .collect(),
        );
    }
    if !items_with_file_selection.is_empty() {
        body["itemsWithFileSelection"] = Value::Array(items_with_file_selection.to_vec());
    }
    body
}

fn parse_file_selections(values: &[String]) -> Result<Vec<Value>> {
    values
        .iter()
        .map(|value| {
            let (item_id, paths) = value.split_once('=').ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Invalid --file-selection '{value}': expected ITEM_ID=PATH[,PATH...]"),
                    "Example: --file-selection <ITEM_ID>=metadata.json",
                )
            })?;
            if item_id.trim().is_empty() {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Invalid --file-selection '{value}': item ID is empty"),
                    "Provide an item object ID before '='.",
                )
                .into());
            }
            let selected_files = if paths.is_empty() {
                Vec::new()
            } else {
                paths
                    .split(',')
                    .map(|path| {
                        validate_selected_file(path)?;
                        Ok(Value::from(path))
                    })
                    .collect::<Result<Vec<_>>>()?
            };
            Ok(serde_json::json!({
                "objectId": item_id,
                "selectedFiles": selected_files,
            }))
        })
        .collect()
}

fn validate_selected_file(path: &str) -> Result<()> {
    let invalid_segment = path.split('/').any(|segment| {
        segment.trim().is_empty() || segment.ends_with('.') || segment != segment.trim_end()
    });
    if path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.contains('*')
        || path.contains('?')
        || invalid_segment
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
    fn status_path_adds_file_details_query_only_when_requested() {
        assert_eq!(status_path("ws", false), "/workspaces/ws/git/status");
        assert_eq!(
            status_path("ws", true),
            "/workspaces/ws/git/status?includeFilesDetails=true"
        );
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
    fn file_level_commit_matches_spec_shape() {
        let selections = parse_file_selections(&[
            "cfafbeb1-8037-4d0c-896e-28c4f2e65573=metadata.json".to_string(),
            "d5a2b3c4-1234-5678-abcd-ef0123456789=".to_string(),
        ])
        .unwrap();
        let body = build_commit_body(
            "FileLevelSelective",
            "eaa737b48cda41b37ffefac772ea48f6fed3eac4",
            Some("Commit only the metadata change from MyNotebook"),
            None,
            &selections,
        );
        assert_eq!(body["mode"], "FileLevelSelective");
        assert_eq!(
            body["itemsWithFileSelection"][0]["selectedFiles"],
            serde_json::json!(["metadata.json"])
        );
        assert_eq!(
            body["itemsWithFileSelection"][1]["selectedFiles"],
            serde_json::json!([])
        );
        assert!(body.get("items").is_none());
    }

    #[test]
    fn file_selection_supports_nested_paths() {
        let selections =
            parse_file_selections(&["item-id=pages/page1.json,config/settings.json".to_string()])
                .unwrap();
        assert_eq!(
            selections[0]["selectedFiles"],
            serde_json::json!(["pages/page1.json", "config/settings.json"])
        );
    }

    #[test]
    fn file_selection_rejects_invalid_paths() {
        for path in [
            "/metadata.json",
            "folder\\file.json",
            "folder//file.json",
            "folder./file.json",
            "folder/file.json ",
            "*.json",
        ] {
            assert!(validate_selected_file(path).is_err(), "{path} should fail");
        }
    }
}
