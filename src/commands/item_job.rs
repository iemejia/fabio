//! Shared "run an item job on demand, optionally wait for completion" helper.
//!
//! `copy-job run`, `data-pipeline run`, and `spark-job-definition run` all
//! trigger an on-demand job, and (with `--wait`) poll the job instance until it
//! reaches a terminal state, cancelling on timeout when asked. The bodies were
//! byte-identical except for the op-name, the `trigger_item_job` job-type, the
//! message label, and the role — captured here as [`RunSpec`].

use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_forbidden;
use crate::output;

/// Per-command parameters for [`run_and_wait`].
pub struct RunSpec<'a> {
    /// Canonical op-name (`"<group> run"`) for the dry-run guard + error hints.
    pub op: &'a str,
    /// Human label used in status/timeout messages (e.g. `"Copy job run"`).
    pub label: &'a str,
    /// The `trigger_item_job` on-demand job type (e.g. `"Execute"`).
    pub job_type: &'a str,
    /// Optional item-type-specific trigger path. When absent, use the generic
    /// item scheduler endpoint with `job_type`. When present, send no body.
    pub trigger_path: Option<&'a str>,
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
    if output::dry_run_guard_maybe_destructive(
        cli,
        spec.op,
        &serde_json::json!({ "workspace": workspace, "id": id, "wait": wait, "timeout": timeout_secs,"cancelOnTimeout":cancel_on_timeout }),
        cancel_on_timeout,
    ) {
        return Ok(());
    }

    // `trigger_item_job` reads the new instance id from the 202 `Location` header.
    let trigger_result = match spec.trigger_path {
        Some(path) => client
            .trigger_item_job_at_with_retry_after(path, None)
            .await
            .map(|job| (job.id, job.retry_after)),
        None => client
            .trigger_item_job(workspace, id, spec.job_type, None)
            .await
            .map(|id| (id, None)),
    };
    let (job_id, first_poll_delay) =
        trigger_result.map_err(|e| enrich_forbidden(e, spec.op, spec.role))?;

    let tracked = crate::commands::jobs::tracking::TrackedJob::record(
        client, &job_id, spec.op, workspace, id,
    );
    crate::commands::jobs::tracking::finish_with_delay(cli, client, tracked, wait, timeout_secs, cancel_on_timeout,
        serde_json::json!({"itemId":id,"jobInstanceId":job_id,"status":"started","message":format!("{} started",spec.label)}), "status", first_poll_delay).await
}
