//! Git sync operations: status, commit, pull, and tracked-item listing.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

fn status_path(workspace: &str, include_files_details: bool) -> String {
    if include_files_details {
        format!("/workspaces/{workspace}/git/status?includeFilesDetails=true")
    } else {
        format!("/workspaces/{workspace}/git/status")
    }
}

pub(super) async fn status(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    include_files_details: bool,
) -> Result<()> {
    let data = client
        .get_with_lro(&status_path(workspace, include_files_details))
        .await?;

    let changes = data
        .get("changes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if changes.is_empty() || include_files_details {
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
    file_selection: &[String],
    all_files_for_item: &[String],
    logical_file_selection: &[String],
    all_files_for_logical_item: &[String],
    workspace_head: Option<&str>,
    wait: bool,
    timeout: u64,
) -> Result<()> {
    let items_with_file_selection = build_items_with_file_selection(
        file_selection,
        all_files_for_item,
        logical_file_selection,
        all_files_for_logical_item,
    )?;
    if !all && items.is_none() && items_with_file_selection.is_empty() {
        bail!(
            "Specify --commit-all, --items, --file-selection, --all-files-for-item, --logical-file-selection, or --all-files-for-logical-item to choose changes"
        );
    }

    let mode = if all {
        "All"
    } else if items.is_some() {
        "Selective"
    } else {
        "FileLevelSelective"
    };
    let selected_items: Vec<Value> = items
        .unwrap_or_default()
        .iter()
        .map(|id| serde_json::json!({"objectId": id}))
        .collect();
    let mut preview = serde_json::json!({
        "workspace": workspace,
        "mode": mode,
    });
    if let Some(message) = message {
        preview["comment"] = Value::from(message);
    }
    if !selected_items.is_empty() {
        preview["items"] = Value::Array(selected_items.clone());
    }
    if !items_with_file_selection.is_empty() {
        preview["itemsWithFileSelection"] = Value::Array(items_with_file_selection.clone());
    }
    if let Some(workspace_head) = workspace_head {
        preview["workspaceHead"] = Value::from(workspace_head);
    }
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

    let mut body = serde_json::json!({
        "mode": mode,
        "workspaceHead": head,
    });

    if let Some(msg) = message {
        body["comment"] = Value::from(msg);
    }

    if !selected_items.is_empty() {
        body["items"] = Value::Array(selected_items);
    }
    if !items_with_file_selection.is_empty() {
        body["itemsWithFileSelection"] = Value::Array(items_with_file_selection);
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

fn build_items_with_file_selection(
    file_selections: &[String],
    all_files_for_item: &[String],
    logical_file_selections: &[String],
    all_files_for_logical_item: &[String],
) -> Result<Vec<Value>> {
    let mut selections = BTreeMap::<(String, String), Vec<String>>::new();
    add_all_files(
        &mut selections,
        "objectId",
        all_files_for_item,
        "--all-files-for-item",
        "ITEM_ID",
    )?;
    add_all_files(
        &mut selections,
        "logicalId",
        all_files_for_logical_item,
        "--all-files-for-logical-item",
        "LOGICAL_ID",
    )?;
    add_file_selections(
        &mut selections,
        "objectId",
        file_selections,
        "--file-selection",
        "ITEM_ID",
    )?;
    add_file_selections(
        &mut selections,
        "logicalId",
        logical_file_selections,
        "--logical-file-selection",
        "LOGICAL_ID",
    )?;
    Ok(selections
        .into_iter()
        .map(|((identifier_field, identifier), selected_files)| {
            serde_json::json!({
                identifier_field: identifier,
                "selectedFiles": selected_files,
            })
        })
        .collect())
}

fn add_all_files(
    selections: &mut BTreeMap<(String, String), Vec<String>>,
    identifier_field: &str,
    item_ids: &[String],
    flag_name: &str,
    id_label: &str,
) -> Result<()> {
    for item_id in item_ids {
        if item_id.trim().is_empty() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag_name} requires a non-empty {id_label}"),
                format!("Provide {flag_name} {id_label} with a non-empty identifier."),
            )
            .into());
        }
        selections
            .entry((identifier_field.to_string(), item_id.clone()))
            .or_default();
    }
    Ok(())
}

fn add_file_selections(
    selections: &mut BTreeMap<(String, String), Vec<String>>,
    identifier_field: &str,
    file_selections: &[String],
    flag_name: &str,
    id_label: &str,
) -> Result<()> {
    for selection in file_selections {
        let (item_id, path) = selection.split_once('=').ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid {flag_name} '{selection}'"),
                format!(
                    "Use {flag_name} {id_label}=RELATIVE/PATH, for example {flag_name} 00000000-0000-0000-0000-000000000000=metadata.json"
                ),
            )
        })?;
        if item_id.trim().is_empty() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag_name} requires a non-empty {id_label}"),
                format!(
                    "Use {flag_name} {id_label}=RELATIVE/PATH, for example {flag_name} 00000000-0000-0000-0000-000000000000=metadata.json"
                ),
            )
            .into());
        }
        validate_git_file_path(path)?;
        let key = (identifier_field.to_string(), item_id.to_string());
        if selections.get(&key).is_some_and(Vec::is_empty) {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Item '{item_id}' selects both all files and an individual file"),
                "Use either an all-files flag or file-selection flags for the same item, not both.",
            )
            .into());
        }
        selections.entry(key).or_default().push(path.to_string());
    }
    Ok(())
}

fn validate_git_file_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('*')
        || path.contains('?')
        || path.split('/').any(|segment| {
            segment.is_empty()
                || segment.trim().is_empty()
                || segment.ends_with('.')
                || segment.chars().last().is_some_and(char::is_whitespace)
        })
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid Git item-relative file path '{path}'"),
            "Use a full relative file path with forward slashes, no leading slash, wildcards, empty segments, or segments ending in a dot or whitespace.",
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
    fn status_path_includes_file_details_only_when_requested() {
        assert_eq!(status_path("ws", false), "/workspaces/ws/git/status");
        assert_eq!(
            status_path("ws", true),
            "/workspaces/ws/git/status?includeFilesDetails=true"
        );
    }

    #[test]
    fn builds_file_level_selective_items() {
        let selections = build_items_with_file_selection(
            &[
                "item-a=metadata.json".to_string(),
                "item-a=config/settings.json".to_string(),
            ],
            &["item-b".to_string()],
            &["logical-a=definition.pbir".to_string()],
            &[],
        )
        .unwrap();
        assert_eq!(
            selections,
            vec![
                serde_json::json!({
                    "logicalId": "logical-a",
                    "selectedFiles": ["definition.pbir"]
                }),
                serde_json::json!({
                    "objectId": "item-a",
                    "selectedFiles": ["metadata.json", "config/settings.json"]
                }),
                serde_json::json!({
                    "objectId": "item-b",
                    "selectedFiles": []
                }),
            ]
        );
    }

    #[test]
    fn rejects_invalid_logical_file_selection_with_correct_flag_name() {
        let err = build_items_with_file_selection(&[], &[], &["logical-a".to_string()], &[])
            .expect_err("invalid selector should fail")
            .to_string();
        assert!(err.contains("Invalid --logical-file-selection"));
    }

    #[test]
    fn rejects_empty_all_files_identifier_as_invalid_input() {
        let err = build_items_with_file_selection(&[], &[], &[], &[" ".to_string()])
            .expect_err("empty identifier should fail");
        let fabio_err = err
            .downcast_ref::<FabioError>()
            .expect("error should remain structured");
        assert_eq!(fabio_err.code, ErrorCode::InvalidInput);
        assert!(
            fabio_err
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("--all-files-for-logical-item LOGICAL_ID"))
        );
    }

    #[test]
    fn rejects_empty_file_selection_identifier_as_invalid_input() {
        let err = build_items_with_file_selection(&["=metadata.json".to_string()], &[], &[], &[])
            .expect_err("empty identifier should fail");
        let fabio_err = err
            .downcast_ref::<FabioError>()
            .expect("error should remain structured");
        assert_eq!(fabio_err.code, ErrorCode::InvalidInput);
        assert!(
            fabio_err
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("--file-selection"))
        );
    }

    #[test]
    fn rejects_invalid_file_level_selective_paths() {
        for path in [
            "/metadata.json",
            "folder//file.json",
            "folder\\file.json",
            "*.json",
            "folder./file.json",
            "folder/file.json ",
            "folder/ /file.json",
        ] {
            assert!(validate_git_file_path(path).is_err(), "{path} must fail");
        }
    }

    #[test]
    fn accepts_file_level_paths_with_leading_segment_whitespace() {
        assert!(validate_git_file_path("folder/ metadata.json").is_ok());
    }

    #[test]
    fn non_conflict_error_is_not_enriched() {
        let err = anyhow::anyhow!("Workspace not connected to Git");
        let out = enrich_pull_conflict(err, false);
        assert!(out.downcast_ref::<FabioError>().is_none());
    }
}
