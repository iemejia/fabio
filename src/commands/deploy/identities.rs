//! Stable identities for plan-bearing generic deployments. Bootstrap identities
//! only on items this operation just created; never silently migrate existing
//! target identities, because other automations may depend on them.

use anyhow::Result;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError};

use super::changeset::{ChangeAction, Changeset};
use super::platform::{PlatformMetadata, SourceWorkspace};

pub(super) fn required(changeset: &Changeset) -> bool {
    changeset.changes.iter().any(|change| {
        change.item_type.eq_ignore_ascii_case("DeploymentPlan")
            && change.action != ChangeAction::Delete
    })
}

fn logical_id(value: Option<&str>) -> Option<Uuid> {
    value
        .and_then(|value| Uuid::parse_str(value).ok())
        .filter(|id| !id.is_nil())
}

pub(super) async fn check_existing(
    client: &FabricClient,
    workspace: &str,
    source: &SourceWorkspace,
    changeset: &Changeset,
) -> Result<()> {
    validate_uuid(workspace, "target workspace ID")?;
    let expected: Vec<_> = changeset
        .changes
        .iter()
        .filter_map(|change| {
            let target = change.deployed_id.as_deref()?;
            let source = source.items.iter().find(|item| {
                item.metadata
                    .item_type
                    .eq_ignore_ascii_case(&change.item_type)
                    && item.metadata.display_name == change.name
            })?;
            Some((target, logical_id(source.metadata.logical_id.as_deref())?))
        })
        .collect();
    if expected.is_empty() {
        return Ok(());
    }
    let items = client
        .get_list(
            &format!("/workspaces/{workspace}/items"),
            "value",
            true,
            None,
        )
        .await?;
    for (target, wanted) in expected {
        let actual = items
            .items
            .iter()
            .find(|item| item["id"].as_str() == Some(target))
            .and_then(|item| logical_id(item["logicalId"].as_str()));
        check_identity(actual, wanted)?;
    }
    Ok(())
}

fn check_identity(actual: Option<Uuid>, expected: Uuid) -> Result<()> {
    if actual != Some(expected) {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,
            "Plan-bearing deployment found an existing item with a different or unassigned logical identity. No existing identities were changed.",
            "Review the source/target logical-ID mapping before deployment. Use item list to inspect metadata; only explicitly change an existing identity after reviewing its dependent automations. Neither --force nor --allow-unresolved bypasses this check.").into());
    }
    Ok(())
}

/// Called only after a successful CREATE in the plan-bearing per-item executor.
/// A getDefinition .platform payload is not authoritative for assigned identity.
pub(super) async fn restore_new(
    client: &FabricClient,
    workspace: &str,
    item: &str,
    metadata: &PlatformMetadata,
    created: &Value,
) -> Result<()> {
    validate_uuid(workspace, "target workspace ID")?;
    validate_uuid(item, "newly created item ID")?;
    let Some(wanted) = logical_id(metadata.logical_id.as_deref()) else {
        return Ok(());
    };
    if logical_id(created["logicalId"].as_str()) == Some(wanted) {
        return Ok(());
    }
    let result = client
        .patch(
            &format!("/workspaces/{workspace}/items/{item}/logicalId"),
            &json!({"logicalId": wanted.to_string()}),
        )
        .await?;
    if logical_id(result["logicalId"].as_str()) != Some(wanted) {
        return Err(FabioError::with_hint(ErrorCode::ApiError,
            "An item was created but its logical identity could not be confirmed.",
            "Inspect the newly created target item before retrying. Do not blindly repeat a partially completed deployment.").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_bootstrap_is_limited_to_selected_plan_artifacts() {
        for action in [
            ChangeAction::Create,
            ChangeAction::Update,
            ChangeAction::Rename,
            ChangeAction::Skip,
            ChangeAction::Delete,
        ] {
            let mut changeset = Changeset::new();
            changeset.changes.push(super::super::changeset::Change {
                name: "Fixture".to_string(),
                item_type: "DeploymentPlan".to_string(),
                action,
                reason: "fixture".to_string(),
                logical_id: None,
                deployed_id: None,
                source_hash: None,
                previous_name: None,
            });
            assert_eq!(required(&changeset), action != ChangeAction::Delete);
            changeset.changes[0].item_type = "Notebook".to_string();
            assert!(!required(&changeset));
        }
        assert!(!required(&Changeset::new()));
    }

    #[test]
    fn existing_identity_mismatches_are_not_silently_migrated() {
        let expected = Uuid::parse_str("aaaaaaaa-1111-2222-3333-444444444444").unwrap();
        check_identity(Some(expected), expected).unwrap();
        for actual in [
            None,
            Some(Uuid::nil()),
            Some(Uuid::parse_str("bbbbbbbb-1111-2222-3333-444444444444").unwrap()),
        ] {
            let error = check_identity(actual, expected).unwrap_err();
            assert_eq!(
                error.downcast_ref::<FabioError>().unwrap().code,
                ErrorCode::InvalidInput
            );
        }
    }

    #[test]
    fn identity_comparison_normalizes_uuid_forms_and_ignores_zero_placeholders() {
        assert_eq!(logical_id(None), None);
        assert_eq!(logical_id(Some("invalid")), None);
        assert_eq!(
            logical_id(Some("00000000-0000-0000-0000-000000000000")),
            None
        );
        assert_eq!(
            logical_id(Some("AAAAAAAA-1111-2222-3333-444444444444")),
            logical_id(Some("aaaaaaaa-1111-2222-3333-444444444444"))
        );
    }
}
