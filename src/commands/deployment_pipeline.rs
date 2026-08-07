use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples deployment-pipeline\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum DeploymentPipelineCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List deployment pipelines
    #[command(display_order = 1)]
    List,
    /// Show details of a deployment pipeline
    #[command(display_order = 2)]
    Show {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,
    },
    /// Create a new deployment pipeline
    #[command(display_order = 3)]
    Create {
        /// Display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// A pipeline stage (repeatable, ordered). Each value is a stage display
        /// name, optionally suffixed with `:public` to make it a public stage
        /// (e.g. `--stage Development --stage Test --stage Production:public`).
        #[arg(long = "stage", value_name = "NAME[:public]")]
        stages: Vec<String>,

        /// Full stages definition as JSON (overrides --stage): an array of
        /// `{"displayName","description"?,"isPublic"?}` (inline, @file, or stdin).
        #[arg(long = "stages-json", value_name = "JSON")]
        stages_json: Option<String>,
    },
    /// Update a deployment pipeline
    #[command(display_order = 4)]
    Update {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a deployment pipeline
    #[command(display_order = 5)]
    Delete {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,
    },

    // ── Stages ───────────────────────────────────────────────────────────
    /// List stages in a deployment pipeline
    #[command(display_order = 10)]
    ListStages {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,
    },
    /// List items in a deployment pipeline stage
    #[command(display_order = 11)]
    ListStageItems {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,

        /// Stage ID
        #[arg(long)]
        stage_id: String,
    },
    /// Assign a workspace to a deployment pipeline stage
    #[command(display_order = 12)]
    AssignWorkspace {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,

        /// Stage ID to assign the workspace to
        #[arg(long)]
        stage_id: String,

        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Unassign the workspace from a deployment pipeline stage
    #[command(display_order = 13)]
    UnassignWorkspace {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,

        /// Stage ID to unassign
        #[arg(long)]
        stage_id: String,
    },

    // ── Operations ─────────────────────────────────────────────────────────
    /// List deploy operations for a deployment pipeline
    #[command(display_order = 20)]
    ListOperations {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,
    },
    /// Show details of a deploy operation
    #[command(display_order = 21)]
    ShowOperation {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,

        /// Operation ID
        #[arg(long)]
        operation_id: String,
    },

    // ── Role Assignments ─────────────────────────────────────────────────
    /// List role assignments for a deployment pipeline
    #[command(display_order = 30)]
    ListRoleAssignments {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,
    },
    /// Add a role assignment to a deployment pipeline
    #[command(display_order = 31)]
    AddRoleAssignment {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,

        /// Principal ID
        #[arg(long)]
        principal_id: String,

        /// Principal type (e.g. User, Group, `ServicePrincipal`)
        #[arg(long)]
        principal_type: String,

        /// Role (e.g. Admin, Contributor, Viewer)
        #[arg(long)]
        role: String,
    },
    /// Delete a role assignment from a deployment pipeline
    #[command(display_order = 32)]
    DeleteRoleAssignment {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,

        /// Principal ID to remove
        #[arg(long)]
        principal_id: String,
    },

    // ── Stage Management ─────────────────────────────────────────────────
    /// Show details of a deployment pipeline stage
    #[command(display_order = 40)]
    ShowStage {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,

        /// Stage ID
        #[arg(long)]
        stage_id: String,
    },
    /// Update a deployment pipeline stage configuration
    #[command(display_order = 41)]
    UpdateStage {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,

        /// Stage ID
        #[arg(long)]
        stage_id: String,

        /// Path to JSON file with stage configuration
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content with stage configuration
        #[arg(long)]
        content: Option<String>,
    },

    // ── Deploy ───────────────────────────────────────────────────────────
    /// Deploy items from one stage to another
    #[command(display_order = 50)]
    Deploy {
        /// Deployment pipeline ID
        #[arg(long)]
        id: String,

        /// Source stage ID
        #[arg(long)]
        source_stage_id: String,

        /// Target stage ID (if omitted, deploys to the next stage)
        #[arg(long)]
        target_stage_id: Option<String>,

        /// Items to deploy as JSON array (if omitted, all items are deployed).
        /// Example: '[{"itemId":"...","itemType":"Notebook"}]'
        #[arg(long)]
        items: Option<String>,

        /// Optional note for this deployment
        #[arg(long)]
        note: Option<String>,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &DeploymentPipelineCommand,
) -> Result<()> {
    match command {
        DeploymentPipelineCommand::List => list(cli, client).await,
        DeploymentPipelineCommand::Show { id } => show(cli, client, id).await,
        DeploymentPipelineCommand::Create {
            name,
            description,
            stages,
            stages_json,
        } => {
            create(
                cli,
                client,
                name,
                description.as_deref(),
                stages,
                stages_json.as_deref(),
            )
            .await
        }
        DeploymentPipelineCommand::Update {
            id,
            name,
            description,
        } => update(cli, client, id, name.as_deref(), description.as_deref()).await,
        DeploymentPipelineCommand::Delete { id } => delete(cli, client, id).await,
        DeploymentPipelineCommand::ListStages { id } => list_stages(cli, client, id).await,
        DeploymentPipelineCommand::ListStageItems { id, stage_id } => {
            list_stage_items(cli, client, id, stage_id).await
        }
        DeploymentPipelineCommand::AssignWorkspace {
            id,
            stage_id,
            workspace,
        } => assign_workspace(cli, client, id, stage_id, workspace).await,
        DeploymentPipelineCommand::UnassignWorkspace { id, stage_id } => {
            unassign_workspace(cli, client, id, stage_id).await
        }
        DeploymentPipelineCommand::ListOperations { id } => list_operations(cli, client, id).await,
        DeploymentPipelineCommand::ShowOperation { id, operation_id } => {
            show_operation(cli, client, id, operation_id).await
        }
        DeploymentPipelineCommand::ListRoleAssignments { id } => {
            list_role_assignments(cli, client, id).await
        }
        DeploymentPipelineCommand::AddRoleAssignment {
            id,
            principal_id,
            principal_type,
            role,
        } => add_role_assignment(cli, client, id, principal_id, principal_type, role).await,
        DeploymentPipelineCommand::DeleteRoleAssignment { id, principal_id } => {
            delete_role_assignment(cli, client, id, principal_id).await
        }
        DeploymentPipelineCommand::ShowStage { id, stage_id } => {
            show_stage(cli, client, id, stage_id).await
        }
        DeploymentPipelineCommand::UpdateStage {
            id,
            stage_id,
            file,
            content,
        } => {
            update_stage(
                cli,
                client,
                id,
                stage_id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        DeploymentPipelineCommand::Deploy {
            id,
            source_stage_id,
            target_stage_id,
            items,
            note,
        } => {
            deploy(
                cli,
                client,
                id,
                source_stage_id,
                target_stage_id.as_deref(),
                items.as_deref(),
                note.as_deref(),
            )
            .await
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient) -> Result<()> {
    let resp = client
        .get_list(
            "/deploymentPipelines",
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    let data = client.get(&format!("/deploymentPipelines/{id}")).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    name: &str,
    description: Option<&str>,
    stages: &[String],
    stages_json: Option<&str>,
) -> Result<()> {
    let stage_values = build_stages(stages, stages_json)?;

    let mut body = serde_json::json!({ "displayName": name, "stages": stage_values });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }

    if output::dry_run_guard(cli, "deployment-pipeline create", &body) {
        return Ok(());
    }

    let data = client
        .post("/deploymentPipelines", &body, false)
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-pipeline create", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

/// Build the required `stages` array for a create request from either the
/// repeatable `--stage NAME[:public]` flag or a full `--stages-json` array.
/// The modern deployment-pipelines API requires at least one stage on create.
fn build_stages(stages: &[String], stages_json: Option<&str>) -> Result<Vec<Value>> {
    if let Some(raw) = stages_json {
        let text = if let Some(path) = raw.strip_prefix('@') {
            std::fs::read_to_string(path)
                .map_err(|e| FabioError::not_found(format!("stages file not found: {path}: {e}")))?
        } else {
            raw.to_string()
        };
        let parsed: Value = serde_json::from_str(text.trim()).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("--stages-json is not valid JSON: {e}"),
                r#"Example: --stages-json '[{"displayName":"Dev"},{"displayName":"Prod","isPublic":true}]'"#
                    .to_string(),
            )
        })?;
        let arr = parsed.as_array().ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--stages-json must be a JSON array of stage objects".to_string(),
                r#"Example: --stages-json '[{"displayName":"Dev"},{"displayName":"Prod","isPublic":true}]'"#
                    .to_string(),
            )
        })?;
        if arr.is_empty() {
            return Err(empty_stages_err());
        }
        return Ok(arr.clone());
    }

    if stages.is_empty() {
        return Err(empty_stages_err());
    }

    Ok(stages
        .iter()
        .map(|s| {
            let (display, is_public) = s
                .strip_suffix(":public")
                .map_or((s.as_str(), false), |base| (base, true));
            serde_json::json!({ "displayName": display, "isPublic": is_public })
        })
        .collect())
}

fn empty_stages_err() -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::InvalidInput,
        "A deployment pipeline requires at least one stage (the create API rejects an empty pipeline).".to_string(),
        "Add stages with repeatable --stage, e.g.: fabio deployment-pipeline create --name \"My Pipeline\" --stage Development --stage Test --stage Production:public".to_string(),
    )
    .into()
}

async fn update(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio deployment-pipeline update --id <ID> --name \"New Name\"".to_string(),
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

    if output::dry_run_guard(cli, "deployment-pipeline update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/deploymentPipelines/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-pipeline update", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "deployment-pipeline delete",
        &serde_json::json!({ "id": id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!("/deploymentPipelines/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-pipeline delete", "Admin"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Stages ──────────────────────────────────────────────────────────────────

async fn list_stages(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/deploymentPipelines/{id}/stages"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["displayName", "id", "order", "workspaceId"],
        &["NAME", "ID", "ORDER", "WORKSPACE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn list_stage_items(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    stage_id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/deploymentPipelines/{id}/stages/{stage_id}/items"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["itemDisplayName", "itemId", "itemType"],
        &["NAME", "ID", "TYPE"],
        "itemId",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn assign_workspace(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    stage_id: &str,
    workspace: &str,
) -> Result<()> {
    let body = serde_json::json!({ "workspaceId": workspace });

    if output::dry_run_guard(cli, "deployment-pipeline assign-workspace", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/deploymentPipelines/{id}/stages/{stage_id}/assignWorkspace"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-pipeline assign-workspace", "Admin"))?;

    let obj = serde_json::json!({
        "pipelineId": id,
        "stageId": stage_id,
        "workspaceId": workspace,
        "status": "assigned"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn unassign_workspace(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    stage_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "deployment-pipeline unassign-workspace",
        &serde_json::json!({ "pipelineId": id, "stageId": stage_id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/deploymentPipelines/{id}/stages/{stage_id}/unassignWorkspace"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-pipeline unassign-workspace", "Admin"))?;

    let obj = serde_json::json!({
        "pipelineId": id,
        "stageId": stage_id,
        "status": "unassigned"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Operations ──────────────────────────────────────────────────────────────

async fn list_operations(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/deploymentPipelines/{id}/operations"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "status", "sourceStageId", "targetStageId"],
        &["ID", "STATUS", "SOURCE STAGE", "TARGET STAGE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show_operation(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    operation_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/deploymentPipelines/{id}/operations/{operation_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

// ─── Role Assignments ────────────────────────────────────────────────────────

async fn list_role_assignments(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/deploymentPipelines/{id}/roleAssignments"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["principal.id", "principal.type", "role"],
        &["PRINCIPAL ID", "PRINCIPAL TYPE", "ROLE"],
        "principal.id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn add_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    principal_id: &str,
    principal_type: &str,
    role: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "principal": {
            "id": principal_id,
            "type": principal_type
        },
        "role": role
    });

    if output::dry_run_guard(cli, "deployment-pipeline add-role-assignment", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/deploymentPipelines/{id}/roleAssignments"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-pipeline add-role-assignment", "Admin"))?;

    let obj = serde_json::json!({
        "pipelineId": id,
        "principalId": principal_id,
        "principalType": principal_type,
        "role": role,
        "status": "added"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn delete_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    principal_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "deployment-pipeline delete-role-assignment",
        &serde_json::json!({ "pipelineId": id, "principalId": principal_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/deploymentPipelines/{id}/roleAssignments/{principal_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-pipeline delete-role-assignment", "Admin"))?;

    let obj = serde_json::json!({
        "pipelineId": id,
        "principalId": principal_id,
        "status": "deleted"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Stage Management ────────────────────────────────────────────────────────

async fn show_stage(cli: &Cli, client: &FabricClient, id: &str, stage_id: &str) -> Result<()> {
    let data = client
        .get(&format!("/deploymentPipelines/{id}/stages/{stage_id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update_stage(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    stage_id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "update-stage")?;

    if output::dry_run_guard(cli, "deployment-pipeline update-stage", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/deploymentPipelines/{id}/stages/{stage_id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-pipeline update-stage", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

// ─── Deploy ──────────────────────────────────────────────────────────────────

async fn deploy(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    source_stage_id: &str,
    target_stage_id: Option<&str>,
    items: Option<&str>,
    note: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "sourceStageId": source_stage_id,
    });
    if let Some(target) = target_stage_id {
        body["targetStageId"] = Value::from(target);
    }
    if let Some(items_json) = items {
        let items_value: Value = serde_json::from_str(items_json)
            .map_err(|e| anyhow::anyhow!("Invalid --items JSON: {e}. Expected array, e.g.: [{{\"itemId\":\"...\",\"itemType\":\"Notebook\"}}]"))?;
        body["items"] = items_value;
    }
    if let Some(n) = note {
        body["note"] = Value::from(n);
    }

    if output::dry_run_guard(cli, "deployment-pipeline deploy", &body) {
        return Ok(());
    }

    let data = client
        .post(&format!("/deploymentPipelines/{id}/deploy"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "deployment-pipeline deploy", "Contributor"))?;

    // API may return LRO or immediate result
    let obj = if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        serde_json::json!({
            "pipelineId": id,
            "status": "accepted"
        })
    } else {
        data
    };
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn read_json_body(file: Option<&str>, content: Option<&str>, command: &str) -> Result<Value> {
    match (file, content) {
        (Some(f), _) => {
            let text = std::fs::read_to_string(f)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{f}': {e}"))?;
            Ok(serde_json::from_str(&text)?)
        }
        (_, Some(c)) => Ok(serde_json::from_str(c)?),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            format!(
                "Example: fabio deployment-pipeline {command} --id <ID> --stage-id <SID> --file stage.json"
            ),
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploy_command_can_parse_items_json() {
        let items = r#"[{"itemId":"abc-123","itemType":"Notebook"}]"#;
        let val: Value = serde_json::from_str(items).unwrap();
        assert!(val.is_array());
        assert_eq!(val.as_array().unwrap().len(), 1);
    }

    #[test]
    fn build_stages_from_repeatable_flag() {
        let stages = vec![
            "Development".to_string(),
            "Test".to_string(),
            "Production:public".to_string(),
        ];
        let out = build_stages(&stages, None).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0]["displayName"], "Development");
        assert_eq!(out[0]["isPublic"], false);
        assert_eq!(out[2]["displayName"], "Production");
        assert_eq!(out[2]["isPublic"], true);
    }

    #[test]
    fn build_stages_from_json_overrides_flag() {
        let json =
            r#"[{"displayName":"Dev","description":"d"},{"displayName":"Prod","isPublic":true}]"#;
        let out = build_stages(&["ignored".to_string()], Some(json)).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["description"], "d");
        assert_eq!(out[1]["isPublic"], true);
    }

    #[test]
    fn build_stages_rejects_empty() {
        assert!(build_stages(&[], None).is_err());
        assert!(build_stages(&[], Some("[]")).is_err());
    }

    #[test]
    fn build_stages_rejects_non_array_json() {
        assert!(build_stages(&[], Some(r#"{"displayName":"x"}"#)).is_err());
        assert!(build_stages(&[], Some("not json")).is_err());
    }
}
