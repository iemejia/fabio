use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_forbidden;
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum ManagedPrivateEndpointCommand {
    /// List managed private endpoints in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a managed private endpoint
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Managed private endpoint ID
        #[arg(long)]
        id: String,
    },
    /// Create a managed private endpoint
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Endpoint name
        #[arg(long)]
        name: String,

        /// Target private link resource ID (ARM resource ID of the target)
        #[arg(long)]
        target_resource_id: String,

        /// Target sub-resource type (e.g., blob, sqlServer, dfs, queue)
        #[arg(long)]
        target_subresource_type: String,

        /// Optional request message for approval
        #[arg(long)]
        request_message: Option<String>,
    },
    /// Delete a managed private endpoint
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Managed private endpoint ID
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &ManagedPrivateEndpointCommand,
) -> Result<()> {
    match command {
        ManagedPrivateEndpointCommand::List { workspace } => list(cli, client, workspace).await,
        ManagedPrivateEndpointCommand::Show { workspace, id } => {
            show(cli, client, workspace, id).await
        }
        ManagedPrivateEndpointCommand::Create {
            workspace,
            name,
            target_resource_id,
            target_subresource_type,
            request_message,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                target_resource_id,
                target_subresource_type,
                request_message.as_deref(),
            )
            .await
        }
        ManagedPrivateEndpointCommand::Delete { workspace, id } => {
            delete(cli, client, workspace, id).await
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/managedPrivateEndpoints"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["name", "id", "provisioningState", "connectionState"],
        &["NAME", "ID", "PROVISIONING", "CONNECTION STATE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/managedPrivateEndpoints/{id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    target_resource_id: &str,
    target_subresource_type: &str,
    request_message: Option<&str>,
) -> Result<()> {
    // The CreateManagedPrivateEndpointRequest fields are
    // `targetPrivateLinkResourceId` (required) + `targetSubresourceType` — NOT
    // `privateLinkResourceId`/`groupId` (those are ignored, and the missing
    // required field fails the request).
    let mut body = serde_json::json!({
        "name": name,
        "targetPrivateLinkResourceId": target_resource_id,
        "targetSubresourceType": target_subresource_type
    });
    if let Some(msg) = request_message {
        body["requestMessage"] = Value::from(msg);
    }

    if output::dry_run_guard(cli, "managed-private-endpoint create", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/managedPrivateEndpoints"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "managed-private-endpoint create", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "managed-private-endpoint delete",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/managedPrivateEndpoints/{id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "managed-private-endpoint delete", "Admin"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}
