//! Shared, GET-only diagnostic contracts. Never copy raw API bodies into evidence.

use std::time::Duration;

use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};
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

pub fn summary<'a>(checks: impl IntoIterator<Item = &'a Check>) -> Status {
    checks
        .into_iter()
        .map(|check| check.status)
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
