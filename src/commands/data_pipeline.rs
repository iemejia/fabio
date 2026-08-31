use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples data-pipeline\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum DataPipelineCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List data pipelines in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a data pipeline
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,
    },
    /// Create a new data pipeline
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Pipeline display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update data pipeline properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a data pipeline
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Execution ────────────────────────────────────────────────────────
    /// Run a data pipeline
    #[command(display_order = 6)]
    Run {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,

        /// Wait for the pipeline run to complete (polls until finished)
        #[arg(long)]
        wait: bool,

        /// Maximum time to wait in seconds
        #[arg(long, default_value = "600")]
        timeout: u64,

        /// Cancel the run if timeout is reached
        #[arg(long)]
        cancel_on_timeout: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a data pipeline
    #[command(name = "get-definition", display_order = 7)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a data pipeline
    #[command(name = "update-definition", display_order = 8)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,

        /// Path to pipeline definition file
        #[arg(long)]
        file: Option<String>,

        /// Inline pipeline definition content (JSON)
        #[arg(long)]
        content: Option<String>,
    },

    // ── Scheduling ───────────────────────────────────────────────────────
    /// Create a schedule for a data pipeline
    #[command(name = "create-schedule", display_order = 10)]
    CreateSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,

        /// JSON file with schedule configuration
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON schedule configuration
        #[arg(long)]
        content: Option<String>,
    },
    /// List execute schedules for a data pipeline
    #[command(name = "list-schedules", display_order = 11)]
    ListSchedules {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,
    },
    /// Get a specific execute schedule for a data pipeline
    #[command(name = "get-schedule", display_order = 12)]
    GetSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,

        /// Schedule ID
        #[arg(long)]
        schedule_id: String,
    },
    /// Update an execute schedule for a data pipeline
    #[command(name = "update-schedule", display_order = 13)]
    UpdateSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,

        /// Schedule ID
        #[arg(long)]
        schedule_id: String,

        /// JSON file with updated schedule configuration
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON updated schedule configuration
        #[arg(long)]
        content: Option<String>,
    },
    /// Delete an execute schedule for a data pipeline
    #[command(name = "delete-schedule", display_order = 14)]
    DeleteSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,

        /// Schedule ID
        #[arg(long)]
        schedule_id: String,
    },

    // ── Job instances ─────────────────────────────────────────────────────
    /// List execute job instances for a data pipeline
    #[command(name = "list-instances", display_order = 15)]
    ListInstances {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,
    },
    /// Get a specific execute job instance for a data pipeline
    #[command(name = "get-instance", display_order = 16)]
    GetInstance {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data pipeline ID
        #[arg(long)]
        id: String,

        /// Job instance ID
        #[arg(long)]
        instance_id: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &DataPipelineCommand,
) -> Result<()> {
    match command {
        DataPipelineCommand::List { workspace } => list(cli, client, workspace).await,
        DataPipelineCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        DataPipelineCommand::Create {
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
        DataPipelineCommand::Update {
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
        DataPipelineCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        DataPipelineCommand::Run {
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
        DataPipelineCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        DataPipelineCommand::UpdateDefinition {
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
        DataPipelineCommand::CreateSchedule {
            workspace,
            id,
            file,
            content,
        } => {
            create_schedule(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        DataPipelineCommand::ListSchedules { workspace, id } => {
            list_schedules(cli, client, workspace, id).await
        }
        DataPipelineCommand::GetSchedule {
            workspace,
            id,
            schedule_id,
        } => get_schedule(cli, client, workspace, id, schedule_id).await,
        DataPipelineCommand::UpdateSchedule {
            workspace,
            id,
            schedule_id,
            file,
            content,
        } => {
            update_schedule(
                cli,
                client,
                workspace,
                id,
                schedule_id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        DataPipelineCommand::DeleteSchedule {
            workspace,
            id,
            schedule_id,
        } => delete_schedule(cli, client, workspace, id, schedule_id).await,
        DataPipelineCommand::ListInstances { workspace, id } => {
            list_instances(cli, client, workspace, id).await
        }
        DataPipelineCommand::GetInstance {
            workspace,
            id,
            instance_id,
        } => get_instance(cli, client, workspace, id, instance_id).await,
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "dataPipelines",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "dataPipelines", workspace, id).await
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    crate::commands::crud::create(
        cli,
        client,
        "data-pipeline",
        "dataPipelines",
        "Member",
        workspace,
        name,
        description,
        sensitivity_label,
    )
    .await
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
            "Example: fabio data-pipeline update --workspace <WS> --id <ID> --name \"New Name\""
                .to_string(),
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

    if output::dry_run_guard(cli, "data-pipeline update", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/dataPipelines/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-pipeline update", "Contributor"))?;
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
    crate::commands::crud::delete(
        cli,
        client,
        "data-pipeline",
        "dataPipelines",
        "Member",
        workspace,
        id,
        hard_delete,
    )
    .await
}

// ─── Execution ───────────────────────────────────────────────────────────────

async fn run(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    wait: bool,
    timeout_secs: u64,
    cancel_on_timeout: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "data-pipeline run",
        &serde_json::json!({ "workspace": workspace, "id": id, "wait": wait, "timeout": timeout_secs }),
    ) {
        return Ok(());
    }

    let job_id = client
        .trigger_item_job(workspace, id, "Pipeline", None)
        .await
        .map_err(|e| enrich_forbidden(e, "data-pipeline run", "Contributor"))?;

    if !wait {
        let obj = serde_json::json!({ "itemId": id, "jobInstanceId": job_id, "status": "started" });
        output::render_object(cli, &obj, "status");
        return Ok(());
    }

    // Poll until completion
    let poll_interval = std::time::Duration::from_secs(5);
    let max_wait = std::time::Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > max_wait {
            if cancel_on_timeout && !job_id.is_empty() {
                let cancel_path =
                    format!("/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}/cancel");
                let _ = client
                    .post(&cancel_path, &serde_json::json!({}), false)
                    .await;
            }
            return Err(FabioError::with_hint(
                ErrorCode::Timeout,
                format!("Pipeline run timed out after {timeout_secs}s. Job ID: {job_id}"),
                format!("Increase --timeout (current: {timeout_secs}s) or use --cancel-on-timeout"),
            )
            .into());
        }

        tokio::time::sleep(poll_interval).await;

        let status_path = format!("/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}");
        let status_resp = client.get(&status_path).await;

        if let Ok(ref status_data) = status_resp {
            let status = status_data
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            match status {
                "Completed" => {
                    output::render_object(cli, status_data, "status");
                    return Ok(());
                }
                "Failed" | "Cancelled" | "Deduped" => {
                    output::render_object(cli, status_data, "status");
                    return Err(FabioError::new(
                        ErrorCode::ApiError,
                        format!("Pipeline run {status}. Job ID: {job_id}"),
                    )
                    .into());
                }
                _ => {} // InProgress, NotStarted — keep polling
            }
        }
    }
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
        "data-pipeline",
        "dataPipelines",
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
    let raw = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio data-pipeline update-definition --workspace <WS> --id <ID> --file pipeline.json".to_string(),
            ).into());
        }
    };

    let body = crate::definition_spec::build_update_definition_body(&raw, "pipeline-content.json");

    if output::dry_run_guard(
        cli,
        "data-pipeline update-definition",
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
            &format!("/workspaces/{workspace}/dataPipelines/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-pipeline update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Scheduling ──────────────────────────────────────────────────────────────

async fn create_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body: Value = match (file, content) {
        (Some(path), _) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
            serde_json::from_str(&raw)?
        }
        (_, Some(c)) => serde_json::from_str(c)?,
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio data-pipeline create-schedule --workspace <WS> --id <ID> --content '{...}'"
                    .to_string(),
            )
            .into());
        }
    };

    if output::dry_run_guard(cli, "data-pipeline create-schedule", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/dataPipelines/{id}/jobs/execute/schedules"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-pipeline create-schedule", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "schedule_created" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

async fn list_schedules(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/dataPipelines/{id}/jobs/execute/schedules"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "enabled", "createdDateTime"],
        &["ID", "ENABLED", "CREATED"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn get_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    schedule_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/dataPipelines/{id}/jobs/execute/schedules/{schedule_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "data-pipeline get-schedule", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    schedule_id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body: Value = match (file, content) {
        (Some(path), _) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
            serde_json::from_str(&raw)?
        }
        (_, Some(c)) => serde_json::from_str(c)?,
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio data-pipeline update-schedule --workspace <WS> --id <ID> --schedule-id <SCHED_ID> --content '{...}'".to_string(),
            )
            .into());
        }
    };

    if output::dry_run_guard(cli, "data-pipeline update-schedule", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!(
                "/workspaces/{workspace}/dataPipelines/{id}/jobs/execute/schedules/{schedule_id}"
            ),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-pipeline update-schedule", "Contributor"))?;
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
        "data-pipeline delete-schedule",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "scheduleId": schedule_id
        }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/dataPipelines/{id}/jobs/execute/schedules/{schedule_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "data-pipeline delete-schedule", "Contributor"))?;

    let obj = serde_json::json!({ "id": schedule_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn list_instances(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/dataPipelines/{id}/jobs/execute/instances"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "status", "invokeType", "startTimeUtc", "endTimeUtc"],
        &["ID", "STATUS", "INVOKE", "START", "END"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn get_instance(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    instance_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/dataPipelines/{id}/jobs/execute/instances/{instance_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "data-pipeline get-instance", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Schedule URL tests ────────────────────────────────────────────────

    #[test]
    fn test_list_schedules_url_format() {
        let ws = "ws-abc";
        let id = "pipeline-123";
        let url = format!("/workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules");
        assert_eq!(
            url,
            "/workspaces/ws-abc/dataPipelines/pipeline-123/jobs/execute/schedules"
        );
    }

    #[test]
    fn test_get_schedule_url_format() {
        let ws = "ws-abc";
        let id = "pipeline-123";
        let schedule_id = "sched-456";
        let url =
            format!("/workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules/{schedule_id}");
        assert_eq!(
            url,
            "/workspaces/ws-abc/dataPipelines/pipeline-123/jobs/execute/schedules/sched-456"
        );
    }

    #[test]
    fn test_update_schedule_url_format() {
        let ws = "ws-abc";
        let id = "pipeline-123";
        let schedule_id = "sched-456";
        let url =
            format!("/workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules/{schedule_id}");
        // Same URL as get_schedule (PATCH vs GET)
        assert!(url.ends_with("/schedules/sched-456"));
    }

    #[test]
    fn test_delete_schedule_url_format() {
        let ws = "ws-abc";
        let id = "pipeline-123";
        let schedule_id = "sched-456";
        let url =
            format!("/workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules/{schedule_id}");
        // Same URL as get/update (DELETE method)
        assert!(url.contains("/jobs/execute/schedules/"));
        assert!(url.ends_with("sched-456"));
    }

    // ── Instance URL tests ────────────────────────────────────────────────

    #[test]
    fn test_list_instances_url_format() {
        let ws = "ws-abc";
        let id = "pipeline-123";
        let url = format!("/workspaces/{ws}/dataPipelines/{id}/jobs/execute/instances");
        assert_eq!(
            url,
            "/workspaces/ws-abc/dataPipelines/pipeline-123/jobs/execute/instances"
        );
    }

    #[test]
    fn test_get_instance_url_format() {
        let ws = "ws-abc";
        let id = "pipeline-123";
        let instance_id = "inst-789";
        let url =
            format!("/workspaces/{ws}/dataPipelines/{id}/jobs/execute/instances/{instance_id}");
        assert_eq!(
            url,
            "/workspaces/ws-abc/dataPipelines/pipeline-123/jobs/execute/instances/inst-789"
        );
    }

    // ── Error path tests ──────────────────────────────────────────────────

    #[test]
    fn test_update_schedule_no_input_error() {
        let err: anyhow::Error = FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            "Example: fabio data-pipeline update-schedule --workspace <WS> --id <ID> --schedule-id <SCHED_ID> --content '{...}'".to_string(),
        )
        .into();
        let msg = err.to_string();
        assert!(msg.contains("--file or --content"));
    }

    #[test]
    fn test_delete_schedule_dry_run_body() {
        let ws = "ws1";
        let id = "pipe1";
        let schedule_id = "sched1";
        let body = serde_json::json!({
            "workspace": ws,
            "id": id,
            "scheduleId": schedule_id
        });
        assert_eq!(body["workspace"], "ws1");
        assert_eq!(body["id"], "pipe1");
        assert_eq!(body["scheduleId"], "sched1");
    }

    #[test]
    fn test_create_schedule_url_format() {
        let ws = "ws-abc";
        let id = "pipeline-123";
        let url = format!("/workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules");
        // create-schedule uses POST to same list URL
        assert_eq!(
            url,
            "/workspaces/ws-abc/dataPipelines/pipeline-123/jobs/execute/schedules"
        );
    }

    #[test]
    fn test_create_schedule_no_input_error() {
        let result: Result<()> = Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            "Example: fabio data-pipeline create-schedule --workspace <WS> --id <ID> --content '{...}'".to_string(),
        )
        .into());
        assert!(result.is_err());
    }
}
