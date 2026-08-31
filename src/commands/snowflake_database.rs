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
pub enum SnowflakeDatabaseCommand {
    /// List Snowflake databases in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a Snowflake database
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Snowflake database ID
        #[arg(long)]
        id: String,
    },
    /// Create a new Snowflake database
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

        /// Connection ID of the Snowflake source connection (`creationPayload.connectionId`).
        /// The live API rejects a shell create without a connection — supply this to mirror a Snowflake source.
        #[arg(long)]
        connection_id: Option<String>,

        /// Source Snowflake database name to mirror (`creationPayload.snowflakeDatabaseName`).
        #[arg(long, requires = "connection_id")]
        snowflake_database_name: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update Snowflake database properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Snowflake database ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a Snowflake database
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Snowflake database ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a Snowflake database
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Snowflake database ID
        #[arg(long)]
        id: String,
        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a Snowflake database
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Snowflake database ID
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
    command: &SnowflakeDatabaseCommand,
) -> Result<()> {
    match command {
        SnowflakeDatabaseCommand::List { workspace } => list(cli, client, workspace).await,
        SnowflakeDatabaseCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        SnowflakeDatabaseCommand::Create {
            workspace,
            name,
            description,
            connection_id,
            snowflake_database_name,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                connection_id.as_deref(),
                snowflake_database_name.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        SnowflakeDatabaseCommand::Update {
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
        SnowflakeDatabaseCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        SnowflakeDatabaseCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        SnowflakeDatabaseCommand::UpdateDefinition {
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
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/snowflakeDatabases"),
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
        .get(&format!("/workspaces/{workspace}/snowflakeDatabases/{id}"))
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
    connection_id: Option<&str>,
    snowflake_database_name: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    // The live API rejects a shell create (`{displayName}`) with
    // `InvalidInput: Invalid payload` and requires a creationPayload naming the
    // Snowflake source connection — despite the swagger marking it optional and
    // shipping a `CreateSnowflakeDatabaseWithoutPayload` example.
    if let Some(conn) = connection_id {
        let mut payload = serde_json::json!({ "connectionId": conn });
        if let Some(db) = snowflake_database_name {
            payload["snowflakeDatabaseName"] = Value::from(db);
        }
        body["creationPayload"] = payload;
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(
        cli,
        "snowflake-database create",
        &serde_json::json!({ "workspace": workspace, "displayName": name, "description": description, "connectionId": connection_id, "snowflakeDatabaseName": snowflake_database_name, "sensitivityLabel": sensitivity_label }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/snowflakeDatabases"),
            &body,
            true,
        )
        .await
        .map_err(|e| {
            let enriched = enrich_forbidden(e, "snowflake-database create", "Contributor");
            // Teach the connection requirement when the API rejects a shell create.
            if enriched.to_string().contains("Invalid payload") && connection_id.is_none() {
                return FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "Snowflake database creation was rejected (Invalid payload)".to_string(),
                    "The live Fabric API requires a Snowflake source connection to create a Snowflake (mirrored) database. Pass --connection-id <id> (and optionally --snowflake-database-name <name>). Create the connection first with: fabio connection create --connection-type Snowflake ...".to_string(),
                )
                .into();
            }
            enriched
        })?;
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
            "Example: fabio snowflake-database update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
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
    if output::dry_run_guard(cli, "snowflake-database update", &body) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!("/workspaces/{workspace}/snowflakeDatabases/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "snowflake-database update", "Contributor"))?;
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
        "snowflake-database delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }
    let url = if hard_delete {
        format!("/workspaces/{workspace}/snowflakeDatabases/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/snowflakeDatabases/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "snowflake-database delete", "Contributor"))?;
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
            &format!("/workspaces/{workspace}/snowflakeDatabases/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "snowflake-database get-definition", "Contributor"))?;
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
                "Example: fabio snowflake-database update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            )
            .into());
        }
    };
    let body = crate::definition_spec::build_update_definition_body(&script, "definition.json");
    if output::dry_run_guard(
        cli,
        "snowflake-database update-definition",
        &serde_json::json!({ "workspace": workspace, "id": id, "contentLength": script.len() }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/snowflakeDatabases/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "snowflake-database update-definition", "Contributor"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}
