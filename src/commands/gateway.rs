use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples gateway\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum GatewayCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List all gateways
    #[command(display_order = 1)]
    List,
    /// Show details of a gateway
    #[command(display_order = 2)]
    Show {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,
    },
    /// Create a new gateway (`VirtualNetwork` type)
    #[command(display_order = 3)]
    Create {
        /// Display name (max 200 characters)
        #[arg(long)]
        name: String,

        /// Capacity ID for the gateway
        #[arg(long, env = "FABIO_CAPACITY")]
        capacity_id: String,

        /// Azure subscription ID containing the virtual network
        #[arg(long)]
        subscription_id: String,

        /// Resource group name containing the virtual network
        #[arg(long)]
        resource_group: String,

        /// Virtual network name
        #[arg(long)]
        vnet_name: String,

        /// Subnet name (must be delegated to Microsoft.PowerPlatform/vnetaccesslinks)
        #[arg(long)]
        subnet: String,

        /// Minutes of inactivity before auto-sleep (30, 60, 90, 120, 150, 240, 360, 480, 720, 1440)
        #[arg(long, default_value = "120")]
        inactivity_minutes: i64,

        /// Number of gateway members (1-9)
        #[arg(long, default_value = "1")]
        member_count: i64,
    },
    /// Create a new streaming virtual network gateway
    #[command(display_order = 3, name = "create-streaming")]
    CreateStreaming {
        /// Display name (max 200 characters)
        #[arg(long)]
        name: String,

        /// Azure subscription ID containing the virtual network
        #[arg(long)]
        subscription_id: String,

        /// Resource group name containing the virtual network
        #[arg(long)]
        resource_group: String,

        /// Virtual network name
        #[arg(long)]
        vnet_name: String,

        /// Subnet name (must be delegated to Microsoft.PowerPlatform/vnetaccesslinks)
        #[arg(long)]
        subnet: String,
    },
    /// Update gateway properties
    #[command(display_order = 4)]
    Update {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// Allow cloud connection refresh
        #[arg(long)]
        allow_cloud_connection_refresh: Option<bool>,

        /// Allow custom connectors
        #[arg(long)]
        allow_custom_connectors: Option<bool>,

        /// Load balancing setting (e.g., Failover, `RoundRobin`)
        #[arg(long)]
        load_balancing: Option<String>,
    },
    /// Delete a gateway
    #[command(display_order = 5)]
    Delete {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,
    },

    // ── Members ──────────────────────────────────────────────────────────
    /// List members of a gateway
    #[command(display_order = 10)]
    ListMembers {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,
    },
    /// Update a gateway member
    #[command(display_order = 11)]
    UpdateMember {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,

        /// Member ID
        #[arg(long)]
        member_id: String,

        /// New display name for the member
        #[arg(long)]
        display_name: Option<String>,

        /// Enable or disable the member
        #[arg(long)]
        enabled: Option<bool>,
    },
    /// Delete a gateway member
    #[command(display_order = 12)]
    DeleteMember {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,

        /// Member ID
        #[arg(long)]
        member_id: String,
    },

    // ── Role Assignments ─────────────────────────────────────────────────
    /// List role assignments for a gateway
    #[command(display_order = 20)]
    ListRoleAssignments {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,
    },
    /// Add a role assignment to a gateway
    #[command(display_order = 21)]
    AddRoleAssignment {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,

        /// Principal ID (user/group/service principal)
        #[arg(long)]
        principal_id: String,

        /// Principal type (User, Group, `ServicePrincipal`)
        #[arg(long)]
        principal_type: String,

        /// Role (Admin, `ConnectionCreator`, `ConnectionCreatorWithResharing`)
        #[arg(long)]
        role: String,
    },
    /// Show a specific role assignment
    #[command(display_order = 22)]
    ShowRoleAssignment {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,

        /// Role assignment ID
        #[arg(long)]
        assignment_id: String,
    },
    /// Update a role assignment
    #[command(display_order = 23)]
    UpdateRoleAssignment {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,

        /// Role assignment ID
        #[arg(long)]
        assignment_id: String,

        /// New role (Admin, `ConnectionCreator`, `ConnectionCreatorWithResharing`)
        #[arg(long)]
        role: String,
    },
    /// Delete a role assignment
    #[command(display_order = 24)]
    DeleteRoleAssignment {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,

        /// Role assignment ID
        #[arg(long)]
        assignment_id: String,
    },

    // ── Lifecycle ─────────────────────────────────────────────────────────
    /// Check the status of a gateway (`VNet` only)
    #[command(display_order = 30, name = "check-status")]
    CheckStatus {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,
    },
    /// Check the status of a gateway member (on-premises only)
    #[command(display_order = 31, name = "check-member-status")]
    CheckMemberStatus {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,

        /// Gateway member ID
        #[arg(long)]
        member_id: String,
    },
    /// Restart a gateway (`VNet` only, LRO)
    #[command(display_order = 32)]
    Restart {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,
    },
    /// Shut down a gateway (`VNet` only, LRO)
    #[command(display_order = 33)]
    Shutdown {
        /// Gateway ID
        #[arg(short, long)]
        gateway: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &GatewayCommand) -> Result<()> {
    match command {
        GatewayCommand::List => list(cli, client).await,
        GatewayCommand::Show { gateway } => show(cli, client, gateway).await,
        GatewayCommand::Create {
            name,
            capacity_id,
            subscription_id,
            resource_group,
            vnet_name,
            subnet,
            inactivity_minutes,
            member_count,
        } => {
            create(
                cli,
                client,
                name,
                capacity_id,
                subscription_id,
                resource_group,
                vnet_name,
                subnet,
                *inactivity_minutes,
                *member_count,
            )
            .await
        }
        GatewayCommand::CreateStreaming {
            name,
            subscription_id,
            resource_group,
            vnet_name,
            subnet,
        } => {
            create_streaming(
                cli,
                client,
                name,
                subscription_id,
                resource_group,
                vnet_name,
                subnet,
            )
            .await
        }
        GatewayCommand::Update {
            gateway,
            name,
            allow_cloud_connection_refresh,
            allow_custom_connectors,
            load_balancing,
        } => {
            update(
                cli,
                client,
                gateway,
                name.as_deref(),
                *allow_cloud_connection_refresh,
                *allow_custom_connectors,
                load_balancing.as_deref(),
            )
            .await
        }
        GatewayCommand::Delete { gateway } => delete(cli, client, gateway).await,
        GatewayCommand::ListMembers { gateway } => list_members(cli, client, gateway).await,
        GatewayCommand::UpdateMember {
            gateway,
            member_id,
            display_name,
            enabled,
        } => {
            update_member(
                cli,
                client,
                gateway,
                member_id,
                display_name.as_deref(),
                *enabled,
            )
            .await
        }
        GatewayCommand::DeleteMember { gateway, member_id } => {
            delete_member(cli, client, gateway, member_id).await
        }
        GatewayCommand::ListRoleAssignments { gateway } => {
            list_role_assignments(cli, client, gateway).await
        }
        GatewayCommand::AddRoleAssignment {
            gateway,
            principal_id,
            principal_type,
            role,
        } => add_role_assignment(cli, client, gateway, principal_id, principal_type, role).await,
        GatewayCommand::ShowRoleAssignment {
            gateway,
            assignment_id,
        } => show_role_assignment(cli, client, gateway, assignment_id).await,
        GatewayCommand::UpdateRoleAssignment {
            gateway,
            assignment_id,
            role,
        } => update_role_assignment(cli, client, gateway, assignment_id, role).await,
        GatewayCommand::DeleteRoleAssignment {
            gateway,
            assignment_id,
        } => delete_role_assignment(cli, client, gateway, assignment_id).await,
        GatewayCommand::CheckStatus { gateway } => check_status(cli, client, gateway).await,
        GatewayCommand::CheckMemberStatus { gateway, member_id } => {
            check_member_status(cli, client, gateway, member_id).await
        }
        GatewayCommand::Restart { gateway } => restart(cli, client, gateway).await,
        GatewayCommand::Shutdown { gateway } => shutdown(cli, client, gateway).await,
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient) -> Result<()> {
    let resp = client
        .get_list(
            "/gateways",
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["displayName", "id", "type"],
        &["NAME", "ID", "TYPE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, gateway: &str) -> Result<()> {
    let data = client.get(&format!("/gateways/{gateway}")).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    name: &str,
    capacity_id: &str,
    subscription_id: &str,
    resource_group: &str,
    vnet_name: &str,
    subnet: &str,
    inactivity_minutes: i64,
    member_count: i64,
) -> Result<()> {
    let body = serde_json::json!({
        "type": "VirtualNetwork",
        "displayName": name,
        "capacityId": capacity_id,
        "virtualNetworkAzureResource": {
            "subscriptionId": subscription_id,
            "resourceGroupName": resource_group,
            "virtualNetworkName": vnet_name,
            "subnetName": subnet
        },
        "inactivityMinutesBeforeSleep": inactivity_minutes,
        "numberOfMemberGateways": member_count
    });

    if output::dry_run_guard(cli, "gateway create", &body) {
        return Ok(());
    }

    let data = client
        .post("/gateways", &body, false)
        .await
        .map_err(|e| enrich_forbidden(e, "gateway create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create_streaming(
    cli: &Cli,
    client: &FabricClient,
    name: &str,
    subscription_id: &str,
    resource_group: &str,
    vnet_name: &str,
    subnet: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "type": "StreamingVirtualNetwork",
        "displayName": name,
        "virtualNetworkAzureResource": {
            "subscriptionId": subscription_id,
            "resourceGroupName": resource_group,
            "virtualNetworkName": vnet_name,
            "subnetName": subnet
        }
    });

    if output::dry_run_guard(cli, "gateway create-streaming", &body) {
        return Ok(());
    }

    let data = client
        .post("/gateways", &body, false)
        .await
        .map_err(|e| enrich_forbidden(e, "gateway create-streaming", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update(
    cli: &Cli,
    client: &FabricClient,
    gateway: &str,
    name: Option<&str>,
    allow_cloud_connection_refresh: Option<bool>,
    allow_custom_connectors: Option<bool>,
    load_balancing: Option<&str>,
) -> Result<()> {
    if name.is_none()
        && allow_cloud_connection_refresh.is_none()
        && allow_custom_connectors.is_none()
        && load_balancing.is_none()
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one field must be provided for update".to_string(),
            "Options: --name, --allow-cloud-connection-refresh, --allow-custom-connectors, --load-balancing".to_string(),
        )
        .into());
    }

    // Determine gateway type via GET so the update body includes the required `type` field.
    let current = client.get(&format!("/gateways/{gateway}")).await?;
    let gw_type = current["type"]
        .as_str()
        .unwrap_or("VirtualNetwork")
        .to_string();

    let mut body = serde_json::json!({ "type": gw_type });
    if let Some(n) = name {
        body["displayName"] = Value::from(n);
    }
    if let Some(v) = allow_cloud_connection_refresh {
        body["allowCloudConnectionRefresh"] = Value::Bool(v);
    }
    if let Some(v) = allow_custom_connectors {
        body["allowCustomConnectors"] = Value::Bool(v);
    }
    if let Some(lb) = load_balancing {
        body["loadBalancingSetting"] = Value::from(lb);
    }

    if output::dry_run_guard(cli, "gateway update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/gateways/{gateway}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "gateway update", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(cli: &Cli, client: &FabricClient, gateway: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "gateway delete",
        &serde_json::json!({ "gatewayId": gateway }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!("/gateways/{gateway}"))
        .await
        .map_err(|e| enrich_forbidden(e, "gateway delete", "Admin"))?;

    let obj = serde_json::json!({ "id": gateway, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Members ─────────────────────────────────────────────────────────────────

async fn list_members(cli: &Cli, client: &FabricClient, gateway: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/gateways/{gateway}/members"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["displayName", "id", "enabled"],
        &["NAME", "ID", "ENABLED"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn update_member(
    cli: &Cli,
    client: &FabricClient,
    gateway: &str,
    member_id: &str,
    display_name: Option<&str>,
    enabled: Option<bool>,
) -> Result<()> {
    if display_name.is_none() && enabled.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least --display-name or --enabled must be provided".to_string(),
            "Example: fabio gateway update-member --gateway <ID> --member-id <MID> --enabled true"
                .to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(n) = display_name {
        body["displayName"] = Value::from(n);
    }
    if let Some(e) = enabled {
        body["enabled"] = Value::Bool(e);
    }

    if output::dry_run_guard(cli, "gateway update-member", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/gateways/{gateway}/members/{member_id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "gateway update-member", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete_member(
    cli: &Cli,
    client: &FabricClient,
    gateway: &str,
    member_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "gateway delete-member",
        &serde_json::json!({ "gatewayId": gateway, "memberId": member_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!("/gateways/{gateway}/members/{member_id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "gateway delete-member", "Admin"))?;

    let obj = serde_json::json!({ "id": member_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Role Assignments ────────────────────────────────────────────────────────

async fn list_role_assignments(cli: &Cli, client: &FabricClient, gateway: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/gateways/{gateway}/roleAssignments"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "principalId", "role"],
        &["ID", "PRINCIPAL_ID", "ROLE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn add_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    gateway: &str,
    principal_id: &str,
    principal_type: &str,
    role: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "principal": {
            "id": principal_id,
            "type": principal_type
        },
        "role": role
    });

    if output::dry_run_guard(cli, "gateway add-role-assignment", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/gateways/{gateway}/roleAssignments"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "gateway add-role-assignment", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn show_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    gateway: &str,
    assignment_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/gateways/{gateway}/roleAssignments/{assignment_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    gateway: &str,
    assignment_id: &str,
    role: &str,
) -> Result<()> {
    let body = serde_json::json!({ "role": role });

    if output::dry_run_guard(cli, "gateway update-role-assignment", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/gateways/{gateway}/roleAssignments/{assignment_id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "gateway update-role-assignment", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    gateway: &str,
    assignment_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "gateway delete-role-assignment",
        &serde_json::json!({ "gatewayId": gateway, "assignmentId": assignment_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/gateways/{gateway}/roleAssignments/{assignment_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "gateway delete-role-assignment", "Admin"))?;

    let obj = serde_json::json!({ "id": assignment_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Lifecycle ────────────────────────────────────────────────────────────────

fn build_check_status_url(gateway: &str) -> String {
    format!("/gateways/{gateway}/checkStatus")
}

fn build_check_member_status_url(gateway: &str, member_id: &str) -> String {
    format!("/gateways/{gateway}/members/{member_id}/checkStatus")
}

fn build_restart_url(gateway: &str) -> String {
    format!("/gateways/{gateway}/restart")
}

fn build_shutdown_url(gateway: &str) -> String {
    format!("/gateways/{gateway}/shutdown")
}

async fn check_status(cli: &Cli, client: &FabricClient, gateway: &str) -> Result<()> {
    let data = client
        .get(&build_check_status_url(gateway))
        .await
        .map_err(|e| enrich_forbidden(e, "gateway check-status", "Admin"))?;
    output::render_object(cli, &data, "status");
    Ok(())
}

async fn check_member_status(
    cli: &Cli,
    client: &FabricClient,
    gateway: &str,
    member_id: &str,
) -> Result<()> {
    let data = client
        .get(&build_check_member_status_url(gateway, member_id))
        .await
        .map_err(|e| enrich_forbidden(e, "gateway check-member-status", "Admin"))?;
    output::render_object(cli, &data, "status");
    Ok(())
}

async fn restart(cli: &Cli, client: &FabricClient, gateway: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "gateway restart",
        &serde_json::json!({ "gatewayId": gateway }),
    ) {
        return Ok(());
    }

    let data = client
        .post(&build_restart_url(gateway), &serde_json::json!({}), true)
        .await
        .map_err(|e| enrich_forbidden(e, "gateway restart", "Admin"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": gateway, "status": "restarted" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "status");
    }
    Ok(())
}

async fn shutdown(cli: &Cli, client: &FabricClient, gateway: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "gateway shutdown",
        &serde_json::json!({ "gatewayId": gateway }),
    ) {
        return Ok(());
    }

    let data = client
        .post(&build_shutdown_url(gateway), &serde_json::json!({}), true)
        .await
        .map_err(|e| enrich_forbidden(e, "gateway shutdown", "Admin"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": gateway, "status": "shutdown" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "status");
    }
    Ok(())
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn check_status_url() {
        assert_eq!(build_check_status_url("gw-1"), "/gateways/gw-1/checkStatus");
    }

    #[test]
    fn check_member_status_url() {
        assert_eq!(
            build_check_member_status_url("gw-1", "mem-2"),
            "/gateways/gw-1/members/mem-2/checkStatus"
        );
    }

    #[test]
    fn restart_url() {
        assert_eq!(build_restart_url("gw-1"), "/gateways/gw-1/restart");
    }

    #[test]
    fn shutdown_url() {
        assert_eq!(build_shutdown_url("gw-1"), "/gateways/gw-1/shutdown");
    }
}
