use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::enrich_forbidden;
use crate::output;

const COLLECTION: &str = "customStreamConnectors";
const FORMAT: &str = "CustomStreamConnectorDefinitionV1";
const PART: &str = "connector.json";

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum CustomStreamConnectorCommand {
    /// List Custom Stream Connectors in a workspace
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Only list connectors directly in the root folder
        #[arg(long)]
        no_recursive: bool,
        /// Filter to a specific root folder ID
        #[arg(long)]
        root_folder_id: Option<String>,
    },
    /// Show a Custom Stream Connector
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Custom Stream Connector ID
        #[arg(long)]
        id: String,
    },
    /// Create a Custom Stream Connector
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
        /// Folder ID (defaults to the workspace root)
        #[arg(long)]
        folder_id: Option<String>,
        /// Sensitivity label ID
        #[arg(long)]
        sensitivity_label: Option<String>,
        /// Sensitivity label application strategy
        #[arg(
            long,
            requires = "sensitivity_label",
            value_parser = ["ApplyOrFail", "Ignore"]
        )]
        sensitivity_label_apply_strategy: Option<String>,
        /// Definition file (raw connector.json or a definition envelope)
        #[arg(long, conflicts_with = "definition_content")]
        definition_file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        definition_content: Option<String>,
        /// Definition format
        #[arg(long, default_value = FORMAT)]
        definition_format: String,
    },
    /// Update Custom Stream Connector properties
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Custom Stream Connector ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a Custom Stream Connector
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Custom Stream Connector ID
        #[arg(long)]
        id: String,
    },
    /// Get a Custom Stream Connector definition
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Custom Stream Connector ID
        #[arg(long)]
        id: String,
        /// Definition format
        #[arg(long)]
        format: Option<String>,
        /// Decode base64 payloads inline
        #[arg(long)]
        decode: bool,
    },
    /// Replace a Custom Stream Connector definition
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Custom Stream Connector ID
        #[arg(long)]
        id: String,
        /// Definition file (raw connector.json or a definition envelope)
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
        /// Definition format
        #[arg(long, default_value = FORMAT)]
        format: String,
        /// Update item metadata from the .platform definition part
        #[arg(long)]
        update_metadata: bool,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &CustomStreamConnectorCommand,
) -> Result<()> {
    let workspace = match command {
        CustomStreamConnectorCommand::List { workspace, .. }
        | CustomStreamConnectorCommand::Show { workspace, .. }
        | CustomStreamConnectorCommand::Create { workspace, .. }
        | CustomStreamConnectorCommand::Update { workspace, .. }
        | CustomStreamConnectorCommand::Delete { workspace, .. }
        | CustomStreamConnectorCommand::GetDefinition { workspace, .. }
        | CustomStreamConnectorCommand::UpdateDefinition { workspace, .. } => workspace,
    };
    validate_uuid(workspace, "--workspace")?;
    if let CustomStreamConnectorCommand::Show { id, .. }
    | CustomStreamConnectorCommand::Update { id, .. }
    | CustomStreamConnectorCommand::Delete { id, .. }
    | CustomStreamConnectorCommand::GetDefinition { id, .. }
    | CustomStreamConnectorCommand::UpdateDefinition { id, .. } = command
    {
        validate_uuid(id, "--id")?;
    }
    match command {
        CustomStreamConnectorCommand::List {
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
        CustomStreamConnectorCommand::Show { workspace, id } => {
            crate::commands::crud::show(cli, client, COLLECTION, workspace, id).await
        }
        CustomStreamConnectorCommand::Create {
            workspace,
            name,
            description,
            folder_id,
            sensitivity_label,
            sensitivity_label_apply_strategy,
            definition_file,
            definition_content,
            definition_format,
        } => {
            let args = CreateArgs {
                workspace,
                name,
                description: description.as_deref(),
                folder_id: folder_id.as_deref(),
                sensitivity_label: sensitivity_label.as_deref(),
                sensitivity_label_apply_strategy: sensitivity_label_apply_strategy.as_deref(),
                definition_file: definition_file.as_deref(),
                definition_content: definition_content.as_deref(),
                definition_format,
            };
            create(cli, client, &args).await
        }
        CustomStreamConnectorCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            crate::commands::crud::update(
                cli,
                client,
                "custom-stream-connector",
                COLLECTION,
                "Contributor",
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        CustomStreamConnectorCommand::Delete { workspace, id } => {
            delete(cli, client, workspace, id).await
        }
        CustomStreamConnectorCommand::GetDefinition {
            workspace,
            id,
            format,
            decode,
        } => get_definition(cli, client, workspace, id, format.as_deref(), *decode).await,
        CustomStreamConnectorCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
            format,
            update_metadata,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
                format,
                *update_metadata,
            )
            .await
        }
    }
}

#[cfg(test)]
fn list_path(workspace: &str, no_recursive: bool, root_folder_id: Option<&str>) -> String {
    crate::commands::crud::item_list_path(COLLECTION, workspace, no_recursive, root_folder_id)
        .unwrap()
}

async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    no_recursive: bool,
    root_folder_id: Option<&str>,
) -> Result<()> {
    crate::commands::crud::list_with_options(
        cli,
        client,
        COLLECTION,
        workspace,
        no_recursive,
        root_folder_id,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

struct CreateArgs<'a> {
    workspace: &'a str,
    name: &'a str,
    description: Option<&'a str>,
    folder_id: Option<&'a str>,
    sensitivity_label: Option<&'a str>,
    sensitivity_label_apply_strategy: Option<&'a str>,
    definition_file: Option<&'a str>,
    definition_content: Option<&'a str>,
    definition_format: &'a str,
}

fn read_definition(file: Option<&str>, content: Option<&str>, part: &str) -> Result<Option<Value>> {
    let raw = match (file, content) {
        (Some(path), _) => Some(
            std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        ),
        (_, Some(value)) => Some(value.to_string()),
        (None, None) => None,
    };
    Ok(raw.map(|value| {
        crate::definition_spec::build_update_definition_body(&value, part)["definition"].clone()
    }))
}

fn build_create_body(args: &CreateArgs<'_>, definition: Option<Value>) -> Value {
    let mut body = json!({ "displayName": args.name });
    if let Some(value) = args.description {
        body["description"] = Value::from(value);
    }
    if let Some(value) = args.folder_id {
        body["folderId"] = Value::from(value);
    }
    if let Some(value) = args.sensitivity_label {
        let mut settings = json!({ "labelId": value });
        if let Some(strategy) = args.sensitivity_label_apply_strategy {
            settings["sensitivityLabelApplyStrategy"] = Value::from(strategy);
        }
        body["sensitivityLabelSettings"] = settings;
    }
    if let Some(mut value) = definition {
        value["format"] = Value::from(args.definition_format);
        body["definition"] = value;
    }
    body
}

async fn create(cli: &Cli, client: &FabricClient, args: &CreateArgs<'_>) -> Result<()> {
    validate_uuid(args.workspace, "--workspace")?;
    if let Some(folder) = args.folder_id {
        validate_uuid(folder, "--folder-id")?;
    }
    let definition = read_definition(args.definition_file, args.definition_content, PART)?;
    let body = build_create_body(args, definition);
    if output::dry_run_guard(
        cli,
        "custom-stream-connector create",
        &json!({ "workspace": args.workspace, "request": body }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{}/{COLLECTION}", args.workspace),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "custom-stream-connector create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(id, "--id")?;
    crate::commands::crud::delete(
        cli,
        client,
        "custom-stream-connector",
        COLLECTION,
        "Contributor",
        workspace,
        id,
        false,
    )
    .await
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<&str>,
    decode: bool,
) -> Result<()> {
    crate::commands::crud::get_definition_with_options(
        cli,
        client,
        "custom-stream-connector",
        COLLECTION,
        "Contributor",
        workspace,
        id,
        format,
        decode,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    format: &str,
    update_metadata: bool,
) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(id, "--id")?;
    crate::commands::crud::update_definition_with_options(
        cli,
        client,
        "custom-stream-connector",
        COLLECTION,
        "Contributor",
        PART,
        workspace,
        id,
        file,
        content,
        Some(format),
        update_metadata,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_body_serializes_all_optional_fields() {
        let args = CreateArgs {
            workspace: "workspace",
            name: "Connector",
            description: Some("Description"),
            folder_id: Some("folder"),
            sensitivity_label: Some("label"),
            sensitivity_label_apply_strategy: Some("Ignore"),
            definition_file: None,
            definition_content: None,
            definition_format: FORMAT,
        };
        let definition = json!({ "parts": [{ "path": PART, "payload": "e30=", "payloadType": "InlineBase64" }] });
        let body = build_create_body(&args, Some(definition));
        assert_eq!(body["folderId"], "folder");
        assert_eq!(body["sensitivityLabelSettings"]["labelId"], "label");
        assert_eq!(
            body["sensitivityLabelSettings"]["sensitivityLabelApplyStrategy"],
            "Ignore"
        );
        assert_eq!(body["definition"]["format"], FORMAT);
        assert_eq!(body["definition"]["parts"][0]["path"], PART);
    }

    #[test]
    fn list_path_serializes_folder_query_parameters() {
        assert_eq!(
            list_path("ws", true, Some("00000000-0000-0000-0000-000000000001")),
            "/workspaces/ws/customStreamConnectors?recursive=false&rootFolderId=00000000-0000-0000-0000-000000000001"
        );
    }
}
