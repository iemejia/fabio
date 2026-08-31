use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum WarehouseSnapshotCommand {
    /// List warehouse snapshots in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a warehouse snapshot
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse snapshot ID
        #[arg(long)]
        id: String,
    },
    /// Create a new warehouse snapshot
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name
        #[arg(long)]
        name: String,

        /// Source warehouse ID to snapshot
        #[arg(long)]
        warehouse_id: String,

        /// Point-in-time for the snapshot in UTC (`YYYY-MM-DDTHH:mm:ssZ`); omit for the current time
        #[arg(long)]
        snapshot_datetime: Option<String>,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update warehouse snapshot properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse snapshot ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a warehouse snapshot
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse snapshot ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &WarehouseSnapshotCommand,
) -> Result<()> {
    match command {
        WarehouseSnapshotCommand::List { workspace } => list(cli, client, workspace).await,
        WarehouseSnapshotCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        WarehouseSnapshotCommand::Create {
            workspace,
            name,
            warehouse_id,
            snapshot_datetime,
            description,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                warehouse_id,
                snapshot_datetime.as_deref(),
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        WarehouseSnapshotCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            update(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        WarehouseSnapshotCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "warehouseSnapshots",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "warehouseSnapshots", workspace, id).await
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    warehouse_id: &str,
    snapshot_datetime: Option<&str>,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    // The creationPayload field is `parentWarehouseId` (NOT `warehouseId`, which
    // the API rejects with `NotGenericWarehouseArtifact`). `snapshotDateTime`
    // (UTC `YYYY-MM-DDTHH:mm:ssZ`) selects a point-in-time snapshot; omitted =
    // current time.
    let mut creation_payload = serde_json::json!({ "parentWarehouseId": warehouse_id });
    if let Some(dt) = snapshot_datetime {
        creation_payload["snapshotDateTime"] = Value::from(dt);
    }
    let mut body = serde_json::json!({
        "displayName": name,
        "creationPayload": creation_payload
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(
        cli,
        "warehouse-snapshot create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "parentWarehouseId": warehouse_id,
            "snapshotDateTime": snapshot_datetime,
            "description": description,
            "sensitivityLabel": sensitivity_label
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/warehouseSnapshots"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse-snapshot create", "Member"))?;
    if data.get("id").and_then(Value::as_str).is_some() {
        output::render_object(cli, &data, "id");
    } else {
        // A 202 async create may return an empty body — surface a useful status.
        output::render_object(
            cli,
            &serde_json::json!({ "displayName": name, "status": "created" }),
            "status",
        );
    }
    Ok(())
}

async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio warehouse-snapshot update --workspace <WS> --id <ID> --name \"New Name\""
                .to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::from(n);
    }
    if let Some(d) = description {
        body["description"] = Value::from(d);
    }

    if output::dry_run_guard(cli, "warehouse-snapshot update", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/warehouseSnapshots/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse-snapshot update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    crate::commands::crud::delete(
        cli,
        client,
        "warehouse-snapshot",
        "warehouseSnapshots",
        "Member",
        workspace,
        id,
        hard_delete,
    )
    .await
}
