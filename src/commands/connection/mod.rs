//! `fabio connection` — manage Fabric connections (data-source connections and
//! their role assignments), plus governance/hygiene helpers built on the
//! connection-recency signals returned by the List Connections API.

mod crud;
mod hygiene;
mod roles;

use anyhow::Result;
use clap::Subcommand;

use crate::cli::Cli;
use crate::client::FabricClient;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples connection\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum ConnectionCommand {
    /// List all connections you have permission to access
    #[command(display_order = 1)]
    List,
    /// Show details of a specific connection
    #[command(display_order = 2)]
    Show {
        /// Connection ID
        #[arg(long)]
        id: String,
    },
    /// Create a new connection
    #[command(display_order = 3)]
    Create {
        /// Display name for the connection
        #[arg(long)]
        name: String,

        /// Connectivity type
        #[arg(long, value_name = "TYPE", value_parser = ["ShareableCloud", "OnPremises", "VirtualNetworkGateway", "StreamingVirtualNetworkGateway", "PersonalCloud"])]
        connectivity_type: String,

        /// Connection type path (e.g., Web, SQL, `GitHubSourceControl`)
        #[arg(long, visible_alias = "type", value_name = "TYPE")]
        connection_type: String,

        /// Creation method (`connectionDetails.creationMethod`). If omitted, fabio auto-resolves it from the connection type via `supportedConnectionTypes` (most types differ, e.g. `SQL`→`Sql`, `EventHub`→`EventHub.Contents`). Specify explicitly only for types with multiple methods (e.g. `AzureDataExplorer`, `Spark`).
        #[arg(long, value_name = "METHOD")]
        creation_method: Option<String>,

        /// Connection parameters as JSON (e.g., '{"server":"host","database":"db"}')
        #[arg(long)]
        parameters: String,

        /// Gateway ID through which the connection is made (required when `--connectivity-type` is `VirtualNetworkGateway` or `StreamingVirtualNetworkGateway`)
        #[arg(long, value_name = "GATEWAY_ID")]
        gateway_id: Option<String>,

        /// Credential type
        #[arg(long, value_parser = ["Basic", "OAuth2", "Key", "Anonymous", "ServicePrincipal", "SharedAccessSignature", "WorkspaceIdentity", "KeyPair"])]
        credential_type: String,

        /// Credentials as JSON (format depends on credential type)
        #[arg(long)]
        credentials: Option<String>,

        /// Privacy level
        #[arg(long, default_value = "Organizational", value_parser = ["None", "Public", "Organizational", "Private"])]
        privacy_level: String,

        /// Skip connection test during creation
        #[arg(long)]
        skip_test_connection: bool,

        /// Allow this connection to be used by code-first artifacts such as Notebooks (`allowUsageInUserControlledCode`). Can ONLY be set at creation time; cannot be changed later.
        #[arg(long, visible_alias = "allow-usage-in-user-controlled-code")]
        allow_code_first_artifacts: bool,

        /// Allow this connection to be used with on-premises or virtual-network data gateways (`allowConnectionUsageInGateway`).
        #[arg(long)]
        allow_gateway_usage: bool,
    },
    /// Update a connection's name, credentials, or privacy level
    #[command(display_order = 4)]
    Update {
        /// Connection ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New privacy level
        #[arg(long, value_parser = ["None", "Public", "Organizational", "Private"])]
        privacy_level: Option<String>,

        /// New credential type
        #[arg(long, value_parser = ["Basic", "OAuth2", "Key", "Anonymous", "ServicePrincipal", "SharedAccessSignature", "WorkspaceIdentity", "KeyPair"])]
        credential_type: Option<String>,

        /// New credentials as JSON
        #[arg(long)]
        credentials: Option<String>,
    },
    /// Delete a connection
    #[command(display_order = 5)]
    Delete {
        /// Connection ID
        #[arg(long)]
        id: String,
    },
    /// List supported connection types (gateway types catalog)
    #[command(display_order = 10)]
    ListSupportedTypes,
    /// List role assignments for a connection
    #[command(display_order = 20)]
    ListRoleAssignments {
        /// Connection ID
        #[arg(long)]
        id: String,
    },
    /// Add a role assignment to a connection
    #[command(display_order = 21)]
    AddRoleAssignment {
        /// Connection ID
        #[arg(long)]
        id: String,

        /// Principal ID (user, group, or service principal)
        #[arg(long)]
        principal_id: String,

        /// Principal type
        #[arg(long, value_parser = ["User", "Group", "ServicePrincipal"])]
        principal_type: String,

        /// Role to assign
        #[arg(long, value_parser = ["Owner", "User", "UserWithReshare"])]
        role: String,
    },
    /// Show a specific role assignment for a connection
    #[command(display_order = 22)]
    ShowRoleAssignment {
        /// Connection ID
        #[arg(long)]
        id: String,

        /// Role assignment ID
        #[arg(long)]
        assignment_id: String,
    },
    /// Update a role assignment for a connection
    #[command(display_order = 23)]
    UpdateRoleAssignment {
        /// Connection ID
        #[arg(long)]
        id: String,

        /// Role assignment ID
        #[arg(long)]
        assignment_id: String,

        /// New role
        #[arg(long, value_parser = ["Owner", "User", "UserWithReshare"])]
        role: String,
    },
    /// Delete a role assignment from a connection
    #[command(display_order = 24)]
    DeleteRoleAssignment {
        /// Connection ID
        #[arg(long)]
        id: String,

        /// Role assignment ID
        #[arg(long)]
        assignment_id: String,
    },
    /// Test a connection (not supported for `StreamingVirtualNetworkGateway` connections)
    #[command(display_order = 30)]
    TestConnection {
        /// Connection ID
        #[arg(long)]
        id: String,
    },
    /// Find stale connections (never bound to an item, or whose credentials
    /// haven't been used recently) — a governance aid for reducing connection
    /// sprawl. Read-only: reports candidates for review, does not delete.
    #[command(display_order = 40)]
    FindStale {
        /// Flag connections whose credentials have not been used in at least
        /// this many days (never-bound connections are always flagged).
        #[arg(long, default_value_t = 90)]
        unused_days: u32,

        /// Only assess connections created on or after this UTC date
        /// (YYYY-MM-DD). Connections created before connection-recency GA
        /// (~March 2026) report NULL recency even when actively used, so the
        /// default cutoff avoids false positives. Pass an earlier date only if
        /// you understand the NULL-recency caveat.
        #[arg(long, default_value = "2026-05-01")]
        created_after: String,
    },
    /// Find duplicate connections — multiple connections that reach the same
    /// target (same type, path, connectivity type, and gateway). Reports the
    /// redundant connections (keeping the most-recently-used one) as candidates
    /// for consolidation. Read-only.
    #[command(display_order = 41)]
    FindDuplicates {
        /// Also require the credential type to match when grouping duplicates
        /// (by default, connections that differ only in credential type are
        /// still considered duplicates of the same target).
        #[arg(long)]
        match_credential_type: bool,
    },
    /// Find connections whose only Owner is a single individual user — an
    /// orphan risk if that user leaves the organization. Prefer adding a
    /// Microsoft Entra group as a second owner. Read-only.
    #[command(display_order = 42)]
    FindSingleOwner,
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &ConnectionCommand) -> Result<()> {
    match command {
        ConnectionCommand::List => crud::list(cli, client).await,
        ConnectionCommand::Show { id } => crud::show(cli, client, id).await,
        ConnectionCommand::Create {
            name,
            connectivity_type,
            connection_type,
            creation_method,
            parameters,
            gateway_id,
            credential_type,
            credentials,
            privacy_level,
            skip_test_connection,
            allow_code_first_artifacts,
            allow_gateway_usage,
        } => {
            crud::create(
                cli,
                client,
                name,
                connectivity_type,
                connection_type,
                creation_method.as_deref(),
                parameters,
                gateway_id.as_deref(),
                credential_type,
                credentials.as_deref(),
                privacy_level,
                *skip_test_connection,
                *allow_code_first_artifacts,
                *allow_gateway_usage,
            )
            .await
        }
        ConnectionCommand::Update {
            id,
            name,
            privacy_level,
            credential_type,
            credentials,
        } => {
            crud::update(
                cli,
                client,
                id,
                name.as_deref(),
                privacy_level.as_deref(),
                credential_type.as_deref(),
                credentials.as_deref(),
            )
            .await
        }
        ConnectionCommand::Delete { id } => crud::delete(cli, client, id).await,
        ConnectionCommand::ListSupportedTypes => crud::list_supported_types(cli, client).await,
        ConnectionCommand::ListRoleAssignments { id } => {
            roles::list_role_assignments(cli, client, id).await
        }
        ConnectionCommand::AddRoleAssignment {
            id,
            principal_id,
            principal_type,
            role,
        } => roles::add_role_assignment(cli, client, id, principal_id, principal_type, role).await,
        ConnectionCommand::ShowRoleAssignment { id, assignment_id } => {
            roles::show_role_assignment(cli, client, id, assignment_id).await
        }
        ConnectionCommand::UpdateRoleAssignment {
            id,
            assignment_id,
            role,
        } => roles::update_role_assignment(cli, client, id, assignment_id, role).await,
        ConnectionCommand::DeleteRoleAssignment { id, assignment_id } => {
            roles::delete_role_assignment(cli, client, id, assignment_id).await
        }
        ConnectionCommand::TestConnection { id } => roles::test_connection(cli, client, id).await,
        ConnectionCommand::FindStale {
            unused_days,
            created_after,
        } => hygiene::find_stale(cli, client, *unused_days, created_after).await,
        ConnectionCommand::FindDuplicates {
            match_credential_type,
        } => hygiene::find_duplicates(cli, client, *match_credential_type).await,
        ConnectionCommand::FindSingleOwner => hygiene::find_single_owner(cli, client).await,
    }
}
