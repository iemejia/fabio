use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::Value;

use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};

use super::model::{Setting, Update, invalid, validate_capabilities};
use super::state::{Checkpoint, Outcome, Phase};

pub(super) async fn run(client: &FabricClient, cp: &mut Checkpoint) -> Result<()> {
    preflight(cp)?;
    cp.phase = Phase::Apply;
    cp.save()?;
    let count = cp.outcomes.len();
    for index in 0..count {
        if matches!(cp.outcomes[index], Outcome::Completed | Outcome::Skipped) {
            continue;
        }
        let update = cp
            .plan
            .as_ref()
            .ok_or_else(|| invalid("Missing plan"))?
            .updates[index]
            .clone();
        cp.outcomes[index] = Outcome::Uncertain;
        cp.save()?;
        let response = client
            .post_once(
                &format!("/admin/tenantsettings/{}/update", update.setting_name),
                &serde_json::to_value(&update.after)?,
            )
            .await;
        match response {
            Ok(value) => {
                confirm_response(&value, &update)?;
                cp.outcomes[index] = Outcome::Completed;
                cp.retry_after = None;
                cp.retry_at = None;
                cp.save()?;
            }
            Err(error) => {
                // Explicit rejection is safe to retry after a fresh preflight.
                // Transport/5xx/malformed success remains uncertain and MUST be
                // reconciled by reading the setting on the next invocation.
                if error.downcast_ref::<FabioError>().is_some_and(|e| {
                    matches!(
                        e.code,
                        ErrorCode::RateLimited
                            | ErrorCode::Forbidden
                            | ErrorCode::AuthRequired
                            | ErrorCode::NotFound
                            | ErrorCode::ReadonlyMode
                    )
                }) {
                    cp.outcomes[index] = Outcome::Pending;
                }
                return Err(error);
            }
        }
    }
    Ok(())
}

fn preflight(cp: &mut Checkpoint) -> Result<()> {
    let current: BTreeMap<_, _> = cp
        .snapshot
        .iter()
        .map(|s| (s.setting_name.as_str(), s))
        .collect();
    let plan = cp.plan.as_ref().ok_or_else(|| invalid("Missing plan"))?;
    let mut outcomes = cp.outcomes.clone();
    for (update, outcome) in plan.updates.iter().zip(&mut outcomes) {
        if matches!(outcome, Outcome::Completed | Outcome::Skipped) {
            continue;
        }
        let live = current.get(update.setting_name.as_str()).ok_or_else(|| {
            invalid(format!(
                "{} no longer exists; create a new plan",
                update.setting_name
            ))
        })?;
        if live.values == update.after {
            *outcome = if *outcome == Outcome::Uncertain {
                Outcome::Completed
            } else {
                Outcome::Skipped
            };
        } else if live.values == update.before {
            validate_capabilities(live, &update.after)?;
            *outcome = Outcome::Pending;
        } else {
            return Err(FabioError::with_hint(ErrorCode::Conflict,
                format!("{} changed since planning; no remaining updates were sent", update.setting_name),
                "Export current settings and create a new reviewed plan. Do not edit the old plan/checkpoint to bypass this conflict.").into());
        }
    }
    cp.outcomes = outcomes;
    Ok(())
}

fn confirm_response(value: &Value, update: &Update) -> Result<()> {
    let settings = value
        .get("tenantSettings")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .ok_or_else(|| {
            FabioError::api_error(
                "Update was not acknowledged with tenantSettings; result is uncertain",
            )
        })?;
    let matching: Vec<_> = settings
        .iter()
        .filter(|v| {
            v.get("settingName").and_then(Value::as_str) == Some(update.setting_name.as_str())
        })
        .collect();
    if matching.len() != 1 {
        return Err(FabioError::api_error(
            "Update response does not identify exactly one requested setting; result is uncertain",
        )
        .into());
    }
    let actual = Setting::from_api(matching[0].clone())?;
    if actual.values != update.after {
        return Err(FabioError::api_error(
            "Update response does not confirm the planned values; result is uncertain",
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::admin::tenant_workflow::model::{Values, parse};

    #[test]
    fn test_group_response_matches_requested_setting_only() {
        let before: Values = parse(b"enabled: false").unwrap();
        let after: Values = parse(b"enabled: true").unwrap();
        let update = Update {
            setting_name: "B".into(),
            changes: before.changes(&after),
            before,
            after,
        };
        assert!(confirm_response(&serde_json::json!({"tenantSettings":[{"settingName":"A","enabled":false},{"settingName":"B","enabled":true}]}), &update).is_ok());
        assert!(
            confirm_response(
                &serde_json::json!({"tenantSettings":[{"settingName":"A","enabled":true}]}),
                &update
            )
            .is_err()
        );
    }
}
