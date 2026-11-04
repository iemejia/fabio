//! Monitoring states and application routes, not interactive Livy state names.

use clap::ValueEnum;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
pub enum JobState {
    #[value(name = "NotStarted")]
    NotStarted,
    #[value(name = "InProgress")]
    InProgress,
    #[value(name = "Succeeded")]
    Succeeded,
    #[value(name = "Failed")]
    Failed,
    #[value(name = "Cancelled")]
    Cancelled,
    #[value(name = "Unknown")]
    Unknown,
}

impl JobState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "NotStarted",
            Self::InProgress => "InProgress",
            Self::Succeeded => "Succeeded",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::Unknown => "Unknown",
        }
    }
    pub fn parse(value: &Value) -> Option<Self> {
        value
            .as_str()
            .and_then(|value| Self::from_str(value, true).ok())
    }
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

pub fn spark_item_segment(item_type: &str) -> &'static str {
    match item_type {
        "spark-job-definition" => "sparkJobDefinitions",
        "lakehouse" => "lakehouses",
        _ => "notebooks",
    }
}

pub fn applications_base(
    workspace: &str,
    segment: &str,
    item_id: &str,
    livy_id: &str,
    app_id: &str,
    attempt_id: Option<&str>,
) -> String {
    let base = format!(
        "/workspaces/{workspace}/{segment}/{item_id}/livySessions/{livy_id}/applications/{app_id}"
    );
    attempt_id.map_or_else(|| base.clone(), |attempt| format!("{base}/{attempt}"))
}

pub fn version(value: &Value) -> Option<&str> {
    value.as_str().filter(|value| {
        !value.is_empty()
            && value.len() <= 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
            && value.bytes().any(|byte| byte.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn monitoring_states_are_typed_and_unknown_is_not_terminal() {
        for state in [
            JobState::NotStarted,
            JobState::InProgress,
            JobState::Succeeded,
            JobState::Failed,
            JobState::Cancelled,
            JobState::Unknown,
        ] {
            assert_eq!(JobState::parse(&json!(state.as_str())), Some(state));
        }
        assert_eq!(
            JobState::parse(&json!("succeeded")),
            Some(JobState::Succeeded)
        );
        assert_eq!(JobState::parse(&json!("idle")), None);
        assert_eq!(JobState::parse(&json!("FutureState")), None);
        assert!(!JobState::Unknown.terminal());
        assert!(JobState::Failed.terminal());
        assert!(JobState::Cancelled.terminal());
    }
}
