use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before creating items, run: fabio context schema MirroredDatabase\nReturns the definition template with required fields and format."
)]
pub enum MirroredDatabaseCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List mirrored databases in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a mirrored database
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored database ID
        #[arg(long)]
        id: String,
    },
    /// Create a new mirrored database
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,

        /// Configure as an OPEN mirrored database (push-based; source type `GenericMirror`).
        /// Push data files to the landing zone (see `mirrored-database landing-zone`) — no source connection needed.
        #[arg(long = "open-mirroring")]
        open_mirroring: bool,

        /// Default schema for the mirrored tables (open mirroring; default: dbo)
        #[arg(long = "default-schema", default_value = "dbo")]
        default_schema: String,
    },
    /// Print the `OneLake` landing-zone URL of an OPEN mirrored database (push data files here)
    #[command(display_order = 3)]
    LandingZone {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored database ID
        #[arg(long)]
        id: String,
    },
    /// Update mirrored database properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored database ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a mirrored database
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored database ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a mirrored database
    #[command(display_order = 10)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored database ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a mirrored database
    #[command(display_order = 11)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored database ID
        #[arg(long)]
        id: String,

        /// Definition as JSON file path
        #[arg(long)]
        file: Option<String>,

        /// Definition as inline JSON
        #[arg(long)]
        content: Option<String>,
    },

    // ── Mirroring control ────────────────────────────────────────────────
    /// Start mirroring
    #[command(display_order = 20)]
    Start {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored database ID
        #[arg(long)]
        id: String,
    },
    /// Stop mirroring
    #[command(display_order = 21)]
    Stop {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored database ID
        #[arg(long)]
        id: String,
    },
    /// Get mirroring status
    #[command(display_order = 22)]
    Status {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored database ID
        #[arg(long)]
        id: String,
    },
    /// Get tables mirroring status
    #[command(display_order = 23)]
    TableStatus {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored database ID
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &MirroredDatabaseCommand,
) -> Result<()> {
    match command {
        MirroredDatabaseCommand::List { workspace } => list(cli, client, workspace).await,
        MirroredDatabaseCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        MirroredDatabaseCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
            open_mirroring,
            default_schema,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
                *open_mirroring,
                default_schema,
            )
            .await
        }
        MirroredDatabaseCommand::LandingZone { workspace, id } => {
            landing_zone(cli, client, workspace, id);
            Ok(())
        }
        MirroredDatabaseCommand::Update {
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
        MirroredDatabaseCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        MirroredDatabaseCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        MirroredDatabaseCommand::UpdateDefinition {
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
        MirroredDatabaseCommand::Start { workspace, id } => start(cli, client, workspace, id).await,
        MirroredDatabaseCommand::Stop { workspace, id } => stop(cli, client, workspace, id).await,
        MirroredDatabaseCommand::Status { workspace, id } => {
            status(cli, client, workspace, id).await
        }
        MirroredDatabaseCommand::TableStatus { workspace, id } => {
            table_status(cli, client, workspace, id).await
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/mirroredDatabases"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_item_list(
        cli,
        &resp.items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/mirroredDatabases/{id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
    open_mirroring: bool,
    default_schema: &str,
) -> Result<()> {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(cli, "mirrored-database create", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/mirroredDatabases"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-database create", "Member"))?;

    // Open mirroring: configure the push-based `GenericMirror` definition so the
    // item gets a landing zone (an empty create leaves it MirroringDefinitionMissing).
    if open_mirroring && let Some(id) = data.get("id").and_then(Value::as_str) {
        let mir = open_mirroring_definition(default_schema);
        let payload = BASE64.encode(serde_json::to_vec(&mir).unwrap_or_default());
        let def_body = serde_json::json!({
            "definition": { "parts": [{
                "path": "mirroring.json",
                "payload": payload,
                "payloadType": "InlineBase64"
            }]}
        });
        let url = format!("/workspaces/{workspace}/mirroredDatabases/{id}/updateDefinition");
        // A freshly-created mirrored database is briefly not ready
        // (`MirroredDatabaseNotReady`); retry the definition push a few times.
        let mut last_err = None;
        for attempt in 0..6 {
            match client.post(&url, &def_body, true).await {
                Ok(_) => {
                    last_err = None;
                    break;
                }
                Err(e) => {
                    if attempt < 5 && e.to_string().contains("MirroredDatabaseNotReady") {
                        tokio::time::sleep(std::time::Duration::from_secs(12)).await;
                        last_err = Some(e);
                    } else {
                        last_err = Some(e);
                        break;
                    }
                }
            }
        }
        if let Some(e) = last_err {
            return Err(enrich_forbidden(
                e,
                "mirrored-database create --open-mirroring",
                "Member",
            ));
        }
    }

    output::render_object(cli, &data, "id");
    Ok(())
}

/// The push-based open-mirroring `mirroring.json` definition (source `GenericMirror`,
/// no connection; target `MountedRelationalDatabase` Delta).
fn open_mirroring_definition(default_schema: &str) -> Value {
    serde_json::json!({
        "properties": {
            "source": { "type": "GenericMirror", "typeProperties": {} },
            "target": {
                "type": "MountedRelationalDatabase",
                "typeProperties": { "defaultSchema": default_schema, "format": "Delta" }
            }
        }
    })
}

/// Print the `OneLake` landing-zone URL for an OPEN mirrored database. Data files
/// (a numbered `<n>.parquet` per table + a `_metadata.json` with `keyColumns`)
/// are pushed under `Files/LandingZone/<TableName>/`.
fn landing_zone(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) {
    let url = client.onelake_dfs_item_url(workspace, &format!("{id}/Files/LandingZone"));
    let obj = serde_json::json!({
        "landingZoneUrl": url,
        "note": "Open mirroring: push a numbered <n>.parquet (20-digit, zero-padded) per table under Files/LandingZone/<TableName>/, plus a _metadata.json ({\"keyColumns\":[...]}) in that folder. Then: fabio mirrored-database start.",
        "uploadExample": format!("fabio lakehouse upload --workspace {workspace} --id {id} --source-path ./00000000000000000001.parquet --dest-path Files/LandingZone/<Table>/00000000000000000001.parquet")
    });
    output::render_object(cli, &obj, "landingZoneUrl");
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
            "Example: fabio mirrored-database update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "mirrored-database update", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/mirroredDatabases/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-database update", "Contributor"))?;
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
        "mirrored-database delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/mirroredDatabases/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/mirroredDatabases/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-database delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/mirroredDatabases/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-database get-definition", "Contributor"))?;
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
    let definition_json = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio mirrored-database update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            ).into());
        }
    };

    let body =
        crate::definition_spec::build_update_definition_body(&definition_json, "mirroring.json");

    if output::dry_run_guard(
        cli,
        "mirrored-database update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": definition_json.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/mirroredDatabases/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-database update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Mirroring control ───────────────────────────────────────────────────────

async fn start(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "mirrored-database start",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/mirroredDatabases/{id}/startMirroring"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-database start", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "mirroring_started" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn stop(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "mirrored-database stop",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/mirroredDatabases/{id}/stopMirroring"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-database stop", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "mirroring_stopped" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn status(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    // getMirroringStatus is a POST action (NOT a GET).
    let data = client
        .post(
            &format!("/workspaces/{workspace}/mirroredDatabases/{id}/getMirroringStatus"),
            &serde_json::json!({}),
            false,
        )
        .await?;
    output::render_object(cli, &data, "status");
    Ok(())
}

async fn table_status(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    // getTablesMirroringStatus is a POST action (NOT a GET). It returns a
    // paginated per-table list: {continuationToken, data:[{sourceSchemaName,
    // sourceTableName, status, metrics:{processedBytes, processedRows,
    // lastSyncDateTime}}]}. Render the per-table entries as a list so agents can
    // filter (e.g. --query "[?status!='Replicating']").
    let data = client
        .post(
            &format!("/workspaces/{workspace}/mirroredDatabases/{id}/getTablesMirroringStatus"),
            &serde_json::json!({}),
            false,
        )
        .await?;
    if let Some(tables) = data.get("data").and_then(Value::as_array) {
        let token = data
            .get("continuationToken")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        output::render_list_with_token(
            cli,
            tables,
            &["sourceSchemaName", "sourceTableName", "status"],
            &["SCHEMA", "TABLE", "STATUS"],
            "sourceTableName",
            token,
        );
    } else {
        output::render_object(cli, &data, "data");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::open_mirroring_definition;

    #[test]
    fn open_mirroring_definition_is_generic_mirror_with_target_schema() {
        let d = open_mirroring_definition("sales");
        assert_eq!(d["properties"]["source"]["type"], "GenericMirror");
        // Push-based: no source connection/typeProperties keys.
        assert!(
            d["properties"]["source"]["typeProperties"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            d["properties"]["target"]["type"],
            "MountedRelationalDatabase"
        );
        assert_eq!(
            d["properties"]["target"]["typeProperties"]["defaultSchema"],
            "sales"
        );
        assert_eq!(
            d["properties"]["target"]["typeProperties"]["format"],
            "Delta"
        );
    }
}
