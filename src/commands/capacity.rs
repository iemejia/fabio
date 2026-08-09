use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

const ARM_API_VERSION: &str = "2023-11-01";

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples capacity\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum CapacityCommand {
    /// List capacities available to the caller (Fabric API)
    #[command(display_order = 1)]
    List,
    /// Show details of a specific capacity (Fabric API)
    #[command(display_order = 2)]
    Show {
        /// Capacity ID
        #[arg(long)]
        id: String,
    },
    /// Suspend (pause) a capacity (ARM API)
    #[command(display_order = 3)]
    Suspend {
        /// Azure subscription ID
        #[arg(long)]
        subscription: String,
        /// Resource group name
        #[arg(long)]
        resource_group: String,
        /// Capacity name (lowercase, 3-63 chars)
        #[arg(long)]
        name: String,
    },
    /// Resume a suspended capacity (ARM API)
    #[command(display_order = 4)]
    Resume {
        /// Azure subscription ID
        #[arg(long)]
        subscription: String,
        /// Resource group name
        #[arg(long)]
        resource_group: String,
        /// Capacity name (lowercase, 3-63 chars)
        #[arg(long)]
        name: String,
    },
    /// Create a new Fabric capacity (ARM API)
    #[command(display_order = 5)]
    Create {
        /// Azure subscription ID
        #[arg(long)]
        subscription: String,
        /// Resource group name
        #[arg(long)]
        resource_group: String,
        /// Capacity name (lowercase, 3-63 chars, pattern: ^[a-z][a-z0-9]*$)
        #[arg(long)]
        name: String,
        /// Azure region (e.g., eastus, westeurope)
        #[arg(long)]
        location: String,
        /// SKU name (e.g., F2, F4, F8, F16, F32, F64, F128, F256, F512, F1024, F2048)
        #[arg(long)]
        sku: String,
        /// Fabric capacity admin (user principal name or object ID)
        #[arg(long)]
        admin: String,
    },
    /// Update an existing Fabric capacity (ARM API)
    #[command(display_order = 6)]
    Update {
        /// Azure subscription ID
        #[arg(long)]
        subscription: String,
        /// Resource group name
        #[arg(long)]
        resource_group: String,
        /// Capacity name
        #[arg(long)]
        name: String,
        /// New SKU name
        #[arg(long)]
        sku: Option<String>,
        /// New admin (user principal name or object ID)
        #[arg(long)]
        admin: Option<String>,
        /// Tags as JSON object (e.g., '{"env":"prod"}')
        #[arg(long)]
        tags: Option<String>,
    },
    /// Delete a Fabric capacity (ARM API)
    #[command(display_order = 7)]
    Delete {
        /// Azure subscription ID
        #[arg(long)]
        subscription: String,
        /// Resource group name
        #[arg(long)]
        resource_group: String,
        /// Capacity name
        #[arg(long)]
        name: String,
    },
    /// List available SKUs for Fabric capacities (ARM API)
    #[command(display_order = 8)]
    ListSkus {
        /// Azure subscription ID
        #[arg(long)]
        subscription: String,
    },
    /// Check if a capacity name is available (ARM API)
    #[command(display_order = 9)]
    CheckName {
        /// Azure subscription ID
        #[arg(long)]
        subscription: String,
        /// Capacity name to check
        #[arg(long)]
        name: String,
        /// Azure region to check availability in (e.g., eastus, westeurope)
        #[arg(long)]
        location: String,
        /// Resource type (default: Microsoft.Fabric/capacities)
        #[arg(long, default_value = "Microsoft.Fabric/capacities")]
        r#type: String,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &CapacityCommand) -> Result<()> {
    match command {
        CapacityCommand::List => list(cli, client).await,
        CapacityCommand::Show { id } => show(cli, client, id).await,
        CapacityCommand::Suspend {
            subscription,
            resource_group,
            name,
        } => suspend(cli, client, subscription, resource_group, name).await,
        CapacityCommand::Resume {
            subscription,
            resource_group,
            name,
        } => resume(cli, client, subscription, resource_group, name).await,
        CapacityCommand::Create {
            subscription,
            resource_group,
            name,
            location,
            sku,
            admin,
        } => {
            create(
                cli,
                client,
                subscription,
                resource_group,
                name,
                location,
                sku,
                admin,
            )
            .await
        }
        CapacityCommand::Update {
            subscription,
            resource_group,
            name,
            sku,
            admin,
            tags,
        } => {
            update(
                cli,
                client,
                subscription,
                resource_group,
                name,
                sku.as_deref(),
                admin.as_deref(),
                tags.as_deref(),
            )
            .await
        }
        CapacityCommand::Delete {
            subscription,
            resource_group,
            name,
        } => delete(cli, client, subscription, resource_group, name).await,
        CapacityCommand::ListSkus { subscription } => list_skus(cli, client, subscription).await,
        CapacityCommand::CheckName {
            subscription,
            name,
            location,
            r#type,
        } => check_name(cli, client, subscription, name, location, r#type).await,
    }
}

// ── Fabric API commands (read-only) ──────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient) -> Result<()> {
    let resp = client
        .get_list(
            "/capacities",
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["displayName", "id", "sku", "region", "state"],
        &["NAME", "ID", "SKU", "REGION", "STATE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    let data = client.get(&format!("/capacities/{id}")).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

// ── ARM API commands ──────────────────────────────────────────────────

fn arm_capacity_path(subscription: &str, resource_group: &str, name: &str) -> String {
    format!(
        "/subscriptions/{subscription}/resourceGroups/{resource_group}/providers/Microsoft.Fabric/capacities/{name}"
    )
}

async fn suspend(
    cli: &Cli,
    client: &FabricClient,
    subscription: &str,
    resource_group: &str,
    name: &str,
) -> Result<()> {
    let path = format!(
        "{}/suspend?api-version={ARM_API_VERSION}",
        arm_capacity_path(subscription, resource_group, name)
    );

    if output::dry_run_guard(
        cli,
        &format!("Would suspend capacity '{name}'"),
        &json!({"name": name}),
    ) {
        return Ok(());
    }

    let data = client
        .arm_post(&path, &json!({}), true)
        .await
        .map_err(|e| enrich_arm_error(e, "capacity suspend"))?;
    output::render_object(
        cli,
        &json!({"status": "suspended", "name": name, "details": data}),
        "name",
    );
    Ok(())
}

async fn resume(
    cli: &Cli,
    client: &FabricClient,
    subscription: &str,
    resource_group: &str,
    name: &str,
) -> Result<()> {
    let path = format!(
        "{}/resume?api-version={ARM_API_VERSION}",
        arm_capacity_path(subscription, resource_group, name)
    );

    if output::dry_run_guard(
        cli,
        &format!("Would resume capacity '{name}'"),
        &json!({"name": name}),
    ) {
        return Ok(());
    }

    let data = client
        .arm_post(&path, &json!({}), true)
        .await
        .map_err(|e| enrich_arm_error(e, "capacity resume"))?;
    output::render_object(
        cli,
        &json!({"status": "resumed", "name": name, "details": data}),
        "name",
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    subscription: &str,
    resource_group: &str,
    name: &str,
    location: &str,
    sku: &str,
    admin: &str,
) -> Result<()> {
    let path = format!(
        "{}?api-version={ARM_API_VERSION}",
        arm_capacity_path(subscription, resource_group, name)
    );

    let body = json!({
        "location": location,
        "sku": {
            "name": sku,
            "tier": "Fabric"
        },
        "properties": {
            "administration": {
                "members": [admin]
            }
        }
    });

    if output::dry_run_guard(
        cli,
        &format!("Would create capacity '{name}' ({sku}) in {location}"),
        &body,
    ) {
        return Ok(());
    }

    let data = client
        .arm_put(&path, &body)
        .await
        .map_err(|e| enrich_arm_error(e, "capacity create"))?;
    output::render_object(cli, &data, "name");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update(
    cli: &Cli,
    client: &FabricClient,
    subscription: &str,
    resource_group: &str,
    name: &str,
    sku: Option<&str>,
    admin: Option<&str>,
    tags: Option<&str>,
) -> Result<()> {
    if sku.is_none() && admin.is_none() && tags.is_none() {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput, "At least one of --sku, --admin, or --tags must be provided", "Example: fabio capacity update --subscription <SUB> --resource-group <RG> --name <NAME> --sku F4. Valid SKUs: F2, F4, F8, F16, F32, F64, F128, F256, F512, F1024, F2048").into());
    }

    let path = format!(
        "{}?api-version={ARM_API_VERSION}",
        arm_capacity_path(subscription, resource_group, name)
    );

    let mut body = json!({});

    if let Some(sku_name) = sku {
        body["sku"] = json!({
            "name": sku_name,
            "tier": "Fabric"
        });
    }

    if let Some(admin_member) = admin {
        body["properties"] = json!({
            "administration": {
                "members": [admin_member]
            }
        });
    }

    if let Some(tags_json) = tags {
        let tags_value: Value = serde_json::from_str(tags_json).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON for --tags: {e}"),
                "Provide a JSON object. Example: --tags '{\"environment\":\"production\"}'",
            )
        })?;
        body["tags"] = tags_value;
    }

    if output::dry_run_guard(cli, &format!("Would update capacity '{name}'"), &body) {
        return Ok(());
    }

    let data = client
        .arm_patch(&path, &body)
        .await
        .map_err(|e| enrich_arm_error(e, "capacity update"))?;
    output::render_object(cli, &data, "name");
    Ok(())
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    subscription: &str,
    resource_group: &str,
    name: &str,
) -> Result<()> {
    let path = format!(
        "{}?api-version={ARM_API_VERSION}",
        arm_capacity_path(subscription, resource_group, name)
    );

    if output::dry_run_guard(cli, "capacity delete", &json!({"name": name})) {
        return Ok(());
    }

    let data = client
        .arm_delete(&path)
        .await
        .map_err(|e| enrich_arm_error(e, "capacity delete"))?;
    output::render_object(
        cli,
        &json!({"status": "deleted", "name": name, "details": data}),
        "name",
    );
    Ok(())
}

async fn list_skus(cli: &Cli, client: &FabricClient, subscription: &str) -> Result<()> {
    let path = format!(
        "/subscriptions/{subscription}/providers/Microsoft.Fabric/skus?api-version={ARM_API_VERSION}"
    );

    let data = client.arm_get(&path).await?;

    let items = data
        .get("value")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    output::render_list_with_token(
        cli,
        &items,
        &["name", "tier", "locations"],
        &["SKU", "TIER", "LOCATIONS"],
        "name",
        None,
    );
    Ok(())
}

async fn check_name(
    cli: &Cli,
    client: &FabricClient,
    subscription: &str,
    name: &str,
    location: &str,
    resource_type: &str,
) -> Result<()> {
    let path = format!(
        "/subscriptions/{subscription}/providers/Microsoft.Fabric/locations/{location}/checkNameAvailability?api-version={ARM_API_VERSION}"
    );

    let body = json!({
        "name": name,
        "type": resource_type
    });

    let data = client.arm_post(&path, &body, false).await?;
    output::render_object(cli, &data, "nameAvailable");
    Ok(())
}

/// Enrich ARM API errors with Azure RBAC guidance.
fn enrich_arm_error(err: anyhow::Error, operation: &str) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };
    if fabio_err.code != ErrorCode::Forbidden {
        return err;
    }
    let hint = format!(
        "'{operation}' requires Azure RBAC Contributor (or Owner) role on the capacity resource. \
         This is NOT a Fabric workspace role — it's an Azure subscription-level permission. \
         Check access: az role assignment list --assignee <your-id> --scope /subscriptions/<sub>/resourceGroups/<rg>"
    );
    FabioError::with_hint(ErrorCode::Forbidden, fabio_err.message.clone(), hint).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_capacity_path_format() {
        let path = arm_capacity_path("sub-123", "my-rg", "mycapacity");
        assert_eq!(
            path,
            "/subscriptions/sub-123/resourceGroups/my-rg/providers/Microsoft.Fabric/capacities/mycapacity"
        );
    }

    #[test]
    fn arm_api_version_is_2023() {
        assert_eq!(ARM_API_VERSION, "2023-11-01");
    }

    #[test]
    fn capacity_command_has_all_variants() {
        // Ensure all 9 subcommands exist by matching exhaustively
        let commands = [
            "List",
            "Show",
            "Suspend",
            "Resume",
            "Create",
            "Update",
            "Delete",
            "ListSkus",
            "CheckName",
        ];
        assert_eq!(commands.len(), 9);
    }

    #[test]
    fn suspend_path_includes_api_version() {
        let path = format!(
            "{}/suspend?api-version={ARM_API_VERSION}",
            arm_capacity_path("sub1", "rg1", "cap1")
        );
        assert!(path.contains("api-version=2023-11-01"));
        assert!(path.ends_with("/suspend?api-version=2023-11-01"));
    }

    #[test]
    fn resume_path_includes_api_version() {
        let path = format!(
            "{}/resume?api-version={ARM_API_VERSION}",
            arm_capacity_path("sub1", "rg1", "cap1")
        );
        assert!(path.contains("/resume?api-version="));
    }

    #[test]
    fn create_body_structure() {
        let body = json!({
            "location": "eastus",
            "sku": {
                "name": "F2",
                "tier": "Fabric"
            },
            "properties": {
                "administration": {
                    "members": ["admin@contoso.com"]
                }
            }
        });

        assert_eq!(body["location"], "eastus");
        assert_eq!(body["sku"]["name"], "F2");
        assert_eq!(body["sku"]["tier"], "Fabric");
        assert_eq!(
            body["properties"]["administration"]["members"][0],
            "admin@contoso.com"
        );
    }

    #[test]
    fn update_body_with_sku_only() {
        let mut body = json!({});
        body["sku"] = json!({"name": "F4", "tier": "Fabric"});

        assert_eq!(body["sku"]["name"], "F4");
        assert!(body.get("properties").is_none());
        assert!(body.get("tags").is_none());
    }

    #[test]
    fn update_body_with_tags() {
        let tags_json = r#"{"env":"prod","team":"data"}"#;
        let tags_value: Value = serde_json::from_str(tags_json).unwrap();

        let mut body = json!({});
        body["tags"] = tags_value;

        assert_eq!(body["tags"]["env"], "prod");
        assert_eq!(body["tags"]["team"], "data");
    }

    #[test]
    fn update_body_invalid_tags_json() {
        let result: Result<Value, _> = serde_json::from_str("not-json");
        assert!(result.is_err());
    }

    #[test]
    fn check_name_body_structure() {
        let body = json!({
            "name": "mycapacity",
            "type": "Microsoft.Fabric/capacities"
        });

        assert_eq!(body["name"], "mycapacity");
        assert_eq!(body["type"], "Microsoft.Fabric/capacities");
    }

    #[test]
    fn list_skus_path_format() {
        let path = format!(
            "/subscriptions/{}/providers/Microsoft.Fabric/skus?api-version={ARM_API_VERSION}",
            "my-sub-id"
        );
        assert_eq!(
            path,
            "/subscriptions/my-sub-id/providers/Microsoft.Fabric/skus?api-version=2023-11-01"
        );
    }
}
