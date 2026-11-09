//! Shared read-only diagnostic contracts. Never copy raw API bodies into evidence.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::time::Instant;
use uuid::Uuid;

use crate::client::FabricClient;
use crate::commands::remediation::RecoveryHint;
use crate::errors::{ErrorCode, FabioError};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ready,
    Blocked,
    NeedsAction,
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    #[serde(rename = "checkId")]
    pub key: &'static str,
    pub status: Status,
    pub message: String,
    pub evidence: Value,
    pub next_steps: Vec<RecoveryHint>,
}

impl Check {
    pub fn new(
        key: &'static str,
        status: Status,
        message: impl Into<String>,
        evidence: Value,
        next: String,
    ) -> Self {
        Self {
            key,
            status,
            message: message.into(),
            evidence,
            next_steps: vec![RecoveryHint::inspection(
                "Inspect the same scoped resource without changing it.",
                next,
            )],
        }
    }

    pub fn error(key: &'static str, error: &anyhow::Error, source: &str, next: String) -> Self {
        Self::new(
            key,
            Status::Unknown,
            "The requested evidence could not be read; this is not proof of readiness or resource absence.",
            json!({"source": source, "error": error_evidence(error)}),
            next,
        )
    }
}

pub fn error_evidence(error: &anyhow::Error) -> Value {
    error.downcast_ref::<FabioError>().map_or_else(
        || json!({"code": "UNKNOWN"}),
        |error| json!({"code": error.code, "requestId": error.request_id.as_deref().filter(|value| value.len() <= 128), "retriable": error.retriable}),
    )
}

/// Reject flat and wrapped error-bearing projections, including HTTP successes.
pub fn has_error(body: &Value) -> bool {
    ["error", "errorCode"]
        .iter()
        .any(|key| body.get(*key).is_some_and(|value| !value.is_null()))
}

pub fn summary<'a>(checks: impl IntoIterator<Item = &'a Check>) -> Status {
    summary_statuses(checks.into_iter().map(|check| check.status))
}

pub fn summary_statuses(statuses: impl IntoIterator<Item = Status>) -> Status {
    statuses
        .into_iter()
        .max_by_key(|status| match status {
            Status::Ready => 0,
            Status::Unknown => 1,
            Status::NeedsAction => 2,
            Status::Blocked => 3,
        })
        .unwrap_or(Status::Unknown)
}

pub fn id(value: &Value) -> Option<Uuid> {
    value
        .as_str()
        .and_then(|value| Uuid::parse_str(value).ok())
        .filter(|id| !id.is_nil())
}

pub async fn read(client: &FabricClient, path: &str, timeout: Duration) -> Result<Value> {
    tokio::time::timeout(timeout, client.get(path))
        .await
        .map_err(|_| FabioError::new(ErrorCode::Timeout, "Diagnostic read reached its deadline"))?
}

/// One deadline shared by every probe, including retries and pagination.
pub struct ReadBudget(Instant);

impl ReadBudget {
    pub fn new(seconds: u64) -> Self {
        Self(Instant::now() + Duration::from_secs(seconds))
    }

    pub async fn get(&self, client: &FabricClient, path: &str) -> Result<Value> {
        self.run(client.get(path)).await
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.0
    }

    pub fn remaining(&self) -> Duration {
        self.0.saturating_duration_since(Instant::now())
    }

    /// Apply the SAME deadline to a semantically read-only query or probe.
    /// Callers must never pass a mutation to this helper.
    pub async fn run<T>(&self, probe: impl std::future::Future<Output = Result<T>>) -> Result<T> {
        let remaining = self.0.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(FabioError::new(
                ErrorCode::Timeout,
                "Diagnostic deadline reached; no further reads attempted",
            )
            .into());
        }
        tokio::time::timeout(remaining, probe).await.map_err(|_| {
            FabioError::new(ErrorCode::Timeout, "Diagnostic read reached its deadline")
        })?
    }
}

pub fn uuid_target(value: &str, flag: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .ok()
        .filter(|value| !value.is_nil())
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} must be a non-zero UUID"),
                "Supply the explicit resource ID, not a name, path, or wildcard.",
            )
            .into()
        })
}

pub fn encode_id(value: Option<Uuid>) -> Option<String> {
    value.map(|value| value.to_string())
}

/// A strict token-only cursor. Never follows a response's arbitrary URI.
#[derive(Debug, Default)]
pub struct PageCursor(HashSet<String>);

impl PageCursor {
    pub fn next(&mut self, body: &Value) -> std::result::Result<Option<String>, &'static str> {
        let token = &body["continuationToken"];
        let uri = &body["continuationUri"];
        if (!token.is_null() && !token.is_string()) || (!uri.is_null() && !uri.is_string()) {
            return Err("malformed_continuation");
        }
        if let Some(token) = token.as_str().filter(|token| !token.is_empty()) {
            if token.len() > 16_384 {
                return Err("continuation_token_limit");
            }
            if !self.0.insert(token.to_string()) {
                return Err("repeated_continuation");
            }
            return Ok(Some(token.to_string()));
        }
        if uri.as_str().is_some_and(|uri| !uri.is_empty()) {
            return Err("uri_only_continuation");
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_summary_has_one_shared_severity_order() {
        assert_eq!(summary_statuses([]), Status::Unknown);
        assert_eq!(
            summary_statuses([Status::Ready, Status::Unknown]),
            Status::Unknown
        );
        assert_eq!(
            summary_statuses([Status::Unknown, Status::NeedsAction]),
            Status::NeedsAction
        );
        assert_eq!(
            summary_statuses([Status::NeedsAction, Status::Blocked]),
            Status::Blocked
        );
    }

    #[tokio::test]
    async fn expired_budget_does_not_poll_a_readonly_probe() {
        let budget = ReadBudget::new(0);
        let calls = std::cell::Cell::new(0);
        assert!(budget.expired());
        assert!(
            budget
                .run(async {
                    calls.set(calls.get() + 1);
                    Ok(())
                })
                .await
                .is_err()
        );
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn cursor_is_token_only_bounded_and_cycle_aware() {
        let mut cursor = PageCursor::default();
        let body = json!({"continuationToken":"opaque/?&value","continuationUri":"https://example.org/private"});
        assert_eq!(cursor.next(&body), Ok(Some("opaque/?&value".to_string())));
        assert_eq!(cursor.next(&body), Err("repeated_continuation"));
        assert_eq!(
            cursor.next(&json!({"continuationUri":"https://example.org/private"})),
            Err("uri_only_continuation")
        );
        assert_eq!(
            cursor.next(&json!({"continuationToken":42})),
            Err("malformed_continuation")
        );
        assert_eq!(
            cursor.next(&json!({"continuationToken":"x".repeat(16_385)})),
            Err("continuation_token_limit")
        );
        assert_eq!(cursor.next(&json!({})), Ok(None));
    }
}
