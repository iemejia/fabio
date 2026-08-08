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
pub enum MirroredDatabricksCatalogCommand {
    /// List mirrored Azure Databricks catalogs in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a mirrored Azure Databricks catalog
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Databricks catalog ID
        #[arg(long)]
        id: String,
    },
    /// Create a new mirrored Azure Databricks catalog
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

        /// Azure Databricks workspace connection ID (`creationPayload.databricksWorkspaceConnectionId`).
        /// Required to actually MIRROR a catalog — without it, an empty shell is created.
        #[arg(long, requires = "catalog_name", requires = "mirroring_mode")]
        databricks_connection_id: Option<String>,

        /// Unity Catalog name to mirror (`creationPayload.catalogName`).
        #[arg(long)]
        catalog_name: Option<String>,

        /// Mirroring mode: `Full` (all tables, auto-sync new ones) or `Partial` (selected tables).
        #[arg(long, value_parser = ["Full", "Partial"])]
        mirroring_mode: Option<String>,

        /// Optional storage connection ID (`creationPayload.storageConnectionId`).
        #[arg(long)]
        storage_connection_id: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update mirrored Databricks catalog properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Databricks catalog ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a mirrored Azure Databricks catalog
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Databricks catalog ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a mirrored Databricks catalog
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Databricks catalog ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a mirrored Databricks catalog
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Databricks catalog ID
        #[arg(long)]
        id: String,

        /// Path to definition file
        #[arg(long)]
        file: Option<String>,

        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
    /// Refresh catalog metadata
    #[command(display_order = 10)]
    RefreshMetadata {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Databricks catalog ID
        #[arg(long)]
        id: String,
    },
    /// Discover available Databricks catalogs (workspace-level)
    #[command(display_order = 11)]
    DiscoverCatalogs {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Databricks workspace connection ID (required).
        #[arg(long)]
        connection_id: String,
    },
    /// Discover schemas in a Databricks catalog
    #[command(display_order = 12)]
    DiscoverSchemas {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Catalog name
        #[arg(long)]
        catalog_name: String,

        /// Databricks workspace connection ID (required).
        #[arg(long)]
        connection_id: String,
    },
    /// Discover tables in a Databricks catalog schema
    #[command(display_order = 13)]
    DiscoverTables {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Catalog name
        #[arg(long)]
        catalog_name: String,

        /// Schema name
        #[arg(long)]
        schema_name: String,

        /// Databricks workspace connection ID (required).
        #[arg(long)]
        connection_id: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &MirroredDatabricksCatalogCommand,
) -> Result<()> {
    match command {
        MirroredDatabricksCatalogCommand::List { workspace } => list(cli, client, workspace).await,
        MirroredDatabricksCatalogCommand::Show { workspace, id } => {
            show(cli, client, workspace, id).await
        }
        MirroredDatabricksCatalogCommand::Create {
            workspace,
            name,
            description,
            databricks_connection_id,
            catalog_name,
            mirroring_mode,
            storage_connection_id,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                CreatePayload {
                    databricks_connection_id: databricks_connection_id.as_deref(),
                    catalog_name: catalog_name.as_deref(),
                    mirroring_mode: mirroring_mode.as_deref(),
                    storage_connection_id: storage_connection_id.as_deref(),
                },
                sensitivity_label.as_deref(),
            )
            .await
        }
        MirroredDatabricksCatalogCommand::Update {
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
        MirroredDatabricksCatalogCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        MirroredDatabricksCatalogCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        MirroredDatabricksCatalogCommand::UpdateDefinition {
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
        MirroredDatabricksCatalogCommand::RefreshMetadata { workspace, id } => {
            refresh_metadata(cli, client, workspace, id).await
        }
        MirroredDatabricksCatalogCommand::DiscoverCatalogs {
            workspace,
            connection_id,
        } => discover_catalogs(cli, client, workspace, connection_id).await,
        MirroredDatabricksCatalogCommand::DiscoverSchemas {
            workspace,
            catalog_name,
            connection_id,
        } => discover_schemas(cli, client, workspace, catalog_name, connection_id).await,
        MirroredDatabricksCatalogCommand::DiscoverTables {
            workspace,
            catalog_name,
            schema_name,
            connection_id,
        } => {
            discover_tables(
                cli,
                client,
                workspace,
                catalog_name,
                schema_name,
                connection_id,
            )
            .await
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/mirroredAzureDatabricksCatalogs"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    let has_labels = resp
        .items
        .iter()
        .any(|item| item.get("sensitivityLabel").is_some_and(|v| !v.is_null()));
    let has_tags = output::has_tags(&resp.items);

    let display_items;
    let items_ref: &[Value] = if has_tags {
        display_items = output::enrich_with_tags_display(&resp.items);
        &display_items
    } else {
        &resp.items
    };

    match (has_labels, has_tags) {
        (true, true) => output::render_list_with_token(
            cli,
            items_ref,
            &[
                "displayName",
                "id",
                "description",
                "sensitivityLabel.id",
                "_tagsDisplay",
            ],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (true, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "sensitivityLabel.id"],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, true) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "_tagsDisplay"],
            &["NAME", "ID", "DESCRIPTION", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description"],
            &["NAME", "ID", "DESCRIPTION"],
            "id",
            resp.continuation_token.as_deref(),
        ),
    }
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/mirroredAzureDatabricksCatalogs/{id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

/// Optional `creationPayload` fields for creating an actual mirror (all three of
/// `databricks_connection_id`/`catalog_name`/`mirroring_mode` required together).
struct CreatePayload<'a> {
    databricks_connection_id: Option<&'a str>,
    catalog_name: Option<&'a str>,
    mirroring_mode: Option<&'a str>,
    storage_connection_id: Option<&'a str>,
}

#[allow(clippy::too_many_lines)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    payload: CreatePayload<'_>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    // A creationPayload (catalog + Databricks connection + mirroring mode) makes
    // this an ACTUAL mirror; without it the API creates an empty shell.
    if let (Some(conn), Some(cat), Some(mode)) = (
        payload.databricks_connection_id,
        payload.catalog_name,
        payload.mirroring_mode,
    ) {
        let mut cp = serde_json::json!({
            "catalogName": cat,
            "databricksWorkspaceConnectionId": conn,
            "mirroringMode": mode,
        });
        if let Some(sc) = payload.storage_connection_id {
            cp["storageConnectionId"] = Value::from(sc);
        }
        body["creationPayload"] = cp;
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(
        cli,
        "mirrored-databricks-catalog create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "catalogName": payload.catalog_name,
            "databricksWorkspaceConnectionId": payload.databricks_connection_id,
            "mirroringMode": payload.mirroring_mode,
            "sensitivityLabel": sensitivity_label
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/mirroredAzureDatabricksCatalogs"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-databricks-catalog create", "Member"))?;
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
            "Example: fabio mirrored-databricks-catalog update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
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

    if output::dry_run_guard(cli, "mirrored-databricks-catalog update", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/mirroredAzureDatabricksCatalogs/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-databricks-catalog update", "Contributor"))?;
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
        "mirrored-databricks-catalog delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/mirroredAzureDatabricksCatalogs/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/mirroredAzureDatabricksCatalogs/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-databricks-catalog delete", "Member"))?;

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
            &format!("/workspaces/{workspace}/mirroredAzureDatabricksCatalogs/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "mirrored-databricks-catalog get-definition",
                "Contributor",
            )
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
    let definition_json = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio mirrored-databricks-catalog update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            ).into());
        }
    };

    let body =
        crate::definition_spec::build_update_definition_body(&definition_json, "mirroring.json");

    if output::dry_run_guard(
        cli,
        "mirrored-databricks-catalog update-definition",
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
            &format!(
                "/workspaces/{workspace}/mirroredAzureDatabricksCatalogs/{id}/updateDefinition"
            ),
            &body,
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "mirrored-databricks-catalog update-definition",
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

// ─── Extra operations ────────────────────────────────────────────────────────

async fn refresh_metadata(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "mirrored-databricks-catalog refresh-metadata",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/mirroredAzureDatabricksCatalogs/{id}/refreshCatalogMetadata"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "mirrored-databricks-catalog refresh-metadata",
                "Contributor",
            )
        })?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "refresh_triggered" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

async fn discover_catalogs(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    connection_id: &str,
) -> Result<()> {
    // `databricksWorkspaceConnectionId` is a REQUIRED query param.
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/azureDatabricks/catalogs?databricksWorkspaceConnectionId={connection_id}"
        ))
        .await?;
    output::render_object(cli, &data, "data");
    Ok(())
}

async fn discover_schemas(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    catalog_name: &str,
    connection_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/azureDatabricks/catalogs/{catalog_name}/schemas?databricksWorkspaceConnectionId={connection_id}"
        ))
        .await?;
    output::render_object(cli, &data, "data");
    Ok(())
}

async fn discover_tables(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    catalog_name: &str,
    schema_name: &str,
    connection_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/azureDatabricks/catalogs/{catalog_name}/schemas/{schema_name}/tables?databricksWorkspaceConnectionId={connection_id}"
        ))
        .await?;
    output::render_object(cli, &data, "data");
    Ok(())
}
