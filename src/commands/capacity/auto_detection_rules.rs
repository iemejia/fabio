use anyhow::Result;
use serde_json::Value;

use super::AutoDetectionBlockDuration;
use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, enrich_capacity};
use crate::output;

fn block_duration(duration: AutoDetectionBlockDuration, hours: Option<u32>) -> Result<Value> {
    match (duration, hours) {
        (AutoDetectionBlockDuration::Indefinite, None) => {
            Ok(serde_json::json!({"type": "Indefinite"}))
        }
        (AutoDetectionBlockDuration::Hours, Some(hours)) if hours > 0 => {
            Ok(serde_json::json!({"type": "Hours", "hours": hours}))
        }
        (AutoDetectionBlockDuration::Indefinite, Some(_)) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--hours cannot be used with --block-duration Indefinite",
            "Remove --hours or use --block-duration Hours.",
        )
        .into()),
        (AutoDetectionBlockDuration::Hours, _) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--hours must be at least 1 with --block-duration Hours",
            "Example: --block-duration Hours --hours 24",
        )
        .into()),
    }
}

fn base_path(capacity_id: &str) -> String {
    format!("/capacities/{capacity_id}/surgeProtection/actorAutoDetectionRules")
}

pub(super) async fn create(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    threshold: u8,
    duration: AutoDetectionBlockDuration,
    hours: Option<u32>,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    let body = serde_json::json!({
        "actorsSubjectToRule": {"type": "AllWorkspaces"},
        "capacityUnitsConsumptionThresholdPercentage": threshold,
        "blockDuration": block_duration(duration, hours)?
    });
    if output::dry_run_guard(
        cli,
        "capacity create-actor-auto-detection-rule",
        &serde_json::json!({"capacityId": capacity_id, "request": body}),
    ) {
        return Ok(());
    }
    let data = client
        .post(&base_path(capacity_id), &body, false)
        .await
        .map_err(|error| enrich_capacity(error, "capacity create-actor-auto-detection-rule"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn list(cli: &Cli, client: &FabricClient, capacity_id: &str) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    let response = client
        .get_list(
            &base_path(capacity_id),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|error| enrich_capacity(error, "capacity list-actor-auto-detection-rules"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        &[
            "actorsSubjectToRule.type",
            "capacityUnitsConsumptionThresholdPercentage",
            "blockDuration.type",
            "blockDuration.hours",
            "id",
        ],
        &["ACTORS", "THRESHOLD %", "DURATION", "HOURS", "ID"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn show(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    rule_id: &str,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(rule_id, "--rule-id")?;
    let data = client
        .get(&format!("{}/{}", base_path(capacity_id), rule_id))
        .await
        .map_err(|error| enrich_capacity(error, "capacity show-actor-auto-detection-rule"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    rule_id: &str,
    threshold: Option<u8>,
    duration: Option<AutoDetectionBlockDuration>,
    hours: Option<u32>,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(rule_id, "--rule-id")?;
    if threshold.is_none() && duration.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Provide --threshold and/or --block-duration",
            "Example: --threshold 40 --block-duration Hours --hours 24",
        )
        .into());
    }
    if duration.is_none() && hours.is_some() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--hours requires --block-duration Hours",
            "Add --block-duration Hours.",
        )
        .into());
    }
    let mut body = serde_json::Map::new();
    if let Some(value) = threshold {
        body.insert(
            "capacityUnitsConsumptionThresholdPercentage".to_string(),
            Value::from(value),
        );
    }
    if let Some(value) = duration {
        body.insert("blockDuration".to_string(), block_duration(value, hours)?);
    }
    let body = Value::Object(body);
    if output::dry_run_guard(
        cli,
        "capacity update-actor-auto-detection-rule",
        &serde_json::json!({
            "capacityId": capacity_id,
            "ruleId": rule_id,
            "request": body
        }),
    ) {
        return Ok(());
    }
    let data = client
        .patch(&format!("{}/{}", base_path(capacity_id), rule_id), &body)
        .await
        .map_err(|error| enrich_capacity(error, "capacity update-actor-auto-detection-rule"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    rule_id: &str,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(rule_id, "--rule-id")?;
    if output::dry_run_guard(
        cli,
        "capacity delete-actor-auto-detection-rule",
        &serde_json::json!({"capacityId": capacity_id, "ruleId": rule_id}),
    ) {
        return Ok(());
    }
    client
        .delete(&format!("{}/{}", base_path(capacity_id), rule_id))
        .await
        .map_err(|error| enrich_capacity(error, "capacity delete-actor-auto-detection-rule"))?;
    output::render_object(
        cli,
        &serde_json::json!({"status": "deleted", "id": rule_id}),
        "status",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_duration_serializes_discriminated_variants() {
        assert_eq!(
            block_duration(AutoDetectionBlockDuration::Indefinite, None).unwrap(),
            serde_json::json!({"type": "Indefinite"})
        );
        assert_eq!(
            block_duration(AutoDetectionBlockDuration::Hours, Some(24)).unwrap(),
            serde_json::json!({"type": "Hours", "hours": 24})
        );
    }

    #[test]
    fn block_duration_rejects_mismatched_hours() {
        assert!(block_duration(AutoDetectionBlockDuration::Hours, None).is_err());
        assert!(block_duration(AutoDetectionBlockDuration::Hours, Some(0)).is_err());
        assert!(block_duration(AutoDetectionBlockDuration::Indefinite, Some(24)).is_err());
    }
}
