use std::time::Duration;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64;
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
    after_help = "Before using this command, run: fabio context examples notebook\nAlso available: fabio context schema Notebook | fabio context workflow lakehouse-etl"
)]
pub enum NotebookCommand {
    // ── Lifecycle ────────────────────────────────────────────────────────
    /// List notebooks in a workspace
    #[command(display_order = 0)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a notebook
    #[command(display_order = 0)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,
    },
    /// Create a new notebook
    #[command(display_order = 1)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook display name
        #[arg(long)]
        name: String,

        /// Notebook content (Python/PySpark code inline)
        #[arg(long, visible_alias = "source", conflicts_with = "file")]
        content: Option<String>,

        /// Path to .py or .ipynb file (auto-detected: .py is wrapped into ipynb; .ipynb is sent directly)
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,

        /// Default lakehouse ID (binds the notebook so relative paths like Files/ and Tables/ work)
        #[arg(long)]
        lakehouse: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update notebook properties (name and/or description)
    #[command(display_order = 2)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Get the definition (source code) of a notebook
    #[command(display_order = 3)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,

        /// Strip cell outputs and execution counts (useful for version control)
        #[arg(long)]
        strip_output: bool,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition (source code) of a notebook
    #[command(display_order = 4)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,

        /// Python/PySpark code content (replaces entire notebook)
        #[arg(long)]
        content: Option<String>,

        /// Path to .ipynb or .py file
        #[arg(long)]
        file: Option<String>,
    },
    /// Delete a notebook
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Execution ────────────────────────────────────────────────────────
    /// Run a notebook
    #[command(display_order = 10)]
    Run {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,

        /// Notebook parameters as JSON array (e.g., '[{"name":"p1","value":"v1","type":"Text"}]')
        #[arg(long)]
        parameters: Option<String>,

        /// Compute type: `Spark` (default), `Jupyter`, or `DataWarehouse`
        #[arg(long)]
        compute_type: Option<String>,

        /// Full execution data as JSON (advanced; overrides --compute-type). Supports @file.json or @- for stdin.
        #[arg(long)]
        execution_data: Option<String>,

        /// Wait for the notebook run to complete (polls until finished)
        #[arg(long)]
        wait: bool,

        /// Maximum time to wait in seconds (default: 600)
        #[arg(long, default_value = "600")]
        timeout: u64,

        /// Cancel the notebook run if timeout is reached
        #[arg(long)]
        cancel_on_timeout: bool,
    },
    /// Check the status of a notebook run
    #[command(display_order = 11)]
    Status {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,

        /// Job instance ID
        #[arg(long)]
        job_id: String,
    },
    /// Get details of a specific job instance
    #[command(name = "get-job-instance", display_order = 12)]
    GetJobInstance {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,

        /// Job instance ID
        #[arg(long)]
        job_instance_id: String,
    },
    /// Stop a running notebook
    #[command(display_order = 13)]
    Stop {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,

        /// Job instance ID
        #[arg(long)]
        job_id: String,
    },

    // ── Livy Sessions ────────────────────────────────────────────────────
    /// List Livy sessions for a notebook
    #[command(name = "list-livy-sessions", display_order = 15)]
    ListLivySessions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,
    },
    /// Get details of a Livy session
    #[command(name = "get-livy-session", display_order = 16)]
    GetLivySession {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Notebook ID
        #[arg(long)]
        id: String,

        /// Livy session ID
        #[arg(long)]
        livy_id: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &NotebookCommand) -> Result<()> {
    match command {
        NotebookCommand::List { workspace } => list(cli, client, workspace).await,
        NotebookCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        NotebookCommand::Create {
            workspace,
            name,
            content,
            file,
            lakehouse,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                content.as_deref(),
                file.as_deref(),
                lakehouse.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        NotebookCommand::Update {
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
        NotebookCommand::GetDefinition {
            workspace,
            id,
            strip_output,
            decode,
        } => get_definition(cli, client, workspace, id, *strip_output, *decode).await,
        NotebookCommand::UpdateDefinition {
            workspace,
            id,
            content,
            file,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                content.as_deref(),
                file.as_deref(),
            )
            .await
        }
        NotebookCommand::Run {
            workspace,
            id,
            parameters,
            compute_type,
            execution_data,
            wait,
            timeout,
            cancel_on_timeout,
        } => {
            run(
                cli,
                client,
                workspace,
                id,
                parameters.as_deref(),
                compute_type.as_deref(),
                execution_data.as_deref(),
                *wait,
                *timeout,
                *cancel_on_timeout,
            )
            .await
        }
        NotebookCommand::Status {
            workspace,
            id,
            job_id,
        } => status(cli, client, workspace, id, job_id).await,
        NotebookCommand::Stop {
            workspace,
            id,
            job_id,
        } => stop(cli, client, workspace, id, job_id).await,
        NotebookCommand::GetJobInstance {
            workspace,
            id,
            job_instance_id,
        } => get_job_instance(cli, client, workspace, id, job_instance_id).await,
        NotebookCommand::ListLivySessions { workspace, id } => {
            list_livy_sessions(cli, client, workspace, id).await
        }
        NotebookCommand::GetLivySession {
            workspace,
            id,
            livy_id,
        } => get_livy_session(cli, client, workspace, id, livy_id).await,
        NotebookCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "notebooks",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "notebooks", workspace, id).await
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
            "Example: fabio notebook update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "notebook update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/notebooks/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "notebook update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    content: Option<&str>,
    file: Option<&str>,
) -> Result<()> {
    if content.is_none() && file.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --content or --file must be provided".to_string(),
            "Example: fabio notebook update-definition --workspace <WS> --id <ID> --content \"print('hello')\""
                .to_string(),
        )
        .into());
    }

    // Build the definition payload. A `--file` that is a full definition
    // envelope (e.g. notebook-content.ipynb + notebook-settings.json) is passed
    // through verbatim; otherwise the file/content is treated as notebook source
    // and wrapped as notebook-content.py (with shared format detection).
    let envelope_raw = file
        .and_then(|p| std::fs::read_to_string(p).ok())
        .filter(|raw| {
            serde_json::from_str::<serde_json::Value>(raw).is_ok_and(|v| {
                v.get("definition")
                    .and_then(|d| d.get("parts"))
                    .or_else(|| v.get("parts"))
                    .and_then(serde_json::Value::as_array)
                    .is_some()
            })
        });
    let body = if let Some(raw) = envelope_raw.as_deref() {
        crate::definition_spec::build_update_definition_body(raw, "notebook-content.py")
    } else {
        let encoded = if let Some(file_path) = file {
            encode_notebook_file(file_path, workspace, None)?
        } else {
            encode_notebook_code(content.unwrap(), workspace, None)
        };
        serde_json::json!({
            "definition": {
                "format": "ipynb",
                "parts": [{
                    "path": "notebook-content.py",
                    "payload": encoded,
                    "payloadType": "InlineBase64"
                }]
            }
        })
    };

    if output::dry_run_guard(cli, "notebook update-definition", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "notebook update-definition", "Contributor"))?;

    let obj = serde_json::json!({
        "id": id,
        "workspace": workspace,
        "status": "definition_updated"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Notebook Format Helpers ─────────────────────────────────────────────────

/// Build ipynb metadata with optional lakehouse binding.
fn build_notebook_metadata(workspace: &str, lakehouse: Option<&str>) -> Value {
    lakehouse.map_or_else(
        || {
            serde_json::json!({
                "language_info": { "name": "python" }
            })
        },
        |lh_id| {
            // Include Fabric trident metadata to bind the default lakehouse.
            // Without this, relative paths (Files/, Tables/) and saveAsTable() won't work.
            serde_json::json!({
                "language_info": { "name": "python" },
                "trident": {
                    "lakehouse": {
                        "default_lakehouse": lh_id,
                        "default_lakehouse_name": "",
                        "default_lakehouse_workspace_id": workspace,
                        "known_lakehouses": [{ "id": lh_id }]
                    }
                }
            })
        },
    )
}

/// Encode inline Python/PySpark code into a base64 ipynb payload.
///
/// Wraps the code into a minimal Jupyter notebook JSON structure with a single
/// code cell. The `source` field is an array of line strings (Fabric requirement).
fn encode_notebook_code(code: &str, workspace: &str, lakehouse: Option<&str>) -> String {
    let metadata = build_notebook_metadata(workspace, lakehouse);
    let notebook_json = serde_json::json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": metadata,
        "cells": [{
            "cell_type": "code",
            "metadata": {},
            "source": code.lines().map(|l| format!("{l}\n")).collect::<Vec<_>>(),
            "outputs": []
        }]
    });
    base64::Engine::encode(
        &BASE64,
        serde_json::to_string(&notebook_json).expect("notebook JSON serialization cannot fail"),
    )
}

/// Read a .py or .ipynb file and encode it as a base64 ipynb payload.
///
/// Format detection logic:
/// - If the file content is valid JSON with `"nbformat"` key → treated as .ipynb (sent as-is)
/// - Otherwise → treated as Python code, wrapped into ipynb JSON structure
///
/// This means agents can pass EITHER format and it always works correctly.
fn encode_notebook_file(
    file_path: &str,
    workspace: &str,
    lakehouse: Option<&str>,
) -> Result<String> {
    let file_content = std::fs::read(file_path).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Failed to read file '{file_path}': {e}"),
            "Provide a valid .py or .ipynb file path.",
        )
    })?;

    // Try to detect if this is a valid ipynb (JSON with nbformat key)
    if let Ok(parsed) = serde_json::from_slice::<Value>(&file_content)
        && parsed.get("nbformat").is_some()
    {
        // Valid .ipynb — send the raw bytes as-is (base64-encode the JSON content)
        return Ok(base64::Engine::encode(&BASE64, &file_content));
    }

    // Not a valid ipynb → treat as Python source code, wrap into ipynb structure
    let code = String::from_utf8(file_content).map_err(|_| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("File '{file_path}' is not valid UTF-8 text"),
            "Notebook files must be UTF-8 encoded Python (.py) or Jupyter (.ipynb) files.",
        )
    })?;

    Ok(encode_notebook_code(&code, workspace, lakehouse))
}

// ─── Create ──────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    content: Option<&str>,
    file: Option<&str>,
    lakehouse: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let encoded = if let Some(file_path) = file {
        // Read file and auto-detect format from content
        encode_notebook_file(file_path, workspace, lakehouse)?
    } else {
        // Build ipynb from inline content (or default)
        let code = content.unwrap_or("# New notebook\nprint('Hello from Fabric!')");
        encode_notebook_code(code, workspace, lakehouse)
    };

    let mut body = serde_json::json!({
        "displayName": name,
        "type": "Notebook",
        "definition": {
            "format": "ipynb",
            "parts": [{
                "path": "notebook-content.py",
                "payload": encoded,
                "payloadType": "InlineBase64"
            }]
        }
    });
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(cli, "notebook create", &body) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/items"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "notebook create", "Member"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    strip_output: bool,
    decode: bool,
) -> Result<()> {
    let mut data = client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await?;

    if strip_output {
        strip_notebook_outputs(&mut data);
    }

    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    parameters: Option<&str>,
    compute_type: Option<&str>,
    execution_data: Option<&str>,
    wait: bool,
    timeout_secs: u64,
    cancel_on_timeout: bool,
) -> Result<()> {
    // Build request body from parameters/compute-type/execution-data
    let body = build_run_body(parameters, compute_type, execution_data)?;

    if output::dry_run_guard(
        cli,
        "notebook run",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "wait": wait,
            "timeout": timeout_secs,
            "body": body
        }),
    ) {
        return Ok(());
    }

    let job_id = client
        .run_notebook(
            workspace,
            id,
            if body.is_null() { None } else { Some(&body) },
        )
        .await
        .map_err(|e| enrich_forbidden(e, "notebook run", "Contributor"))?;

    // Record job in local ledger
    let entry = JobEntry::new(&job_id, "notebook-run", workspace, id);
    let _ = JobLedger::append(&entry);

    if !wait {
        let obj = serde_json::json!({
            "itemId": id,
            "jobId": job_id,
            "status": "started"
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
                let cancel_path =
                    format!("/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}/cancel");
                let _ = client
                    .post(&cancel_path, &serde_json::json!({}), false)
                    .await;
            }
            let _ = JobLedger::update(&job_id, "timeout", None);
            return Err(FabioError::with_hint(
                ErrorCode::Timeout,
                format!(
                    "Notebook run timed out after {timeout_secs}s. Job ID: {job_id}. Use 'notebook status' to check progress."
                ),
                format!("Increase --timeout (current: {timeout_secs}s) or use --cancel-on-timeout"),
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
                    .unwrap_or("Notebook run failed");
                let _ = JobLedger::update(&job_id, "failed", Some(message));
                return Err(FabioError::with_hint(ErrorCode::ApiError, message, "Check Spark logs in the Fabric portal for details. Common causes: out-of-memory, missing dependencies, or code errors.").into());
            }
            "Cancelled" => {
                let _ = JobLedger::update(&job_id, "cancelled", None);
                return Err(FabioError::with_hint(
                    ErrorCode::ApiError,
                    "Notebook run was cancelled",
                    "Re-run with: fabio notebook run --workspace <WS> --id <ID> --wait",
                )
                .into());
            }
            // NotStarted, InProgress, Deduped - keep polling
            _ => {}
        }
    }
}

async fn status(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    job_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}"
        ))
        .await?;
    output::render_object(cli, &data, "status");
    Ok(())
}

async fn stop(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    job_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "notebook stop",
        &serde_json::json!({ "workspace": workspace, "id": id, "jobId": job_id }),
    ) {
        return Ok(());
    }
    client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}/cancel"),
            &serde_json::json!({}),
            false,
        )
        .await?;

    let obj = serde_json::json!({
        "jobId": job_id,
        "status": "cancelled"
    });
    output::render_object(cli, &obj, "status");
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
        "notebook",
        "items",
        "Member",
        workspace,
        id,
        hard_delete,
    )
    .await
}

// ─── Job Instance & Livy Sessions ────────────────────────────────────────────

async fn get_job_instance(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    job_instance_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/notebooks/{id}/jobs/execute/instances/{job_instance_id}?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "status");
    Ok(())
}

async fn list_livy_sessions(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/notebooks/{id}/livySessions"
        ))
        .await?;

    if let Some(arr) = data.get("value").and_then(|v| v.as_array()) {
        output::render_list_with_token(
            cli,
            arr,
            &["id", "state", "appId"],
            &["ID", "STATE", "APP_ID"],
            "id",
            None,
        );
    } else {
        output::render_object(cli, &data, "sessions");
    }
    Ok(())
}

async fn get_livy_session(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    livy_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/notebooks/{id}/livySessions/{livy_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

// ─── Utilities ───────────────────────────────────────────────────────────────

/// Strip cell outputs and execution counts from notebook definition parts.
///
/// Walks through the `definition.parts` array, finds the `notebook-content.py`
/// part, decodes its base64 payload as ipynb JSON, clears `outputs` and
/// `execution_count` on every cell, then re-encodes the payload.
fn strip_notebook_outputs(data: &mut Value) {
    let base64_engine = BASE64;

    let Some(parts) = data
        .get_mut("definition")
        .and_then(|d| d.get_mut("parts"))
        .and_then(|p| p.as_array_mut())
    else {
        return;
    };

    for part in parts {
        let path = part
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or_default();

        // Only process the notebook content part
        if !path.contains("notebook-content") {
            continue;
        }

        let payload_str = match part.get("payload").and_then(|p| p.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        // Decode base64
        let Ok(decoded) = base64::Engine::decode(&base64_engine, &payload_str) else {
            continue;
        };

        // Parse as UTF-8 string, then as JSON
        let Ok(text) = String::from_utf8(decoded) else {
            continue;
        };

        let mut notebook: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Strip outputs and execution_count from all cells
        if let Some(cells) = notebook.get_mut("cells").and_then(|c| c.as_array_mut()) {
            for cell in cells {
                if let Some(obj) = cell.as_object_mut() {
                    obj.insert("outputs".to_string(), Value::Array(vec![]));
                    obj.remove("execution_count");
                }
            }
        }

        // Re-encode to base64 and update the part
        let cleaned_json = serde_json::to_string(&notebook).unwrap_or_default();
        let encoded = base64::Engine::encode(&base64_engine, cleaned_json.as_bytes());
        part["payload"] = Value::from(encoded);
    }
}

/// Resolve a CLI value that may be inline JSON, `@file.json`, or `@-` (stdin).
fn resolve_json_input(input: &str, flag_name: &str) -> Result<String> {
    if input == "@-" {
        Ok(std::io::read_to_string(std::io::stdin())?)
    } else if let Some(file_path) = input.strip_prefix('@') {
        std::fs::read_to_string(file_path).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read file '{file_path}': {e}"),
                format!("Provide a valid file path after @, e.g.: {flag_name} @path/to/file.json"),
            )
            .into()
        })
    } else {
        Ok(input.to_string())
    }
}

/// Build the request body for notebook run from CLI flags.
fn build_run_body(
    parameters: Option<&str>,
    compute_type: Option<&str>,
    execution_data: Option<&str>,
) -> Result<Value> {
    // If nothing specified, return null (empty body)
    if parameters.is_none() && compute_type.is_none() && execution_data.is_none() {
        return Ok(Value::Null);
    }

    let mut body = serde_json::json!({});

    // Parse parameters
    if let Some(params_str) = parameters {
        let resolved = resolve_json_input(params_str, "--parameters")?;
        let params: Value = serde_json::from_str(resolved.trim()).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --parameters JSON: {e}"),
                r#"Expected JSON array, e.g.: [{"name":"p1","value":"v1","type":"Text"}]. Supports @file.json or @- for stdin."#
                    .to_string(),
            )
        })?;
        body["parameters"] = params;
    }

    // Build executionData
    if let Some(ed_str) = execution_data {
        let resolved = resolve_json_input(ed_str, "--execution-data")?;
        // Full execution data overrides --compute-type
        let ed: Value = serde_json::from_str(resolved.trim()).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --execution-data JSON: {e}"),
                r#"Expected JSON object, e.g.: {"compute":"Spark","computeConfiguration":{...}}. Supports @file.json or @- for stdin."#
                    .to_string(),
            )
        })?;
        body["executionData"] = ed;
    } else if let Some(ct) = compute_type {
        // Validate compute type
        match ct {
            "Spark" | "Jupyter" | "DataWarehouse" => {}
            _ => {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!(
                        "Invalid --compute-type '{ct}'. Valid values: Spark, Jupyter, DataWarehouse"
                    ),
                    "Example: fabio notebook run --workspace <WS> --id <ID> --compute-type Spark"
                        .to_string(),
                )
                .into());
            }
        }
        body["executionData"] = serde_json::json!({ "compute": ct });
    }

    Ok(body)
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::*;
    use serde_json::json;

    #[test]
    fn strip_notebook_outputs_clears_cells() {
        let base64_engine = BASE64;
        let notebook = json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {},
            "cells": [
                {
                    "cell_type": "code",
                    "source": ["print('hello')\n"],
                    "outputs": [{"output_type": "stream", "text": ["hello\n"]}],
                    "execution_count": 1
                },
                {
                    "cell_type": "code",
                    "source": ["x = 42\n"],
                    "outputs": [{"output_type": "execute_result", "data": {"text/plain": ["42"]}}],
                    "execution_count": 2
                }
            ]
        });

        let payload = base64_engine.encode(serde_json::to_string(&notebook).unwrap());
        let mut data = json!({
            "definition": {
                "parts": [
                    {
                        "path": "notebook-content.py",
                        "payload": payload,
                        "payloadType": "InlineBase64"
                    }
                ]
            }
        });

        strip_notebook_outputs(&mut data);

        // Decode the result and verify outputs are stripped
        let result_payload = data["definition"]["parts"][0]["payload"].as_str().unwrap();
        let decoded = base64_engine.decode(result_payload).unwrap();
        let result: Value = serde_json::from_slice(&decoded).unwrap();

        let cells = result["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 2);
        // outputs should be empty arrays
        assert_eq!(cells[0]["outputs"], json!([]));
        assert_eq!(cells[1]["outputs"], json!([]));
        // execution_count should be removed
        assert!(cells[0].get("execution_count").is_none());
        assert!(cells[1].get("execution_count").is_none());
        // source should be preserved
        assert_eq!(cells[0]["source"], json!(["print('hello')\n"]));
    }

    #[test]
    fn strip_notebook_outputs_preserves_non_notebook_parts() {
        let mut data = json!({
            "definition": {
                "parts": [
                    {
                        "path": ".platform",
                        "payload": "eyJ0ZXN0IjogdHJ1ZX0=",
                        "payloadType": "InlineBase64"
                    }
                ]
            }
        });

        let original = data.clone();
        strip_notebook_outputs(&mut data);
        // Non-notebook parts should be unchanged
        assert_eq!(data, original);
    }

    #[test]
    fn strip_notebook_outputs_handles_no_definition() {
        let mut data = json!({"id": "test"});
        strip_notebook_outputs(&mut data);
        // Should not panic
        assert_eq!(data, json!({"id": "test"}));
    }

    #[test]
    fn resolve_json_input_inline_passthrough() {
        let input = r#"{"compute":"Spark"}"#;
        let result = resolve_json_input(input, "--execution-data").unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn resolve_json_input_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("exec.json");
        std::fs::write(&file_path, r#"{"compute":"Jupyter"}"#).unwrap();

        let input = format!("@{}", file_path.display());
        let result = resolve_json_input(&input, "--execution-data").unwrap();
        assert_eq!(result, r#"{"compute":"Jupyter"}"#);
    }

    #[test]
    fn resolve_json_input_file_not_found() {
        let result = resolve_json_input("@/nonexistent/path.json", "--execution-data");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to read file"));
    }

    #[test]
    fn build_run_body_execution_data_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("exec_data.json");
        std::fs::write(
            &file_path,
            r#"{"compute":"Spark","computeConfiguration":{"x":1}}"#,
        )
        .unwrap();

        let input = format!("@{}", file_path.display());
        let body = build_run_body(None, None, Some(&input)).unwrap();
        assert_eq!(
            body["executionData"],
            json!({"compute":"Spark","computeConfiguration":{"x":1}})
        );
    }

    #[test]
    fn build_run_body_parameters_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("params.json");
        std::fs::write(&file_path, r#"[{"name":"p1","value":"v1","type":"Text"}]"#).unwrap();

        let input = format!("@{}", file_path.display());
        let body = build_run_body(Some(&input), None, None).unwrap();
        assert_eq!(
            body["parameters"],
            json!([{"name":"p1","value":"v1","type":"Text"}])
        );
    }

    #[test]
    fn build_run_body_execution_data_invalid_file_json() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("bad.json");
        std::fs::write(&file_path, "not valid json").unwrap();

        let input = format!("@{}", file_path.display());
        let result = build_run_body(None, None, Some(&input));
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid --execution-data JSON"));
    }

    #[test]
    fn build_run_body_inline_still_works() {
        let body = build_run_body(None, None, Some(r#"{"compute":"Spark"}"#)).unwrap();
        assert_eq!(body["executionData"], json!({"compute":"Spark"}));
    }

    // ─── encode_notebook_code / encode_notebook_file tests ───────────────────

    #[test]
    fn encode_notebook_code_produces_valid_ipynb() {
        let encoded = encode_notebook_code("print('hello')\nx = 42", "ws-1", None);
        let decoded = String::from_utf8(BASE64.decode(&encoded).unwrap()).unwrap();
        let nb: Value = serde_json::from_str(&decoded).unwrap();

        assert_eq!(nb["nbformat"], 4);
        assert_eq!(nb["nbformat_minor"], 5);
        assert_eq!(nb["metadata"]["language_info"]["name"], "python");
        // Source must be array of strings (Fabric requirement)
        let source = nb["cells"][0]["source"].as_array().unwrap();
        assert_eq!(source[0], "print('hello')\n");
        assert_eq!(source[1], "x = 42\n");
    }

    #[test]
    fn encode_notebook_code_with_lakehouse_includes_trident() {
        let encoded = encode_notebook_code("x = 1", "ws-1", Some("lh-123"));
        let decoded = String::from_utf8(BASE64.decode(&encoded).unwrap()).unwrap();
        let nb: Value = serde_json::from_str(&decoded).unwrap();

        let trident = &nb["metadata"]["trident"]["lakehouse"];
        assert_eq!(trident["default_lakehouse"], "lh-123");
        assert_eq!(trident["default_lakehouse_workspace_id"], "ws-1");
    }

    #[test]
    fn encode_notebook_file_detects_ipynb() {
        use std::io::Write;
        let ipynb = json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {"language_info": {"name": "python"}},
            "cells": [{"cell_type": "code", "source": ["x = 1\n"], "outputs": [], "metadata": {}}]
        });
        let tmp = std::env::temp_dir().join("test_notebook.ipynb");
        let mut f = std::fs::File::create(&tmp).unwrap();
        f.write_all(serde_json::to_string(&ipynb).unwrap().as_bytes())
            .unwrap();

        let encoded = encode_notebook_file(tmp.to_str().unwrap(), "ws-1", None).unwrap();
        let decoded = String::from_utf8(BASE64.decode(&encoded).unwrap()).unwrap();
        let parsed: Value = serde_json::from_str(&decoded).unwrap();

        // Should be passed through as-is (valid ipynb detected)
        assert_eq!(parsed["nbformat"], 4);
        assert_eq!(parsed["cells"][0]["source"][0], "x = 1\n");

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn encode_notebook_file_wraps_py_into_ipynb() {
        use std::io::Write;
        let py_code = "# ETL script\nprint('hello')\ndf = spark.read.csv('data.csv')";
        let tmp = std::env::temp_dir().join("test_etl.py");
        let mut f = std::fs::File::create(&tmp).unwrap();
        f.write_all(py_code.as_bytes()).unwrap();

        let encoded = encode_notebook_file(tmp.to_str().unwrap(), "ws-1", None).unwrap();
        let decoded = String::from_utf8(BASE64.decode(&encoded).unwrap()).unwrap();
        let parsed: Value = serde_json::from_str(&decoded).unwrap();

        // Should be wrapped into ipynb structure
        assert_eq!(parsed["nbformat"], 4);
        let source = parsed["cells"][0]["source"].as_array().unwrap();
        assert_eq!(source[0], "# ETL script\n");
        assert_eq!(source[1], "print('hello')\n");
        assert_eq!(source[2], "df = spark.read.csv('data.csv')\n");

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn encode_notebook_file_nonexistent_returns_error() {
        let result = encode_notebook_file("/nonexistent/path.py", "ws-1", None);
        assert!(result.is_err());
    }
}
