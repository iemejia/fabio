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

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples dataflow\nAlso available: fabio context schema Dataflow"
)]
pub enum DataflowCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List dataflows in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a dataflow
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Dataflow ID
        #[arg(long)]
        id: String,
    },
    /// Create a new dataflow
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Dataflow display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update dataflow properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Dataflow ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a dataflow
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Dataflow ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a dataflow
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Dataflow ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a dataflow
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Dataflow ID
        #[arg(long)]
        id: String,

        /// Dataflow definition file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// Dataflow definition content (inline)
        #[arg(long)]
        content: Option<String>,
    },

    // ── Execution ────────────────────────────────────────────────────────
    /// Run a dataflow on demand
    #[command(display_order = 8)]
    Run {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Dataflow ID
        #[arg(long)]
        id: String,

        /// Job type: execute (default) or apply-changes
        #[arg(long, default_value = "execute")]
        job_type: String,

        /// Execute option (only for execute job type): `SkipApplyChanges` (default), `ApplyChangesIfNeeded`
        #[arg(long)]
        execute_option: Option<String>,

        /// Parameters JSON array for execution (e.g., '[{"parameterName":"X","type":"Automatic","value":25}]')
        #[arg(long)]
        parameters: Option<String>,

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

    // ── Parameters ───────────────────────────────────────────────────────
    /// Discover parameters of a dataflow
    #[command(name = "discover-parameters", display_order = 9)]
    DiscoverParameters {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Dataflow ID
        #[arg(long)]
        id: String,
    },
    /// Execute a query against a dataflow (returns Apache Arrow IPC)
    #[command(visible_alias = "query", display_order = 15)]
    ExecuteQuery {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Dataflow ID
        #[arg(long)]
        id: String,

        /// Name of the query to execute from the dataflow
        #[arg(long)]
        query_name: String,

        /// Optional custom mashup document (M expression) to override the dataflow's default
        #[arg(long)]
        mashup: Option<String>,

        /// Output file path (writes raw Apache Arrow IPC bytes). If not specified, reports metadata only.
        #[arg(long)]
        file: Option<String>,

        /// Apache Arrow version for response format: 1 (default) or 2
        #[arg(long, default_value = "1")]
        arrow_version: u8,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &DataflowCommand) -> Result<()> {
    match command {
        DataflowCommand::List { workspace } => list(cli, client, workspace).await,
        DataflowCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        DataflowCommand::Create {
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
        DataflowCommand::Update {
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
        DataflowCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        DataflowCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        DataflowCommand::UpdateDefinition {
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
        DataflowCommand::DiscoverParameters { workspace, id } => {
            discover_parameters(cli, client, workspace, id).await
        }
        DataflowCommand::ExecuteQuery {
            workspace,
            id,
            query_name,
            mashup,
            file,
            arrow_version,
        } => {
            execute_query(
                cli,
                client,
                workspace,
                id,
                query_name,
                mashup.as_deref(),
                file.as_deref(),
                *arrow_version,
            )
            .await
        }
        DataflowCommand::Run {
            workspace,
            id,
            job_type,
            execute_option,
            parameters,
            wait,
            timeout,
            cancel_on_timeout,
        } => {
            run(
                cli,
                client,
                workspace,
                id,
                job_type,
                execute_option.as_deref(),
                parameters.as_deref(),
                *wait,
                *timeout,
                *cancel_on_timeout,
            )
            .await
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "dataflows",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "dataflows", workspace, id).await
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

    if output::dry_run_guard(cli, "dataflow create", &body) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/dataflows"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "dataflow create", "Member"))?;
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
        "dataflow",
        "dataflows",
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
        "dataflow",
        "dataflows",
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
        "dataflow",
        "dataflows",
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
                    "Dataflow",
                    "dataflow",
                    "update-definition",
                ),
            )
            .into());
        }
    };

    let body = crate::definition_spec::build_update_definition_body(&script, "queryMetadata.json");

    if output::dry_run_guard(
        cli,
        "dataflow update-definition",
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
            &format!("/workspaces/{workspace}/dataflows/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "dataflow update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Parameters ──────────────────────────────────────────────────────────────

async fn discover_parameters(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/dataflows/{id}/parameters"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "dataflow discover-parameters", "Contributor"))?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["name", "type", "currentValue", "isRequired"],
        &["NAME", "TYPE", "CURRENT VALUE", "REQUIRED"],
        "name",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

// ─── Execution ───────────────────────────────────────────────────────────────

/// Build the hint for a failed `dataflow run`, teaching the common
/// `QueriesMetadata must not be empty` case (the `queryMetadata.json` part must
/// list every `shared` query from `mashup.pq`) and otherwise returning the
/// job id for follow-up.
fn dataflow_run_failure_hint(message: &str, job_id: &str) -> String {
    if message.contains("QueriesMetadata") {
        format!(
            "The dataflow's queryMetadata.json has an empty 'queriesMetadata'. Each `shared` query \
             in mashup.pq needs an entry, e.g. \"queriesMetadata\": {{\"<QueryName>\": \
             {{\"queryId\": \"<uuid>\", \"queryName\": \"<QueryName>\", \"loadEnabled\": true}}}}. \
             Author it, then: fabio dataflow update-definition --file <envelope.json>. Job ID: {job_id}"
        )
    } else {
        format!("Job ID: {job_id}")
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
async fn run(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    job_type: &str,
    execute_option: Option<&str>,
    parameters: Option<&str>,
    wait: bool,
    timeout_secs: u64,
    cancel_on_timeout: bool,
) -> Result<()> {
    // Validate job type
    let job_path = match job_type {
        "execute" => "execute",
        "apply-changes" | "applyChanges" => "applyChanges",
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --job-type '{job_type}'. Valid values: execute, apply-changes"),
                "Example: fabio dataflow run --workspace <WS> --id <ID> --job-type execute"
                    .to_string(),
            )
            .into());
        }
    };

    // Build execution data body
    let body = build_execution_body(job_path, execute_option, parameters)?;

    if output::dry_run_guard(
        cli,
        "dataflow run",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "jobType": job_path,
            "wait": wait,
            "timeout": timeout_secs,
            "body": body
        }),
    ) {
        return Ok(());
    }

    let url = format!("/workspaces/{workspace}/dataflows/{id}/jobs/{job_path}/instances");
    let job_id = client
        .trigger_item_job_at(&url, if body.is_null() { None } else { Some(&body) })
        .await
        .map_err(|e| enrich_forbidden(e, "dataflow run", "Member"))?;

    // Record in local job ledger
    let entry = JobEntry::new(&job_id, &format!("dataflow-{job_path}"), workspace, id);
    let _ = JobLedger::append(&entry);

    if !wait {
        let obj = serde_json::json!({
            "itemId": id,
            "jobId": job_id,
            "jobType": job_path,
            "status": "accepted"
        });
        output::render_object(cli, &obj, "jobId");
        return Ok(());
    }

    // Poll until completion
    let poll_interval = Duration::from_secs(5);
    let max_wait = Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > max_wait {
            if cancel_on_timeout {
                let _ = client
                    .post(
                        &format!(
                            "/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}/cancel"
                        ),
                        &serde_json::json!({}),
                        false,
                    )
                    .await;
                let _ = JobLedger::update(&job_id, "cancelled", None);
            } else {
                let _ = JobLedger::update(&job_id, "timeout", None);
            }
            return Err(FabioError::with_hint(
                ErrorCode::Timeout,
                format!("Dataflow run timed out after {timeout_secs}s. Job ID: {job_id}"),
                format!(
                    "Check status: fabio job-scheduler get-instance --workspace {workspace} --id {id} --job-instance-id {job_id}"
                ),
            )
            .into());
        }

        sleep(poll_interval).await;

        let data = client
            .get(&format!(
                "/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}"
            ))
            .await?;

        let status_str = data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");

        match status_str {
            "Completed" => {
                let _ = JobLedger::update(&job_id, "completed", None);
                let obj = serde_json::json!({
                    "itemId": id,
                    "jobId": job_id,
                    "jobType": job_path,
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
                let _ = JobLedger::update(&job_id, "failed", Some(message));
                return Err(FabioError::with_hint(
                    ErrorCode::ApiError,
                    format!("Dataflow run failed: {message}"),
                    dataflow_run_failure_hint(message, &job_id),
                )
                .into());
            }
            "Cancelled" => {
                let _ = JobLedger::update(&job_id, "cancelled", None);
                return Err(FabioError::new(
                    ErrorCode::ApiError,
                    format!("Dataflow run was cancelled. Job ID: {job_id}"),
                )
                .into());
            }
            _ => {} // NotStarted, InProgress, Deduped → keep polling
        }
    }
}

/// Build the request body for a dataflow run.
fn build_execution_body(
    job_path: &str,
    execute_option: Option<&str>,
    parameters: Option<&str>,
) -> Result<Value> {
    // apply-changes job doesn't support execution data
    if job_path == "applyChanges" {
        if execute_option.is_some() || parameters.is_some() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--execute-option and --parameters are only supported for execute job type"
                    .to_string(),
                "Example: fabio dataflow run --workspace <WS> --id <ID> --job-type apply-changes"
                    .to_string(),
            )
            .into());
        }
        return Ok(Value::Null);
    }

    // execute job — build executionData if options/parameters provided
    if execute_option.is_none() && parameters.is_none() {
        return Ok(Value::Null);
    }

    let mut exec_data = serde_json::json!({});

    if let Some(opt) = execute_option {
        match opt {
            "SkipApplyChanges" | "ApplyChangesIfNeeded" => {
                exec_data["executeOption"] = Value::from(opt);
            }
            _ => {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!(
                        "Invalid --execute-option '{opt}'. Valid values: SkipApplyChanges, ApplyChangesIfNeeded"
                    ),
                    "Example: fabio dataflow run --workspace <WS> --id <ID> --execute-option ApplyChangesIfNeeded".to_string(),
                ).into());
            }
        }
    }

    if let Some(params_str) = parameters {
        let params: Value = serde_json::from_str(params_str).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --parameters JSON: {e}"),
                "Expected JSON array, e.g.: [{\"parameterName\":\"X\",\"type\":\"Automatic\",\"value\":25}]".to_string(),
            )
        })?;
        exec_data["parameters"] = params;
    }

    Ok(serde_json::json!({ "executionData": exec_data }))
}

// ─── Execute Query ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn execute_query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    query_name: &str,
    mashup: Option<&str>,
    file: Option<&str>,
    arrow_version: u8,
) -> Result<()> {
    let mut body = serde_json::json!({ "queryName": query_name });
    if let Some(m) = mashup {
        body["customMashupDocument"] = Value::from(m);
    }

    if output::dry_run_guard(cli, "dataflow execute-query", &body) {
        return Ok(());
    }

    // The executeQuery endpoint is LRO-aware (may return 202).
    // Use post_fabric_bytes_lro which handles both 200 (immediate) and 202 (poll).
    let accept = format!("application/vnd.apache.arrow.stream;pq-arrow-version={arrow_version}");
    let bytes = client
        .post_fabric_bytes_with_accept(
            &format!("/workspaces/{workspace}/dataflows/{id}/executeQuery"),
            &body,
            &accept,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "dataflow execute-query", "Contributor"))?;

    if let Some(path) = file {
        // Create parent dirs if needed
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &bytes)?;
        let obj = serde_json::json!({
            "status": "written",
            "file": path,
            "sizeBytes": bytes.len(),
            "format": format!("apache-arrow-ipc-v{arrow_version}")
        });
        output::render_object(cli, &obj, "status");
    } else {
        let obj = serde_json::json!({
            "status": "executed",
            "queryName": query_name,
            "sizeBytes": bytes.len(),
            "format": format!("apache-arrow-ipc-v{arrow_version}")
        });
        output::render_object(cli, &obj, "status");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::dataflow_run_failure_hint;

    #[test]
    fn queries_metadata_failure_teaches_the_shape() {
        let hint = dataflow_run_failure_hint(
            "Invalid QueriesMetadata for dataflow with id X. QueriesMetadata must not be empty",
            "job-1",
        );
        assert!(hint.contains("queriesMetadata"), "got: {hint}");
        assert!(hint.contains("loadEnabled"), "got: {hint}");
        assert!(hint.contains("job-1"));
    }

    #[test]
    fn other_failure_returns_job_id_only() {
        let hint = dataflow_run_failure_hint("some other failure", "job-2");
        assert_eq!(hint, "Job ID: job-2");
    }
}
