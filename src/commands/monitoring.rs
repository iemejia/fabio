use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

const COLLECTION: &str = "monitoringItems";
const DEFINITION_PART: &str = "MonitoringProperties.json";

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before creating items, run: fabio context schema Monitoring\nReturns the definition template, immutable settings, and format."
)]
pub enum MonitoringCommand {
    /// List Monitoring items in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Only list Monitoring items directly in the root folder
        #[arg(long)]
        no_recursive: bool,

        /// Filter Monitoring items to a specific root folder ID
        #[arg(long)]
        root_folder_id: Option<String>,
    },
    /// Show a Monitoring item
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Monitoring item ID
        #[arg(long)]
        id: String,
    },
    /// Create a Monitoring item
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name (2-128 ASCII letters, digits, underscores, periods, or hyphens)
        #[arg(long)]
        name: String,

        /// Optional description (maximum 256 characters)
        #[arg(long)]
        description: Option<String>,

        /// Folder ID (defaults to the workspace root)
        #[arg(long)]
        folder_id: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,

        /// Monitoring definition file (full definition JSON or raw MonitoringProperties.json)
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,

        /// Inline Monitoring definition (full definition JSON or raw MonitoringProperties.json)
        #[arg(long)]
        content: Option<String>,
    },
    /// Update Monitoring item properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Monitoring item ID
        #[arg(long)]
        id: String,

        /// New display name (maximum 128 characters)
        #[arg(long)]
        name: Option<String>,

        /// New description (maximum 256 characters)
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a Monitoring item
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Monitoring item ID
        #[arg(long)]
        id: String,
    },
    /// Get a Monitoring definition
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Monitoring item ID
        #[arg(long)]
        id: String,

        /// Definition format (currently `MonitoringV1`)
        #[arg(long)]
        format: Option<String>,

        /// Decode base64 payloads inline
        #[arg(long)]
        decode: bool,
    },
    /// Update a Monitoring definition
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Monitoring item ID
        #[arg(long)]
        id: String,

        /// Definition file (full definition JSON or raw MonitoringProperties.json)
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,

        /// Inline definition (full definition JSON or raw MonitoringProperties.json)
        #[arg(long)]
        content: Option<String>,

        /// Update item metadata from a supplied .platform definition part
        #[arg(long)]
        update_metadata: bool,
    },
    /// Enable data collection using the configured Monitoring definition
    #[command(display_order = 8)]
    EnableDataCollection {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Monitoring item ID
        #[arg(long)]
        id: String,
    },
    /// Disable data collection without changing the Monitoring definition
    #[command(display_order = 9)]
    DisableDataCollection {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Monitoring item ID
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &MonitoringCommand) -> Result<()> {
    match command {
        MonitoringCommand::List {
            workspace,
            no_recursive,
            root_folder_id,
        } => {
            list(
                cli,
                client,
                workspace,
                *no_recursive,
                root_folder_id.as_deref(),
            )
            .await
        }
        MonitoringCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        MonitoringCommand::Create {
            workspace,
            name,
            description,
            folder_id,
            sensitivity_label,
            file,
            content,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                folder_id.as_deref(),
                sensitivity_label.as_deref(),
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        MonitoringCommand::Update {
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
        MonitoringCommand::Delete { workspace, id } => delete(cli, client, workspace, id).await,
        MonitoringCommand::GetDefinition {
            workspace,
            id,
            format,
            decode,
        } => get_definition(cli, client, workspace, id, format.as_deref(), *decode).await,
        MonitoringCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
            update_metadata,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
                *update_metadata,
            )
            .await
        }
        MonitoringCommand::EnableDataCollection { workspace, id } => {
            set_data_collection(cli, client, workspace, id, true).await
        }
        MonitoringCommand::DisableDataCollection { workspace, id } => {
            set_data_collection(cli, client, workspace, id, false).await
        }
    }
}

fn list_path(workspace: &str, no_recursive: bool, root_folder_id: Option<&str>) -> String {
    let mut path = format!("/workspaces/{workspace}/{COLLECTION}");
    let mut params = Vec::new();
    if no_recursive {
        params.push("recursive=false".to_string());
    }
    if let Some(folder_id) = root_folder_id {
        params.push(format!("rootFolderId={folder_id}"));
    }
    if !params.is_empty() {
        path.push('?');
        path.push_str(&params.join("&"));
    }
    path
}

async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    no_recursive: bool,
    root_folder_id: Option<&str>,
) -> Result<()> {
    let response = client
        .get_list(
            &list_path(workspace, no_recursive, root_folder_id),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;
    output::render_item_list(
        cli,
        &response.items,
        &[
            "displayName",
            "id",
            "properties.dataCollectionState",
            "description",
        ],
        &["NAME", "ID", "DATA COLLECTION", "DESCRIPTION"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, COLLECTION, workspace, id).await
}

fn read_definition_input(file: Option<&str>, content: Option<&str>) -> Result<Option<Value>> {
    let Some(text) = (match (file, content) {
        (Some(path), _) => Some(
            std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        ),
        (_, Some(value)) => Some(value.to_string()),
        (None, None) => None,
    }) else {
        return Ok(None);
    };
    let body = crate::definition_spec::build_update_definition_body(&text, DEFINITION_PART);
    Ok(body.get("definition").cloned())
}

fn build_create_body(
    name: &str,
    description: Option<&str>,
    folder_id: Option<&str>,
    sensitivity_label: Option<&str>,
    definition: Option<Value>,
) -> Value {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(value) = description {
        body["description"] = Value::from(value);
    }
    if let Some(value) = folder_id {
        body["folderId"] = Value::from(value);
    }
    if let Some(value) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({ "sensitivityLabelId": value });
    }
    if let Some(value) = definition {
        body["definition"] = value;
    }
    body
}

fn validate_create_fields(name: &str, description: Option<&str>) -> Result<()> {
    let valid_name = (2..=128).contains(&name.len())
        && name.is_ascii()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        && !name.starts_with("__")
        && !name.ends_with("__");
    if !valid_name {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Monitoring display name must be 2-128 ASCII letters, digits, underscores, periods, or hyphens, with no spaces or leading/trailing double underscore",
            "Example: --name Monitoring_1",
        )
        .into());
    }
    validate_description(description)
}

fn validate_description(description: Option<&str>) -> Result<()> {
    if description.is_some_and(|value| value.chars().count() > 256) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Monitoring description must not exceed 256 characters",
            "Shorten the value passed to --description.",
        )
        .into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    folder_id: Option<&str>,
    sensitivity_label: Option<&str>,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    validate_create_fields(name, description)?;
    let definition = read_definition_input(file, content)?;
    let body = build_create_body(name, description, folder_id, sensitivity_label, definition);
    if output::dry_run_guard(cli, "monitoring create", &body) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/{COLLECTION}"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "monitoring create", "Contributor"))?;
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
    if name.is_some_and(|value| value.chars().count() > 128) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Monitoring display name must not exceed 128 characters",
            "Shorten the value passed to --name.",
        )
        .into());
    }
    validate_description(description)?;
    crate::commands::crud::update(
        cli,
        client,
        "monitoring",
        COLLECTION,
        "Contributor",
        workspace,
        id,
        name,
        description,
    )
    .await
}

async fn delete(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "monitoring delete",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }
    client
        .delete(&format!("/workspaces/{workspace}/{COLLECTION}/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "monitoring delete", "Contributor"))?;
    output::render_object(
        cli,
        &serde_json::json!({ "id": id, "status": "deleted" }),
        "status",
    );
    Ok(())
}

fn get_definition_path(workspace: &str, id: &str, format: Option<&str>) -> String {
    format.map_or_else(
        || format!("/workspaces/{workspace}/{COLLECTION}/{id}/getDefinition"),
        |value| format!("/workspaces/{workspace}/{COLLECTION}/{id}/getDefinition?format={value}"),
    )
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<&str>,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &get_definition_path(workspace, id, format),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "monitoring get-definition", "Contributor"))?;
    if decode {
        output::render_object(cli, &output::decode_definition_parts(data), "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

fn update_definition_path(workspace: &str, id: &str, update_metadata: bool) -> String {
    let base = format!("/workspaces/{workspace}/{COLLECTION}/{id}/updateDefinition");
    if update_metadata {
        format!("{base}?updateMetadata=true")
    } else {
        base
    }
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    update_metadata: bool,
) -> Result<()> {
    let body = read_definition_input(file, content)?.ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided",
            "Example: fabio monitoring update-definition --workspace <WS> --id <ID> --file MonitoringProperties.json",
        )
    })?;
    let request = serde_json::json!({ "definition": body });
    if output::dry_run_guard(
        cli,
        "monitoring update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "updateMetadata": update_metadata,
            "definition": request["definition"],
        }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &update_definition_path(workspace, id, update_metadata),
            &request,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "monitoring update-definition", "Contributor"))?;
    let result = if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        serde_json::json!({ "id": id, "status": "definition_updated" })
    } else {
        data
    };
    output::render_object(cli, &result, "status");
    Ok(())
}

async fn set_data_collection(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    enabled: bool,
) -> Result<()> {
    let (operation, action, status) = if enabled {
        (
            "monitoring enable-data-collection",
            "enableDataCollection",
            "enabled",
        )
    } else {
        (
            "monitoring disable-data-collection",
            "disableDataCollection",
            "disabled",
        )
    };
    if output::dry_run_guard(
        cli,
        operation,
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }
    client
        .post_empty(
            &format!("/workspaces/{workspace}/{COLLECTION}/{id}/{action}"),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, operation, "Admin"))?;
    output::render_object(
        cli,
        &serde_json::json!({ "id": id, "dataCollectionState": status }),
        "dataCollectionState",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_path_includes_folder_filters() {
        assert_eq!(
            list_path("ws", true, Some("folder")),
            "/workspaces/ws/monitoringItems?recursive=false&rootFolderId=folder"
        );
    }

    #[test]
    fn create_body_matches_spec_fields() {
        let body = build_create_body(
            "Monitoring_1",
            Some("description"),
            Some("folder"),
            Some("label"),
            Some(serde_json::json!({
                "format": "MonitoringV1",
                "parts": []
            })),
        );
        assert_eq!(body["displayName"], "Monitoring_1");
        assert_eq!(body["folderId"], "folder");
        assert_eq!(
            body["sensitivityLabelSettings"]["sensitivityLabelId"],
            "label"
        );
        assert_eq!(body["definition"]["format"], "MonitoringV1");
    }

    #[test]
    fn create_name_constraints_match_spec() {
        for valid in ["M1", "Monitoring_1", "a.b-c"] {
            assert!(validate_create_fields(valid, None).is_ok(), "{valid}");
        }
        for invalid in ["M", "has space", "__prefix", "suffix__", "mönitoring"] {
            assert!(validate_create_fields(invalid, None).is_err(), "{invalid}");
        }
    }

    #[test]
    fn description_length_is_enforced() {
        assert!(validate_description(Some(&"a".repeat(256))).is_ok());
        assert!(validate_description(Some(&"a".repeat(257))).is_err());
    }

    #[test]
    fn raw_definition_becomes_monitoring_part() {
        let definition =
            read_definition_input(None, Some(r#"{"operationsAgent":{"enabled":false}}"#))
                .unwrap()
                .unwrap();
        assert_eq!(definition["parts"][0]["path"], DEFINITION_PART);
        assert_eq!(definition["parts"][0]["payloadType"], "InlineBase64");
    }

    #[test]
    fn definition_paths_include_optional_query_parameters() {
        assert_eq!(
            get_definition_path("ws", "id", Some("MonitoringV1")),
            "/workspaces/ws/monitoringItems/id/getDefinition?format=MonitoringV1"
        );
        assert_eq!(
            update_definition_path("ws", "id", true),
            "/workspaces/ws/monitoringItems/id/updateDefinition?updateMetadata=true"
        );
    }
}
