mod capacity;
mod crud;
mod folders;
mod identity;
mod networking;
mod roles;
mod settings;

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};

/// Known workspace roles for error hints.
pub(super) const KNOWN_ROLES: &[&str] = &["Admin", "Member", "Contributor", "Viewer"];

/// Known principal types for error hints.
pub(super) const KNOWN_PRINCIPAL_TYPES: &[&str] = &[
    "User",
    "Group",
    "ServicePrincipal",
    "ServicePrincipalProfile",
];

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples workspace\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum WorkspaceCommand {
    /// List all workspaces
    #[command(display_order = 1)]
    List {
        /// Filter by role: Admin, Member, Contributor, Viewer (comma-separated)
        #[arg(long)]
        roles: Option<String>,
        /// Filter by capacity ID (client-side filter)
        #[arg(long)]
        capacity: Option<String>,
    },
    /// Show details of a workspace
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(long)]
        id: String,
    },
    /// Get the Fabric portal URL for a workspace
    #[command(display_order = 3)]
    Url {
        /// Workspace ID
        #[arg(long)]
        id: String,
    },
    /// Create a new workspace
    #[command(display_order = 10)]
    Create {
        /// Display name for the workspace
        #[arg(long)]
        name: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
    },
    /// Update workspace properties (name and/or description)
    #[command(display_order = 11)]
    Update {
        /// Workspace ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a workspace
    #[command(display_order = 12)]
    Delete {
        /// Workspace ID
        #[arg(long)]
        id: String,
    },
    /// Clone workspace items from one workspace to another using bulk APIs
    ///
    /// Uses Bulk Export Definitions (source) → Bulk Import Definitions (target)
    /// to replicate item definitions. Items are matched by logicalId for updates
    /// or created new. Use --allow-pairing-by-name for initial clones where
    /// logicalIds may not match.
    #[command(display_order = 13)]
    Clone {
        /// Source workspace ID or name
        #[arg(long)]
        source: String,
        /// Destination workspace ID or name
        #[arg(long)]
        dest: String,
        /// Only clone specific item types (comma-separated, e.g., "Notebook,DataPipeline")
        #[arg(long, value_delimiter = ',')]
        item_types: Option<Vec<String>>,
        /// Match items by display name (instead of logicalId) for initial clones
        #[arg(long)]
        allow_pairing_by_name: bool,
    },
    /// Assign a workspace to a capacity
    #[command(display_order = 20)]
    AssignCapacity {
        /// Workspace ID
        #[arg(long, visible_alias = "workspace")]
        id: String,
        /// Target capacity ID
        #[arg(short, long, visible_alias = "capacity-id", env = "FABIO_CAPACITY")]
        capacity: String,
    },
    /// Unassign a workspace from its capacity
    #[command(display_order = 21)]
    UnassignCapacity {
        /// Workspace ID
        #[arg(long)]
        id: String,
    },
    /// Provision a workspace identity (managed identity)
    #[command(display_order = 30)]
    ProvisionIdentity {
        /// Workspace ID
        #[arg(long)]
        id: String,
    },
    /// Deprovision a workspace identity
    #[command(display_order = 31)]
    DeprovisionIdentity {
        /// Workspace ID
        #[arg(long)]
        id: String,
    },
    /// List workspace role assignments
    #[command(display_order = 40)]
    ListRoleAssignments {
        /// Workspace ID
        #[arg(long)]
        id: String,
    },
    /// Add a role assignment to a workspace
    #[command(display_order = 41)]
    AddRoleAssignment {
        /// Workspace ID
        #[arg(long)]
        id: String,
        /// Principal ID (user, group, or service principal object ID)
        #[arg(long)]
        principal_id: String,
        /// Principal type: User, Group, `ServicePrincipal`, or `ServicePrincipalProfile`
        #[arg(long)]
        principal_type: String,
        /// Role to assign (Admin, Member, Contributor, Viewer)
        #[arg(long)]
        role: String,
    },
    /// Update a workspace role assignment
    #[command(display_order = 42)]
    UpdateRoleAssignment {
        /// Workspace ID
        #[arg(long)]
        id: String,
        /// Role assignment ID (same as the principal ID)
        #[arg(long)]
        assignment_id: String,
        /// New role (Admin, Member, Contributor, Viewer)
        #[arg(long)]
        role: String,
    },
    /// Delete a workspace role assignment
    #[command(display_order = 43)]
    DeleteRoleAssignment {
        /// Workspace ID
        #[arg(long)]
        id: String,
        /// Role assignment ID (same as the principal ID)
        #[arg(long)]
        assignment_id: String,
    },
    /// Show a specific workspace role assignment
    #[command(display_order = 16)]
    ShowRoleAssignment {
        /// Workspace ID
        #[arg(long)]
        id: String,
        /// Role assignment ID
        #[arg(long)]
        assignment_id: String,
    },
    /// List workspace folders
    #[command(display_order = 30)]
    ListFolders {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Create a folder in a workspace
    #[command(display_order = 31)]
    CreateFolder {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Folder display name
        #[arg(long)]
        name: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
        /// Optional parent folder ID (omit for root)
        #[arg(long)]
        parent_folder_id: Option<String>,
    },
    /// Show details of a workspace folder
    #[command(display_order = 32)]
    ShowFolder {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Folder ID
        #[arg(long)]
        folder_id: String,
    },
    /// Update a workspace folder
    #[command(display_order = 33)]
    UpdateFolder {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Folder ID
        #[arg(long)]
        folder_id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a workspace folder
    #[command(display_order = 34)]
    DeleteFolder {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Folder ID
        #[arg(long)]
        folder_id: String,
    },
    /// Move a folder to another parent (or root)
    #[command(display_order = 35)]
    MoveFolder {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Folder ID to move
        #[arg(long)]
        folder_id: String,
        /// Target parent folder ID (omit to move to root)
        #[arg(long)]
        target_folder_id: Option<String>,
    },
    /// Apply tags to a workspace
    #[command(display_order = 40)]
    ApplyTags {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Comma-separated tag IDs
        #[arg(long, value_delimiter = ',')]
        tag_ids: Vec<String>,
    },
    /// Remove tags from a workspace
    #[command(display_order = 41)]
    UnapplyTags {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Comma-separated tag IDs
        #[arg(long, value_delimiter = ',')]
        tag_ids: Vec<String>,
    },
    /// Assign workspace to a domain
    #[command(display_order = 45)]
    AssignToDomain {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Domain ID
        #[arg(long)]
        domain_id: String,
    },
    /// Unassign workspace from its domain
    #[command(display_order = 46)]
    UnassignFromDomain {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Get `OneLake` settings for a workspace
    #[command(display_order = 55)]
    GetOnelakeSettings {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Modify `OneLake` default tier (Hot, Cool, or Cold)
    #[command(display_order = 56)]
    ModifyDefaultTier {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Tier: "Hot", "Cool", or "Cold"
        #[arg(long)]
        tier: String,
    },
    /// Modify `OneLake` diagnostics configuration
    #[command(display_order = 57)]
    ModifyDiagnostics {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Modify `OneLake` immutability policy
    #[command(display_order = 58)]
    ModifyImmutabilityPolicy {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Export `OneLake` lifecycle policy
    #[command(display_order = 59)]
    ExportLifecyclePolicy {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Import `OneLake` lifecycle policy
    #[command(display_order = 60)]
    ImportLifecyclePolicy {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Reset `OneLake` shortcut cache for a workspace
    #[command(display_order = 61)]
    ResetShortcutCache {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Get workspace network communication policy
    #[command(display_order = 50)]
    GetNetworkPolicy {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Set workspace network communication policy
    #[command(display_order = 51)]
    SetNetworkPolicy {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Get workspace IP firewall rules
    #[command(display_order = 52)]
    GetFirewallRules {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Set workspace IP firewall rules (replaces all existing rules)
    #[command(display_order = 53)]
    SetFirewallRules {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        file: Option<String>,
        /// Inline JSON firewall rules (e.g. '{"rules":[{"displayName":"Allow Office","value":"10.0.0.0/24"}]}')
        #[arg(long)]
        content: Option<String>,
    },
    /// Get workspace git outbound policy
    #[command(display_order = 54)]
    GetGitOutboundPolicy {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Set workspace git outbound policy (requires Outbound Access Protection enabled)
    #[command(display_order = 54)]
    SetGitOutboundPolicy {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Get workspace inbound Azure resource instance rules
    #[command(display_order = 54)]
    GetInboundAzureResourceRules {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Set workspace inbound Azure resource instance rules
    #[command(display_order = 54)]
    SetInboundAzureResourceRules {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Get workspace outbound cloud connection rules (requires OAP enabled)
    #[command(display_order = 54)]
    GetOutboundCloudConnectionRules {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Set workspace outbound cloud connection rules (requires OAP enabled)
    #[command(display_order = 54)]
    SetOutboundCloudConnectionRules {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Get workspace outbound gateway rules (requires OAP enabled)
    #[command(display_order = 54)]
    GetOutboundGatewayRules {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Set workspace outbound gateway rules (requires OAP enabled)
    #[command(display_order = 54)]
    SetOutboundGatewayRules {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Get workspace inbound External Data Shares bypass policy
    #[command(display_order = 54)]
    GetInboundExternalDataSharesPolicy {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Set workspace inbound External Data Shares bypass policy (preview API, requires *admin* role)
    #[command(display_order = 54)]
    SetInboundExternalDataSharesPolicy {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Whether External Data Shares traffic may bypass inbound networking restrictions
        #[arg(long, value_parser = ["Allow", "Deny"])]
        default_action: String,
        /// `ETag` from a previous `get-inbound-external-data-shares-policy` call, for optimistic concurrency
        #[arg(long)]
        if_match: Option<String>,
    },
    /// Get workspace settings (properties including `automaticMetadataSync`)
    #[command(display_order = 62)]
    GetSettings {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Update workspace settings (e.g. enable automatic metadata sync)
    ///
    /// Pass a JSON object with the properties to update. Example:
    ///   fabio workspace update-settings -w <WS> --content '{"automaticMetadataSync":"Enabled"}'
    #[command(display_order = 63)]
    UpdateSettings {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Set default dataset storage format (Small or Large) via Power BI API
    #[command(display_order = 64)]
    SetDatasetStorageFormat {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Storage format: "Small" or "Large"
        #[arg(long)]
        format: String,
    },
    /// Get default dataset storage format via Power BI API
    #[command(display_order = 65)]
    GetDatasetStorageFormat {
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    // ─── CMK Encryption ──────────────────────────────────────────────────────
    /// Get workspace Customer-Managed Key (CMK) encryption settings (Preview)
    #[command(display_order = 70)]
    GetEncryption {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Assign a Customer-Managed Key (CMK) to a workspace, enabling or rotating encryption (Preview)
    #[command(display_order = 71)]
    AssignEncryption {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Azure Key Vault key identifier (must be a versionless key URI)
        #[arg(long)]
        key_identifier: String,
    },
    /// Reset workspace encryption by removing the CMK configuration (reverts to Microsoft-managed keys) (Preview)
    #[command(display_order = 72)]
    ResetEncryption {
        /// Workspace ID
        #[arg(short = 'w', long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &WorkspaceCommand) -> Result<()> {
    match command {
        WorkspaceCommand::List { roles, capacity } => {
            crud::list(cli, client, roles.as_deref(), capacity.as_deref()).await
        }
        WorkspaceCommand::Show { id } => crud::show(cli, client, id).await,
        WorkspaceCommand::Url { id } => crud::url(cli, id),
        WorkspaceCommand::Create { name, description } => {
            crud::create(cli, client, name, description.as_deref()).await
        }
        WorkspaceCommand::Update {
            id,
            name,
            description,
        } => crud::update(cli, client, id, name.as_deref(), description.as_deref()).await,
        WorkspaceCommand::Delete { id } => crud::delete(cli, client, id).await,
        WorkspaceCommand::Clone {
            source,
            dest,
            item_types,
            allow_pairing_by_name,
        } => {
            crud::clone_workspace(
                cli,
                client,
                source,
                dest,
                item_types.as_deref(),
                *allow_pairing_by_name,
            )
            .await
        }
        WorkspaceCommand::AssignCapacity { id, capacity } => {
            capacity::assign_capacity(cli, client, id, capacity).await
        }
        WorkspaceCommand::UnassignCapacity { id } => {
            capacity::unassign_capacity(cli, client, id).await
        }
        WorkspaceCommand::ProvisionIdentity { id } => {
            identity::provision_identity(cli, client, id).await
        }
        WorkspaceCommand::DeprovisionIdentity { id } => {
            identity::deprovision_identity(cli, client, id).await
        }
        WorkspaceCommand::ListRoleAssignments { id } => {
            roles::list_role_assignments(cli, client, id).await
        }
        WorkspaceCommand::AddRoleAssignment {
            id,
            principal_id,
            principal_type,
            role,
        } => roles::add_role_assignment(cli, client, id, principal_id, principal_type, role).await,
        WorkspaceCommand::UpdateRoleAssignment {
            id,
            assignment_id,
            role,
        } => roles::update_role_assignment(cli, client, id, assignment_id, role).await,
        WorkspaceCommand::DeleteRoleAssignment { id, assignment_id } => {
            roles::delete_role_assignment(cli, client, id, assignment_id).await
        }
        WorkspaceCommand::ShowRoleAssignment { id, assignment_id } => {
            roles::show_role_assignment(cli, client, id, assignment_id).await
        }
        WorkspaceCommand::ListFolders { workspace } => {
            folders::list_folders(cli, client, workspace).await
        }
        WorkspaceCommand::CreateFolder {
            workspace,
            name,
            description,
            parent_folder_id,
        } => {
            folders::create_folder(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                parent_folder_id.as_deref(),
            )
            .await
        }
        WorkspaceCommand::ShowFolder {
            workspace,
            folder_id,
        } => folders::show_folder(cli, client, workspace, folder_id).await,
        WorkspaceCommand::UpdateFolder {
            workspace,
            folder_id,
            name,
            description,
        } => {
            folders::update_folder(
                cli,
                client,
                workspace,
                folder_id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        WorkspaceCommand::DeleteFolder {
            workspace,
            folder_id,
        } => folders::delete_folder(cli, client, workspace, folder_id).await,
        WorkspaceCommand::MoveFolder {
            workspace,
            folder_id,
            target_folder_id,
        } => {
            folders::move_folder(
                cli,
                client,
                workspace,
                folder_id,
                target_folder_id.as_deref(),
            )
            .await
        }
        WorkspaceCommand::ApplyTags { workspace, tag_ids } => {
            settings::apply_tags(cli, client, workspace, tag_ids).await
        }
        WorkspaceCommand::UnapplyTags { workspace, tag_ids } => {
            settings::unapply_tags(cli, client, workspace, tag_ids).await
        }
        WorkspaceCommand::AssignToDomain {
            workspace,
            domain_id,
        } => settings::assign_to_domain(cli, client, workspace, domain_id).await,
        WorkspaceCommand::UnassignFromDomain { workspace } => {
            settings::unassign_from_domain(cli, client, workspace).await
        }
        WorkspaceCommand::GetOnelakeSettings { workspace } => {
            settings::get_onelake_settings(cli, client, workspace).await
        }
        WorkspaceCommand::ModifyDefaultTier { workspace, tier } => {
            settings::modify_default_tier(cli, client, workspace, tier).await
        }
        WorkspaceCommand::ModifyDiagnostics {
            workspace,
            file,
            content,
        } => {
            settings::modify_diagnostics(
                cli,
                client,
                workspace,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        WorkspaceCommand::ModifyImmutabilityPolicy {
            workspace,
            file,
            content,
        } => {
            settings::modify_immutability_policy(
                cli,
                client,
                workspace,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        WorkspaceCommand::ExportLifecyclePolicy { workspace } => {
            settings::export_lifecycle_policy(cli, client, workspace).await
        }
        WorkspaceCommand::ImportLifecyclePolicy {
            workspace,
            file,
            content,
        } => {
            settings::import_lifecycle_policy(
                cli,
                client,
                workspace,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        WorkspaceCommand::ResetShortcutCache { workspace } => {
            settings::reset_shortcut_cache(cli, client, workspace).await
        }
        WorkspaceCommand::GetNetworkPolicy { workspace } => {
            networking::get_network_policy(cli, client, workspace).await
        }
        WorkspaceCommand::SetNetworkPolicy {
            workspace,
            file,
            content,
        } => {
            networking::set_network_policy(
                cli,
                client,
                workspace,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        WorkspaceCommand::GetFirewallRules { workspace } => {
            networking::get_firewall_rules(cli, client, workspace).await
        }
        WorkspaceCommand::SetFirewallRules {
            workspace,
            file,
            content,
        } => {
            networking::set_firewall_rules(
                cli,
                client,
                workspace,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        WorkspaceCommand::GetGitOutboundPolicy { workspace } => {
            networking::get_git_outbound_policy(cli, client, workspace).await
        }
        WorkspaceCommand::SetGitOutboundPolicy {
            workspace,
            file,
            content,
        } => {
            networking::set_git_outbound_policy(
                cli,
                client,
                workspace,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        WorkspaceCommand::GetInboundAzureResourceRules { workspace } => {
            networking::get_inbound_azure_resource_rules(cli, client, workspace).await
        }
        WorkspaceCommand::SetInboundAzureResourceRules {
            workspace,
            file,
            content,
        } => {
            networking::set_inbound_azure_resource_rules(
                cli,
                client,
                workspace,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        WorkspaceCommand::GetOutboundCloudConnectionRules { workspace } => {
            networking::get_outbound_cloud_connection_rules(cli, client, workspace).await
        }
        WorkspaceCommand::SetOutboundCloudConnectionRules {
            workspace,
            file,
            content,
        } => {
            networking::set_outbound_cloud_connection_rules(
                cli,
                client,
                workspace,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        WorkspaceCommand::GetOutboundGatewayRules { workspace } => {
            networking::get_outbound_gateway_rules(cli, client, workspace).await
        }
        WorkspaceCommand::SetOutboundGatewayRules {
            workspace,
            file,
            content,
        } => {
            networking::set_outbound_gateway_rules(
                cli,
                client,
                workspace,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        WorkspaceCommand::GetInboundExternalDataSharesPolicy { workspace } => {
            networking::get_inbound_external_data_shares_policy(cli, client, workspace).await
        }
        WorkspaceCommand::SetInboundExternalDataSharesPolicy {
            workspace,
            default_action,
            if_match,
        } => {
            networking::set_inbound_external_data_shares_policy(
                cli,
                client,
                workspace,
                default_action,
                if_match.as_deref(),
            )
            .await
        }
        WorkspaceCommand::GetSettings { workspace } => {
            settings::get_settings(cli, client, workspace).await
        }
        WorkspaceCommand::UpdateSettings {
            workspace,
            file,
            content,
        } => {
            settings::update_settings(cli, client, workspace, file.as_deref(), content.as_deref())
                .await
        }
        WorkspaceCommand::SetDatasetStorageFormat { workspace, format } => {
            settings::set_dataset_storage_format(cli, client, workspace, format).await
        }
        WorkspaceCommand::GetDatasetStorageFormat { workspace } => {
            settings::get_dataset_storage_format(cli, client, workspace).await
        }
        WorkspaceCommand::GetEncryption { workspace } => {
            settings::get_encryption(cli, client, workspace).await
        }
        WorkspaceCommand::AssignEncryption {
            workspace,
            key_identifier,
        } => settings::assign_encryption(cli, client, workspace, key_identifier).await,
        WorkspaceCommand::ResetEncryption { workspace } => {
            settings::reset_encryption(cli, client, workspace).await
        }
    }
}

/// Read JSON body from --file or --content flag.
pub(super) fn read_json_body(
    file: Option<&str>,
    content: Option<&str>,
    command: &str,
) -> Result<Value> {
    match (file, content) {
        (Some(path), _) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
            serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("Invalid JSON: {e}"))
        }
        (_, Some(c)) => serde_json::from_str(c).map_err(|e| anyhow::anyhow!("Invalid JSON: {e}")),
        (None, None) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            format!("Example: fabio {command} --workspace <WS> --file config.json"),
        )
        .into()),
    }
}

/// Enrich capacity assignment errors with actionable hints.
pub(super) fn enrich_assign_capacity_error(err: anyhow::Error, capacity: &str) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };
    let hint = format!(
        "Capacity '{capacity}' was not found or is not accessible. List available capacities with: az fabric capacity list --query '[].{{name:name, id:id, state:properties.state}}' -o table. Create one with: az fabric capacity create --name <name> --resource-group <rg> --location <region> --sku F2 --administration-members <email>"
    );
    FabioError::with_hint(fabio_err.code, fabio_err.message.clone(), hint).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_roles_are_capitalized() {
        for role in KNOWN_ROLES {
            let first = role.chars().next().unwrap();
            assert!(first.is_uppercase(), "Role '{role}' should be capitalized");
        }
    }

    #[test]
    fn known_principal_types_are_capitalized() {
        for pt in KNOWN_PRINCIPAL_TYPES {
            let first = pt.chars().next().unwrap();
            assert!(
                first.is_uppercase(),
                "Principal type '{pt}' should be capitalized"
            );
        }
    }

    #[test]
    fn role_validation_is_case_insensitive() {
        assert!(KNOWN_ROLES.iter().any(|r| r.eq_ignore_ascii_case("admin")));
        assert!(KNOWN_ROLES.iter().any(|r| r.eq_ignore_ascii_case("MEMBER")));
        assert!(!KNOWN_ROLES.iter().any(|r| r.eq_ignore_ascii_case("Owner")));
    }

    #[test]
    fn principal_type_validation_is_case_insensitive() {
        assert!(
            KNOWN_PRINCIPAL_TYPES
                .iter()
                .any(|t| t.eq_ignore_ascii_case("user"))
        );
        assert!(
            KNOWN_PRINCIPAL_TYPES
                .iter()
                .any(|t| t.eq_ignore_ascii_case("GROUP"))
        );
        assert!(
            !KNOWN_PRINCIPAL_TYPES
                .iter()
                .any(|t| t.eq_ignore_ascii_case("Application"))
        );
    }

    #[test]
    fn enrich_capacity_error_preserves_non_fabio_errors() {
        let err = anyhow::anyhow!("generic error");
        let enriched = enrich_assign_capacity_error(err, "some-cap");
        assert!(enriched.to_string().contains("generic error"));
    }

    #[test]
    fn enrich_capacity_error_adds_hint() {
        let err: anyhow::Error =
            FabioError::new(ErrorCode::NotFound, "capacity not found".to_string()).into();
        let enriched = enrich_assign_capacity_error(err, "bad-cap-id");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert!(
            fabio_err
                .hint
                .as_ref()
                .unwrap()
                .contains("az fabric capacity")
        );
        assert!(fabio_err.hint.as_ref().unwrap().contains("bad-cap-id"));
    }

    #[test]
    fn read_json_body_from_content() {
        let result = read_json_body(None, Some(r#"{"automaticMetadataSync":"Enabled"}"#), "test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["automaticMetadataSync"], "Enabled");
    }

    #[test]
    fn read_json_body_invalid_json_returns_error() {
        let result = read_json_body(None, Some("not json"), "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid JSON"));
    }

    #[test]
    fn read_json_body_missing_both_returns_error() {
        let result = read_json_body(None, None, "workspace update-settings");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("--file or --content")
        );
    }

    #[test]
    fn update_settings_body_is_passed_directly() {
        let body: Value = serde_json::from_str(r#"{"automaticMetadataSync":"Enabled"}"#).unwrap();
        assert_eq!(body["automaticMetadataSync"], "Enabled");
        assert!(body.get("properties").is_none());
    }

    #[test]
    fn update_settings_supports_complex_body() {
        let body: Value = serde_json::from_str(
            r#"{"displayName":"NewName","properties":{"automaticMetadataSync":"Enabled"}}"#,
        )
        .unwrap();
        assert_eq!(body["displayName"], "NewName");
        assert_eq!(body["properties"]["automaticMetadataSync"], "Enabled");
    }
}
