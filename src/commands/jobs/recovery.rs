//! Explicit local recovery of retained evidence, never remote correlation by guess.
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

use anyhow::Result;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{JobEntry, JobLedger};
use crate::cli::Cli;
use crate::commands::diagnostics::uuid_target;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

fn validate(
    mut entry: JobEntry,
    workspace: &str,
    receipt: &str,
    targets: &[String],
    endpoint: &str,
) -> Result<JobEntry> {
    let invalid = || {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Retained receipt scope, kind, state, timestamp or endpoint does not match the explicitly selected original work.",
            "Preserve the original evidence privately. Import one complete original receipt with its original endpoint and exact IDs; never reconstruct an ID from a trace or latest history, repair corrupt rows, or replay work.",
        )
    };
    let expected: BTreeSet<_> = targets
        .iter()
        .map(|id| uuid_target(id, "--item-ids"))
        .collect::<Result<_>>()?;
    if expected.is_empty() || expected.len() != targets.len() {
        return Err(invalid().into());
    }
    let workspace = uuid_target(workspace, "--workspace")?;
    let receipt = uuid_target(receipt, "--id")?;
    let actual_workspace = uuid_target(&entry.workspace, "receipt workspace")?;
    let actual_receipt = uuid_target(&entry.id, "receipt ID")?;
    let item = uuid_target(&entry.item_id, "receipt item")?;
    let is_upgrade = entry.kind == "gen1-upgrade";
    let known_job = matches!(
        entry.kind.as_str(),
        "notebook-run"
            | "dataflow-execute"
            | "dataflow-applyChanges"
            | "data-build-tool-job-run"
            | "copy-job run"
            | "data-pipeline run"
            | "spark-job-definition run"
    ) || entry.kind.strip_prefix("job-").is_some_and(|kind| {
        !kind.is_empty()
            && kind.len() <= 128
            && kind
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    });
    let actual: BTreeSet<Uuid> = if is_upgrade {
        let set = entry
            .target_ids
            .iter()
            .map(|id| uuid_target(id, "receipt target"))
            .collect::<Result<BTreeSet<_>>>()?;
        if set.len() != entry.target_ids.len() || !set.contains(&item) {
            return Err(invalid().into());
        }
        set
    } else {
        BTreeSet::from([item])
    };
    if (!is_upgrade && !known_job)
        || workspace != actual_workspace
        || receipt != actual_receipt
        || actual != expected
        || (!is_upgrade && !entry.target_ids.is_empty())
        || entry.fabric_endpoint.as_deref() != Some(endpoint)
        || !crate::client::is_secure_or_loopback(endpoint)
        || chrono::DateTime::parse_from_rfc3339(&entry.started_at).is_err()
        || !matches!(
            entry.submission_state.as_deref(),
            Some("accepted" | "acknowledged" | "unknown")
        )
    {
        return Err(invalid().into());
    }
    if let Some(operation) = &entry.operation_id {
        uuid_target(operation, "receipt operation")?;
    }
    if !is_upgrade && entry.operation_id.is_some() {
        return Err(invalid().into());
    }
    // Imported claims never become verified remote completion or pruneable success.
    entry
        .extra
        .insert("importedStatus".into(), json!(entry.status));
    if let Some(result) = entry.result.take() {
        entry
            .extra
            .insert("unverifiedImportedResult".into(), result);
    }
    entry.extra.insert("remoteVerified".into(), json!(false));
    entry
        .extra
        .insert("importedAt".into(), json!(chrono::Utc::now().to_rfc3339()));
    entry.status = "unknown".to_string();
    entry.completed_at = None;
    entry.duration_secs = None;
    Ok(entry)
}

pub(super) fn import(
    cli: &Cli,
    file: &Path,
    workspace: &str,
    id: &str,
    targets: &[String],
    endpoint: &str,
) -> Result<()> {
    const LIMIT: u64 = 1_048_576;
    let mut bytes = Vec::new();
    std::fs::File::open(file)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(
            FabioError::invalid_input("Receipt file exceeds the 1 MiB input limit.").into(),
        );
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        FabioError::invalid_input(
            "Receipt file is not valid JSON; raw evidence was not echoed or changed.",
        )
    })?;
    let value = value.get("data").unwrap_or(&value);
    let value = value.get("jobReceipt").unwrap_or(value);
    let entry = serde_json::from_value(value.clone()).map_err(|_|FabioError::invalid_input("Expected one complete original JobEntry or jobReceipt, not a transport trace or partial acknowledgement."))?;
    let entry = validate(entry, workspace, id, targets, endpoint)?;
    let preview = json!({"id":entry.id,"workspace":entry.workspace,"itemIds":targets,"kind":entry.kind,"destructive":false,"remoteVerified":false,"retryAllowed":false});
    if output::dry_run_guard(cli, "jobs import", &preview) {
        return Ok(());
    }
    if cli.readonly {
        return Err(FabioError::new(
            ErrorCode::ReadonlyMode,
            "Receipt import changes local evidence and is blocked by --readonly; use --dry-run.",
        )
        .into());
    }
    JobLedger::append(&entry)?;
    output::render_object(
        cli,
        &json!({"status":"imported","id":entry.id,"remoteVerified":false,"retryAllowed":false,"destructive":false}),
        "id",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const ITEM: &str = "bbbbbbbb-1111-2222-3333-444444444444";
    const JOB: &str = "cccccccc-1111-2222-3333-444444444444";
    const ENDPOINT: &str = "https://api.fabric.microsoft.com/v1";
    fn entry() -> JobEntry {
        let mut entry = JobEntry::new(JOB, "notebook-run", WS, ITEM);
        entry.fabric_endpoint = Some(ENDPOINT.into());
        entry.submission_state = Some("accepted".into());
        entry
    }
    #[test]
    fn recovered_claims_are_unverified_and_nonterminal() {
        let mut entry = entry();
        entry.status = "completed".into();
        entry.result = Some(json!({"status":"Completed"}));
        let entry = validate(entry, WS, JOB, &[ITEM.into()], ENDPOINT).unwrap();
        assert_eq!(entry.status, "unknown");
        assert!(entry.result.is_none());
        assert!(entry.completed_at.is_none());
        assert_eq!(entry.extra["remoteVerified"], false);
        assert_eq!(
            entry.extra["unverifiedImportedResult"]["status"],
            "Completed"
        );
    }
    #[test]
    fn recovery_rejects_mismatched_unknown_missing_duplicate_and_forged_scope() {
        for field in [
            "workspace",
            "id",
            "item_id",
            "fabric_endpoint",
            "kind",
            "started_at",
            "submission_state",
        ] {
            let mut value = serde_json::to_value(entry()).unwrap();
            value[field] = json!("wrong");
            assert!(
                validate(
                    serde_json::from_value(value).unwrap(),
                    WS,
                    JOB,
                    &[ITEM.into()],
                    ENDPOINT
                )
                .is_err(),
                "{field}"
            );
        }
        assert!(validate(entry(), WS, JOB, &[ITEM.into(), ITEM.into()], ENDPOINT).is_err());
        assert!(validate(entry(), WS, JOB, &[], ENDPOINT).is_err());
    }
}
