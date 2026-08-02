//! Git branch-out: create/recycle a feature workspace from a branch.

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

/// Create a feature workspace from the current workspace's Git branch.
///
/// Automates the "Branch out to workspace" pattern:
/// 1. Gets the current Git connection details from the source workspace
/// 2. Creates a new workspace (or uses an existing one)
/// 3. Connects the new workspace to the specified feature branch
/// 4. Initializes with `prefer-remote` to populate items from the branch
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn branch_out(
    cli: &Cli,
    client: &FabricClient,
    source_workspace: &str,
    branch_name: &str,
    new_workspace_name: Option<&str>,
    capacity_id: Option<&str>,
    existing_workspace: Option<&str>,
    connection_id: Option<&str>,
    wait: bool,
    timeout: u64,
) -> Result<()> {
    // Step 1: Get Git connection details from source workspace
    let conn_data = client
        .get(&format!("/workspaces/{source_workspace}/git/connection"))
        .await
        .map_err(|e| enrich_forbidden(e, "git branch-out", "Admin"))?;

    let git_provider = conn_data
        .get("gitProviderDetails")
        .filter(|v| !v.is_null())
        .ok_or_else(|| FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Source workspace is not connected to Git".to_string(),
            "Connect the source workspace first: fabio git connect --workspace <WS> --provider <PROVIDER> --repo <REPO> --branch <BRANCH>".to_string(),
        ))?;

    if output::dry_run_guard(
        cli,
        "git branch-out",
        &serde_json::json!({
            "source_workspace": source_workspace,
            "branch": branch_name,
            "new_workspace": new_workspace_name.unwrap_or(branch_name),
            "existing_workspace": existing_workspace,
        }),
    ) {
        return Ok(());
    }

    // Step 2: Create or use existing workspace
    let target_workspace_id = if let Some(existing) = existing_workspace {
        if !cli.quiet {
            eprintln!("[git branch-out] using existing workspace: {existing}");
        }
        crate::commands::deploy::plan::resolve_workspace(client, existing).await?
    } else {
        let ws_name = new_workspace_name.unwrap_or(branch_name);
        if !cli.quiet {
            eprintln!("[git branch-out] creating workspace: {ws_name}");
        }
        let body = serde_json::json!({ "displayName": ws_name });
        let result = client
            .post("/workspaces", &body, false)
            .await
            .map_err(|e| enrich_forbidden(e, "git branch-out (create workspace)", "Admin"))?;

        let ws_id = result
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                FabioError::new(
                    ErrorCode::ApiError,
                    "Failed to get workspace ID from creation response",
                )
            })?
            .to_owned();

        // Assign capacity if specified
        if let Some(cap_id) = capacity_id {
            if !cli.quiet {
                eprintln!("[git branch-out] assigning capacity: {cap_id}");
            }
            let cap_body = serde_json::json!({ "capacityId": cap_id });
            client
                .post(
                    &format!("/workspaces/{ws_id}/assignToCapacity"),
                    &cap_body,
                    false,
                )
                .await
                .map_err(|e| enrich_forbidden(e, "git branch-out (assign capacity)", "Admin"))?;
        }

        ws_id
    };

    // Step 3: Connect the new workspace to the feature branch
    // Reuse the same Git provider details (repo, org, owner, connection-id)
    // but with the new branch name
    if !cli.quiet {
        eprintln!("[git branch-out] connecting workspace to branch: {branch_name}");
    }

    // Fetch credentials from source workspace (required for GitHub)
    let credentials = client
        .get(&format!(
            "/workspaces/{source_workspace}/git/myGitCredentials"
        ))
        .await
        .ok();

    // Build the connect body from the source connection details
    let mut connect_body = serde_json::json!({
        "gitProviderDetails": git_provider.clone()
    });

    // Override the branch name
    if let Some(details) = connect_body
        .get_mut("gitProviderDetails")
        .and_then(|v| v.as_object_mut())
    {
        details.insert("branchName".to_owned(), Value::from(branch_name));
    }

    // Include credentials: prefer explicit --connection-id, fall back to source workspace credentials
    if let Some(conn_id) = connection_id {
        connect_body["myGitCredentials"] = serde_json::json!({
            "source": "ConfiguredConnection",
            "connectionId": conn_id,
        });
    } else if let Some(ref creds) = credentials
        && creds.get("source").is_some()
    {
        connect_body["myGitCredentials"] = creds.clone();
    }

    client
        .post(
            &format!("/workspaces/{target_workspace_id}/git/connect"),
            &connect_body,
            false,
        )
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("AuthenticationFailed") || msg.contains("GitProvider") {
                FabioError::with_hint(
                    ErrorCode::AuthRequired,
                    "Git provider authentication failed for the new workspace".to_string(),
                    format!(
                        "The Git connection credentials may be workspace-scoped. Options:\n\
                         1. For GitHub: configure a connection on the new workspace first, then use --existing-workspace\n\
                         2. For Azure DevOps with Automatic credentials: should work without --connection-id\n\
                         3. Manual flow: fabio git connect --workspace {target_workspace_id} --provider <PROVIDER> --repo <REPO> --branch {branch_name} --connection-id <ID>"
                    ),
                ).into()
            } else {
                enrich_forbidden(e, "git branch-out (connect)", "Admin")
            }
        })?;

    // Step 4: Initialize with prefer-remote to populate from branch
    if !cli.quiet {
        eprintln!("[git branch-out] initializing workspace from branch...");
    }

    let init_body = serde_json::json!({
        "initializationStrategy": "PreferRemote"
    });

    let init_result = client
        .post(
            &format!("/workspaces/{target_workspace_id}/git/initializeConnection"),
            &init_body,
            wait,
        )
        .await;

    match init_result {
        Ok(_) => {}
        Err(e) => {
            let err_str = e.to_string();
            // Retry transient "Git provider failed" errors (same pattern as checkout)
            if err_str.contains("Git provider failed") || err_str.contains("initializeConnection") {
                if !cli.quiet {
                    eprintln!("[git branch-out] init returned transient error, retrying in 2s...");
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                client
                    .post(
                        &format!("/workspaces/{target_workspace_id}/git/initializeConnection"),
                        &init_body,
                        wait,
                    )
                    .await
                    .map_err(|e| enrich_forbidden(e, "git branch-out (init retry)", "Admin"))?;
            } else {
                return Err(enrich_forbidden(e, "git branch-out (init)", "Admin"));
            }
        }
    }

    // Render result
    let _ = timeout; // Used when wait=true via the poll mechanism
    let result = serde_json::json!({
        "status": if wait { "initialized" } else { "connecting" },
        "source_workspace": source_workspace,
        "target_workspace": target_workspace_id,
        "branch": branch_name,
        "workspace_name": new_workspace_name.unwrap_or(branch_name),
    });
    output::render_object(cli, &result, "status");
    Ok(())
}
