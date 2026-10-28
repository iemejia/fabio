use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before creating deployment plans, run: fabio context schema DeploymentPlan\nReturns the definition template with required fields and format."
)]
pub enum DeploymentPlanCommand {
    /// List deployment plans in a workspace
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Only list plans directly in the root folder
        #[arg(long)]
        no_recursive: bool,
        /// Filter plans to a specific root folder ID
        #[arg(long)]
        root_folder_id: Option<String>,
    },
    /// Show a deployment plan
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Deployment plan ID
        #[arg(long)]
        id: String,
    },
    /// Create a deployment plan
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Deployment plan display name
        #[arg(long)]
        name: String,
        /// Optional description (maximum 256 characters)
        #[arg(long)]
        description: Option<String>,
        /// Folder ID to create the deployment plan in
        #[arg(long)]
        folder_id: Option<String>,
        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
        /// Path to a deployment plan definition or definition envelope
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        /// Inline deployment plan definition or definition envelope
        #[arg(long)]
        content: Option<String>,
    },
    /// Update deployment plan properties
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Deployment plan ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description (maximum 256 characters)
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a deployment plan
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Deployment plan ID
        #[arg(long)]
        id: String,
        /// Permanently delete the deployment plan
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get a deployment plan definition
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Deployment plan ID
        #[arg(long)]
        id: String,
        /// Definition format
        #[arg(long, value_parser = ["DeploymentPlanFormat"])]
        format: Option<String>,
        /// Decode base64 payloads inline
        #[arg(long)]
        decode: bool,
    },
    /// Replace a deployment plan definition
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Deployment plan ID
        #[arg(long)]
        id: String,
        /// Path to a deployment plan definition or definition envelope
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        /// Inline deployment plan definition or definition envelope
        #[arg(long)]
        content: Option<String>,
        /// Update item metadata from the definition
        #[arg(long)]
        update_metadata: bool,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &DeploymentPlanCommand,
) -> Result<()> {
    match command {
        DeploymentPlanCommand::List {
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
        DeploymentPlanCommand::Show { workspace, id } => {
            crate::commands::crud::show(cli, client, "deploymentPlans", workspace, id).await
        }
        DeploymentPlanCommand::Create {
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
        DeploymentPlanCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            crate::commands::crud::update(
                cli,
                client,
                "deployment-plan",
                "deploymentPlans",
                "Contributor",
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        DeploymentPlanCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => {
            crate::commands::crud::delete(
                cli,
                client,
                "deployment-plan",
                "deploymentPlans",
                "Contributor",
                workspace,
                id,
                *hard_delete,
            )
            .await
        }
        DeploymentPlanCommand::GetDefinition {
            workspace,
            id,
            format,
            decode,
        } => get_definition(cli, client, workspace, id, format.as_deref(), *decode).await,
        DeploymentPlanCommand::UpdateDefinition {
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

async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    no_recursive: bool,
    root_folder_id: Option<&str>,
) -> Result<()> {
    let mut url = format!("/workspaces/{workspace}/deploymentPlans");
    let mut params = Vec::new();
    if no_recursive {
        params.push("recursive=false".to_string());
    }
    if let Some(folder_id) = root_folder_id {
        params.push(format!("rootFolderId={folder_id}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    let response = client
        .get_list(&url, "value", cli.all, cli.continuation_token.as_deref())
        .await?;
    output::render_item_list(
        cli,
        &response.items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
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
    raw.map(|value| {
        let body = crate::definition_spec::build_update_definition_body(&value, "plan.yml");
        Ok(body["definition"].clone())
    })
    .transpose()
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
    let mut body = serde_json::json!({"displayName": name});
    if let Some(value) = description {
        body["description"] = Value::from(value);
    }
    if let Some(value) = folder_id {
        body["folderId"] = Value::from(value);
    }
    if let Some(value) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({"sensitivityLabelId": value});
    }
    if let Some(value) = read_definition(file, content)? {
        body["definition"] = value;
    }
    if output::dry_run_guard(
        cli,
        "deployment-plan create",
        &serde_json::json!({"workspace": workspace, "request": body}),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/deploymentPlans"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-plan create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<&str>,
    decode: bool,
) -> Result<()> {
    let suffix = format.map_or_else(String::new, |value| format!("?format={value}"));
    let data = client
        .post(
            &format!("/workspaces/{workspace}/deploymentPlans/{id}/getDefinition{suffix}"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-plan get-definition", "Contributor"))?;
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
            "Example: fabio deployment-plan update-definition --workspace <WS> --id <ID> --file plan.yml",
        )
    })?;
    let body = serde_json::json!({"definition": definition});
    if output::dry_run_guard(
        cli,
        "deployment-plan update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "updateMetadata": update_metadata,
            "request": body
        }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/deploymentPlans/{id}/updateDefinition?updateMetadata={update_metadata}"
            ),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-plan update-definition", "Contributor"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        output::render_object(
            cli,
            &serde_json::json!({"id": id, "status": "definition_updated"}),
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
    fn raw_definition_uses_plan_yml_part() {
        let definition = read_definition(None, Some("executableFormat: DeploymentPlanV1"))
            .unwrap()
            .unwrap();
        assert_eq!(definition["parts"][0]["path"], "plan.yml");
        assert_eq!(definition["parts"][0]["payloadType"], "InlineBase64");
    }

    #[test]
    fn definition_envelope_is_preserved() {
        let input = serde_json::json!({
            "definition": {
                "format": "DeploymentPlanFormat",
                "parts": [{"path": "plan.yml", "payload": "e30=", "payloadType": "InlineBase64"}]
            }
        });
        let definition = read_definition(None, Some(&input.to_string()))
            .unwrap()
            .unwrap();
        assert_eq!(definition["format"], "DeploymentPlanFormat");
        assert_eq!(definition["parts"][0]["path"], "plan.yml");
    }
}
