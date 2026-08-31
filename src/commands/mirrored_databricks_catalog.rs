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
    /// Update mirrored Databricks catalog properties (name, description, auto-sync, mirroring mode)
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

        /// Enable/disable auto-sync of newly added Databricks tables. Set to
        /// `Enabled` to start replication — a freshly created mirror defaults to
        /// `Disabled` and never syncs until this is enabled.
        #[arg(long, value_parser = ["Enabled", "Disabled"])]
        auto_sync: Option<String>,

        /// Change the mirroring mode: `Full` (all tables), `Partial` (selected),
        /// or `Exclude` (all except selected).
        #[arg(long, value_parser = ["Full", "Partial", "Exclude"])]
        mirroring_mode: Option<String>,

        /// Storage connection ID for the mirror
        #[arg(long)]
        storage_connection_id: Option<String>,
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
            auto_sync,
            mirroring_mode,
            storage_connection_id,
        } => {
            update(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
                auto_sync.as_deref(),
                mirroring_mode.as_deref(),
                storage_connection_id.as_deref(),
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
    crate::commands::crud::list(
        cli,
        client,
        "mirroredAzureDatabricksCatalogs",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
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

/// Build the PATCH body for `update`. Property fields (`autoSync`,
/// `mirroringMode`, `storageConnectionId`) are nested under `properties`. When
/// any property is set the API REQUIRES a `displayName` in the body (a body
/// carrying `properties` without one is rejected with `Invalid Display Name: ''`),
/// so the caller must pass the resolved current name via `name`.
fn build_update_body(
    name: Option<&str>,
    description: Option<&str>,
    auto_sync: Option<&str>,
    mirroring_mode: Option<&str>,
    storage_connection_id: Option<&str>,
) -> Value {
    let mut body = serde_json::Map::new();
    if let Some(n) = name {
        body.insert("displayName".to_string(), Value::from(n));
    }
    if let Some(d) = description {
        body.insert("description".to_string(), Value::from(d));
    }
    let mut props = serde_json::Map::new();
    if let Some(a) = auto_sync {
        props.insert("autoSync".to_string(), Value::from(a));
    }
    if let Some(m) = mirroring_mode {
        props.insert("mirroringMode".to_string(), Value::from(m));
    }
    if let Some(s) = storage_connection_id {
        props.insert("storageConnectionId".to_string(), Value::from(s));
    }
    if !props.is_empty() {
        body.insert("properties".to_string(), Value::Object(props));
    }
    Value::Object(body)
}

#[allow(clippy::too_many_arguments)]
async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
    auto_sync: Option<&str>,
    mirroring_mode: Option<&str>,
    storage_connection_id: Option<&str>,
) -> Result<()> {
    let has_props =
        auto_sync.is_some() || mirroring_mode.is_some() || storage_connection_id.is_some();
    if name.is_none() && description.is_none() && !has_props {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name, --description, --auto-sync, --mirroring-mode, or --storage-connection-id must be provided".to_string(),
            "Example: fabio mirrored-databricks-catalog update --workspace <WS> --id <ID> --auto-sync Enabled".to_string(),
        )
        .into());
    }

    // The PATCH endpoint rejects a body carrying `properties` without a
    // `displayName` ("Invalid Display Name: ''"), so resolve the current display
    // name when properties are being updated but --name was not supplied.
    let resolved_name: Option<String> = if has_props && name.is_none() {
        let current = client
            .get(&format!(
                "/workspaces/{workspace}/mirroredAzureDatabricksCatalogs/{id}"
            ))
            .await
            .map_err(|e| {
                enrich_forbidden(e, "mirrored-databricks-catalog update", "Contributor")
            })?;
        current
            .get("displayName")
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        name.map(str::to_string)
    };

    let body = build_update_body(
        resolved_name.as_deref(),
        description,
        auto_sync,
        mirroring_mode,
        storage_connection_id,
    );

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

#[cfg(test)]
mod tests {
    use super::build_update_body;

    #[test]
    fn update_body_nests_properties_under_properties_key() {
        // Regression for the auto-sync gap: autoSync/mirroringMode/storageConnectionId
        // MUST be nested under `properties` (the UpdatePayload shape), not top-level.
        let body = build_update_body(Some("MyMirror"), None, Some("Enabled"), Some("Full"), None);
        assert_eq!(body["displayName"], "MyMirror");
        assert_eq!(body["properties"]["autoSync"], "Enabled");
        assert_eq!(body["properties"]["mirroringMode"], "Full");
        // No top-level autoSync leakage.
        assert!(body.get("autoSync").is_none());
    }

    #[test]
    fn update_body_includes_display_name_with_properties() {
        // The API rejects a body carrying `properties` without a displayName
        // ("Invalid Display Name: ''"), so the resolved name must be present.
        let body = build_update_body(Some("Name"), None, Some("Enabled"), None, None);
        assert_eq!(body["displayName"], "Name");
        assert_eq!(body["properties"]["autoSync"], "Enabled");
    }

    #[test]
    fn update_body_name_and_description_only_omits_properties() {
        let body = build_update_body(Some("N"), Some("D"), None, None, None);
        assert_eq!(body["displayName"], "N");
        assert_eq!(body["description"], "D");
        assert!(body.get("properties").is_none());
    }

    #[test]
    fn update_body_storage_connection_id_nested() {
        let body = build_update_body(Some("N"), None, None, None, Some("conn-123"));
        assert_eq!(body["properties"]["storageConnectionId"], "conn-123");
    }
}
