use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

const COLLECTION: &str = "shares";
const PART: &str = "share.json";

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum ShareCommand {
    /// List shares in a workspace
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Only list shares directly in the root folder
        #[arg(long)]
        no_recursive: bool,
        /// Filter to a specific root folder ID
        #[arg(long)]
        root_folder_id: Option<String>,
    },
    /// Show a share
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
    },
    /// Create a share
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
        /// Definition file (raw share.json or a definition envelope)
        #[arg(long, conflicts_with = "definition_content")]
        definition_file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        definition_content: Option<String>,
    },
    /// Update share properties
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// New sensitivity label ID
        #[arg(long)]
        sensitivity_label: Option<String>,
        /// Sensitivity label application strategy
        #[arg(
            long,
            requires = "sensitivity_label",
            value_parser = ["ApplyOrFail", "Ignore"]
        )]
        sensitivity_label_apply_strategy: Option<String>,
    },
    /// Delete a share
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
    },
    /// Get a share definition
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// Decode base64 payloads inline
        #[arg(long)]
        decode: bool,
    },
    /// Replace a share definition
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// Definition file (raw share.json or a definition envelope)
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
        /// Update item metadata from the .platform definition part
        #[arg(long)]
        update_metadata: bool,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &ShareCommand) -> Result<()> {
    match command {
        ShareCommand::List {
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
        ShareCommand::Show { workspace, id } => {
            crate::commands::crud::show(cli, client, COLLECTION, workspace, id).await
        }
        ShareCommand::Create {
            workspace,
            name,
            description,
            folder_id,
            sensitivity_label,
            sensitivity_label_apply_strategy,
            definition_file,
            definition_content,
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
            };
            create(cli, client, &args).await
        }
        ShareCommand::Update {
            workspace,
            id,
            name,
            description,
            sensitivity_label,
            sensitivity_label_apply_strategy,
        } => {
            update(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
                sensitivity_label.as_deref(),
                sensitivity_label_apply_strategy.as_deref(),
            )
            .await
        }
        ShareCommand::Delete { workspace, id } => delete(cli, client, workspace, id).await,
        ShareCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        ShareCommand::UpdateDefinition {
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
    }
}

fn list_path(workspace: &str, no_recursive: bool, root_folder_id: Option<&str>) -> String {
    let mut path = format!("/workspaces/{workspace}/{COLLECTION}");
    let mut params = Vec::new();
    if no_recursive {
        params.push("recursive=false".to_string());
    }
    if let Some(folder) = root_folder_id {
        params.push(format!("rootFolderId={folder}"));
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
        &["displayName", "id", "description", "properties.shareStatus"],
        &["NAME", "ID", "DESCRIPTION", "STATUS"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
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
}

fn read_definition(file: Option<&str>, content: Option<&str>) -> Result<Option<Value>> {
    let raw = match (file, content) {
        (Some(path), _) => Some(
            std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        ),
        (_, Some(value)) => Some(value.to_string()),
        (None, None) => None,
    };
    Ok(raw.map(|value| {
        crate::definition_spec::build_update_definition_body(&value, PART)["definition"].clone()
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
    if let Some(value) = definition {
        body["definition"] = value;
    }
    body
}

async fn create(cli: &Cli, client: &FabricClient, args: &CreateArgs<'_>) -> Result<()> {
    let body = build_create_body(
        args,
        read_definition(args.definition_file, args.definition_content)?,
    );
    if output::dry_run_guard(
        cli,
        "share create",
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
        .map_err(|e| enrich_forbidden(e, "share create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
    sensitivity_label_apply_strategy: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() && sensitivity_label.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one update field must be provided",
            "Provide --name, --description, or --sensitivity-label.",
        )
        .into());
    }
    let mut body = json!({});
    if let Some(value) = name {
        body["displayName"] = Value::from(value);
    }
    if let Some(value) = description {
        body["description"] = Value::from(value);
    }
    if let Some(value) = sensitivity_label {
        let mut settings = json!({ "labelId": value });
        if let Some(strategy) = sensitivity_label_apply_strategy {
            settings["sensitivityLabelApplyStrategy"] = Value::from(strategy);
        }
        body["sensitivityLabelSettings"] = settings;
    }
    if output::dry_run_guard(
        cli,
        "share update",
        &json!({ "workspace": workspace, "id": id, "request": body }),
    ) {
        return Ok(());
    }
    let data = client
        .patch(&format!("/workspaces/{workspace}/{COLLECTION}/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "share update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "share delete",
        &json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }
    client
        .delete(&format!("/workspaces/{workspace}/{COLLECTION}/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "share delete", "Contributor"))?;
    output::render_object(cli, &json!({ "id": id, "status": "deleted" }), "status");
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
        .post_readonly_with_lro(
            &format!("/workspaces/{workspace}/{COLLECTION}/{id}/getDefinition"),
            &json!({}),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "share get-definition", "Contributor"))?;
    let data = if decode {
        output::decode_definition_parts(data)
    } else {
        data
    };
    output::render_object(cli, &data, "definition");
    Ok(())
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
    let definition = read_definition(file, content)?.ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided",
            "Provide a raw share.json file or a complete definition envelope.",
        )
    })?;
    let body = json!({ "definition": definition });
    if output::dry_run_guard(
        cli,
        "share update-definition",
        &json!({ "workspace": workspace, "id": id, "updateMetadata": update_metadata }),
    ) {
        return Ok(());
    }
    let suffix = if update_metadata {
        "?updateMetadata=true"
    } else {
        ""
    };
    let data = client
        .post(
            &format!("/workspaces/{workspace}/{COLLECTION}/{id}/updateDefinition{suffix}"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "share update-definition", "Contributor"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        output::render_object(
            cli,
            &json!({ "id": id, "status": "definition_updated" }),
            "status",
        );
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_body_serializes_all_optional_fields() {
        let args = CreateArgs {
            workspace: "workspace",
            name: "Share",
            description: Some("Description"),
            folder_id: Some("folder"),
            sensitivity_label: Some("label"),
            sensitivity_label_apply_strategy: Some("ApplyOrFail"),
            definition_file: None,
            definition_content: None,
        };
        let definition = json!({ "parts": [{ "path": PART, "payload": "e30=", "payloadType": "InlineBase64" }] });
        let body = build_create_body(&args, Some(definition));
        assert_eq!(body["folderId"], "folder");
        assert_eq!(body["sensitivityLabelSettings"]["labelId"], "label");
        assert_eq!(
            body["sensitivityLabelSettings"]["sensitivityLabelApplyStrategy"],
            "ApplyOrFail"
        );
        assert_eq!(body["definition"]["parts"][0]["path"], PART);
    }

    #[test]
    fn list_path_serializes_folder_query_parameters() {
        assert_eq!(
            list_path("ws", true, Some("folder")),
            "/workspaces/ws/shares?recursive=false&rootFolderId=folder"
        );
    }
}
