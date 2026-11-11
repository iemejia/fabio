use anyhow::Result;
use clap::Subcommand;
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

    /// Run a copy job on demand
    #[command(display_order = 10)]
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
    /// List execute job instances for a copy job
    #[command(display_order = 11)]
    ListRuns {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,
    },
    /// Show an execute job instance
    ShowRun {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Copy job ID
        #[arg(long)]
        id: String,
        /// Execute job instance ID
        #[arg(long)]
        job_instance_id: String,
    },
    /// List execute schedules for a copy job
    ListSchedules {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Copy job ID
        #[arg(long)]
        id: String,
    },
    /// Create an execute schedule
    CreateSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Copy job ID
        #[arg(long)]
        id: String,
        /// Whether the schedule is enabled
        #[arg(long, default_value_t = true)]
        enabled: bool,
        /// Schedule configuration JSON (`Cron`, `Daily`, `Weekly`, or `Monthly`)
        #[arg(long)]
        config: String,
    },
    /// Show an execute schedule
    ShowSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Copy job ID
        #[arg(long)]
        id: String,
        /// Schedule ID
        #[arg(long)]
        schedule_id: String,
    },
    /// Update an execute schedule
    UpdateSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Copy job ID
        #[arg(long)]
        id: String,
        /// Schedule ID
        #[arg(long)]
        schedule_id: String,
        /// Whether the schedule is enabled
        #[arg(long)]
        enabled: Option<bool>,
        /// Replacement schedule configuration JSON
        #[arg(long)]
        config: Option<String>,
    },
    /// Delete an execute schedule
    DeleteSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Copy job ID
        #[arg(long)]
        id: String,
        /// Schedule ID
        #[arg(long)]
        schedule_id: String,
    },
}

#[allow(clippy::too_many_lines)]
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
        CopyJobCommand::ListRuns { workspace, id } => list_runs(cli, client, workspace, id).await,
        CopyJobCommand::ShowRun {
            workspace,
            id,
            job_instance_id,
        } => show_run(cli, client, workspace, id, job_instance_id).await,
        CopyJobCommand::ListSchedules { workspace, id } => {
            list_schedules(cli, client, workspace, id).await
        }
        CopyJobCommand::CreateSchedule {
            workspace,
            id,
            enabled,
            config,
        } => create_schedule(cli, client, workspace, id, *enabled, config).await,
        CopyJobCommand::ShowSchedule {
            workspace,
            id,
            schedule_id,
        } => show_schedule(cli, client, workspace, id, schedule_id).await,
        CopyJobCommand::UpdateSchedule {
            workspace,
            id,
            schedule_id,
            enabled,
            config,
        } => {
            update_schedule(
                cli,
                client,
                workspace,
                id,
                schedule_id,
                *enabled,
                config.as_deref(),
            )
            .await
        }
        CopyJobCommand::DeleteSchedule {
            workspace,
            id,
            schedule_id,
        } => delete_schedule(cli, client, workspace, id, schedule_id).await,
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

async fn list_runs(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let response = client
        .get_list(
            &execute_instances_path(workspace, id),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job list-runs", "Viewer"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["id", "status", "invokeType", "startTimeUtc", "endTimeUtc"],
        &["ID", "STATUS", "INVOKE TYPE", "START", "END"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

fn execute_instances_path(workspace: &str, id: &str) -> String {
    format!("/workspaces/{workspace}/copyJobs/{id}/jobs/execute/instances")
}

async fn show_run(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    job_instance_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "{}/{job_instance_id}",
            execute_instances_path(workspace, id)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job show-run", "Viewer"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

fn execute_schedules_path(workspace: &str, id: &str) -> String {
    format!("/workspaces/{workspace}/copyJobs/{id}/jobs/execute/schedules")
}

fn schedule_configuration(config: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(config).map_err(|error| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --config JSON: {error}"),
            r#"Example: --config '{"type":"Cron","startDateTime":"2026-01-01T00:00:00","endDateTime":"2026-12-31T23:59:00","localTimeZoneId":"UTC","interval":10}'"#,
        )
    })?;
    Ok(value
        .get("configuration")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or(value))
}

async fn list_schedules(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let response = client
        .get_list(
            &execute_schedules_path(workspace, id),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job list-schedules", "Viewer"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["id", "enabled", "configuration.type", "createdDateTime"],
        &["ID", "ENABLED", "TYPE", "CREATED"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

async fn create_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    enabled: bool,
    config: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "enabled": enabled,
        "configuration": schedule_configuration(config)?,
    });
    if output::dry_run_guard(cli, "copy-job create-schedule", &body) {
        return Ok(());
    }
    let data = client
        .post(&execute_schedules_path(workspace, id), &body, false)
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job create-schedule", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn show_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    schedule_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "{}/{schedule_id}",
            execute_schedules_path(workspace, id)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job show-schedule", "Viewer"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    schedule_id: &str,
    enabled: Option<bool>,
    config: Option<&str>,
) -> Result<()> {
    if enabled.is_none() && config.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --enabled or --config must be provided",
            "Provide --enabled false and/or a replacement --config JSON object.",
        )
        .into());
    }
    let mut body = serde_json::json!({});
    if let Some(value) = enabled {
        body["enabled"] = Value::Bool(value);
    }
    if let Some(value) = config {
        body["configuration"] = schedule_configuration(value)?;
    }
    if output::dry_run_guard(cli, "copy-job update-schedule", &body) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!("{}/{schedule_id}", execute_schedules_path(workspace, id)),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job update-schedule", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    schedule_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "copy-job delete-schedule",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "scheduleId": schedule_id,
        }),
    ) {
        return Ok(());
    }
    client
        .delete(&format!(
            "{}/{schedule_id}",
            execute_schedules_path(workspace, id)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job delete-schedule", "Contributor"))?;
    output::render_object(
        cli,
        &serde_json::json!({"id": schedule_id, "status": "deleted"}),
        "status",
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
    fn execute_instances_path_matches_spec() {
        assert_eq!(
            execute_instances_path("ws", "copy-job"),
            "/workspaces/ws/copyJobs/copy-job/jobs/execute/instances"
        );
    }
}
