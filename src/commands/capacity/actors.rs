use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, enrich_capacity};
use crate::output;

use super::BlockedActorExpirationPolicy;

fn expiration_policy(
    policy: BlockedActorExpirationPolicy,
    expire_after: Option<&str>,
) -> Result<Value> {
    match (policy, expire_after) {
        (BlockedActorExpirationPolicy::NeverExpire, None) => {
            Ok(serde_json::json!({"policyType": "NeverExpire"}))
        }
        (BlockedActorExpirationPolicy::ExpireAfterDate, Some(date)) => {
            chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%SZ").map_err(|_| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--expire-after must be a UTC timestamp in YYYY-MM-DDTHH:mm:ssZ format",
                    "Example: --expire-after 2026-12-31T23:59:59Z.",
                )
            })?;
            Ok(serde_json::json!({
                "policyType": "ExpireAfterDate",
                "expireAfterDateTime": date
            }))
        }
        (BlockedActorExpirationPolicy::NeverExpire, Some(_)) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--expire-after cannot be used with --expiration-policy NeverExpire",
            "Remove --expire-after or use --expiration-policy ExpireAfterDate.",
        )
        .into()),
        (BlockedActorExpirationPolicy::ExpireAfterDate, None) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--expire-after is required with --expiration-policy ExpireAfterDate",
            "Provide a UTC timestamp such as --expire-after 2026-12-31T23:59:59Z.",
        )
        .into()),
    }
}

fn actor_body(workspace: &str) -> Value {
    serde_json::json!({
        "actorType": "Workspace",
        "workspaceId": workspace
    })
}

pub(super) async fn create_blocked(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    workspace: &str,
    policy: BlockedActorExpirationPolicy,
    expire_after: Option<&str>,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(workspace, "--workspace")?;
    let body = serde_json::json!({
        "actorDetails": actor_body(workspace),
        "expirationPolicy": expiration_policy(policy, expire_after)?
    });
    if output::dry_run_guard(cli, "capacity create-blocked-actor", &body) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/capacities/{capacity_id}/surgeProtection/blockedActors"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_capacity(e, "capacity create-blocked-actor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn list_blocked(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    let response = client
        .get_list(
            &format!("/capacities/{capacity_id}/surgeProtection/blockedActors"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_capacity(e, "capacity list-blocked-actors"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        &[
            "actorDetails.workspaceId",
            "expirationPolicy.policyType",
            "expirationPolicy.expireAfterDateTime",
            "id",
        ],
        &["WORKSPACE", "POLICY", "EXPIRES", "ID"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn show_blocked(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    actor_id: &str,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(actor_id, "--actor-id")?;
    let data = client
        .get(&format!(
            "/capacities/{capacity_id}/surgeProtection/blockedActors/{actor_id}"
        ))
        .await
        .map_err(|e| enrich_capacity(e, "capacity show-blocked-actor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn update_blocked(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    actor_id: &str,
    policy: BlockedActorExpirationPolicy,
    expire_after: Option<&str>,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(actor_id, "--actor-id")?;
    let body = serde_json::json!({"expirationPolicy": expiration_policy(policy, expire_after)?});
    if output::dry_run_guard(cli, "capacity update-blocked-actor", &body) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!("/capacities/{capacity_id}/surgeProtection/blockedActors/{actor_id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_capacity(e, "capacity update-blocked-actor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete_blocked(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    actor_id: &str,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(actor_id, "--actor-id")?;
    if output::dry_run_guard(
        cli,
        "capacity delete-blocked-actor",
        &serde_json::json!({"capacityId": capacity_id, "actorId": actor_id}),
    ) {
        return Ok(());
    }
    client
        .delete(&format!(
            "/capacities/{capacity_id}/surgeProtection/blockedActors/{actor_id}"
        ))
        .await
        .map_err(|e| enrich_capacity(e, "capacity delete-blocked-actor"))?;
    output::render_object(
        cli,
        &serde_json::json!({"id": actor_id, "status": "deleted"}),
        "status",
    );
    Ok(())
}

pub(super) async fn create_mission_critical(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    workspace: &str,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(workspace, "--workspace")?;
    let body = serde_json::json!({"actorDetails": actor_body(workspace)});
    if output::dry_run_guard(cli, "capacity create-mission-critical-actor", &body) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/capacities/{capacity_id}/surgeProtection/missionCriticalActors"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_capacity(e, "capacity create-mission-critical-actor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn list_mission_critical(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    let response = client
        .get_list(
            &format!("/capacities/{capacity_id}/surgeProtection/missionCriticalActors"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_capacity(e, "capacity list-mission-critical-actors"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["actorDetails.workspaceId", "id"],
        &["WORKSPACE", "ID"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn show_mission_critical(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    actor_id: &str,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(actor_id, "--actor-id")?;
    let data = client
        .get(&format!(
            "/capacities/{capacity_id}/surgeProtection/missionCriticalActors/{actor_id}"
        ))
        .await
        .map_err(|e| enrich_capacity(e, "capacity show-mission-critical-actor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete_mission_critical(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    actor_id: &str,
) -> Result<()> {
    validate_uuid(capacity_id, "--id")?;
    validate_uuid(actor_id, "--actor-id")?;
    if output::dry_run_guard(
        cli,
        "capacity delete-mission-critical-actor",
        &serde_json::json!({"capacityId": capacity_id, "actorId": actor_id}),
    ) {
        return Ok(());
    }
    client
        .delete(&format!(
            "/capacities/{capacity_id}/surgeProtection/missionCriticalActors/{actor_id}"
        ))
        .await
        .map_err(|e| enrich_capacity(e, "capacity delete-mission-critical-actor"))?;
    output::render_object(
        cli,
        &serde_json::json!({"id": actor_id, "status": "deleted"}),
        "status",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expire_after_policy_serializes_spec_shape() {
        let value = expiration_policy(
            BlockedActorExpirationPolicy::ExpireAfterDate,
            Some("2026-12-31T23:59:59Z"),
        )
        .unwrap();
        assert_eq!(value["policyType"], "ExpireAfterDate");
        assert_eq!(value["expireAfterDateTime"], "2026-12-31T23:59:59Z");
    }

    #[test]
    fn expire_after_policy_requires_timestamp() {
        assert!(expiration_policy(BlockedActorExpirationPolicy::ExpireAfterDate, None).is_err());
    }

    #[test]
    fn expire_after_policy_rejects_non_utc_format() {
        assert!(
            expiration_policy(
                BlockedActorExpirationPolicy::ExpireAfterDate,
                Some("2026-12-31T23:59:59+01:00")
            )
            .is_err()
        );
    }
}
