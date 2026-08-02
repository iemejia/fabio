//! Git connection lifecycle: connect, disconnect, init, checkout, and the
//! provider connection & credentials subcommands.

use anyhow::{Result, bail};
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::enrich_git_connect_error;

#[allow(clippy::too_many_arguments)]
pub(super) async fn connect(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    provider: &str,
    repo: &str,
    branch: &str,
    org: Option<&str>,
    project: Option<&str>,
    owner: Option<&str>,
    directory: Option<&str>,
    custom_domain: Option<&str>,
    connection_id: Option<&str>,
) -> Result<()> {
    let git_provider_details = match provider {
        "azure-devops" => {
            let org_name =
                org.ok_or_else(|| FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--org is required for Azure DevOps provider",
                    "Example: fabio git connect --workspace <WS> --provider azure-devops --org <ORG> --project <PROJECT> --repo <REPO> --branch <BRANCH>",
                ))?;
            let project_name = project.ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--project is required for Azure DevOps provider",
                    "Example: fabio git connect --workspace <WS> --provider azure-devops --org <ORG> --project <PROJECT> --repo <REPO> --branch <BRANCH>",
                )
            })?;
            let dir_name = directory.unwrap_or("/");
            let details = serde_json::json!({
                "gitProviderType": "AzureDevOps",
                "organizationName": org_name,
                "projectName": project_name,
                "repositoryName": repo,
                "branchName": branch,
                "directoryName": dir_name,
            });
            details
        }
        "github" => {
            let owner_name =
                owner.ok_or_else(|| FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--owner is required for GitHub provider",
                    "Example: fabio git connect --workspace <WS> --provider github --owner <OWNER> --repo <REPO> --branch <BRANCH> --connection-id <CONN_ID>",
                ))?;
            let dir_name = directory.unwrap_or("/");
            let mut details = serde_json::json!({
                "gitProviderType": "GitHub",
                "ownerName": owner_name,
                "repositoryName": repo,
                "branchName": branch,
                "directoryName": dir_name,
            });
            if let Some(domain) = custom_domain {
                details["customDomainName"] = Value::from(domain);
            }
            details
        }
        _ => bail!("Unsupported provider: {provider}. Use 'azure-devops' or 'github'."),
    };

    let mut body = serde_json::json!({
        "gitProviderDetails": git_provider_details,
    });

    if let Some(conn_id) = connection_id {
        body["myGitCredentials"] = serde_json::json!({
            "source": "ConfiguredConnection",
            "connectionId": conn_id,
        });
    } else if provider == "github" {
        return Err(FabioError {
            code: ErrorCode::InvalidInput,
            message: "GitHub provider requires --connection-id for authentication".into(),
            hint: Some(
                "Find existing connections: fabio connection list\n\
                 Create one: fabio connection create --name \"GitHub\" \
                 --connectivity-type ShareableCloud --connection-type GitHubSourceControl \
                 --credential-type OAuth2 --parameters '{}'  --skip-test-connection\n\
                 Then: fabio git connect --provider github --connection-id <ID> ..."
                    .into(),
            ),
            hint_type: None,
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
        }
        .into());
    }

    let _data = client
        .post(
            &format!("/workspaces/{workspace}/git/connect"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_git_connect_error(e, provider, repo, branch, owner, org))?;

    let result = serde_json::json!({"status": "connected"});
    output::render_object(cli, &result, "status");
    Ok(())
}

pub(super) async fn disconnect(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let _data = client
        .post(
            &format!("/workspaces/{workspace}/git/disconnect"),
            &serde_json::json!({}),
            false,
        )
        .await?;

    let result = serde_json::json!({"status": "disconnected"});
    output::render_object(cli, &result, "status");
    Ok(())
}

pub(super) async fn init(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    strategy: Option<&str>,
    wait: bool,
    timeout: u64,
) -> Result<()> {
    let body = strategy.map_or_else(
        || serde_json::json!({}),
        |s| {
            let api_strategy = match s {
                "prefer-remote" => "PreferRemote",
                "prefer-workspace" => "PreferWorkspace",
                "none" => "None",
                _ => s,
            };
            serde_json::json!({"initializationStrategy": api_strategy})
        },
    );

    let _data = client
        .post_with_timeout(
            &format!("/workspaces/{workspace}/git/initializeConnection"),
            &body,
            wait,
            timeout,
        )
        .await?;

    let result = serde_json::json!({"status": "initialized"});
    output::render_object(cli, &result, "status");
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn checkout(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    branch: &str,
    strategy: Option<&str>,
    wait: bool,
    timeout: u64,
) -> Result<()> {
    // Pre-flight: check for uncommitted workspace changes
    if !cli.force {
        let status_data = client
            .get_with_lro(&format!("/workspaces/{workspace}/git/status"))
            .await?;

        let has_workspace_changes = status_data
            .get("changes")
            .and_then(Value::as_array)
            .is_some_and(|changes| {
                changes.iter().any(|c| {
                    c.get("workspaceChange")
                        .and_then(Value::as_str)
                        .is_some_and(|s| s != "None")
                })
            });

        if has_workspace_changes {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Workspace has uncommitted changes that would be lost by switching branches.",
                "Commit first with 'fabio git commit --commit-all -w <workspace>', \
                 or use --force to discard uncommitted changes."
                    .to_string(),
            )
            .into());
        }
    }

    // Step 1: Get current connection details to preserve provider config
    let connection = client
        .get(&format!("/workspaces/{workspace}/git/connection"))
        .await?;

    let provider_details = connection
        .get("gitProviderDetails")
        .ok_or_else(|| FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Workspace is not connected to Git",
            "Connect the workspace first: fabio git connect --workspace <WS> --provider <PROVIDER> --repo <REPO> --branch <BRANCH>",
        ))?;

    // Step 2: Get current credentials (needed for GitHub reconnect)
    let credentials = client
        .get(&format!("/workspaces/{workspace}/git/myGitCredentials"))
        .await
        .ok();

    // Step 3: Disconnect from current branch
    client
        .post(
            &format!("/workspaces/{workspace}/git/disconnect"),
            &serde_json::json!({}),
            false,
        )
        .await?;

    // Step 4: Reconnect with the new branch
    let mut new_provider_details = provider_details.clone();
    new_provider_details["branchName"] = Value::from(branch);

    let mut connect_body = serde_json::json!({
        "gitProviderDetails": new_provider_details,
    });

    // Include credentials if available (required for GitHub)
    if let Some(ref creds) = credentials
        && creds.get("source").is_some()
    {
        connect_body["myGitCredentials"] = creds.clone();
    }

    let provider_type = provider_details
        .get("gitProviderType")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let repo_name = provider_details
        .get("repositoryName")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let owner_name = provider_details.get("ownerName").and_then(Value::as_str);
    let org_name = provider_details
        .get("organizationName")
        .and_then(Value::as_str);

    let connect_result = client
        .post(
            &format!("/workspaces/{workspace}/git/connect"),
            &connect_body,
            false,
        )
        .await;

    if let Err(e) = connect_result {
        // Reconnect to original branch failed — try to restore previous connection
        let mut rollback_body = serde_json::json!({
            "gitProviderDetails": provider_details,
        });
        if let Some(ref creds) = credentials
            && creds.get("source").is_some()
        {
            rollback_body["myGitCredentials"] = creds.clone();
        }
        let _ = client
            .post(
                &format!("/workspaces/{workspace}/git/connect"),
                &rollback_body,
                false,
            )
            .await;

        return Err(enrich_git_connect_error(
            e,
            provider_type,
            repo_name,
            branch,
            owner_name,
            org_name,
        ));
    }

    // Step 5: Initialize the connection
    // Default to prefer-remote: when switching branches the user expects the
    // workspace to update to match the target branch content.
    let effective_strategy = strategy.unwrap_or("prefer-remote");
    let api_strategy = match effective_strategy {
        "prefer-remote" => "PreferRemote",
        "prefer-workspace" => "PreferWorkspace",
        "none" => "None",
        _ => effective_strategy,
    };
    let init_body = serde_json::json!({"initializationStrategy": api_strategy});

    // The Git provider sometimes needs a moment after connect before init works.
    // Retry up to 3 times with a 2s delay to handle transient "Git provider failed" errors.
    let init_url = format!("/workspaces/{workspace}/git/initializeConnection");
    let mut last_err = None;
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        match client
            .post_with_timeout(&init_url, &init_body, wait, timeout)
            .await
        {
            Ok(_data) => {
                let result = serde_json::json!({"status": "switched", "branch": branch});
                output::render_object(cli, &result, "status");
                return Ok(());
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("Git provider failed") && attempt < 2 {
                    last_err = Some(e);
                    continue;
                }
                return Err(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| FabioError::with_hint(
        ErrorCode::ApiError,
        "initializeConnection failed",
        "Retry the operation. If using Azure DevOps, ensure the user has Contributor access to the repo.",
    ).into()))
}

pub(super) async fn connection_show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/git/connection"))
        .await?;

    output::render_object(cli, &data, "status");
    Ok(())
}

pub(super) async fn credentials_show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/git/myGitCredentials"))
        .await?;

    output::render_object(cli, &data, "status");
    Ok(())
}

pub(super) async fn credentials_update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    source: &str,
    connection_id: Option<&str>,
) -> Result<()> {
    let body = match source {
        "automatic" => serde_json::json!({"source": "Automatic"}),
        "none" => serde_json::json!({"source": "None"}),
        "configured-connection" => {
            let conn_id = connection_id.ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--connection-id is required when source is 'configured-connection'",
                    "Find available connections with: fabio connection list",
                )
            })?;
            serde_json::json!({
                "source": "ConfiguredConnection",
                "connectionId": conn_id,
            })
        }
        _ => bail!(
            "Unsupported source: {source}. Use 'automatic', 'configured-connection', or 'none'."
        ),
    };

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/git/myGitCredentials"),
            &body,
        )
        .await?;

    output::render_object(cli, &data, "status");
    Ok(())
}
