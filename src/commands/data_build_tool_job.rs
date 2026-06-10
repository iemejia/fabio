use std::time::Duration;

use anyhow::Result;
use base64::Engine;
use clap::Subcommand;
use serde_json::Value;
use tokio::time::sleep;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::jobs::{JobEntry, JobLedger};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
pub enum DataBuildToolJobCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List data build tool jobs in a workspace [preview]
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a data build tool job [preview]
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data build tool job ID
        #[arg(long)]
        id: String,
    },
    /// Create a new data build tool job [preview]
    #[command(display_order = 3)]
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
    },
    /// Update data build tool job properties (name and/or description) [preview]
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data build tool job ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a data build tool job [preview]
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data build tool job ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a data build tool job [preview]
    #[command(display_order = 6, name = "get-definition")]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data build tool job ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a data build tool job [preview]
    #[command(display_order = 7, name = "update-definition")]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data build tool job ID
        #[arg(long)]
        id: String,

        /// Definition file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// Definition content (inline JSON)
        #[arg(long)]
        content: Option<String>,
    },

    // ── Run ──────────────────────────────────────────────────────────────
    /// Run a data build tool job on-demand [preview]
    #[command(display_order = 8)]
    Run {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data build tool job ID
        #[arg(long)]
        id: String,

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
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &DataBuildToolJobCommand,
) -> Result<()> {
    match command {
        DataBuildToolJobCommand::List { workspace } => list(cli, client, workspace).await,
        DataBuildToolJobCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        DataBuildToolJobCommand::Create {
            workspace,
            name,
            description,
        } => create(cli, client, workspace, name, description.as_deref()).await,
        DataBuildToolJobCommand::Update {
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
        DataBuildToolJobCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        DataBuildToolJobCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        DataBuildToolJobCommand::UpdateDefinition {
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
        DataBuildToolJobCommand::Run {
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

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn build_list_url(workspace: &str) -> String {
    format!("/workspaces/{workspace}/dataBuildToolJobs")
}

fn build_item_url(workspace: &str, id: &str) -> String {
    format!("/workspaces/{workspace}/dataBuildToolJobs/{id}")
}

fn build_delete_url(workspace: &str, id: &str, hard_delete: bool) -> String {
    if hard_delete {
        format!("/workspaces/{workspace}/dataBuildToolJobs/{id}?hardDelete=true")
    } else {
        build_item_url(workspace, id)
    }
}

fn build_create_body(name: &str, description: Option<&str>) -> Value {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = Value::String(desc.to_string());
    }
    body
}

fn build_update_body(name: Option<&str>, description: Option<&str>) -> Result<Value> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio data-build-tool-job update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::String(n.to_string());
    }
    if let Some(d) = description {
        body["description"] = Value::String(d.to_string());
    }
    Ok(body)
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &build_list_url(workspace),
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

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client.get(&build_item_url(workspace, id)).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
) -> Result<()> {
    let body = build_create_body(name, description);

    if output::dry_run_guard(cli, "data-build-tool-job create", &body) {
        return Ok(());
    }

    let data = client
        .post(&build_list_url(workspace), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "data-build-tool-job create", "Contributor"))?;
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
    let body = build_update_body(name, description)?;

    if output::dry_run_guard(cli, "data-build-tool-job update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&build_item_url(workspace, id), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "data-build-tool-job update", "Contributor"))?;
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
    if output::dry_run_guard(
        cli,
        "data-build-tool-job delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = build_delete_url(workspace, id, hard_delete);

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "data-build-tool-job delete", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/dataBuildToolJobs/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-build-tool-job get-definition", "Contributor"))?;

    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
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
                "Example: fabio data-build-tool-job update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            )
            .into());
        }
    };

    let encoded = base64::engine::general_purpose::STANDARD.encode(script.as_bytes());

    let body = serde_json::json!({
        "definition": {
            "parts": [
                {
                    "path": "definition.json",
                    "payload": encoded,
                    "payloadType": "InlineBase64"
                }
            ]
        }
    });

    if output::dry_run_guard(
        cli,
        "data-build-tool-job update-definition",
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
            &format!("/workspaces/{workspace}/dataBuildToolJobs/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-build-tool-job update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Run ─────────────────────────────────────────────────────────────────────

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
        "data-build-tool-job run",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "wait": wait,
            "timeout": timeout_secs,
            "cancelOnTimeout": cancel_on_timeout
        }),
    ) {
        return Ok(());
    }

    let job_id = client
        .trigger_item_job(workspace, id, "execute", None)
        .await
        .map_err(|e| enrich_forbidden(e, "data-build-tool-job run", "Member"))?;

    // Record in local job ledger
    let entry = JobEntry::new(&job_id, "data-build-tool-job-run", workspace, id);
    let _ = JobLedger::append(&entry);

    if !wait {
        let obj = serde_json::json!({
            "itemId": id,
            "jobId": job_id,
            "status": "accepted"
        });
        output::render_object(cli, &obj, "jobId");
        return Ok(());
    }

    poll_run_to_completion(
        cli,
        client,
        workspace,
        id,
        &job_id,
        timeout_secs,
        cancel_on_timeout,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn poll_run_to_completion(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    job_id: &str,
    timeout_secs: u64,
    cancel_on_timeout: bool,
) -> Result<()> {
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
                let _ = JobLedger::update(job_id, "cancelled", None);
            } else {
                let _ = JobLedger::update(job_id, "timeout", None);
            }
            return Err(FabioError::with_hint(
                ErrorCode::Timeout,
                format!("Job did not complete within {timeout_secs}s"),
                format!(
                    "Check status: fabio job-scheduler get-instance --workspace {workspace} --id {id} --job-id {job_id}"
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
                let _ = JobLedger::update(job_id, "completed", None);
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
                return Err(FabioError::with_hint(
                    ErrorCode::ApiError,
                    "Job was cancelled".to_string(),
                    format!("Job ID: {job_id}"),
                )
                .into());
            }
            _ => {} // NotStarted, InProgress, Deduped — keep polling
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_url_construction() {
        let url = build_list_url("ws-123");
        assert_eq!(url, "/workspaces/ws-123/dataBuildToolJobs");
    }

    #[test]
    fn item_url_construction() {
        let url = build_item_url("ws-123", "item-456");
        assert_eq!(url, "/workspaces/ws-123/dataBuildToolJobs/item-456");
    }

    #[test]
    fn delete_url_without_hard_delete() {
        let url = build_delete_url("ws-123", "item-456", false);
        assert_eq!(url, "/workspaces/ws-123/dataBuildToolJobs/item-456");
        assert!(!url.contains("hardDelete"));
    }

    #[test]
    fn delete_url_with_hard_delete() {
        let url = build_delete_url("ws-123", "item-456", true);
        assert!(url.contains("hardDelete=true"));
    }

    #[test]
    fn create_body_with_description() {
        let body = build_create_body("MyJob", Some("A description"));
        assert_eq!(body["displayName"], "MyJob");
        assert_eq!(body["description"], "A description");
    }

    #[test]
    fn create_body_without_description() {
        let body = build_create_body("MyJob", None);
        assert_eq!(body["displayName"], "MyJob");
        assert!(body.get("description").is_none());
    }

    #[test]
    fn update_body_name_only() {
        let body = build_update_body(Some("New Name"), None).unwrap();
        assert_eq!(body["displayName"], "New Name");
        assert!(body.get("description").is_none());
    }

    #[test]
    fn update_body_description_only() {
        let body = build_update_body(None, Some("New Desc")).unwrap();
        assert!(body.get("displayName").is_none());
        assert_eq!(body["description"], "New Desc");
    }

    #[test]
    fn update_body_both_fields() {
        let body = build_update_body(Some("Name"), Some("Desc")).unwrap();
        assert_eq!(body["displayName"], "Name");
        assert_eq!(body["description"], "Desc");
    }

    #[test]
    fn update_body_no_fields_errors() {
        let result = build_update_body(None, None);
        assert!(result.is_err());
    }
}
