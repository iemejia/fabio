//! Fabric Git integration: connect/commit/pull/checkout lifecycle, branch-out,
//! provider connection & credentials, and workspace Git relations.

use anyhow::Result;
use clap::Subcommand;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};

mod branch_out;
mod connect;
mod relation;
mod sync;

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
        GitCommand::Status { workspace } => sync::status(cli, client, workspace).await,
        GitCommand::Commit {
            workspace,
            message,
            all,
            items,
            workspace_head,
            wait,
            timeout,
        } => sync::commit(
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
        } => sync::pull(
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
        } => connect::connect(
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
        GitCommand::Disconnect { workspace } => connect::disconnect(cli, client, workspace)
            .await
            .map_err(|e| enrich_forbidden(e, "git disconnect", "Admin")),
        GitCommand::Init {
            workspace,
            strategy,
            wait,
            timeout,
        } => connect::init(cli, client, workspace, strategy.as_deref(), *wait, *timeout)
            .await
            .map_err(|e| enrich_forbidden(e, "git init", "Admin")),
        GitCommand::Checkout {
            workspace,
            branch,
            strategy,
            wait,
            timeout,
        } => connect::checkout(
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
            branch_out::branch_out(
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
            ConnectionCommand::Show { workspace } => {
                connect::connection_show(cli, client, workspace).await
            }
        },
        GitCommand::Credentials(sub) => match sub {
            CredentialsCommand::Show { workspace } => {
                connect::credentials_show(cli, client, workspace).await
            }
            CredentialsCommand::Update {
                workspace,
                source,
                connection_id,
            } => connect::credentials_update(
                cli,
                client,
                workspace,
                source,
                connection_id.as_deref(),
            )
            .await
            .map_err(|e| enrich_forbidden(e, "git credentials update", "Admin")),
        },
        GitCommand::Relation(sub) => relation::execute(cli, client, sub).await,
        GitCommand::ShowTracked { workspace } => sync::show_tracked(cli, client, workspace).await,
    }
}

/// Enrich a git connect/checkout error with actionable hints for agents.
///
/// The Fabric API returns generic messages like "The requested operation can't
/// be completed because the Git provider resource could not be found" — this
/// function adds context about what likely went wrong and how to fix it.
pub(super) fn enrich_git_connect_error(
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
