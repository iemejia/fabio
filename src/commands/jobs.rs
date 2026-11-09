use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::Cli;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

mod ledger;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum JobsCommand {
    /// List recent jobs from the local ledger
    List {
        /// Filter by status (e.g., running, completed, failed)
        #[arg(long)]
        status: Option<String>,
    },
    /// Get details of a specific job
    Get {
        /// Job ID
        #[arg(long)]
        id: String,
    },
    /// Prune recognized terminal receipts; retain unknown/future states and local timeouts
    Prune {
        /// Irreversibly delete all local receipts, including accepted/unknown work; does not cancel jobs
        #[arg(long)]
        include_running: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobEntry {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub workspace: String,
    pub item_id: String,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fabric_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_evidence: Option<Value>,
    // Preserve fields written by a newer binary when updating a legacy snapshot.
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

impl JobEntry {
    pub fn new(id: &str, kind: &str, workspace: &str, item_id: &str) -> Self {
        Self {
            id: id.to_string(),
            kind: kind.to_string(),
            status: "running".to_string(),
            workspace: workspace.to_string(),
            item_id: item_id.to_string(),
            started_at: Utc::now().to_rfc3339(),
            completed_at: None,
            duration_secs: None,
            error: None,
            target_ids: Vec::new(),
            operation_id: None,
            submission_state: None,
            http_status: None,
            result: None,
            poll_interval_secs: None,
            response_body_status: None,
            fabric_endpoint: None,
            correlation_evidence: None,
            extra: serde_json::Map::new(),
        }
    }
}

pub struct JobLedger;

impl JobLedger {
    fn store() -> Result<ledger::Ledger> {
        let home = home::home_dir().ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Cannot locate the home directory for the local receipt ledger.",
                "Restore the home-directory configuration. Never submit again to replace a missing receipt.",
            )
        })?;
        Ok(ledger::Ledger::new(home.join(".fabio").join("jobs.jsonl")))
    }

    pub fn append(entry: &JobEntry) -> Result<()> {
        Self::store()?.append(entry)
    }

    pub fn update(id: &str, status: &str, error: Option<&str>) -> Result<()> {
        Self::store()?.edit(|entries| {
            for entry in entries.iter_mut().filter(|entry| entry.id == id) {
                entry.status = status.to_string();
                entry.completed_at = Some(Utc::now().to_rfc3339());
                if let (Ok(start), Some(end)) = (
                    DateTime::parse_from_rfc3339(&entry.started_at),
                    &entry.completed_at,
                ) && let Ok(end) = DateTime::parse_from_rfc3339(end)
                {
                    entry.duration_secs = Some(
                        end.signed_duration_since(start)
                            .num_seconds()
                            .unsigned_abs(),
                    );
                }
                if let Some(error) = error {
                    entry.error = Some(error.to_string());
                }
            }
            Ok(())
        })
    }

    /// Replace a receipt snapshot without falsely completing an accepted/running job.
    pub fn replace(entry: &JobEntry) -> Result<()> {
        Self::store()?.edit(|entries| {
            if let Some(existing) = entries.iter_mut().find(|existing| existing.id == entry.id) {
                let extra = std::mem::take(&mut existing.extra);
                existing.clone_from(entry);
                for (key, value) in extra {
                    existing.extra.entry(key).or_insert(value);
                }
            } else {
                entries.push(entry.clone());
            }
            Ok(())
        })
    }

    pub fn find(id: &str) -> Result<Option<JobEntry>> {
        Ok(Self::read_all()?.into_iter().find(|entry| entry.id == id))
    }

    fn read_all() -> Result<Vec<JobEntry>> {
        Self::store()?.read_all()
    }
}

pub fn execute(cli: &Cli, command: &JobsCommand) -> Result<()> {
    match command {
        JobsCommand::List { status } => list(cli, status.as_deref()),
        JobsCommand::Get { id } => get(cli, id),
        JobsCommand::Prune { include_running } => prune(cli, *include_running),
    }
}

fn list(cli: &Cli, status_filter: Option<&str>) -> Result<()> {
    let entries = JobLedger::read_all()?;
    // Use global --limit with a default of 20 for jobs
    let limit = cli.limit.unwrap_or(20);
    let filtered: Vec<Value> = entries
        .iter()
        .rev()
        .filter(|e| status_filter.is_none_or(|s| e.status == s))
        .take(limit)
        .map(|e| serde_json::to_value(e).unwrap_or_default())
        .collect();

    output::render_list(
        cli,
        &filtered,
        &["id", "kind", "status", "started_at", "duration_secs"],
        &["JOB_ID", "KIND", "STATUS", "STARTED", "DURATION"],
        "id",
    );
    Ok(())
}

fn get(cli: &Cli, id: &str) -> Result<()> {
    let entries = JobLedger::read_all()?;
    let entry = entries.iter().find(|e| e.id == id).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Job '{id}' not found in local ledger"),
            "Use 'fabio jobs list' to see available jobs.".to_string(),
        )
    })?;

    let obj = serde_json::to_value(entry)?;
    output::render_object(cli, &obj, "id");
    Ok(())
}

fn prune(cli: &Cli, include_running: bool) -> Result<()> {
    if cli.readonly && !cli.dry_run {
        return Err(FabioError::new(
            ErrorCode::ReadonlyMode,
            "Pruning local receipts is blocked by --readonly; use --dry-run to preview.",
        )
        .into());
    }
    let store = JobLedger::store()?;
    if cli.dry_run {
        let entries = store.read_all()?;
        let ids: Vec<_> = entries
            .iter()
            .filter(|entry| prune_candidate(entry, include_running))
            .map(|entry| &entry.id)
            .collect();
        output::dry_run_guard(
            cli,
            "jobs prune",
            &serde_json::json!({
                "include_running":include_running,"ids":ids,"count":ids.len(),
                "remaining":entries.len()-ids.len()
            }),
        );
        return Ok(());
    }
    let (pruned, remaining) = store.edit(|entries| {
        let before_count = entries.len();
        entries.retain(|entry| !prune_candidate(entry, include_running));
        Ok((before_count - entries.len(), entries.len()))
    })?;

    let obj = serde_json::json!({
        "pruned": pruned,
        "remaining": remaining,
        "status": "pruned",
        "destructive": true,
        "warnings": if include_running { vec!["--include-running deletes ALL local receipts irreversibly, including accepted or uncertain operations. It does not cancel remote work."] } else { Vec::new() }
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

fn prune_candidate(entry: &JobEntry, include_running: bool) -> bool {
    // Unknown/future states and local timeouts are not proof that remote work
    // finished. Only recognized terminal receipts are eligible by default.
    include_running
        || matches!(
            entry.status.as_str(),
            "completed" | "succeeded" | "failed" | "cancelled" | "partial_success"
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pruning_requires_a_recognized_terminal_state_unless_all_receipts_are_selected() {
        for (status, eligible) in [
            ("completed", true),
            ("succeeded", true),
            ("failed", true),
            ("cancelled", true),
            ("partial_success", true),
            ("running", false),
            ("preparing", false),
            ("accepted", false),
            ("unknown", false),
            ("timeout", false),
            ("future-state", false),
            ("Succeeded", false),
            ("", false),
        ] {
            let mut entry = JobEntry::new("mock-id", "mock-job", "mock-workspace", "mock-item");
            entry.status = status.to_string();
            assert_eq!(prune_candidate(&entry, false), eligible, "{status}");
            assert!(prune_candidate(&entry, true), "{status}");
        }
    }

    #[test]
    fn legacy_job_records_do_not_invent_batch_targets_or_operation_identity() {
        let value = json!({"id":"old","kind":"notebook","status":"running","workspace":"mock","item_id":"mock","started_at":"2026-01-01T00:00:00Z"});
        let entry: JobEntry = serde_json::from_value(value).unwrap();
        assert_eq!(entry.target_ids.len(), 0);
        assert!(entry.operation_id.is_none());
        assert!(entry.result.is_none());
        assert!(entry.fabric_endpoint.is_none());
    }
}
