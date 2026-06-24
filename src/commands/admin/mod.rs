mod domains;
mod items;
mod tags;
mod tenant_settings;
mod users;
mod workloads;
mod workspaces;

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    // ── Tenant Settings ──────────────────────────────────────────────────
    /// List all tenant settings
    #[command(display_order = 1)]
    ListTenantSettings,
    /// Update a tenant setting
    #[command(display_order = 2)]
    UpdateTenantSetting {
        /// Tenant setting name
        #[arg(long)]
        setting_name: String,

        /// Path to JSON file with setting body
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content for setting body
        #[arg(long)]
        content: Option<String>,
    },
    /// List all capacities' delegated tenant setting overrides
    #[command(display_order = 3)]
    ListCapacitiesTenantOverrides,
    /// List delegated tenant setting overrides for a capacity
    #[command(display_order = 4)]
    ListCapacityTenantOverrides {
        /// Capacity ID
        #[arg(long)]
        capacity_id: String,
    },
    /// Delete a capacity delegated tenant setting override
    #[command(display_order = 5)]
    DeleteCapacityTenantOverride {
        /// Capacity ID
        #[arg(long)]
        capacity_id: String,

        /// Tenant setting name
        #[arg(long)]
        setting_name: String,
    },
    /// Update a capacity delegated tenant setting override
    #[command(display_order = 6)]
    UpdateCapacityTenantOverride {
        /// Capacity ID
        #[arg(long)]
        capacity_id: String,

        /// Tenant setting name
        #[arg(long)]
        setting_name: String,

        /// Path to JSON file with override body
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content for override body
        #[arg(long)]
        content: Option<String>,
    },
    /// List all domains' delegated tenant setting overrides
    #[command(display_order = 7)]
    ListDomainsTenantOverrides,
    /// List all workspaces' delegated tenant setting overrides
    #[command(display_order = 8)]
    ListWorkspacesTenantOverrides,

    // ── Tags ─────────────────────────────────────────────────────────────
    /// List tags
    #[command(display_order = 10)]
    ListTags,
    /// Bulk-create tags
    #[command(display_order = 11)]
    CreateTags {
        /// Path to JSON file with tag definitions
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content with tag definitions
        #[arg(long)]
        content: Option<String>,
    },
    /// Update a tag
    #[command(display_order = 12)]
    UpdateTag {
        /// Tag ID
        #[arg(long)]
        tag_id: String,

        /// New tag name
        #[arg(long)]
        name: Option<String>,

        /// New tag description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a tag
    #[command(display_order = 13)]
    DeleteTag {
        /// Tag ID
        #[arg(long)]
        tag_id: String,
    },

    // ── Workloads ────────────────────────────────────────────────────────
    /// List workloads
    #[command(display_order = 20)]
    ListWorkloads,
    /// List workload assignments
    #[command(display_order = 21)]
    ListWorkloadAssignments,
    /// Create a workload assignment
    #[command(display_order = 22)]
    CreateWorkloadAssignment {
        /// Path to JSON file with assignment body
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content for assignment body
        #[arg(long)]
        content: Option<String>,
    },
    /// Delete a workload assignment
    #[command(display_order = 23)]
    DeleteWorkloadAssignment {
        /// Assignment ID
        #[arg(long)]
        assignment_id: String,
    },

    // ── Workspaces ───────────────────────────────────────────────────────
    /// List workspaces (admin view)
    #[command(display_order = 30)]
    ListWorkspaces {
        /// Include additional data in the response (e.g. "encryption")
        #[arg(long)]
        include: Option<String>,

        /// Filter workspaces by encryption status (only valid when --include=encryption is set).
        /// Valid values: `Disabled`, `Active`, `EnableInProgress`, `DisableInProgress`, `Failed`
        #[arg(long)]
        encryption_status: Option<String>,
    },
    /// Show workspace details (admin view)
    #[command(display_order = 31)]
    ShowWorkspace {
        /// Workspace ID
        #[arg(long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// List users in a workspace (admin view)
    #[command(display_order = 32)]
    ListWorkspaceUsers {
        /// Workspace ID
        #[arg(long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// List git connections across workspaces
    #[command(display_order = 33)]
    ListGitConnections,
    /// Grant temporary admin access to a workspace
    #[command(display_order = 34)]
    GrantAdminAccess {
        /// Workspace ID
        #[arg(long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Remove temporary admin access from a workspace
    #[command(display_order = 35)]
    RemoveAdminAccess {
        /// Workspace ID
        #[arg(long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Restore a deleted workspace
    #[command(display_order = 36)]
    RestoreWorkspace {
        /// Workspace ID
        #[arg(long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Restored workspace name
        #[arg(long)]
        name: String,

        /// Capacity ID to assign the restored workspace
        #[arg(long)]
        capacity_id: String,
    },
    /// List network communication policies
    #[command(display_order = 37)]
    ListNetworkPolicies,

    // ── Items ────────────────────────────────────────────────────────────
    /// List items (admin view)
    #[command(display_order = 40)]
    ListItems,
    /// Show item details (admin view)
    #[command(display_order = 41)]
    ShowItem {
        /// Workspace ID
        #[arg(long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        item_id: String,
    },
    /// List users with access to an item (admin view)
    #[command(display_order = 42)]
    ListItemUsers {
        /// Workspace ID
        #[arg(long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        item_id: String,
    },
    /// Bulk-set sensitivity labels on items
    #[command(display_order = 43)]
    BulkSetLabels {
        /// Path to JSON file with label assignments
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content with label assignments
        #[arg(long)]
        content: Option<String>,
    },
    /// Bulk-remove sensitivity labels from items
    #[command(display_order = 44)]
    BulkRemoveLabels {
        /// Path to JSON file with item IDs
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content with item IDs
        #[arg(long)]
        content: Option<String>,
    },
    /// List external data shares
    #[command(display_order = 45)]
    ListExternalDataShares,
    /// Revoke an external data share
    #[command(display_order = 46)]
    RevokeExternalDataShare {
        /// Workspace ID
        #[arg(long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        item_id: String,

        /// External data share ID
        #[arg(long)]
        share_id: String,
    },
    /// Remove all sharing links for specified items
    #[command(display_order = 47)]
    RemoveAllSharingLinks {
        /// Path to JSON file with items
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content with items
        #[arg(long)]
        content: Option<String>,
    },
    /// Bulk-remove sharing links
    #[command(display_order = 48)]
    BulkRemoveSharingLinks {
        /// Path to JSON file with sharing link removals
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content with sharing link removals
        #[arg(long)]
        content: Option<String>,
    },

    // ── Domains ──────────────────────────────────────────────────────────
    /// List domains (admin view)
    #[command(display_order = 50)]
    ListDomains,
    /// Create a domain
    #[command(display_order = 51)]
    CreateDomain {
        /// Domain name
        #[arg(long)]
        name: String,

        /// Domain description
        #[arg(long)]
        description: Option<String>,

        /// Parent domain ID (for subdomains)
        #[arg(long)]
        parent_id: Option<String>,
    },
    /// Show domain details
    #[command(display_order = 52)]
    ShowDomain {
        /// Domain ID
        #[arg(long)]
        domain_id: String,
    },
    /// Update a domain
    #[command(display_order = 53)]
    UpdateDomain {
        /// Domain ID
        #[arg(long)]
        domain_id: String,

        /// New name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a domain
    #[command(display_order = 54)]
    DeleteDomain {
        /// Domain ID
        #[arg(long)]
        domain_id: String,
    },
    /// List workspaces in a domain
    #[command(display_order = 55)]
    ListDomainWorkspaces {
        /// Domain ID
        #[arg(long)]
        domain_id: String,
    },
    /// Assign workspaces to a domain
    #[command(display_order = 56)]
    AssignDomainWorkspaces {
        /// Domain ID
        #[arg(long)]
        domain_id: String,

        /// Workspace IDs (comma-separated)
        #[arg(long, value_delimiter = ',')]
        workspace_ids: Vec<String>,
    },
    /// Unassign workspaces from a domain
    #[command(display_order = 57)]
    UnassignDomainWorkspaces {
        /// Domain ID
        #[arg(long)]
        domain_id: String,

        /// Workspace IDs (comma-separated)
        #[arg(long, value_delimiter = ',')]
        workspace_ids: Vec<String>,
    },
    /// Unassign all workspaces from a domain
    #[command(display_order = 58)]
    UnassignAllDomainWorkspaces {
        /// Domain ID
        #[arg(long)]
        domain_id: String,
    },
    /// List role assignments for a domain
    #[command(display_order = 59)]
    ListDomainRoleAssignments {
        /// Domain ID
        #[arg(long)]
        domain_id: String,
    },
    /// Bulk-assign roles to a domain
    #[command(display_order = 60)]
    BulkAssignDomainRoles {
        /// Domain ID
        #[arg(long)]
        domain_id: String,

        /// Path to JSON file with role assignments
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content with role assignments
        #[arg(long)]
        content: Option<String>,
    },
    /// Bulk-unassign roles from a domain
    #[command(display_order = 61)]
    BulkUnassignDomainRoles {
        /// Domain ID
        #[arg(long)]
        domain_id: String,

        /// Path to JSON file with role unassignments
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content with role unassignments
        #[arg(long)]
        content: Option<String>,
    },
    /// Sync domain role assignments to subdomains
    #[command(display_order = 62)]
    SyncDomainRolesToSubdomains {
        /// Domain ID
        #[arg(long)]
        domain_id: String,

        /// Role to sync (Contributor or Admin). Note: syncing Admins is not supported by the API.
        #[arg(long, default_value = "Contributor")]
        role: String,
    },
    /// Assign workspaces to a domain by capacities
    #[command(display_order = 63)]
    AssignDomainWorkspacesByCapacities {
        /// Domain ID
        #[arg(long)]
        domain_id: String,

        /// Capacity IDs (comma-separated)
        #[arg(long, value_delimiter = ',')]
        capacity_ids: Vec<String>,
    },
    /// Assign workspaces to a domain by principals
    #[command(display_order = 64)]
    AssignDomainWorkspacesByPrincipals {
        /// Domain ID
        #[arg(long)]
        domain_id: String,

        /// Principal IDs (comma-separated)
        #[arg(long, value_delimiter = ',')]
        principal_ids: Vec<String>,

        /// Principal type (User, Group, `ServicePrincipal`, `ServicePrincipalProfile`)
        #[arg(long, default_value = "User")]
        principal_type: String,
    },

    // ── Users ────────────────────────────────────────────────────────────
    /// List access details for a user
    #[command(display_order = 70)]
    ListUserAccess {
        /// User ID
        #[arg(long)]
        user_id: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &AdminCommand) -> Result<()> {
    match command {
        // Tenant Settings
        AdminCommand::ListTenantSettings => {
            tenant_settings::list_tenant_settings(cli, client).await
        }
        AdminCommand::UpdateTenantSetting {
            setting_name,
            file,
            content,
        } => {
            tenant_settings::update_tenant_setting(
                cli,
                client,
                setting_name,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        AdminCommand::ListCapacitiesTenantOverrides => {
            tenant_settings::list_capacities_tenant_overrides(cli, client).await
        }
        AdminCommand::ListCapacityTenantOverrides { capacity_id } => {
            tenant_settings::list_capacity_tenant_overrides(cli, client, capacity_id).await
        }
        AdminCommand::DeleteCapacityTenantOverride {
            capacity_id,
            setting_name,
        } => {
            tenant_settings::delete_capacity_tenant_override(cli, client, capacity_id, setting_name)
                .await
        }
        AdminCommand::UpdateCapacityTenantOverride {
            capacity_id,
            setting_name,
            file,
            content,
        } => {
            tenant_settings::update_capacity_tenant_override(
                cli,
                client,
                capacity_id,
                setting_name,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        AdminCommand::ListDomainsTenantOverrides => {
            tenant_settings::list_domains_tenant_overrides(cli, client).await
        }
        AdminCommand::ListWorkspacesTenantOverrides => {
            tenant_settings::list_workspaces_tenant_overrides(cli, client).await
        }
        // Tags
        AdminCommand::ListTags => tags::list_tags(cli, client).await,
        AdminCommand::CreateTags { file, content } => {
            tags::create_tags(cli, client, file.as_deref(), content.as_deref()).await
        }
        AdminCommand::UpdateTag {
            tag_id,
            name,
            description,
        } => tags::update_tag(cli, client, tag_id, name.as_deref(), description.as_deref()).await,
        AdminCommand::DeleteTag { tag_id } => tags::delete_tag(cli, client, tag_id).await,
        // Workloads
        AdminCommand::ListWorkloads => workloads::list_workloads(cli, client).await,
        AdminCommand::ListWorkloadAssignments => {
            workloads::list_workload_assignments(cli, client).await
        }
        AdminCommand::CreateWorkloadAssignment { file, content } => {
            workloads::create_workload_assignment(cli, client, file.as_deref(), content.as_deref())
                .await
        }
        AdminCommand::DeleteWorkloadAssignment { assignment_id } => {
            workloads::delete_workload_assignment(cli, client, assignment_id).await
        }
        // Workspaces
        AdminCommand::ListWorkspaces {
            include,
            encryption_status,
        } => {
            workspaces::list_workspaces(
                cli,
                client,
                include.as_deref(),
                encryption_status.as_deref(),
            )
            .await
        }
        AdminCommand::ShowWorkspace { workspace } => {
            workspaces::show_workspace(cli, client, workspace).await
        }
        AdminCommand::ListWorkspaceUsers { workspace } => {
            workspaces::list_workspace_users(cli, client, workspace).await
        }
        AdminCommand::ListGitConnections => workspaces::list_git_connections(cli, client).await,
        AdminCommand::GrantAdminAccess { workspace } => {
            workspaces::grant_admin_access(cli, client, workspace).await
        }
        AdminCommand::RemoveAdminAccess { workspace } => {
            workspaces::remove_admin_access(cli, client, workspace).await
        }
        AdminCommand::RestoreWorkspace {
            workspace,
            name,
            capacity_id,
        } => workspaces::restore_workspace(cli, client, workspace, name, capacity_id).await,
        AdminCommand::ListNetworkPolicies => workspaces::list_network_policies(cli, client).await,
        // Items
        AdminCommand::ListItems => items::list_items(cli, client).await,
        AdminCommand::ShowItem { workspace, item_id } => {
            items::show_item(cli, client, workspace, item_id).await
        }
        AdminCommand::ListItemUsers { workspace, item_id } => {
            items::list_item_users(cli, client, workspace, item_id).await
        }
        AdminCommand::BulkSetLabels { file, content } => {
            items::bulk_set_labels(cli, client, file.as_deref(), content.as_deref()).await
        }
        AdminCommand::BulkRemoveLabels { file, content } => {
            items::bulk_remove_labels(cli, client, file.as_deref(), content.as_deref()).await
        }
        AdminCommand::ListExternalDataShares => items::list_external_data_shares(cli, client).await,
        AdminCommand::RevokeExternalDataShare {
            workspace,
            item_id,
            share_id,
        } => items::revoke_external_data_share(cli, client, workspace, item_id, share_id).await,
        AdminCommand::RemoveAllSharingLinks { file, content } => {
            items::remove_all_sharing_links(cli, client, file.as_deref(), content.as_deref()).await
        }
        AdminCommand::BulkRemoveSharingLinks { file, content } => {
            items::bulk_remove_sharing_links(cli, client, file.as_deref(), content.as_deref()).await
        }
        // Domains
        AdminCommand::ListDomains => domains::list_domains(cli, client).await,
        AdminCommand::CreateDomain {
            name,
            description,
            parent_id,
        } => {
            domains::create_domain(
                cli,
                client,
                name,
                description.as_deref(),
                parent_id.as_deref(),
            )
            .await
        }
        AdminCommand::ShowDomain { domain_id } => {
            domains::show_domain(cli, client, domain_id).await
        }
        AdminCommand::UpdateDomain {
            domain_id,
            name,
            description,
        } => {
            domains::update_domain(
                cli,
                client,
                domain_id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        AdminCommand::DeleteDomain { domain_id } => {
            domains::delete_domain(cli, client, domain_id).await
        }
        AdminCommand::ListDomainWorkspaces { domain_id } => {
            domains::list_domain_workspaces(cli, client, domain_id).await
        }
        AdminCommand::AssignDomainWorkspaces {
            domain_id,
            workspace_ids,
        } => domains::assign_domain_workspaces(cli, client, domain_id, workspace_ids).await,
        AdminCommand::UnassignDomainWorkspaces {
            domain_id,
            workspace_ids,
        } => domains::unassign_domain_workspaces(cli, client, domain_id, workspace_ids).await,
        AdminCommand::UnassignAllDomainWorkspaces { domain_id } => {
            domains::unassign_all_domain_workspaces(cli, client, domain_id).await
        }
        AdminCommand::ListDomainRoleAssignments { domain_id } => {
            domains::list_domain_role_assignments(cli, client, domain_id).await
        }
        AdminCommand::BulkAssignDomainRoles {
            domain_id,
            file,
            content,
        } => {
            domains::bulk_assign_domain_roles(
                cli,
                client,
                domain_id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        AdminCommand::BulkUnassignDomainRoles {
            domain_id,
            file,
            content,
        } => {
            domains::bulk_unassign_domain_roles(
                cli,
                client,
                domain_id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        AdminCommand::SyncDomainRolesToSubdomains { domain_id, role } => {
            domains::sync_domain_roles_to_subdomains(cli, client, domain_id, role).await
        }
        AdminCommand::AssignDomainWorkspacesByCapacities {
            domain_id,
            capacity_ids,
        } => {
            domains::assign_domain_workspaces_by_capacities(cli, client, domain_id, capacity_ids)
                .await
        }
        AdminCommand::AssignDomainWorkspacesByPrincipals {
            domain_id,
            principal_ids,
            principal_type,
        } => {
            domains::assign_domain_workspaces_by_principals(
                cli,
                client,
                domain_id,
                principal_ids,
                principal_type,
            )
            .await
        }
        // Users
        AdminCommand::ListUserAccess { user_id } => {
            users::list_user_access(cli, client, user_id).await
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub(super) fn read_body(file: Option<&str>, content: Option<&str>, command: &str) -> Result<Value> {
    let raw = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                format!("Example: fabio admin {command} --file body.json"),
            )
            .into());
        }
    };
    let body: Value =
        serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("Invalid JSON in input: {e}"))?;
    Ok(body)
}
