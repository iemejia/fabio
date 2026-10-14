use anyhow::Result;
use clap::Subcommand;
use serde::Deserialize;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before creating items, run: fabio context schema CopyJob\nReturns the definition template with required fields and format."
)]
pub enum CopyJobCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List copy jobs in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a copy job
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,
    },
    /// Create a new copy job
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update copy job properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a copy job
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a copy job
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Reset a copy job (all entities or selected entities)
    #[command(display_order = 8)]
    Reset {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// Reset all copy job entities (mutually exclusive with --entity-ids)
        #[arg(long = "all-entities", conflicts_with = "entity_ids")]
        all_entities: bool,

        /// Comma-separated list of entity IDs to reset (mutually exclusive with --all-entities)
        #[arg(long, value_delimiter = ',', conflicts_with = "all_entities")]
        entity_ids: Vec<String>,
    },

    /// Update the definition of a copy job
    #[command(display_order = 9)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// Definition file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// Definition content (inline)
        #[arg(long)]
        content: Option<String>,
    },

    /// List execute job instances for a copy job
    #[command(name = "list-instances", display_order = 10)]
    ListInstances {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,
    },

    /// Run a copy job on demand
    #[command(display_order = 11)]
    Run {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// Wait for the job to complete (poll the job instance)
        #[arg(long)]
        wait: bool,

        /// Max seconds to wait when --wait is set
        #[arg(long, default_value_t = 300)]
        timeout: u64,

        /// Cancel the job instance if the wait times out
        #[arg(long)]
        cancel_on_timeout: bool,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &CopyJobCommand) -> Result<()> {
    match command {
        CopyJobCommand::List { workspace } => list(cli, client, workspace).await,
        CopyJobCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        CopyJobCommand::Create {
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
        CopyJobCommand::Update {
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
        CopyJobCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        CopyJobCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        CopyJobCommand::Reset {
            workspace,
            id,
            all_entities,
            entity_ids,
        } => reset(cli, client, workspace, id, *all_entities, entity_ids).await,
        CopyJobCommand::UpdateDefinition {
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
        CopyJobCommand::ListInstances { workspace, id } => {
            list_instances(cli, client, workspace, id).await
        }
        CopyJobCommand::Run {
            workspace,
            id,
            wait,
            timeout,
            cancel_on_timeout,
        } => {
            run(
                cli,
                client,
                workspace,
                id,
                *wait,
                *timeout,
                *cancel_on_timeout,
            )
            .await
        }
    }
}

// ─── Run ─────────────────────────────────────────────────────────────────────

/// Run a copy job on demand through its dedicated execute-job endpoint.
async fn run(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    wait: bool,
    timeout_secs: u64,
    cancel_on_timeout: bool,
) -> Result<()> {
    let trigger_path = execute_instances_path(workspace, id);
    crate::commands::item_job::run_and_wait(
        cli,
        client,
        workspace,
        id,
        wait,
        timeout_secs,
        cancel_on_timeout,
        &crate::commands::item_job::RunSpec {
            op: "copy-job run",
            label: "Copy job run",
            job_type: "Execute",
            trigger_path: Some(&trigger_path),
            role: "Member",
        },
    )
    .await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct CopyJobExecuteJobInstance {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    item_id: Option<String>,
    #[serde(default)]
    job_type: Option<String>,
    #[serde(default)]
    invoke_type: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    root_activity_id: Option<String>,
    #[serde(default)]
    start_time_utc: Option<String>,
    #[serde(default)]
    end_time_utc: Option<String>,
    #[serde(default)]
    failure_reason: Option<Value>,
    #[serde(flatten)]
    additional_properties: serde_json::Map<String, Value>,
}

fn execute_instances_path(workspace: &str, id: &str) -> String {
    format!("/workspaces/{workspace}/copyJobs/{id}/jobs/execute/instances")
}

async fn list_instances(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let response = client
        .get_list(
            &execute_instances_path(workspace, id),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job list-instances", "Viewer"))?;
    for item in &response.items {
        serde_json::from_value::<CopyJobExecuteJobInstance>(item.clone())?;
    }

    output::render_list_with_token(
        cli,
        &response.items,
        &["id", "status", "invokeType", "startTimeUtc", "endTimeUtc"],
        &["ID", "STATUS", "INVOKE", "START", "END"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

// ─── CRUD ────────────────────────────────────────────────────────────────────
async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "copyJobs",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "copyJobs", workspace, id).await
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

    if output::dry_run_guard(cli, "copy-job create", &body) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/copyJobs"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job create", "Member"))?;
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
    crate::commands::crud::update(
        cli,
        client,
        "copy-job",
        "copyJobs",
        "Contributor",
        workspace,
        id,
        name,
        description,
    )
    .await
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    crate::commands::crud::delete(
        cli,
        client,
        "copy-job",
        "copyJobs",
        "Member",
        workspace,
        id,
        hard_delete,
    )
    .await
}

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    crate::commands::crud::get_definition(
        cli,
        client,
        "copy-job",
        "copyJobs",
        "Contributor",
        workspace,
        id,
        decode,
    )
    .await
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
                crate::definition_spec::definition_input_hint(
                    "CopyJob",
                    "copy-job",
                    "update-definition",
                ),
            )
            .into());
        }
    };

    let body =
        crate::definition_spec::build_update_definition_body(&script, "copyjob-content.json");

    if output::dry_run_guard(
        cli,
        "copy-job update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": script.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/copyJobs/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Reset ────────────────────────────────────────────────────────────────────

fn build_reset_body(reset_all: bool, entity_ids: &[String]) -> Result<Value> {
    if !reset_all && entity_ids.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --all-entities or --entity-ids must be provided".to_string(),
            "Example: fabio copy-job reset --workspace <WS> --id <ID> --all-entities".to_string(),
        )
        .into());
    }

    let body = if reset_all {
        serde_json::json!({ "resetAllCopyJobEntities": true })
    } else {
        let entities: Vec<_> = entity_ids
            .iter()
            .map(|eid| serde_json::json!({ "copyJobEntityId": eid }))
            .collect();
        serde_json::json!({
            "resetAllCopyJobEntities": false,
            "copyJobEntitiesToReset": entities
        })
    };

    Ok(body)
}

async fn reset(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    reset_all: bool,
    entity_ids: &[String],
) -> Result<()> {
    let body = build_reset_body(reset_all, entity_ids)?;

    if output::dry_run_guard(cli, "copy-job reset", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/copyJobs/{id}/resetCopyJob"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job reset", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "reset" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_body_all() {
        let body = build_reset_body(true, &[]).unwrap();
        assert_eq!(body["resetAllCopyJobEntities"], true);
        assert!(body.get("copyJobEntitiesToReset").is_none());
    }

    #[test]
    fn reset_body_specific_entities() {
        let ids = vec!["id-1".to_string(), "id-2".to_string()];
        let body = build_reset_body(false, &ids).unwrap();
        assert_eq!(body["resetAllCopyJobEntities"], false);
        let entities = body["copyJobEntitiesToReset"].as_array().unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0]["copyJobEntityId"], "id-1");
        assert_eq!(entities[1]["copyJobEntityId"], "id-2");
    }

    #[test]
    fn reset_body_no_flags_errors() {
        let result = build_reset_body(false, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn execute_instances_url_matches_spec() {
        assert_eq!(
            execute_instances_path("workspace-id", "copy-job-id"),
            "/workspaces/workspace-id/copyJobs/copy-job-id/jobs/execute/instances"
        );
    }

    #[test]
    fn execute_job_instance_deserializes_spec_example() {
        let instance: CopyJobExecuteJobInstance = serde_json::from_value(serde_json::json!({
            "id": "aaaaaaaa-0000-1111-2222-bbbbbbbbbbbb",
            "itemId": "dddddddd-9999-0000-1111-eeeeeeeeeeee",
            "jobType": "execute",
            "invokeType": "Manual",
            "status": "Completed",
            "rootActivityId": "11111111-2222-3333-4444-555555555555",
            "startTimeUtc": "2024-06-22T06:35:00.7812154",
            "endTimeUtc": "2024-06-22T06:35:00.8033333",
            "failureReason": null
        }))
        .unwrap();

        assert_eq!(instance.status.as_deref(), Some("Completed"));
        assert_eq!(instance.invoke_type.as_deref(), Some("Manual"));
        assert_eq!(instance.failure_reason, None);
    }
}
