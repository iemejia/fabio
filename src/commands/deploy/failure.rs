//! Additive failure metadata and batch-level no-replay guidance. A failed action
//! may include successful substeps; a batch is not an atomic transaction.

use serde_json::{Value, json};

use super::changeset::{Change, DeployFailure};
use crate::errors::{ErrorCode, FabioError, HintType};

pub(super) const NO_REPLAY_HINT: &str = "Do not automatically retry the deployment or its entire batch. Some actions or substeps may already have applied. Preserve this result, inspect each failure's errorDetails/submission and the existing resources read-only, then reconcile and review a new scoped plan before any further mutation.";

/// Preserve the existing hook record/text while adding canonical typed fields.
pub(super) fn hook_failure(mut record: Value, error: &anyhow::Error) -> Value {
    record["retryAllowed"] = json!(false);
    if let Some(error) = error.downcast_ref::<FabioError>() {
        record["errorDetails"] = crate::output::error_details(error);
    }
    record
}

pub(super) fn hooks_failed(hooks: &[Value]) -> bool {
    hooks.iter().any(|hook| hook["retryAllowed"] == false)
}

pub(super) fn prerequisite_failure(hook: &str, name: &str, error: &anyhow::Error) -> Value {
    hook_failure(
        json!({"hook":hook,"item_name":name,"status":"failed","error":error.to_string()}),
        error,
    )
}

pub(super) fn annotate_summary(summary: &mut Value, action_failures: bool, hooks: &[Value]) {
    if action_failures || hooks.iter().any(|hook| hook["retryAllowed"] == false) {
        summary["retryAllowed"] = json!(false);
        if let Some(hint) = summary.get("hint").cloned() {
            summary["diagnosticHint"] = hint;
        }
        summary["hint"] = json!(NO_REPLAY_HINT);
    }
}

pub(super) fn terminal_error(failures: usize) -> anyhow::Error {
    // Keep the existing UNKNOWN code/exit 1 and message, but never let the generic
    // final stderr envelope lose the stdout batch's no-replay boundary.
    FabioError::with_typed_hint(
        ErrorCode::Unknown,
        format!("Deployment completed with {failures} failures"),
        NO_REPLAY_HINT,
        HintType::SemanticCorrection,
    )
    .set_retriable(Some(false))
    .into()
}

fn result_text<'a>(row: &'a Value, current: &str, legacy: &str) -> Option<&'a str> {
    match (row.get(current), row.get(legacy)) {
        (Some(current), Some(legacy)) => current
            .as_str()
            .zip(legacy.as_str())
            .filter(|(current, legacy)| current.eq_ignore_ascii_case(legacy))
            .map(|(value, _)| value),
        (Some(value), None) | (None, Some(value)) => value.as_str(),
        _ => None,
    }
}

/// A successful HTTP/LRO response is not a successful result for every member.
/// Accept documented field names, plus coherent legacy aliases; never latest/name
/// inventory lookups or a missing/duplicate/partial row as deployment success.
pub(super) fn bulk_item_result(
    change: &Change,
    response: &Value,
) -> Result<String, Box<DeployFailure>> {
    let unverified = || {
        let error = FabioError::with_typed_hint(
            ErrorCode::ApiError,
            "The bulk response did not establish a unique complete result for this requested item.",
            NO_REPLAY_HINT,
            HintType::SemanticCorrection,
        )
        .set_retriable(Some(false));
        Box::new(DeployFailure::from_error_with_code(
            change.clone(),
            &error.into(),
            "BULK_RESULT_UNVERIFIED",
        ))
    };
    if response.get("error").is_some_and(|error| !error.is_null()) {
        return Err(unverified());
    }
    let rows = response["importItemDefinitionsDetails"]
        .as_array()
        .ok_or_else(unverified)?;
    let mut matched = rows.iter().filter(|row| {
        ["itemDisplayName", "displayName"].iter().any(|key| {
            row[*key]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(&change.name))
        }) && ["itemType", "type"].iter().any(|key| {
            row[*key]
                .as_str()
                .is_some_and(|kind| kind.eq_ignore_ascii_case(&change.item_type))
        })
    });
    let row = matched.next().ok_or_else(unverified)?;
    if matched.next().is_some() {
        return Err(unverified());
    }
    if result_text(row, "itemDisplayName", "displayName").is_none()
        || result_text(row, "itemType", "type").is_none()
    {
        return Err(unverified());
    }
    let status = result_text(row, "operationStatus", "status").ok_or_else(unverified)?;
    if status.eq_ignore_ascii_case("Failed")
        || status.eq_ignore_ascii_case("SucceededDespiteFailures")
    {
        // A Failed result does not necessarily mean earlier substeps had no effect.
        let details = row.get("errorDetails")
            .filter(|error| error.is_object()).cloned().unwrap_or_else(|| {
                json!({"message":format!("Bulk import reported {status}; reconcile this item's effects before further mutation.")})
            });
        let body = json!({"error":details});
        let error = crate::client::failed_lro_error(&body);
        let mut failure = DeployFailure::from_error_with_code(
            change.clone(),
            &error.into(),
            "BULK_IMPORT_FAILED",
        );
        failure.error = format!("Bulk import reported {status}: {}", failure.error);
        return Err(Box::new(failure));
    }
    if !status.eq_ignore_ascii_case("Succeeded") {
        return Err(unverified());
    }
    let id = result_text(row, "itemId", "id")
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .filter(|id| !id.is_nil())
        .ok_or_else(unverified)?;
    if change
        .deployed_id
        .as_ref()
        .is_some_and(|expected| uuid::Uuid::parse_str(expected).ok() != Some(id))
    {
        return Err(unverified());
    }
    if rows
        .iter()
        .filter(|row| {
            result_text(row, "itemId", "id").and_then(|id| uuid::Uuid::parse_str(id).ok())
                == Some(id)
        })
        .count()
        != 1
        || ["errorDetails", "error"]
            .iter()
            .any(|key| row.get(*key).is_some_and(|error| !error.is_null()))
    {
        return Err(unverified());
    }
    Ok(id.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::changeset::ChangeAction;
    use super::*;

    const ID: &str = "bbbbbbbb-1111-2222-3333-444444444444";

    fn change() -> Change {
        Change {
            name: "Mock".into(),
            item_type: "Notebook".into(),
            action: ChangeAction::Create,
            reason: "test".into(),
            logical_id: None,
            deployed_id: None,
            source_hash: None,
            previous_name: None,
        }
    }

    fn row() -> Value {
        json!({"itemDisplayName":"Mock","itemType":"Notebook","itemId":ID,"operationStatus":"Succeeded"})
    }

    fn response(rows: Vec<Value>) -> Value {
        json!({"importItemDefinitionsDetails":Value::Array(rows)})
    }

    #[test]
    fn bulk_accepts_complete_documented_and_coherent_legacy_results() {
        for row in [
            row(),
            json!({"displayName":"Mock","type":"Notebook","id":ID,"status":"Succeeded"}),
            json!({"itemDisplayName":"Mock","displayName":"mock","itemType":"Notebook","type":"notebook","itemId":ID,"id":ID,"operationStatus":"Succeeded","status":"succeeded"}),
        ] {
            assert_eq!(
                bulk_item_result(&change(), &response(vec![row.clone()])).unwrap(),
                ID
            );
            let mut update = change();
            update.action = ChangeAction::Update;
            update.deployed_id = Some(ID.into());
            assert_eq!(bulk_item_result(&update, &response(vec![row])).unwrap(), ID);
        }
    }

    #[test]
    fn bulk_rejects_missing_duplicate_contradictory_and_unrecognized_results() {
        let mut cases = vec![
            json!({}),
            json!({"importItemDefinitionsDetails":{}}),
            response(vec![]),
            response(vec![row(), row()]),
        ];
        for (field, value) in [
            ("itemDisplayName", json!("other")),
            ("itemType", json!("Report")),
            ("displayName", json!("other")),
            ("type", json!("Report")),
            ("id", json!("aaaaaaaa-1111-2222-3333-444444444444")),
            ("status", json!("Failed")),
            ("itemId", json!(null)),
            ("itemId", json!("not-an-id")),
            ("itemId", json!(uuid::Uuid::nil().to_string())),
            ("operationStatus", json!(null)),
            ("operationStatus", json!("Running")),
            ("operationStatus", json!("FutureStatus")),
            ("errorDetails", json!({})),
            ("error", json!({"message":"contradictory failure"})),
        ] {
            let mut invalid = row();
            invalid[field] = value;
            cases.push(response(vec![invalid]));
        }
        let mut conflicting = row();
        conflicting["displayName"] = json!("other");
        cases.push(response(vec![row(), conflicting]));
        let mut reused_id = row();
        reused_id["itemDisplayName"] = json!("Other");
        cases.push(response(vec![row(), reused_id]));
        let mut top_error = response(vec![row()]);
        top_error["error"] = json!({"message":"contradictory failure"});
        cases.push(top_error);
        for response in cases {
            let failure = bulk_item_result(&change(), &response).unwrap_err();
            assert_eq!(failure.code, "BULK_RESULT_UNVERIFIED", "{response}");
            assert!(!failure.retry_allowed);
            let details = failure.error_details.unwrap();
            assert_eq!(details["retriable"], false);
            assert!(details.get("submission").is_none());
        }
        let mut update = change();
        update.deployed_id = Some("aaaaaaaa-1111-2222-3333-444444444444".into());
        assert_eq!(
            bulk_item_result(&update, &response(vec![row()]))
                .unwrap_err()
                .code,
            "BULK_RESULT_UNVERIFIED"
        );
    }

    #[test]
    fn bulk_failed_and_partial_members_keep_api_metadata_without_fake_http_evidence() {
        for status in ["Failed", "SucceededDespiteFailures"] {
            let mut row = row();
            row["operationStatus"] = json!(status);
            row["errorDetails"] = json!({"errorCode":"MockFailure","message":"mock failure","requestId":"mock-trace","isRetriable":true,"parameters":[{"name":"mock","value":"preserved"}]});
            let failure = bulk_item_result(&change(), &response(vec![row])).unwrap_err();
            assert_eq!(failure.code, "BULK_IMPORT_FAILED");
            assert!(failure.error.contains(status));
            assert!(!failure.retry_allowed);
            let details = failure.error_details.unwrap();
            assert_eq!(details["code"], "API_ERROR");
            assert_eq!(details["requestId"], "mock-trace");
            assert_eq!(details["parameters"][0]["value"], "preserved");
            assert_eq!(details["retriable"], true);
            assert!(details.get("submission").is_none());
            assert!(details["message"].as_str().unwrap().contains("MockFailure"));
        }
    }

    #[test]
    fn hook_errors_preserve_fields_and_do_not_invent_typed_metadata() {
        let record = json!({"hook":"mock","status":"failed","error":"legacy"});
        let error = anyhow::Error::new(FabioError::api_error("mock")).context("wrapped");
        let value = hook_failure(record.clone(), &error);
        assert_eq!(value["hook"], record["hook"]);
        assert_eq!(value["error"], record["error"]);
        assert_eq!(value["retryAllowed"], false);
        assert_eq!(value["errorDetails"]["code"], "API_ERROR");
        let value = hook_failure(record, &anyhow::anyhow!("mock"));
        assert_eq!(value["retryAllowed"], false);
        assert!(value.get("errorDetails").is_none());
        assert!(hooks_failed(&[value]));
        assert!(!hooks_failed(&[json!({"status":"triggered"})]));
    }

    #[test]
    fn summary_no_replay_dominates_diagnostics_without_changing_legacy_counts_or_status() {
        for (action_failures, hooks) in
            [(true, vec![]), (false, vec![json!({"retryAllowed":false})])]
        {
            let mut summary =
                json!({"status":"succeeded","succeeded":2,"failed":0,"hint":"diagnostic"});
            annotate_summary(&mut summary, action_failures, &hooks);
            assert_eq!(summary["status"], "succeeded");
            assert_eq!(summary["succeeded"], 2);
            assert_eq!(summary["failed"], 0);
            assert_eq!(summary["diagnosticHint"], "diagnostic");
            assert_eq!(summary["hint"], NO_REPLAY_HINT);
            assert_eq!(summary["retryAllowed"], false);
        }
        let mut success = json!({"status":"succeeded"});
        let expected = success.clone();
        annotate_summary(&mut success, false, &[json!({"status":"succeeded"})]);
        assert_eq!(success, expected);
        let error = terminal_error(2);
        let error = error.downcast_ref::<FabioError>().unwrap();
        assert_eq!(error.code, ErrorCode::Unknown);
        assert_eq!(error.message, "Deployment completed with 2 failures");
        assert_eq!(error.retriable, Some(false));
        assert_eq!(error.hint.as_deref(), Some(NO_REPLAY_HINT));
    }
}
