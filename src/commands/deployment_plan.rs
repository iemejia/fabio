use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before creating deployment plans, run: fabio context schema DeploymentPlan\nReturns the definition template with required fields and format."
)]
pub enum DeploymentPlanCommand {
    /// Validate plan YAML or a definition envelope offline (does not execute actions)
    Validate {
        /// Raw plan.yml or definition-envelope file
        #[arg(long, required_unless_present = "content", conflicts_with = "content")]
        file: Option<String>,
        /// Inline plan YAML or definition envelope
        #[arg(long)]
        content: Option<String>,
        /// Optional local .platform source directory for item/type reference checks
        #[arg(long)]
        source: Option<std::path::PathBuf>,
        /// Treat warnings (including references absent from source) as failures
        #[arg(long)]
        strict: bool,
    },
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
    validate_scope(command)?;
    match command {
        DeploymentPlanCommand::Validate {
            file,
            content,
            source,
            strict,
        } => validate(
            cli,
            file.as_deref(),
            content.as_deref(),
            source.as_deref(),
            *strict,
        ),
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

fn validate_scope(command: &DeploymentPlanCommand) -> Result<()> {
    let (workspace, id) = match command {
        DeploymentPlanCommand::Validate { .. } => return Ok(()),
        DeploymentPlanCommand::List { workspace, .. }
        | DeploymentPlanCommand::Create { workspace, .. } => (workspace, None),
        DeploymentPlanCommand::Show { workspace, id }
        | DeploymentPlanCommand::Update { workspace, id, .. }
        | DeploymentPlanCommand::Delete { workspace, id, .. }
        | DeploymentPlanCommand::GetDefinition { workspace, id, .. }
        | DeploymentPlanCommand::UpdateDefinition { workspace, id, .. } => (workspace, Some(id)),
    };
    validate_uuid(workspace, "--workspace")?;
    if let Some(id) = id {
        validate_uuid(id, "--id")?;
    }
    Ok(())
}

fn validate(
    cli: &Cli,
    file: Option<&str>,
    content: Option<&str>,
    source: Option<&std::path::Path>,
    strict: bool,
) -> Result<()> {
    use base64::Engine as _;
    let definition = read_definition(file, content)?.ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Provide --file or --content",
            "Use: fabio deployment-plan validate --file plan.yml",
        )
    })?;
    let findings = crate::definition_spec::validate_definition(
        Some("DeploymentPlan"),
        &serde_json::json!({"definition": definition}),
    );
    let source = source
        .map(crate::commands::deploy::platform::parse_source_directory)
        .transpose()?;
    let catalog = source.as_ref().map(|source| {
        crate::deployment_plan::catalog(source.items.iter().filter_map(|item| {
            item.metadata
                .logical_id
                .as_deref()
                .map(|id| (id, item.metadata.item_type.as_str()))
        }))
    });
    let plan = definition["parts"]
        .as_array()
        .and_then(|parts| parts.iter().find(|part| part["path"] == "plan.yml"));
    let mut result = if let Some(plan) = plan
        && let Some(payload) = plan["payload"].as_str()
        && let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(payload)
    {
        crate::deployment_plan::validate(&decoded, catalog.as_ref())
    } else {
        crate::deployment_plan::Validation::default()
    };
    // Keep universal envelope errors and missing plan.yml errors in the same
    // report; avoid repeating semantic findings already produced by the core.
    for finding in findings {
        if finding.code == "INVALID_DEPLOYMENT_PLAN" || finding.code == "DEPLOYMENT_PLAN_SCOPE" {
            continue;
        }
        if finding.severity == crate::definition_spec::Severity::Error
            || finding.code == "MISSING_CANONICAL_PART"
        {
            result.errors.push(finding.message);
        } else {
            result.warnings.push(finding.message);
        }
    }
    if result.errors.len() > 100 || result.warnings.len() > 100 {
        result.truncated = true;
        result.errors.truncate(100);
        result.warnings.truncate(100);
    }
    result.valid = result.errors.is_empty() && (!strict || result.warnings.is_empty());
    let valid = result.valid;
    output::render_object(cli, &serde_json::to_value(&result)?, "valid");
    if !valid {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,
            "Deployment plan validation failed; inspect data.errors and data.warnings.",
            "Use the current template from 'fabio context schema DeploymentPlan'. --source checks a local catalog, not target permissions/readiness; --strict also rejects warnings.").into());
    }
    Ok(())
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
        (Some(path), _) => Some(std::fs::read_to_string(path).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read file '{path}': {e}"),
                "Provide a readable UTF-8 plan.yml or definition-envelope file.",
            )
        })?),
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
    use base64::Engine as _;

    use super::*;

    const PLAN_YAML: &str = "$schema: https://developer.microsoft.com/json-schemas/fabric/item/deploymentPlan/definition/plan/1.0.0/schema.json\r\nversion: 1.0.0\r\ngroups: []\r\n# Unicode: déploiement\r\n";

    #[test]
    fn delete_scope_rejects_path_injection() {
        let valid = "aaaaaaaa-1111-2222-3333-444444444444";
        for invalid in [
            "",
            "..",
            "../../../workspaces/other",
            "%2e%2e",
            "bad?id=other",
        ] {
            for (workspace, id) in [(invalid, valid), (valid, invalid)] {
                let command = DeploymentPlanCommand::Delete {
                    workspace: workspace.to_string(),
                    id: id.to_string(),
                    hard_delete: false,
                };
                let error = validate_scope(&command).unwrap_err();
                assert_eq!(
                    error.downcast_ref::<FabioError>().unwrap().code,
                    ErrorCode::InvalidInput
                );
            }
        }
        assert!(
            validate_scope(&DeploymentPlanCommand::Delete {
                workspace: valid.to_string(),
                id: valid.to_string(),
                hard_delete: false,
            })
            .is_ok()
        );
    }

    #[test]
    fn raw_definition_uses_plan_yml_part() {
        let definition = read_definition(None, Some(PLAN_YAML)).unwrap().unwrap();
        assert_eq!(definition["parts"][0]["path"], "plan.yml");
        assert_eq!(definition["parts"][0]["payloadType"], "InlineBase64");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(definition["parts"][0]["payload"].as_str().unwrap())
            .unwrap();
        assert_eq!(decoded, PLAN_YAML.as_bytes());
    }

    #[test]
    fn no_definition_is_allowed_for_empty_creation() {
        assert_eq!(read_definition(None, None).unwrap(), None);
    }

    #[test]
    fn definition_files_preserve_raw_yaml_and_both_envelope_forms() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("plan.yml");
        let definition = read_definition(None, Some(PLAN_YAML)).unwrap().unwrap();
        for content in [
            PLAN_YAML.to_string(),
            definition.to_string(),
            serde_json::json!({"definition": definition}).to_string(),
        ] {
            std::fs::write(&file, content).unwrap();
            assert_eq!(
                read_definition(file.to_str(), None).unwrap(),
                Some(definition.clone())
            );
        }
    }

    #[test]
    fn unreadable_definition_files_fail_without_using_inline_content() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("missing.yml");
        let error = read_definition(file.to_str(), Some(PLAN_YAML)).unwrap_err();
        assert!(error.to_string().contains("Failed to read file"));

        std::fs::write(&file, [0xff, 0xfe]).unwrap();
        let error = read_definition(file.to_str(), None).unwrap_err();
        assert!(error.to_string().contains("Failed to read file"));
    }

    #[test]
    fn every_subcommand_validates_its_workspace_and_optional_item_scope() {
        fn commands(workspace: &str, id: &str) -> Vec<DeploymentPlanCommand> {
            let workspace = workspace.to_string();
            let id = id.to_string();
            vec![
                DeploymentPlanCommand::List {
                    workspace: workspace.clone(),
                    no_recursive: false,
                    root_folder_id: None,
                },
                DeploymentPlanCommand::Create {
                    workspace: workspace.clone(),
                    name: "Fixture".to_string(),
                    description: None,
                    folder_id: None,
                    sensitivity_label: None,
                    file: None,
                    content: None,
                },
                DeploymentPlanCommand::Show {
                    workspace: workspace.clone(),
                    id: id.clone(),
                },
                DeploymentPlanCommand::Update {
                    workspace: workspace.clone(),
                    id: id.clone(),
                    name: None,
                    description: None,
                },
                DeploymentPlanCommand::Delete {
                    workspace: workspace.clone(),
                    id: id.clone(),
                    hard_delete: false,
                },
                DeploymentPlanCommand::GetDefinition {
                    workspace: workspace.clone(),
                    id: id.clone(),
                    format: None,
                    decode: false,
                },
                DeploymentPlanCommand::UpdateDefinition {
                    workspace,
                    id,
                    file: None,
                    content: None,
                    update_metadata: false,
                },
            ]
        }
        let valid = "aaaaaaaa-1111-2222-3333-444444444444";
        for command in commands(valid, valid) {
            validate_scope(&command).unwrap();
        }
        for command in commands("invalid", valid) {
            let error = validate_scope(&command).unwrap_err();
            let error = error.downcast_ref::<FabioError>().unwrap();
            assert_eq!(error.code, ErrorCode::InvalidInput);
            assert!(error.message.contains("--workspace"));
        }
        for command in commands(valid, "invalid").into_iter().skip(2) {
            let error = validate_scope(&command).unwrap_err();
            let error = error.downcast_ref::<FabioError>().unwrap();
            assert_eq!(error.code, ErrorCode::InvalidInput);
            assert!(error.message.contains("--id"));
        }
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
