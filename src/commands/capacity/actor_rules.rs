use anyhow::Result;
use serde_json::Value;

use super::{ActorRuleBlockDuration, ActorRuleSubject};
use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, enrich_capacity};
use crate::output;

fn block_duration(kind: ActorRuleBlockDuration, hours: Option<u32>) -> Result<Value> {
    match (kind, hours) {
        (ActorRuleBlockDuration::Hours, Some(hours)) => {
            Ok(serde_json::json!({"type": "Hours", "hours": hours}))
        }
        (ActorRuleBlockDuration::Indefinite, None) => Ok(serde_json::json!({"type": "Indefinite"})),
        (ActorRuleBlockDuration::Hours, None) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--hours is required when --block-duration Hours",
            "Provide --hours with an integer of at least 1.",
        )
        .into()),
        (ActorRuleBlockDuration::Indefinite, Some(_)) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--hours cannot be used when --block-duration Indefinite",
            "Remove --hours or use --block-duration Hours.",
        )
        .into()),
    }
}

fn rule_base(capacity_id: &str) -> String {
    format!("/capacities/{capacity_id}/surgeProtection/actorAutoDetectionRules")
}

fn create_body(
    subject: ActorRuleSubject,
    threshold: u8,
    duration: ActorRuleBlockDuration,
    hours: Option<u32>,
) -> Result<Value> {
    Ok(serde_json::json!({
        "actorsSubjectToRule": {"type": subject.as_str()},
        "capacityUnitsConsumptionThresholdPercentage": threshold,
        "blockDuration": block_duration(duration, hours)?,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn create(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    subject: ActorRuleSubject,
    threshold: u8,
    duration: ActorRuleBlockDuration,
    hours: Option<u32>,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    let body = create_body(subject, threshold, duration, hours)?;
    let mut preview = body.clone();
    preview["capacityId"] = Value::from(capacity_id);
    if output::dry_run_guard(cli, "capacity create-actor-auto-detection-rule", &preview) {
        return Ok(());
    }
    let data = client
        .post(&rule_base(capacity_id), &body, false)
        .await
        .map_err(|e| enrich_capacity(e, "capacity create-actor-auto-detection-rule"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn list(cli: &Cli, client: &FabricClient, capacity_id: &str) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    let response = client
        .get_list(
            &rule_base(capacity_id),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_capacity(e, "capacity list-actor-auto-detection-rules"))?;
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
        .get(&format!("{}/{}", rule_base(capacity_id), rule_id))
        .await
        .map_err(|e| enrich_capacity(e, "capacity show-actor-auto-detection-rule"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    rule_id: &str,
    threshold: Option<u8>,
    duration: Option<ActorRuleBlockDuration>,
    hours: Option<u32>,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(rule_id, "--rule-id")?;
    if threshold.is_none() && duration.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "No actor auto-detection rule updates were provided",
            "Provide --threshold and/or --block-duration.",
        )
        .into());
    }
    let mut body = serde_json::Map::new();
    if let Some(value) = threshold {
        body.insert(
            "capacityUnitsConsumptionThresholdPercentage".into(),
            Value::from(value),
        );
    }
    if let Some(kind) = duration {
        body.insert("blockDuration".into(), block_duration(kind, hours)?);
    } else if hours.is_some() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--hours requires --block-duration Hours",
            "Add --block-duration Hours or remove --hours.",
        )
        .into());
    }
    let body = Value::Object(body);
    let mut preview = body.clone();
    preview["capacityId"] = Value::from(capacity_id);
    preview["ruleId"] = Value::from(rule_id);
    if output::dry_run_guard(cli, "capacity update-actor-auto-detection-rule", &preview) {
        return Ok(());
    }
    let data = client
        .patch(&format!("{}/{}", rule_base(capacity_id), rule_id), &body)
        .await
        .map_err(|e| enrich_capacity(e, "capacity update-actor-auto-detection-rule"))?;
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
        &serde_json::json!({
            "capacityId": capacity_id,
            "ruleId": rule_id,
            "existingBlocksRemain": true,
        }),
    ) {
        return Ok(());
    }
    client
        .delete(&format!("{}/{}", rule_base(capacity_id), rule_id))
        .await
        .map_err(|e| enrich_capacity(e, "capacity delete-actor-auto-detection-rule"))?;
    output::render_object(
        cli,
        &serde_json::json!({"id": rule_id, "status": "deleted"}),
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
            block_duration(ActorRuleBlockDuration::Hours, Some(24)).unwrap(),
            serde_json::json!({"type": "Hours", "hours": 24})
        );
        assert_eq!(
            block_duration(ActorRuleBlockDuration::Indefinite, None).unwrap(),
            serde_json::json!({"type": "Indefinite"})
        );
    }

    #[test]
    fn block_duration_rejects_mismatched_hours() {
        assert!(block_duration(ActorRuleBlockDuration::Hours, None).is_err());
        assert!(block_duration(ActorRuleBlockDuration::Indefinite, Some(24)).is_err());
    }

    #[test]
    fn create_request_matches_spec_example() {
        assert_eq!(
            create_body(
                ActorRuleSubject::AllWorkspaces,
                25,
                ActorRuleBlockDuration::Hours,
                Some(24)
            )
            .unwrap(),
            serde_json::json!({
                "actorsSubjectToRule": {"type": "AllWorkspaces"},
                "capacityUnitsConsumptionThresholdPercentage": 25,
                "blockDuration": {"type": "Hours", "hours": 24}
            })
        );
    }
}
