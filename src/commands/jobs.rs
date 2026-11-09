use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Subcommand;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::Cli;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

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
    /// Remove completed/failed jobs from the ledger
    Prune {
        /// Remove all jobs including currently running ones
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
        }
    }
}

pub struct JobLedger;

impl JobLedger {
    fn ledger_path() -> PathBuf {
        let home = home::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".fabio").join("jobs.jsonl")
    }

    pub fn append(entry: &JobEntry) -> Result<()> {
        let path = Self::ledger_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }
        // Open with restricted permissions atomically to avoid TOCTOU
        #[cfg(unix)]
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(&path)?
        };
        #[cfg(not(unix))]
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;

        // Advisory lock to prevent concurrent append corruption
        file.lock_exclusive()?;
        let line = serde_json::to_string(entry)?;
        writeln!(file, "{line}")?;
        // Lock released on drop
        Ok(())
    }

    pub fn update(id: &str, status: &str, error: Option<&str>) -> Result<()> {
        Self::edit(|entries| {
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
        })
    }

    /// Replace a receipt snapshot without falsely completing an accepted/running job.
    pub fn replace(entry: &JobEntry) -> Result<()> {
        Self::edit(|entries| {
            if let Some(existing) = entries.iter_mut().find(|existing| existing.id == entry.id) {
                existing.clone_from(entry);
            } else {
                entries.push(entry.clone());
            }
        })
    }

    pub fn find(id: &str) -> Result<Option<JobEntry>> {
        Ok(Self::read_all()?.into_iter().find(|entry| entry.id == id))
    }

    fn edit(edit: impl FnOnce(&mut Vec<JobEntry>)) -> Result<()> {
        let path = Self::ledger_path();
        // Acquire exclusive lock for the read-modify-write operation
        #[cfg(unix)]
        let lock_file = {
            use std::os::unix::fs::OpenOptionsExt;
            OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(&path)?
        };
        #[cfg(not(unix))]
        let lock_file = OpenOptions::new().create(true).append(true).open(&path)?;
        lock_file.lock_exclusive()?;

        let mut entries = Self::read_all()?;
        edit(&mut entries);
        Self::write_all(&entries)?;

        // Lock released on drop
        drop(lock_file);
        Ok(())
    }

    fn read_all() -> Result<Vec<JobEntry>> {
        let path = Self::ledger_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path)?;
        let reader = BufReader::new(file);
        let entries: Vec<JobEntry> = reader
            .lines()
            .map_while(Result::ok)
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(&line).ok())
            .collect();
        Ok(entries)
    }

    fn write_all(entries: &[JobEntry]) -> Result<()> {
        let path = Self::ledger_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }
        // Create with restricted permissions atomically to avoid TOCTOU
        #[cfg(unix)]
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?
        };
        #[cfg(not(unix))]
        let mut file = fs::File::create(&path)?;

        for entry in entries {
            let line = serde_json::to_string(entry)?;
            writeln!(file, "{line}")?;
        }
        Ok(())
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
    if output::dry_run_guard(
        cli,
        "jobs prune",
        &serde_json::json!({ "include_running": include_running }),
    ) {
        return Ok(());
    }

    let entries = JobLedger::read_all()?;
    let before_count = entries.len();
    let remaining: Vec<JobEntry> = if include_running {
        Vec::new()
    } else {
        entries
            .into_iter()
            .filter(|e| {
                matches!(
                    e.status.as_str(),
                    "running" | "preparing" | "accepted" | "unknown"
                )
            })
            .collect()
    };
    let pruned = before_count - remaining.len();
    JobLedger::write_all(&remaining)?;

    let obj = serde_json::json!({
        "pruned": pruned,
        "remaining": remaining.len(),
        "status": "pruned"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
