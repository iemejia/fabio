//! Shared "run an item job on demand, optionally wait for completion" helper.
//!
//! `copy-job run`, `data-pipeline run`, and `spark-job-definition run` all
//! trigger an on-demand job, and (with `--wait`) poll the job instance until it
//! reaches a terminal state, cancelling on timeout when asked. The bodies were
//! byte-identical except for the op-name, the `trigger_item_job` job-type, the
//! message label, and the role — captured here as [`RunSpec`].

use serde_json::Value;

use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

/// Per-command parameters for [`run_and_wait`].
pub struct RunSpec<'a> {
    /// Canonical op-name (`"<group> run"`) for the dry-run guard + error hints.
    pub op: &'a str,
    /// Human label used in status/timeout messages (e.g. `"Copy job run"`).
    pub label: &'a str,
    /// The `trigger_item_job` on-demand job type (e.g. `"Execute"`).
    pub job_type: &'a str,
    /// Item collection for a path-based execute endpoint, when the API exposes one.
    pub path_collection: Option<&'a str>,
    /// Minimum role named in the permission-error hint.
    pub role: &'a str,
}

/// Trigger an item job and, when `wait`, poll it to completion.
///
/// Dry-run guarded (the guard fires before any network call). On `--wait`
/// timeout, optionally cancels the running instance, then returns a `Timeout`
/// error.
#[allow(clippy::too_many_arguments)]
pub async fn run_and_wait(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    wait: bool,
    timeout_secs: u64,
    cancel_on_timeout: bool,
    spec: &RunSpec<'_>,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        spec.op,
        &serde_json::json!({ "workspace": workspace, "id": id, "wait": wait, "timeout": timeout_secs }),
    ) {
        return Ok(());
    }

    // Job trigger helpers read the instance id from the 202 `Location` header.
    let (job_id, first_poll_delay) = if let Some(collection) = spec.path_collection {
        let path = format!("/workspaces/{workspace}/{collection}/{id}/jobs/execute/instances");
        let trigger = client
            .trigger_item_job_at_with_retry_after(&path, None)
            .await
            .map_err(|e| enrich_forbidden(e, spec.op, spec.role))?;
        (trigger.job_id, trigger.retry_after)
    } else {
        let job_id = client
            .trigger_item_job(workspace, id, spec.job_type, None)
            .await
            .map_err(|e| enrich_forbidden(e, spec.op, spec.role))?;
        (job_id, None)
    };

    if !wait {
        let obj = serde_json::json!({ "itemId": id, "jobInstanceId": job_id, "status": "started" });
        output::render_object(cli, &obj, "status");
        return Ok(());
    }

    let poll_interval = std::time::Duration::from_secs(5);
    let mut next_poll_delay = first_poll_delay.unwrap_or(poll_interval);
    let max_wait = std::time::Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();

    loop {
        let elapsed = start.elapsed();
        if elapsed >= max_wait || next_poll_delay > max_wait.saturating_sub(elapsed) {
            if cancel_on_timeout && !job_id.is_empty() {
                let cancel_path =
                    format!("/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}/cancel");
                let _ = client
                    .post(&cancel_path, &serde_json::json!({}), false)
                    .await;
            }
            return Err(FabioError::with_hint(
                ErrorCode::Timeout,
                format!(
                    "{} timed out after {timeout_secs}s. Job ID: {job_id}",
                    spec.label
                ),
                format!("Increase --timeout (current: {timeout_secs}s) or use --cancel-on-timeout"),
            )
            .into());
        }

        tokio::time::sleep(next_poll_delay).await;
        next_poll_delay = poll_interval;

        let status_path = format!("/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}");
        if let Ok(ref status_data) = client.get(&status_path).await {
            let status = status_data
                .get("status")
                .and_then(Value::as_str)
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
                        format!("{} {status}. Job ID: {job_id}", spec.label),
                    )
                    .into());
                }
                _ => {} // InProgress, NotStarted — keep polling
            }
        }
    }
}
