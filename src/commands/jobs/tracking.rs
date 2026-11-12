//! Acknowledged jobs retain their exact scope even if local storage or observation
//! fails. A local timeout/cancel acknowledgement never establishes remote completion.

use anyhow::Result;
use serde_json::{Value, json};

use super::{JobEntry, JobLedger};
use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, HintType};
use crate::output;

const HINT: &str = "Preserve the exact job receipt privately. Do not resubmit or rerun to recover local bookkeeping. Inspect this existing job with job-scheduler get-instance using its workspace, item and job-instance-id. A timeout or cancel acknowledgement is not proof of terminal completion.";

pub struct TrackedJob {
    entry: JobEntry,
    persisted: bool,
    registered: bool,
    warnings: Vec<Value>,
}

impl TrackedJob {
    pub fn record(
        client: &FabricClient,
        id: &str,
        kind: &str,
        workspace: &str,
        item: &str,
    ) -> Self {
        let mut entry = JobEntry::new(id, kind, workspace, item);
        entry.submission_state = Some("accepted".to_string());
        entry.fabric_endpoint = Some(client.fabric_endpoint());
        let mut tracked = Self {
            entry,
            persisted: false,
            registered: false,
            warnings: Vec::new(),
        };
        tracked.save(true);
        tracked
    }

    fn save(&mut self, append: bool) {
        let result = if append || !self.registered {
            JobLedger::append(&self.entry)
        } else {
            JobLedger::replace(&self.entry)
        };
        self.persisted = result.is_ok();
        if self.persisted {
            self.registered = true;
        }
        if result.is_err() {
            let warning = json!({"code":"RECEIPT_PERSISTENCE_FAILED","message":"Local receipt persistence failed after job acceptance; retain the emitted receipt. The write may already have committed.","retryAllowed":false});
            eprintln!("{}", json!({"warning":warning}));
            self.warnings.push(warning);
        }
    }

    fn status(&mut self, status: &str) {
        self.entry.status = status.to_string();
        if matches!(status, "completed" | "failed" | "cancelled" | "deduped") {
            self.entry.completed_at = Some(chrono::Utc::now().to_rfc3339());
            if let Ok(start) = chrono::DateTime::parse_from_rfc3339(&self.entry.started_at) {
                self.entry.duration_secs = Some(
                    chrono::Utc::now()
                        .signed_duration_since(start)
                        .num_seconds()
                        .unsigned_abs(),
                );
            }
        }
        self.save(false);
    }

    fn output(&self, mut value: Value) -> Value {
        value["jobReceipt"] = json!(self.entry);
        value["receiptPersisted"] = json!(self.persisted);
        value["retryAllowed"] = json!(false);
        if !self.warnings.is_empty() {
            value["warnings"] = json!(self.warnings);
        }
        value
    }

    fn failure(&self, cli: &Cli, value: Value, error: anyhow::Error) -> anyhow::Error {
        output::render_object(cli, &self.output(value), "status");
        error
            .downcast::<FabioError>()
            .map_or_else(
                |_| {
                    FabioError::with_typed_hint(
                        ErrorCode::ApiError,
                        "Job was accepted but observation failed.",
                        HINT,
                        HintType::SemanticCorrection,
                    )
                },
                |error| error.with_replaced_hint(HINT, Some(HintType::SemanticCorrection)),
            )
            .set_retriable(Some(false))
            .into()
    }
}

/// Shared bounded job observation for all runners. Never repeats a submission.
#[allow(clippy::too_many_arguments)]
pub async fn finish(
    cli: &Cli,
    client: &FabricClient,
    tracked: TrackedJob,
    wait: bool,
    timeout_secs: u64,
    cancel_on_timeout: bool,
    value: Value,
    plain_key: &str,
) -> Result<()> {
    finish_with_delay(
        cli,
        client,
        tracked,
        wait,
        timeout_secs,
        cancel_on_timeout,
        value,
        plain_key,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn finish_with_delay(
    cli: &Cli,
    client: &FabricClient,
    mut tracked: TrackedJob,
    wait: bool,
    timeout_secs: u64,
    cancel_on_timeout: bool,
    mut value: Value,
    plain_key: &str,
    first_poll_delay: Option<std::time::Duration>,
) -> Result<()> {
    if !wait {
        output::render_object(cli, &tracked.output(value), plain_key);
        return Ok(());
    }
    if [
        &tracked.entry.workspace,
        &tracked.entry.item_id,
        &tracked.entry.id,
    ]
    .iter()
    .any(|id| {
        id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    }) {
        return Err(tracked.failure(
            cli,
            value,
            FabioError::invalid_input(
                "The acknowledged job scope cannot safely select an observation route.",
            )
            .into(),
        ));
    }
    let path = format!(
        "/workspaces/{}/items/{}/jobs/instances/{}",
        tracked.entry.workspace, tracked.entry.item_id, tracked.entry.id
    );
    let Some(deadline) =
        tokio::time::Instant::now().checked_add(std::time::Duration::from_secs(timeout_secs))
    else {
        return Err(tracked.failure(
            cli,
            value,
            FabioError::invalid_input("The observation timeout exceeds the supported clock range.")
                .into(),
        ));
    };
    if let Some(delay) = first_poll_delay {
        tokio::time::sleep_until((tokio::time::Instant::now() + delay).min(deadline)).await;
    }
    loop {
        if tokio::time::Instant::now() >= deadline {
            tracked.status("timeout");
            value["status"] = json!("timeout");
            if cancel_on_timeout {
                match client
                    .post(&format!("{path}/cancel"), &json!({}), false)
                    .await
                {
                    Ok(_) => {
                        tracked.status("cancel_requested");
                        value["cancelRequested"] = json!(true);
                    }
                    Err(error) => {
                        value["cancelError"] = error
                            .downcast_ref::<FabioError>()
                            .map_or_else(|| json!({"code":"UNKNOWN"}), output::error_details);
                        return Err(tracked.failure(cli, value, error));
                    }
                }
            }
            return Err(tracked.failure(
                cli,
                value,
                FabioError::new(
                    ErrorCode::Timeout,
                    "Local job observation deadline expired; remote completion is unknown.",
                )
                .into(),
            ));
        }
        let response = match tokio::time::timeout_at(deadline, client.get(&path)).await {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return Err(tracked.failure(cli, value, error)),
            Err(_) => continue,
        };
        let status = response["status"].as_str().unwrap_or("Unknown");
        value["jobResult"] = response.clone();
        tracked.entry.result = Some(response.clone());
        if value.get("jobInstanceId").is_some()
            && let Some(fields) = response.as_object()
        {
            value
                .as_object_mut()
                .expect("job output is an object")
                .extend(fields.clone());
        }
        value["status"] = json!(status);
        match status {
            "Completed" => {
                tracked.status("completed");
                output::render_object(cli, &tracked.output(value), "status");
                return Ok(());
            }
            "Failed" | "Cancelled" | "Deduped" => {
                tracked.entry.error = response["failureReason"]["message"]
                    .as_str()
                    .map(str::to_string);
                if tracked.entry.kind.starts_with("dataflow-")
                    && let Some(message) = tracked.entry.error.as_deref()
                {
                    value["diagnosticHint"] =
                        json!(crate::commands::dataflow::dataflow_run_failure_hint(
                            message,
                            &tracked.entry.id
                        ));
                }
                tracked.status(&status.to_ascii_lowercase());
                value["jobResult"] = response;
                return Err(tracked.failure(
                    cli,
                    value,
                    FabioError::api_error("The exact job reached a non-success terminal state.")
                        .into(),
                ));
            }
            "NotStarted" | "InProgress" => {}
            _ => {
                return Err(tracked.failure(
                    cli,
                    value,
                    FabioError::api_error(
                        "The job returned an unrecognized status; completion is unknown.",
                    )
                    .into(),
                ));
            }
        }
        tokio::time::sleep_until(
            (tokio::time::Instant::now() + std::time::Duration::from_secs(5)).min(deadline),
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_output_never_authorizes_replay_or_invents_storage_success() {
        let tracked = TrackedJob {
            entry: JobEntry::new("mock", "mock", "mock", "mock"),
            persisted: false,
            registered: false,
            warnings: vec![json!({"code":"RECEIPT_PERSISTENCE_FAILED"})],
        };
        let value = tracked.output(json!({"status":"started"}));
        assert_eq!(value["retryAllowed"], false);
        assert_eq!(value["receiptPersisted"], false);
        assert_eq!(value["jobReceipt"]["id"], "mock");
        assert_eq!(value["warnings"][0]["code"], "RECEIPT_PERSISTENCE_FAILED");
    }
}
