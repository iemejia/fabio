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
pub enum DigitalTwinBuilderFlowCommand {
    /// List Digital Twin Builder flows in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a Digital Twin Builder flow
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder flow ID
        #[arg(long)]
        id: String,
    },
    /// Create a new Digital Twin Builder flow
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Display name (letters, numbers, underscores only — no hyphens)
        #[arg(long)]
        name: String,
        /// Parent Digital Twin Builder item ID (required)
        #[arg(long)]
        dtb_id: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update Digital Twin Builder flow properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder flow ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a Digital Twin Builder flow
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder flow ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a Digital Twin Builder flow
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder flow ID
        #[arg(long)]
        id: String,
        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a Digital Twin Builder flow
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder flow ID
        #[arg(long)]
        id: String,
        /// Path to definition file
        #[arg(long)]
        file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &DigitalTwinBuilderFlowCommand,
) -> Result<()> {
    match command {
        DigitalTwinBuilderFlowCommand::List { workspace } => list(cli, client, workspace).await,
        DigitalTwinBuilderFlowCommand::Show { workspace, id } => {
            show(cli, client, workspace, id).await
        }
        DigitalTwinBuilderFlowCommand::Create {
            workspace,
            name,
            dtb_id,
            description,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                dtb_id,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        DigitalTwinBuilderFlowCommand::Update {
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
        DigitalTwinBuilderFlowCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        DigitalTwinBuilderFlowCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        DigitalTwinBuilderFlowCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "digitalTwinBuilderFlows",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/digitalTwinBuilderFlows/{id}"
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
    dtb_id: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
        "creationPayload": {
            "digitalTwinBuilderItemReference": {
                "referenceType": "ById",
                "itemId": dtb_id,
                "workspaceId": workspace
            }
        }
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
        "digital-twin-builder-flow create",
        &serde_json::json!({ "workspace": workspace, "displayName": name, "dtbId": dtb_id, "description": description , "sensitivityLabel": sensitivity_label }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/digitalTwinBuilderFlows"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "digital-twin-builder-flow create", "Contributor"))?;
    output::render_object(cli, &data, "id");
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
            "Example: fabio digital-twin-builder-flow update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
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
    if output::dry_run_guard(cli, "digital-twin-builder-flow update", &body) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!("/workspaces/{workspace}/digitalTwinBuilderFlows/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "digital-twin-builder-flow update", "Contributor"))?;
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
    if output::dry_run_guard(
        cli,
        "digital-twin-builder-flow delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }
    let url = if hard_delete {
        format!("/workspaces/{workspace}/digitalTwinBuilderFlows/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/digitalTwinBuilderFlows/{id}")
    };
    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "digital-twin-builder-flow delete", "Contributor"))?;
    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/digitalTwinBuilderFlows/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "digital-twin-builder-flow get-definition", "Contributor")
        })?;
    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let script = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio digital-twin-builder-flow update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            )
            .into());
        }
    };
    let body = crate::definition_spec::build_update_definition_body(&script, "definition.json");
    if output::dry_run_guard(
        cli,
        "digital-twin-builder-flow update-definition",
        &serde_json::json!({ "workspace": workspace, "id": id, "contentLength": script.len() }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/digitalTwinBuilderFlows/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "digital-twin-builder-flow update-definition",
                "Contributor",
            )
        })?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}
