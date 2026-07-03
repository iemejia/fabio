use std::time::Duration;

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;
use tokio::time::sleep;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::jobs::{JobEntry, JobLedger};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

/// Known job types for the Fabric Job Scheduler API.
#[allow(dead_code)]
pub const KNOWN_JOB_TYPES: &[&str] = &[
    "DefaultJob",
    "RunNotebook",
    "Pipeline",
    "TableMaintenance",
    "SparkJob",
    "RefreshMaterializedLakeViews",
];

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples job-scheduler\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum JobSchedulerCommand {
    // ── Instances ────────────────────────────────────────────────────────
    /// List job instances for an item
    #[command(display_order = 1)]
    ListInstances {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,
    },
    /// Show details of a job instance
    #[command(display_order = 2)]
    GetInstance {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Job instance ID
        #[arg(long)]
        job_instance_id: String,
    },
    /// Run an on-demand job for an item
    #[command(display_order = 3)]
    RunOnDemand {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Job type (e.g., `DefaultJob`, `RunNotebook`, `Pipeline`, `TableMaintenance`, `SparkJob`)
        #[arg(long, default_value = "DefaultJob")]
        job_type: String,

        /// Execution data as JSON (optional, depends on job type)
        #[arg(long)]
        execution_data: Option<String>,

        /// Wait for the job to complete (polls until finished)
        #[arg(long)]
        wait: bool,

        /// Maximum time to wait in seconds (default: 600). Only used with --wait
        #[arg(long, default_value = "600")]
        timeout: u64,

        /// Cancel the job if --wait times out (default: leave running)
        #[arg(long)]
        cancel_on_timeout: bool,
    },
    /// Cancel a running job instance
    #[command(display_order = 4)]
    CancelInstance {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Job instance ID
        #[arg(long)]
        job_instance_id: String,
    },

    // ── Schedules ────────────────────────────────────────────────────────
    /// List schedules for an item job type
    #[command(display_order = 10)]
    ListSchedules {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Job type (e.g., `DefaultJob`, `RunNotebook`, `Pipeline`)
        #[arg(long, default_value = "DefaultJob")]
        job_type: String,
    },
    /// Show details of a specific schedule
    #[command(display_order = 11)]
    GetSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Job type
        #[arg(long, default_value = "DefaultJob")]
        job_type: String,

        /// Schedule ID
        #[arg(long)]
        schedule_id: String,
    },
    /// Create a schedule for an item job type
    #[command(display_order = 12)]
    CreateSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Job type
        #[arg(long, default_value = "DefaultJob")]
        job_type: String,

        /// Whether the schedule is enabled
        #[arg(long, default_value_t = true)]
        enabled: bool,

        /// Schedule configuration as JSON (cron or recurrence)
        /// Example: '{"type":"Cron","expression":"0 0 * * *","timezone":"UTC"}'
        #[arg(long)]
        config: String,
    },
    /// Update an existing schedule
    #[command(display_order = 13)]
    UpdateSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Job type
        #[arg(long, default_value = "DefaultJob")]
        job_type: String,

        /// Schedule ID
        #[arg(long)]
        schedule_id: String,

        /// Whether the schedule is enabled
        #[arg(long)]
        enabled: Option<bool>,

        /// Updated schedule configuration as JSON
        #[arg(long)]
        config: Option<String>,
    },
    /// Delete a schedule
    #[command(display_order = 14)]
    DeleteSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Job type
        #[arg(long, default_value = "DefaultJob")]
        job_type: String,

        /// Schedule ID
        #[arg(long)]
        schedule_id: String,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &JobSchedulerCommand,
) -> Result<()> {
    match command {
        // ── Instances ────────────────────────────────────────────────────
        JobSchedulerCommand::ListInstances { workspace, id } => {
            list_instances(cli, client, workspace, id).await
        }
        JobSchedulerCommand::GetInstance {
            workspace,
            id,
            job_instance_id,
        } => get_instance(cli, client, workspace, id, job_instance_id).await,
        JobSchedulerCommand::RunOnDemand {
            workspace,
            id,
            job_type,
            execution_data,
            wait,
            timeout,
            cancel_on_timeout,
        } => {
            run_on_demand(
                cli,
                client,
                workspace,
                id,
                job_type,
                execution_data.as_deref(),
                *wait,
                *timeout,
                *cancel_on_timeout,
            )
            .await
        }
        JobSchedulerCommand::CancelInstance {
            workspace,
            id,
            job_instance_id,
        } => cancel_instance(cli, client, workspace, id, job_instance_id).await,
        // ── Schedules ────────────────────────────────────────────────────
        JobSchedulerCommand::ListSchedules {
            workspace,
            id,
            job_type,
        } => list_schedules(cli, client, workspace, id, job_type).await,
        JobSchedulerCommand::GetSchedule {
            workspace,
            id,
            job_type,
            schedule_id,
        } => get_schedule(cli, client, workspace, id, job_type, schedule_id).await,
        JobSchedulerCommand::CreateSchedule {
            workspace,
            id,
            job_type,
            enabled,
            config,
        } => create_schedule(cli, client, workspace, id, job_type, *enabled, config).await,
        JobSchedulerCommand::UpdateSchedule {
            workspace,
            id,
            job_type,
            schedule_id,
            enabled,
            config,
        } => {
            update_schedule(
                cli,
                client,
                workspace,
                id,
                job_type,
                schedule_id,
                *enabled,
                config.as_deref(),
            )
            .await
        }
        JobSchedulerCommand::DeleteSchedule {
            workspace,
            id,
            job_type,
            schedule_id,
        } => delete_schedule(cli, client, workspace, id, job_type, schedule_id).await,
    }
}

// ─── Instances ───────────────────────────────────────────────────────────────

async fn list_instances(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/items/{item_id}/jobs/instances"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "jobType", "status", "startTimeUtc", "endTimeUtc"],
        &["ID", "JOB TYPE", "STATUS", "START", "END"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn get_instance(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    job_instance_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/items/{item_id}/jobs/instances/{job_instance_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_on_demand(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    job_type: &str,
    execution_data: Option<&str>,
    wait: bool,
    timeout_secs: u64,
    cancel_on_timeout: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "job-scheduler run-on-demand",
        &serde_json::json!({
            "workspace": workspace,
            "itemId": item_id,
            "jobType": job_type,
            "wait": wait,
            "timeout": timeout_secs,
            "cancelOnTimeout": cancel_on_timeout
        }),
    ) {
        return Ok(());
    }

    let exec_value: Option<Value> = if let Some(ed) = execution_data {
        let json_str = if let Some(file_path) = ed.strip_prefix('@') {
            std::fs::read_to_string(file_path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{file_path}': {e}"))?
        } else {
            ed.to_string()
        };
        Some(
            serde_json::from_str(&json_str)
                .map_err(|e| anyhow::anyhow!("Invalid --execution-data JSON: {e}"))?,
        )
    } else {
        None
    };

    let job_id = client
        .trigger_item_job(workspace, item_id, job_type, exec_value.as_ref())
        .await
        .map_err(|e| enrich_forbidden(e, "job-scheduler run-on-demand", "Contributor"))?;

    // Record in local job ledger
    let entry = JobEntry::new(&job_id, &format!("job-{job_type}"), workspace, item_id);
    let _ = JobLedger::append(&entry);

    if !wait {
        let obj = serde_json::json!({
            "itemId": item_id,
            "jobId": job_id,
            "jobType": job_type,
            "status": "accepted"
        });
        output::render_object(cli, &obj, "jobId");
        return Ok(());
    }

    poll_job_to_completion(
        cli,
        client,
        workspace,
        item_id,
        job_type,
        &job_id,
        timeout_secs,
        cancel_on_timeout,
    )
    .await
}

/// Poll a job instance until it reaches a terminal state (Completed/Failed/Cancelled).
#[allow(clippy::too_many_arguments)]
async fn poll_job_to_completion(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    job_type: &str,
    job_id: &str,
    timeout_secs: u64,
    cancel_on_timeout: bool,
) -> Result<()> {
    let poll_interval = Duration::from_secs(5);
    let max_wait = Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > max_wait {
            return handle_job_timeout(
                client,
                workspace,
                item_id,
                job_id,
                timeout_secs,
                cancel_on_timeout,
            )
            .await;
        }

        sleep(poll_interval).await;

        let data = client
            .get(&format!(
                "/workspaces/{workspace}/items/{item_id}/jobs/instances/{job_id}"
            ))
            .await?;

        let status_str = data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");

        match status_str {
            "Completed" => {
                let _ = JobLedger::update(job_id, "completed", None);
                let obj = serde_json::json!({
                    "itemId": item_id,
                    "jobId": job_id,
                    "jobType": job_type,
                    "status": "Completed"
                });
                output::render_object(cli, &obj, "status");
                return Ok(());
            }
            "Failed" => {
                let message = data
                    .get("failureReason")
                    .and_then(|r| r.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("Job failed");
                let _ = JobLedger::update(job_id, "failed", Some(message));
                return Err(FabioError::with_hint(
                    ErrorCode::ApiError,
                    format!("Job failed: {message}"),
                    format!("Job ID: {job_id}"),
                )
                .into());
            }
            "Cancelled" => {
                let _ = JobLedger::update(job_id, "cancelled", None);
                return Err(FabioError::new(
                    ErrorCode::ApiError,
                    format!("Job was cancelled. Job ID: {job_id}"),
                )
                .into());
            }
            // NotStarted, InProgress, Deduped — keep polling
            _ => {}
        }
    }
}

/// Handle timeout: optionally cancel the job, then return a Timeout error.
async fn handle_job_timeout(
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    job_id: &str,
    timeout_secs: u64,
    cancel_on_timeout: bool,
) -> Result<()> {
    if cancel_on_timeout {
        eprintln!("Job timed out after {timeout_secs}s. Cancelling job {job_id}...");
        let cancel_result = client
            .post(
                &format!("/workspaces/{workspace}/items/{item_id}/jobs/instances/{job_id}/cancel"),
                &serde_json::json!({}),
                false,
            )
            .await;
        let _ = JobLedger::update(job_id, "cancelled", Some("timeout+cancel"));
        if let Err(e) = cancel_result {
            eprintln!("Warning: cancel request failed: {e}");
        }
        return Err(FabioError::with_hint(
            ErrorCode::Timeout,
            format!("Job timed out after {timeout_secs}s and was cancelled. Job ID: {job_id}"),
            "Increase --timeout or remove --cancel-on-timeout to leave job running".to_string(),
        )
        .into());
    }
    let _ = JobLedger::update(job_id, "timeout", None);
    Err(FabioError::with_hint(
        ErrorCode::Timeout,
        format!(
            "Job timed out after {timeout_secs}s. Job ID: {job_id}. The job is still running."
        ),
        format!("Check status: fabio job-scheduler get-instance --workspace {workspace} --id {item_id} --job-instance-id {job_id}"),
    )
    .into())
}

async fn cancel_instance(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    job_instance_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "job-scheduler cancel-instance",
        &serde_json::json!({
            "workspace": workspace,
            "itemId": item_id,
            "jobInstanceId": job_instance_id
        }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!(
                "/workspaces/{workspace}/items/{item_id}/jobs/instances/{job_instance_id}/cancel"
            ),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "job-scheduler cancel-instance", "Contributor"))?;

    let obj = serde_json::json!({
        "jobInstanceId": job_instance_id,
        "status": "cancelled"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Schedules ───────────────────────────────────────────────────────────────

async fn list_schedules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    job_type: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/items/{item_id}/jobs/{job_type}/schedules"),
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
    item_id: &str,
    job_type: &str,
    schedule_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/items/{item_id}/jobs/{job_type}/schedules/{schedule_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    job_type: &str,
    enabled: bool,
    config: &str,
) -> Result<()> {
    let config_value: Value = serde_json::from_str(config)
        .map_err(|e| anyhow::anyhow!("Invalid --config JSON: {e}. Expected schedule configuration, e.g.: {{\"type\":\"Cron\",\"expression\":\"0 0 * * *\",\"timezone\":\"UTC\"}}"))?;

    let mut body = serde_json::json!({
        "enabled": enabled,
    });
    // Merge config into body at top-level (Fabric API expects flat schedule object)
    if let Some(config_obj) = config_value.as_object() {
        for (k, v) in config_obj {
            body[k] = v.clone();
        }
    } else {
        body["configuration"] = config_value;
    }

    if output::dry_run_guard(cli, "job-scheduler create-schedule", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/items/{item_id}/jobs/{job_type}/schedules"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "job-scheduler create-schedule", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    job_type: &str,
    schedule_id: &str,
    enabled: Option<bool>,
    config: Option<&str>,
) -> Result<()> {
    if enabled.is_none() && config.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --enabled or --config must be provided".to_string(),
            "Example: fabio job-scheduler update-schedule --workspace <WS> --id <ID> --schedule-id <SID> --enabled false".to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(e) = enabled {
        body["enabled"] = Value::Bool(e);
    }
    if let Some(c) = config {
        let config_value: Value =
            serde_json::from_str(c).map_err(|e| anyhow::anyhow!("Invalid --config JSON: {e}"))?;
        if let Some(config_obj) = config_value.as_object() {
            for (k, v) in config_obj {
                body[k] = v.clone();
            }
        } else {
            body["configuration"] = config_value;
        }
    }

    if output::dry_run_guard(cli, "job-scheduler update-schedule", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!(
                "/workspaces/{workspace}/items/{item_id}/jobs/{job_type}/schedules/{schedule_id}"
            ),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "job-scheduler update-schedule", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    job_type: &str,
    schedule_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "job-scheduler delete-schedule",
        &serde_json::json!({
            "workspace": workspace,
            "itemId": item_id,
            "jobType": job_type,
            "scheduleId": schedule_id
        }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/items/{item_id}/jobs/{job_type}/schedules/{schedule_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "job-scheduler delete-schedule", "Admin"))?;

    let obj = serde_json::json!({
        "scheduleId": schedule_id,
        "status": "deleted"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_job_types_are_non_empty() {
        assert!(!KNOWN_JOB_TYPES.is_empty());
        for t in KNOWN_JOB_TYPES {
            assert!(!t.is_empty());
        }
    }

    #[test]
    fn run_on_demand_variant_has_wait_and_timeout() {
        // Verify the RunOnDemand variant can be constructed with all fields
        let cmd = JobSchedulerCommand::RunOnDemand {
            workspace: "ws-id".to_string(),
            id: "item-id".to_string(),
            job_type: "Pipeline".to_string(),
            execution_data: Some(r#"{"tableName":"test"}"#.to_string()),
            wait: true,
            timeout: 300,
            cancel_on_timeout: true,
        };
        match &cmd {
            JobSchedulerCommand::RunOnDemand {
                wait,
                timeout,
                cancel_on_timeout,
                ..
            } => {
                assert!(*wait);
                assert_eq!(*timeout, 300);
                assert!(*cancel_on_timeout);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn run_on_demand_defaults() {
        // Verify defaults match expected behavior
        let cmd = JobSchedulerCommand::RunOnDemand {
            workspace: "ws".to_string(),
            id: "id".to_string(),
            job_type: "DefaultJob".to_string(),
            execution_data: None,
            wait: false,
            timeout: 600,
            cancel_on_timeout: false,
        };
        match &cmd {
            JobSchedulerCommand::RunOnDemand {
                wait,
                timeout,
                cancel_on_timeout,
                job_type,
                execution_data,
                ..
            } => {
                assert!(!*wait);
                assert_eq!(*timeout, 600);
                assert!(!*cancel_on_timeout);
                assert_eq!(job_type, "DefaultJob");
                assert!(execution_data.is_none());
            }
            _ => panic!("Wrong variant"),
        }
    }
}
