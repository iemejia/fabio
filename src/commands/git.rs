use anyhow::{Result, bail};
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[path = "git_relation.rs"]
mod relation;
pub use relation::RelationCommand;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples git\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum GitCommand {
    // ── Daily Operations ─────────────────────────────────────────────────
    /// Show workspace Git status (changes, conflicts)
    #[command(display_order = 1)]
    Status {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Commit workspace changes to the connected remote branch
    #[command(display_order = 2)]
    Commit {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Commit message (max 300 characters)
        #[arg(short, long)]
        message: Option<String>,

        /// Commit all pending changes
        #[arg(long = "commit-all", visible_alias = "all", conflicts_with = "items")]
        all: bool,

        /// Selective commit: comma-separated item object IDs
        #[arg(long, value_delimiter = ',', conflicts_with = "all")]
        items: Option<Vec<String>>,

        /// Override workspace head (auto-fetched from status if omitted)
        #[arg(long, hide = true)]
        workspace_head: Option<String>,

        /// Wait for the operation to complete
        #[arg(long)]
        wait: bool,

        /// Timeout in seconds when --wait is used (default: 120)
        #[arg(long, default_value = "120")]
        timeout: u64,
    },
    /// Pull remote changes into the workspace (update from Git)
    #[command(display_order = 3)]
    Pull {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Conflict resolution policy
        #[arg(long, value_parser = ["prefer-remote", "prefer-workspace"])]
        conflict_resolution: Option<String>,

        /// Allow overriding workspace items with incoming changes
        #[arg(long)]
        allow_override: bool,

        /// Override workspace head (auto-fetched from status if omitted)
        #[arg(long, hide = true)]
        workspace_head: Option<String>,

        /// Override remote commit hash (auto-fetched from status if omitted)
        #[arg(long, hide = true)]
        remote_commit_hash: Option<String>,

        /// Wait for the operation to complete
        #[arg(long)]
        wait: bool,

        /// Timeout in seconds when --wait is used (default: 120)
        #[arg(long, default_value = "120")]
        timeout: u64,
    },
    // ── Setup ─────────────────────────────────────────────────────────────
    /// Connect a workspace to a Git repository
    #[command(display_order = 10)]
    Connect {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Git provider type
        #[arg(long, value_parser = ["azure-devops", "github"])]
        provider: String,

        /// Repository name
        #[arg(long)]
        repo: String,

        /// Branch name
        #[arg(long)]
        branch: String,

        /// Organization name (Azure DevOps only)
        #[arg(long)]
        org: Option<String>,

        /// Project name (Azure DevOps only)
        #[arg(long)]
        project: Option<String>,

        /// Owner name (GitHub only)
        #[arg(long)]
        owner: Option<String>,

        /// Relative directory path within the repo
        #[arg(long)]
        directory: Option<String>,

        /// Custom domain for GitHub Enterprise (ghe.com)
        #[arg(long)]
        custom_domain: Option<String>,

        /// Connection ID for configured credentials
        #[arg(long)]
        connection_id: Option<String>,
    },
    /// Disconnect a workspace from Git
    #[command(display_order = 11)]
    Disconnect {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Initialize a workspace Git connection (required after connect)
    #[command(visible_alias = "initialize", display_order = 12)]
    Init {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Initialization strategy when both sides have content
        #[arg(long, value_parser = ["none", "prefer-remote", "prefer-workspace"])]
        strategy: Option<String>,

        /// Wait for the operation to complete
        #[arg(long)]
        wait: bool,

        /// Timeout in seconds when --wait is used (default: 120)
        #[arg(long, default_value = "120")]
        timeout: u64,
    },
    /// Switch to a different branch (disconnect + connect + init)
    #[command(visible_alias = "switch", display_order = 13)]
    Checkout {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Target branch name
        #[arg(long)]
        branch: String,

        /// Initialization strategy [default: prefer-remote]
        #[arg(long, value_parser = ["none", "prefer-remote", "prefer-workspace"])]
        strategy: Option<String>,

        /// Wait for initialization to complete
        #[arg(long)]
        wait: bool,

        /// Timeout in seconds when --wait is used (default: 120)
        #[arg(long, default_value = "120")]
        timeout: u64,
    },
    /// Create a feature workspace from the current branch (branch out)
    ///
    /// Automates the Fabric "Branch out to workspace" flow:
    /// 1. Creates a new workspace (or uses --existing-workspace)
    /// 2. Connects it to a new feature branch
    /// 3. Initializes with items from the branch (Update from Git)
    ///
    /// Requires: source workspace connected to Git, permissions to create branches
    /// and workspaces. The new branch is created from the source workspace's branch.
    #[command(display_order = 14)]
    BranchOut {
        /// Source workspace (already connected to Git integration branch)
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Name of the new feature branch to create
        #[arg(long)]
        branch: String,

        /// Name for the new feature workspace (default: branch name)
        #[arg(long)]
        new_workspace: Option<String>,

        /// Capacity ID for the new workspace
        #[arg(short, long, env = "FABIO_CAPACITY")]
        capacity: Option<String>,

        /// Use an existing workspace instead of creating a new one
        #[arg(long, conflicts_with_all = ["new_workspace", "capacity"])]
        existing_workspace: Option<String>,

        /// Connection ID for Git credentials (required for GitHub)
        #[arg(long)]
        connection_id: Option<String>,

        /// Wait for initialization to complete
        #[arg(long)]
        wait: bool,

        /// Timeout in seconds when --wait is used (default: 120)
        #[arg(long, default_value = "120")]
        timeout: u64,
    },
    // ── Configuration ─────────────────────────────────────────────────────
    /// Show or manage Git connection and credentials
    #[command(subcommand, display_order = 20)]
    Connection(ConnectionCommand),
    /// Manage Git credentials
    #[command(subcommand, display_order = 21)]
    Credentials(CredentialsCommand),
    /// Manage workspace relations (base/branch links between workspaces, Preview)
    #[command(subcommand, display_order = 22)]
    Relation(RelationCommand),
    // ── Inspection ───────────────────────────────────────────────────────
    /// Show tracked items and their Git sync status
    #[command(display_order = 30)]
    ShowTracked {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConnectionCommand {
    /// Show Git connection details for the workspace
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum CredentialsCommand {
    /// Show your Git credentials configuration
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Update your Git credentials configuration
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Credentials source
        #[arg(long, value_parser = ["automatic", "configured-connection", "none"])]
        source: String,

        /// Connection ID (required when source is configured-connection)
        #[arg(long)]
        connection_id: Option<String>,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &GitCommand) -> Result<()> {
    match command {
        GitCommand::Status { workspace } => status(cli, client, workspace).await,
        GitCommand::Commit {
            workspace,
            message,
            all,
            items,
            workspace_head,
            wait,
            timeout,
        } => commit(
            cli,
            client,
            workspace,
            message.as_deref(),
            *all,
            items.as_deref(),
            workspace_head.as_deref(),
            *wait,
            *timeout,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "git commit", "Member")),
        GitCommand::Pull {
            workspace,
            conflict_resolution,
            allow_override,
            workspace_head,
            remote_commit_hash,
            wait,
            timeout,
        } => pull(
            cli,
            client,
            workspace,
            conflict_resolution.as_deref(),
            *allow_override,
            workspace_head.as_deref(),
            remote_commit_hash.as_deref(),
            *wait,
            *timeout,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "git pull", "Member")),
        GitCommand::Connect {
            workspace,
            provider,
            repo,
            branch,
            org,
            project,
            owner,
            directory,
            custom_domain,
            connection_id,
        } => connect(
            cli,
            client,
            workspace,
            provider,
            repo,
            branch,
            org.as_deref(),
            project.as_deref(),
            owner.as_deref(),
            directory.as_deref(),
            custom_domain.as_deref(),
            connection_id.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "git connect", "Admin")),
        GitCommand::Disconnect { workspace } => disconnect(cli, client, workspace)
            .await
            .map_err(|e| enrich_forbidden(e, "git disconnect", "Admin")),
        GitCommand::Init {
            workspace,
            strategy,
            wait,
            timeout,
        } => init(cli, client, workspace, strategy.as_deref(), *wait, *timeout)
            .await
            .map_err(|e| enrich_forbidden(e, "git init", "Admin")),
        GitCommand::Checkout {
            workspace,
            branch,
            strategy,
            wait,
            timeout,
        } => checkout(
            cli,
            client,
            workspace,
            branch,
            strategy.as_deref(),
            *wait,
            *timeout,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "git checkout", "Admin")),
        GitCommand::BranchOut {
            workspace,
            branch,
            new_workspace,
            capacity,
            existing_workspace,
            connection_id,
            wait,
            timeout,
        } => {
            branch_out(
                cli,
                client,
                workspace,
                branch,
                new_workspace.as_deref(),
                capacity.as_deref(),
                existing_workspace.as_deref(),
                connection_id.as_deref(),
                *wait,
                *timeout,
            )
            .await
        }
        GitCommand::Connection(sub) => match sub {
            ConnectionCommand::Show { workspace } => connection_show(cli, client, workspace).await,
        },
        GitCommand::Credentials(sub) => match sub {
            CredentialsCommand::Show { workspace } => {
                credentials_show(cli, client, workspace).await
            }
            CredentialsCommand::Update {
                workspace,
                source,
                connection_id,
            } => credentials_update(cli, client, workspace, source, connection_id.as_deref())
                .await
                .map_err(|e| enrich_forbidden(e, "git credentials update", "Admin")),
        },
        GitCommand::Relation(sub) => relation::execute(cli, client, sub).await,
        GitCommand::ShowTracked { workspace } => show_tracked(cli, client, workspace).await,
    }
}

async fn status(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
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
async fn commit(
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
async fn pull(
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

#[allow(clippy::too_many_arguments)]
async fn connect(
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

async fn disconnect(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
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

async fn init(
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
async fn checkout(
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

/// Create a feature workspace from the current workspace's Git branch.
///
/// Automates the "Branch out to workspace" pattern:
/// 1. Gets the current Git connection details from the source workspace
/// 2. Creates a new workspace (or uses an existing one)
/// 3. Connects the new workspace to the specified feature branch
/// 4. Initializes with `prefer-remote` to populate items from the branch
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn branch_out(
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

async fn connection_show(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/git/connection"))
        .await?;

    output::render_object(cli, &data, "status");
    Ok(())
}

async fn credentials_show(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/git/myGitCredentials"))
        .await?;

    output::render_object(cli, &data, "status");
    Ok(())
}

async fn credentials_update(
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
async fn show_tracked(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
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

/// Enrich a git connect/checkout error with actionable hints for agents.
///
/// The Fabric API returns generic messages like "The requested operation can't
/// be completed because the Git provider resource could not be found" — this
/// function adds context about what likely went wrong and how to fix it.
fn enrich_git_connect_error(
    err: anyhow::Error,
    provider: &str,
    repo: &str,
    branch: &str,
    owner: Option<&str>,
    org: Option<&str>,
) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };

    // Only enrich NOT_FOUND and API_ERROR (invalid input) codes
    if fabio_err.code != ErrorCode::NotFound && fabio_err.code != ErrorCode::ApiError {
        return err;
    }

    let msg = &fabio_err.message;
    let provider_lower = provider.to_lowercase();

    let hint = if msg.contains("myGitCredentials is required") || msg.contains("credentials") {
        // Missing --connection-id for GitHub
        if provider_lower.contains("github") {
            format!(
                "GitHub requires --connection-id pointing to a GitHubSourceControl connection. \
                 Find available connections with: fabio connection list --output json | \
                 jq '.data[] | select(.connectivityType==\"ShareableCloud\")'. \
                 Then retry: fabio git connect --provider github --owner {owner} --repo {repo} \
                 --branch {branch} --connection-id <CONNECTION_ID>",
                owner = owner.unwrap_or("OWNER"),
            )
        } else {
            "Add --connection-id pointing to a configured Git connection. \
             Find available connections with: fabio connection list"
                .to_string()
        }
    } else if msg.contains("could not be found") || msg.contains("not found") {
        // Branch/repo/owner not found on the Git provider
        if provider_lower.contains("github") {
            let owner_str = owner.unwrap_or("OWNER");
            format!(
                "Verify the branch '{branch}' exists in the repository '{owner_str}/{repo}'. \
                 List remote branches with: gh api repos/{owner_str}/{repo}/branches --jq '.[].name'"
            )
        } else {
            let org_str = org.unwrap_or("ORG");
            format!(
                "Verify the branch '{branch}' exists in the repository '{org_str}/{repo}'. \
                 Check in Azure DevOps or run: az repos ref list --repository {repo} --org https://dev.azure.com/{org_str}"
            )
        }
    } else if msg.contains("invalid input") || msg.contains("Invalid input") {
        // Generic "invalid input" — usually wrong branch, repo, or connection-id
        if provider_lower.contains("github") {
            let owner_str = owner.unwrap_or("OWNER");
            format!(
                "Check that --owner '{owner_str}', --repo '{repo}', and --branch '{branch}' are correct. \
                 Verify the branch exists: gh api repos/{owner_str}/{repo}/branches --jq '.[].name'. \
                 Also verify --connection-id points to a valid GitHubSourceControl connection."
            )
        } else {
            let org_str = org.unwrap_or("ORG");
            format!(
                "Check that --org '{org_str}', --repo '{repo}', and --branch '{branch}' are correct. \
                 Verify the branch exists and --connection-id is valid."
            )
        }
    } else {
        return err;
    };

    FabioError::with_hint(fabio_err.code, msg.clone(), hint).into()
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrich_git_connect_not_found_github_includes_branch() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::NotFound,
            "Git provider resource could not be found".to_string(),
        )
        .into();

        let enriched = enrich_git_connect_error(
            err,
            "GitHub",
            "my-repo",
            "feature-xyz",
            Some("myowner"),
            None,
        );

        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(hint.contains("feature-xyz"), "Hint should mention branch");
        assert!(
            hint.contains("myowner/my-repo"),
            "Hint should reference repo"
        );
        assert!(
            hint.contains("gh api"),
            "Hint should suggest gh api for listing branches"
        );
    }

    #[test]
    fn enrich_git_connect_not_found_azdo_includes_org() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::NotFound,
            "Git provider resource could not be found".to_string(),
        )
        .into();

        let enriched = enrich_git_connect_error(
            err,
            "AzureDevOps",
            "my-repo",
            "develop",
            None,
            Some("my-org"),
        );

        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(hint.contains("develop"), "Hint should mention branch");
        assert!(
            hint.contains("my-org/my-repo"),
            "Hint should reference repo"
        );
        assert!(
            hint.contains("az repos"),
            "Hint should suggest az repos for Azure DevOps"
        );
    }

    #[test]
    fn enrich_git_connect_preserves_non_fabio_errors() {
        let err = anyhow::anyhow!("generic error");
        let enriched =
            enrich_git_connect_error(err, "GitHub", "repo", "branch", Some("owner"), None);
        assert!(enriched.to_string().contains("generic error"));
    }

    #[test]
    fn enrich_git_connect_invalid_input_github_gives_verification_hint() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::ApiError,
            "Invalid input: something wrong".to_string(),
        )
        .into();

        let enriched =
            enrich_git_connect_error(err, "GitHub", "test-repo", "main", Some("testowner"), None);

        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(
            hint.contains("testowner"),
            "Hint should reference the owner"
        );
        assert!(hint.contains("test-repo"), "Hint should reference the repo");
        assert!(
            hint.contains("--connection-id"),
            "Hint should suggest checking connection-id"
        );
    }

    #[test]
    fn enrich_git_connect_skips_unrelated_error_codes() {
        let err: anyhow::Error =
            FabioError::new(ErrorCode::RateLimited, "Rate limited".to_string()).into();

        let enriched =
            enrich_git_connect_error(err, "GitHub", "repo", "branch", Some("owner"), None);
        // Should return the original error unchanged (rate limit is not enriched)
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::RateLimited);
        assert!(fabio_err.hint.is_none());
    }

    #[test]
    fn enrich_git_connect_missing_credentials_github_suggests_connection_list() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::ApiError,
            "The property myGitCredentials is required for the GitProviderType GitHub.".to_string(),
        )
        .into();

        let enriched =
            enrich_git_connect_error(err, "GitHub", "my-repo", "main", Some("myowner"), None);

        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(
            hint.contains("--connection-id"),
            "Hint should mention --connection-id flag"
        );
        assert!(
            hint.contains("fabio connection list"),
            "Hint should suggest 'fabio connection list' to find connections"
        );
        assert!(
            hint.contains("myowner"),
            "Hint should include the owner in the retry example"
        );
        assert!(
            hint.contains("my-repo"),
            "Hint should include the repo in the retry example"
        );
    }
}
