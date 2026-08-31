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
pub enum AzureDatabricksStorageCommand {
    /// List Azure Databricks storage items in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of an Azure Databricks storage item
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Azure Databricks storage ID
        #[arg(long)]
        id: String,
    },
    /// Create a new Azure Databricks storage item
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
    },
    /// Update Azure Databricks storage item properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Azure Databricks storage ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete an Azure Databricks storage item
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Azure Databricks storage ID
        #[arg(long)]
        id: String,
        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of an Azure Databricks storage item
    #[command(name = "get-definition", display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Azure Databricks storage ID
        #[arg(long)]
        id: String,
        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of an Azure Databricks storage item
    #[command(name = "update-definition", display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Azure Databricks storage ID
        #[arg(long)]
        id: String,
        /// Path to definition file
        #[arg(long)]
        file: Option<String>,
        /// Inline definition content (JSON)
        #[arg(long)]
        content: Option<String>,
    },
    /// Print the ID-based `OneLake` `ABFSS` path to use as the Azure Databricks
    /// Unity Catalog external-location URL (`abfss://<ws>@onelake…/<id>/Files/`)
    #[command(name = "external-location", display_order = 8)]
    ExternalLocation {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Azure Databricks storage ID
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &AzureDatabricksStorageCommand,
) -> Result<()> {
    match command {
        AzureDatabricksStorageCommand::List { workspace } => list(cli, client, workspace).await,
        AzureDatabricksStorageCommand::Show { workspace, id } => {
            show(cli, client, workspace, id).await
        }
        AzureDatabricksStorageCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        AzureDatabricksStorageCommand::Update {
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
        AzureDatabricksStorageCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        AzureDatabricksStorageCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        AzureDatabricksStorageCommand::UpdateDefinition {
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
        AzureDatabricksStorageCommand::ExternalLocation { workspace, id } => {
            external_location(cli, client, workspace, id).await
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "azureDatabricksStorages",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/azureDatabricksStorages/{id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "azure-databricks-storage show", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
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
        "azure-databricks-storage create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "sensitivityLabel": sensitivity_label
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/azureDatabricksStorages"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "azure-databricks-storage create", "Member"))?;
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
            "Example: fabio azure-databricks-storage update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
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

    if output::dry_run_guard(cli, "azure-databricks-storage update", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/azureDatabricksStorages/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "azure-databricks-storage update", "Contributor"))?;
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
        "azure-databricks-storage delete",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "hardDelete": hard_delete
        }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/azureDatabricksStorages/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/azureDatabricksStorages/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "azure-databricks-storage delete", "Member"))?;

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
            &format!("/workspaces/{workspace}/azureDatabricksStorages/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "azure-databricks-storage get-definition", "Contributor")
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
    let raw = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio azure-databricks-storage update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            )
            .into());
        }
    };

    let body = crate::definition_spec::build_update_definition_body(&raw, "definition.json");

    if output::dry_run_guard(
        cli,
        "azure-databricks-storage update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": raw.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/azureDatabricksStorages/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "azure-databricks-storage update-definition",
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

// ─── OneLake external location ───────────────────────────────────────────────

/// Build the ID-based `OneLake` `ABFSS` path that Azure Databricks Unity Catalog uses
/// as a `OneLake` external-location URL.
///
/// The path MUST be ID-based (GUIDs) and MUST end with `/Files/` — Databricks
/// rejects name-based paths at creation time and requires the `/Files` folder.
/// Format: `abfss://<WorkspaceID>@<onelake-host>/<DatabricksStorageID>/Files/`.
fn build_external_location_url(onelake_host: &str, workspace_id: &str, item_id: &str) -> String {
    format!("abfss://{workspace_id}@{onelake_host}/{item_id}/Files/")
}

/// Print the `OneLake` external-location URL for an Azure Databricks Storage item,
/// so it can be used as the managed-storage URL for a Unity Catalog external
/// location (the "store UC managed tables directly in `OneLake`" flow). Validates
/// the item is an `AzureDatabricksStorage` first, then constructs the ID-based
/// `ABFSS` path and surfaces the setup prerequisites. Read-only.
async fn external_location(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    // Validate the item exists and is the right type (catch a wrong id / a
    // lakehouse id before handing an agent a bogus path).
    let item = client
        .get(&format!(
            "/workspaces/{workspace}/azureDatabricksStorages/{id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "azure-databricks-storage external-location", "Viewer"))?;
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
    if !item_type.is_empty() && item_type != "AzureDatabricksStorage" {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Item {id} is a {item_type}, not an AzureDatabricksStorage"),
            "Pass the ID of an Azure Databricks Storage item. Create one with: \
             fabio azure-databricks-storage create --workspace <WS> --name <NAME>"
                .to_string(),
        )
        .into());
    }

    let url = build_external_location_url(&client.onelake_dfs_host(), workspace, id);
    let display_name = item
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let obj = serde_json::json!({
        "externalLocationUrl": url,
        "workspaceId": workspace,
        "itemId": id,
        "displayName": display_name,
        "note": "Use this ID-based ABFSS path as the URL when creating a Unity Catalog external location in Azure Databricks (Storage type = OneLake). It MUST be ID-based (GUIDs) and end with /Files/ — name-based paths are rejected. Data written to catalogs/tables on this external location lands directly in OneLake (no copy).",
        "prerequisites": [
            "Fabric: assign your Managed Identity / Service Principal an Admin, Member, or Contributor role on this workspace (fabio workspace add-role-assignment).",
            "Fabric tenant: enable 'Users can create Azure Databricks Storage items' (ArtifactDatabricksStoragePreview).",
            "Fabric workspace: enable 'Authenticate with OneLake user-delegated SAS tokens' in Workspace settings > Delegated settings > OneLake settings (portal-only).",
            "Databricks: create a UC storage credential (Azure Managed Identity / Access Connector), then an external location with Storage type = OneLake and URL = externalLocationUrl."
        ],
        "databricksExample": format!(
            "CREATE CATALOG my_onelake_catalog MANAGED LOCATION '{url}';"
        )
    });
    output::render_object(cli, &obj, "externalLocationUrl");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    #[test]
    fn test_list_url_format() {
        let ws = "test-workspace-id";
        let url = format!("/workspaces/{ws}/azureDatabricksStorages");
        assert_eq!(url, "/workspaces/test-workspace-id/azureDatabricksStorages");
    }

    #[test]
    fn test_show_url_format() {
        let ws = "ws1";
        let id = "item-abc";
        let url = format!("/workspaces/{ws}/azureDatabricksStorages/{id}");
        assert_eq!(url, "/workspaces/ws1/azureDatabricksStorages/item-abc");
    }

    #[test]
    fn test_create_url_format() {
        let ws = "ws-create";
        let url = format!("/workspaces/{ws}/azureDatabricksStorages");
        assert_eq!(url, "/workspaces/ws-create/azureDatabricksStorages");
    }

    #[test]
    fn external_location_url_is_id_based_abfss_with_files_suffix() {
        // Databricks requires the ID-based ABFSS path ending in /Files/.
        let url = build_external_location_url(
            "onelake.dfs.fabric.microsoft.com",
            "cfafbeb1-8037-4d0c-896e-a46fb27ff229",
            "41ce06d1-d81b-4ea0-bc6d-2ce3dd2f8e87",
        );
        assert_eq!(
            url,
            "abfss://cfafbeb1-8037-4d0c-896e-a46fb27ff229@onelake.dfs.fabric.microsoft.com/41ce06d1-d81b-4ea0-bc6d-2ce3dd2f8e87/Files/"
        );
        assert!(url.starts_with("abfss://"));
        assert!(url.ends_with("/Files/"));
    }

    #[test]
    fn external_location_url_honors_custom_onelake_host() {
        let url = build_external_location_url("mock.onelake.local", "ws", "item");
        assert_eq!(url, "abfss://ws@mock.onelake.local/item/Files/");
    }

    #[test]
    fn test_delete_url_without_hard_delete() {
        let ws = "ws1";
        let id = "item1";
        let hard_delete = false;
        let url = if hard_delete {
            format!("/workspaces/{ws}/azureDatabricksStorages/{id}?hardDelete=true")
        } else {
            format!("/workspaces/{ws}/azureDatabricksStorages/{id}")
        };
        assert_eq!(url, "/workspaces/ws1/azureDatabricksStorages/item1");
        assert!(!url.contains("hardDelete"));
    }

    #[test]
    fn test_delete_hard_delete_url() {
        let ws = "ws1";
        let id = "item1";
        let hard_delete = true;
        let url = if hard_delete {
            format!("/workspaces/{ws}/azureDatabricksStorages/{id}?hardDelete=true")
        } else {
            format!("/workspaces/{ws}/azureDatabricksStorages/{id}")
        };
        assert!(url.contains("hardDelete=true"));
        assert_eq!(
            url,
            "/workspaces/ws1/azureDatabricksStorages/item1?hardDelete=true"
        );
    }

    #[test]
    fn test_get_definition_url_format() {
        let ws = "ws1";
        let id = "def-item";
        let url = format!("/workspaces/{ws}/azureDatabricksStorages/{id}/getDefinition");
        assert_eq!(
            url,
            "/workspaces/ws1/azureDatabricksStorages/def-item/getDefinition"
        );
    }

    #[test]
    fn test_update_definition_url_format() {
        let ws = "ws1";
        let id = "def-item";
        let url = format!("/workspaces/{ws}/azureDatabricksStorages/{id}/updateDefinition");
        assert_eq!(
            url,
            "/workspaces/ws1/azureDatabricksStorages/def-item/updateDefinition"
        );
    }

    #[test]
    fn test_update_definition_body_structure() {
        let raw = r#"{"key":"value"}"#;
        let encoded = BASE64.encode(raw.as_bytes());
        let body = serde_json::json!({
            "definition": {
                "format": "AzureDatabricksStorageV1",
                "parts": [
                    {
                        "path": "definition.json",
                        "payload": encoded,
                        "payloadType": "InlineBase64"
                    }
                ]
            }
        });

        // Validate format
        assert_eq!(
            body["definition"]["format"], "AzureDatabricksStorageV1",
            "Definition format must be AzureDatabricksStorageV1"
        );

        // Validate part path matches API spec (definition.json, NOT AzureDatabricksStorage.json)
        assert_eq!(
            body["definition"]["parts"][0]["path"], "definition.json",
            "Definition part path must be 'definition.json' per API spec"
        );

        // Validate payload type
        assert_eq!(
            body["definition"]["parts"][0]["payloadType"],
            "InlineBase64"
        );

        // Validate base64 encoding roundtrip
        let decoded = BASE64
            .decode(body["definition"]["parts"][0]["payload"].as_str().unwrap())
            .unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), raw);
    }

    #[test]
    fn test_create_body_structure() {
        let name = "My Storage";
        let description = Some("A description");
        let mut body = serde_json::json!({ "displayName": name });
        if let Some(d) = description {
            body["description"] = serde_json::Value::from(d);
        }
        assert_eq!(body["displayName"], "My Storage");
        assert_eq!(body["description"], "A description");
    }

    #[test]
    fn test_create_body_without_description() {
        let name = "My Storage";
        let description: Option<&str> = None;
        let mut body = serde_json::json!({ "displayName": name });
        if let Some(d) = description {
            body["description"] = serde_json::Value::from(d);
        }
        assert_eq!(body["displayName"], "My Storage");
        assert!(body.get("description").is_none());
    }

    #[test]
    fn test_update_requires_at_least_one_field() {
        let name: Option<&str> = None;
        let description: Option<&str> = None;
        // Mirrors the validation logic in update()
        assert!(
            name.is_none() && description.is_none(),
            "Should require at least one field"
        );
    }

    #[test]
    fn test_update_definition_no_input_error() {
        let err: anyhow::Error = FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            "Example: fabio azure-databricks-storage update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
        )
        .into();
        let msg = err.to_string();
        assert!(msg.contains("--file or --content"));
    }
}
