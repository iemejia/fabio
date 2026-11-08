//! Read-only orchestration preparation. The API assesses the whole workspace;
//! local selection never makes it an ID-filtered request. No upgrade, backup,
//! takeover, consumer rebinding, refresh, grant or credential write runs here.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::diagnostics::{
    Check, PageCursor, ReadBudget, Status, error_evidence, id, summary, uuid_target,
};
use crate::commands::remediation::{Kind, RecoveryHint};
use crate::errors::{ErrorCode, FabioError, HintType, enrich_forbidden};
use crate::output;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Scan {
    complete: bool,
    pages: u8,
    rows_inspected: usize,
    reason: Option<&'static str>,
    error: Option<Value>,
    #[serde(skip)]
    selected: HashMap<Uuid, Value>,
    #[serde(skip)]
    duplicates: HashSet<Uuid>,
}

fn error_body(body: &Value) -> bool {
    ["error", "errorCode"]
        .iter()
        .any(|key| body.get(*key).is_some_and(|value| !value.is_null()))
}

#[allow(clippy::too_many_arguments)]
async fn assess(
    client: &FabricClient,
    budget: &ReadBudget,
    workspace: Uuid,
    selected: &HashSet<Uuid>,
    page_size: u8,
    page_limit: u8,
    row_limit: u16,
) -> Scan {
    let mut scan = Scan::default();
    let mut cursor = PageCursor::default();
    let mut token: Option<String> = None;
    let mut seen = HashSet::new();
    for _ in 0..page_limit {
        let base = format!(
            "/workspaces/{workspace}/dataflows/gen1UpgradeReadinessResults?pageSize={page_size}"
        );
        let path = token.as_ref().map_or_else(
            || base.clone(),
            |token| format!("{base}&continuationToken={}", urlencoding::encode(token)),
        );
        let body = match budget.get(client, &path).await {
            Ok(body) => body,
            Err(error) => {
                scan.reason = Some("read_failed");
                scan.error = Some(error_evidence(&enrich_forbidden(
                    error,
                    "dataflow plan-upgrade",
                    "Viewer",
                )));
                return scan;
            }
        };
        scan.pages += 1;
        if error_body(&body)
            || body
                .get("workspaceId")
                .is_some_and(|value| id(value) != Some(workspace))
        {
            scan.reason = Some("error_bearing_or_wrong_scope_page");
            return scan;
        }
        let Some(rows) = body["value"].as_array() else {
            scan.reason = Some("malformed_page");
            return scan;
        };
        let remaining = usize::from(row_limit).saturating_sub(scan.rows_inspected);
        for row in rows.iter().take(remaining) {
            scan.rows_inspected += 1;
            if error_body(row)
                || row
                    .get("workspaceId")
                    .is_some_and(|value| id(value) != Some(workspace))
            {
                scan.reason = Some("error_bearing_or_wrong_scope_row");
                return scan;
            }
            let Some(target) = id(&row["id"]) else {
                scan.reason = Some("malformed_assessment_identity");
                return scan;
            };
            if !seen.insert(target) {
                scan.duplicates.insert(target);
            }
            if selected.contains(&target) {
                scan.selected.insert(target, row.clone());
            }
        }
        if rows.len() > usize::from(page_size) || rows.len() > remaining {
            scan.reason = Some("assessment_row_or_page_size_limit");
            return scan;
        }
        token = match cursor.next(&body) {
            Ok(token) => token,
            Err(reason) => {
                scan.reason = Some(reason);
                return scan;
            }
        };
        if token.is_none() {
            if scan.duplicates.is_empty() {
                scan.complete = true;
            } else {
                scan.reason = Some("duplicate_assessment_identity");
            }
            return scan;
        }
        if scan.rows_inspected >= usize::from(row_limit) {
            scan.reason = Some("assessment_row_limit");
            return scan;
        }
    }
    scan.reason = Some("page_limit");
    scan
}

fn verdict(value: &Value) -> Option<(&'static str, Status)> {
    match value.as_str()? {
        "ReadyToMigrate" => Some(("ReadyToMigrate", Status::Ready)),
        "NeedsAttention" => Some(("NeedsAttention", Status::NeedsAction)),
        "UpgradeUnavailable" => Some(("UpgradeUnavailable", Status::Blocked)),
        "Unknown" => Some(("Unknown", Status::Unknown)),
        _ => None,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Reason {
    code: &'static str,
    reported_status: &'static str,
    review_stage: &'static str,
    guidance: &'static str,
}

// API codes select authored review guidance, not automatic repairs or inferred
// consumer identities/storage modes. Never emit the free-text API messages.
fn guidance(code: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match code {
        "MightBeAccessedByDirectQuery" => (
            "MightBeAccessedByDirectQuery",
            "consumer_review",
            "DirectQuery may be in use; the assessment is not proof of a consumer. Inventory actual consumers and review Import/Direct Lake alternatives separately; never switch their storage/security mode automatically.",
        ),
        "HasByolAsStorage" => (
            "HasByolAsStorage",
            "storage_review",
            "Gen1 bring-your-own-lake storage is not carried over as a Gen2 destination. Review explicit destination and source/security requirements; do not silently repoint storage.",
        ),
        "HasIncrementalRefresh" => (
            "HasIncrementalRefresh",
            "storage_review",
            "Gen1 incremental-refresh settings and cached history are not preserved. Review reload/backups and a supported Gen2 destination before cutover; no settings or data are rewritten.",
        ),
        "HasLinkedEntities" => (
            "HasLinkedEntities",
            "consumer_review",
            "Linked-entity cascading refresh is not supported by Gen2. Review the upstream dependency and separate orchestration; no pipeline or schedule is created.",
        ),
        "IsReferencedByLinkedEntities" => (
            "IsReferencedByLinkedEntities",
            "consumer_review",
            "Review downstream linked consumers and connector compatibility. The flag does not enumerate all consumers; no rebinding or save/refresh occurs.",
        ),
        "HasUnsupportedNameCharacters" => (
            "HasUnsupportedNameCharacters",
            "before_upgrade",
            "The upgrade may sanitize the display name. Review an explicit name choice before proceeding; the planner never renames items.",
        ),
        "DisplayNameAlreadyInUse" => (
            "DisplayNameAlreadyInUse",
            "before_upgrade",
            "The upgrade may prefix the target name to avoid a conflict. Review naming and references explicitly; no winner or rename is selected.",
        ),
        "ExceedsGen2QueryLimit" => (
            "ExceedsGen2QueryLimit",
            "before_upgrade",
            "Reduce load-enabled queries to at most 50 through a separately reviewed change before upgrading; the planner never drops queries.",
        ),
        "HasUnresolvedConnections" => (
            "HasUnresolvedConnections",
            "before_upgrade",
            "Review unresolved source connections and intended identity/handoff. No connection grant, takeover, credential test/write or rebinding is authorized.",
        ),
        "ServiceThrottled" => (
            "ServiceThrottled",
            "assessment_only",
            "The read-only assessment was throttled. Reassess later within explicit bounds; this is not permission to submit or retry an upgrade.",
        ),
        "AssessmentUnavailable" => (
            "AssessmentUnavailable",
            "assessment_only",
            "Assessment could not run. Investigate availability/permissions; absence of assessment is not eligibility and does not authorize an upgrade.",
        ),
        _ => return None,
    })
}

struct Evidence {
    status: Status,
    reported: Option<&'static str>,
    reason: &'static str,
    reasons: Vec<Reason>,
}

fn parse_assessment(row: &Value, workspace: Uuid) -> Evidence {
    let mut result = Evidence {
        status: Status::Unknown,
        reported: None,
        reason: "malformed_or_unrecognized_assessment",
        reasons: Vec::new(),
    };
    if error_body(row)
        || row
            .get("workspaceId")
            .is_some_and(|value| id(value) != Some(workspace))
        || !row["displayName"].is_string()
    {
        return result;
    }
    let Some((reported, status)) = verdict(&row["status"]) else {
        return result;
    };
    result.reported = Some(reported);
    if status == Status::Ready {
        if row.get("reasons").is_none() {
            result.status = status;
            result.reason = "service_reports_ready";
        }
        return result;
    }
    let Some(reasons) = row["reasons"]
        .as_array()
        .filter(|rows| !rows.is_empty() && rows.len() <= 32)
    else {
        return result;
    };
    let mut statuses = Vec::new();
    let mut codes = HashSet::new();
    for reason in reasons {
        let Some((reported, severity)) = verdict(&reason["status"]) else {
            return result;
        };
        let Some((code, stage, message)) = reason["code"].as_str().and_then(guidance) else {
            return result;
        };
        if !reason["message"].is_string() || error_body(reason) || !codes.insert(code) {
            return result;
        }
        result.reasons.push(Reason {
            code,
            reported_status: reported,
            review_stage: stage,
            guidance: message,
        });
        statuses.push(severity);
    }
    let unknown = statuses.contains(&Status::Unknown);
    // Unknown is not an aggregation verdict. Any unknown contributing reason
    // leaves readiness unknown even if the row claims a stronger verdict.
    let highest = crate::commands::diagnostics::summary_statuses(statuses.iter().copied());
    if !unknown && highest == status {
        result.status = status;
        result.reason = "recognized_service_assessment";
    } else if status == Status::Unknown {
        result.reason = "service_assessment_unavailable";
    }
    result
}

fn inspect(workspace: Uuid, target: Uuid) -> String {
    format!(
        "fabio --wrap-untrusted --readonly dataflow plan-upgrade --workspace {workspace} --id {target}"
    )
}

fn preview(workspace: Uuid, targets: &[Uuid]) -> RecoveryHint {
    let ids = targets.iter().fold(String::new(), |mut args, id| {
        write!(args, " --id {id}").expect("writing to a String cannot fail");
        args
    });
    RecoveryHint {
        kind: Kind::Remediation,
        message: "Preview only. In-place upgrade is irreversible and does not preserve cached data. The planner neither records approval nor supplies an execution/receipt workflow; keep BOTH guards and review every blocker.".into(),
        command: Some(format!("fabio --wrap-untrusted --readonly dataflow upgrade-gen1 --workspace {workspace}{ids} --dry-run")),
        hint_type: HintType::SemanticCorrection,
        verify_after: Some(targets.iter().map(|target| format!("fabio --wrap-untrusted --readonly dataflow show --workspace {workspace} --id {target}")).collect::<Vec<_>>().join(" && ")),
        requires_approval: true,
        destructive: Some(crate::agent::is_destructive_operation("dataflow upgrade-gen1")),
        command_template: None,
        missing_inputs: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn plan(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    ids: &[String],
    page_size: u8,
    page_limit: u8,
    row_limit: u16,
    timeout: u64,
    strict: bool,
) -> Result<()> {
    let workspace = uuid_target(workspace, "--workspace")?;
    if ids.is_empty() || ids.len() > 50 || cli.continuation_token.is_some() {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput, "Select 1-50 distinct Gen1 dataflow IDs; aggregate plans cannot resume a list cursor.", "Use repeated --id flags in one workspace. Use list-upgrade-readiness, not plan-upgrade, to resume an assessment cursor.").into());
    }
    let targets: Vec<_> = ids
        .iter()
        .map(|value| uuid_target(value, "--id"))
        .collect::<Result<_>>()?;
    let selected: HashSet<_> = targets.iter().copied().collect();
    if selected.len() != targets.len() {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            "Duplicate Gen1 dataflow IDs in the selected upgrade scope",
        )
        .into());
    }
    let body =
        super::build_upgrade_gen1_body(&targets.iter().map(Uuid::to_string).collect::<Vec<_>>())?;
    let scope = json!({"workspaceId":workspace.to_string(),"selectedDataflowIds":targets.iter().map(Uuid::to_string).collect::<Vec<_>>(),"selectedCount":targets.len(),"assessmentScope":"all_gen1_in_one_workspace","pageSize":page_size,"pageLimit":page_limit,"assessmentRowLimit":row_limit,"timeoutSeconds":timeout,"maxLogicalReads":page_limit,"mutates":false});
    if output::dry_run_guard(
        cli,
        "dataflow plan-upgrade",
        &json!({"scope":scope,"proposedUpgradeRequestBytes":serde_json::to_vec(&body)?.len(),"executesUpgrade":false}),
    ) {
        return Ok(());
    }
    let budget = ReadBudget::new(timeout);
    let scan = assess(
        client, &budget, workspace, &selected, page_size, page_limit, row_limit,
    )
    .await;
    let mut checks = Vec::new();
    for target in &targets {
        let evidence = scan.selected.get(target).map_or_else(
            || Evidence {
                status: Status::Unknown,
                reported: None,
                reason: if scan.complete {
                    "not_in_gen1_assessment"
                } else {
                    "selected_row_not_observed"
                },
                reasons: Vec::new(),
            },
            |row| parse_assessment(row, workspace),
        );
        let status = if scan.complete {
            evidence.status
        } else {
            Status::Unknown
        };
        let mut check = Check::new(
            "gen1_upgrade_assessment",
            status,
            "Service assessment is not consumer compatibility, backup, cached-data preservation, upgrade authorization, approval or refreshed Gen2 readiness. Missing rows may be Gen2/excluded/inaccessible, not deleted.",
            json!({"dataflowId":target.to_string(),"reportedAssessmentStatus":evidence.reported,"reason":if scan.complete{evidence.reason}else{"assessment_scan_incomplete"},"assessmentReasons":evidence.reasons,"scanComplete":scan.complete}),
            inspect(workspace, *target),
        );
        check.next_steps.push(RecoveryHint::review("Review ALL consumers, historical-data reload/backups, storage/security modes, names, schedules, permissions, connector and post-upgrade refresh changes before separate explicit approval. No takeover, grants, backups, rewrite, refresh or upgrade ran."));
        checks.push(check);
    }
    let status = summary(checks.iter());
    let all_ready = status == Status::Ready;
    output::render_object(
        cli,
        &json!({
            "observedAt":Utc::now().to_rfc3339(),"scope":scope,"assessmentScan":scan,"status":status,
            "partial":!scan.complete || checks.iter().any(|check|check.status==Status::Unknown),"mutates":false,"destructive":true,"checks":checks,
            "reviewPlan":{"applied":false,"readyToApply":false,"requiresApproval":true,"destructive":true,"mode":"gen1_to_gen2_in_place","selectedCount":targets.len(),"allSelectedAssessmentsReady":all_ready,"guardedPreview":all_ready.then(||preview(workspace,&targets)),
            "blockers":["explicit_approval_not_recorded","all_consumers_and_storage_security_not_verified","historical_data_backup_or_reload_not_verified","post_upgrade_refresh_and_consumer_validation_not_planned","guarded_execution_and_correlated_receipt_workflow_not_implemented_by_planner"]},
            "notChecked":["all Fabric, external, Desktop and Excel consumers or legacy connectors","actual DirectQuery consumers or approved storage-mode changes","source reloadability, cached history and backup","ownership, grants, capacity/tenant enablement, costs and permissions to upgrade","names, schedules, Gen2 destinations and incremental refresh handoff","upgrade submission, acceptance, completion and post-upgrade data"],
            "limitations":"GET-only bounded planning. The API evaluates all workspace Gen1 dataflows without ID filtering; only selected evidence is emitted. Successful in-place upgrades cannot be reverted and do not preserve cached data. No apply, conversion, import, grants, takeover, credentials, mode changes, refresh or blind upgrade retry occurs."
        }),
        "status",
    );
    if strict && !all_ready {
        return Err(FabioError::with_typed_hint(ErrorCode::ApiError, "Selected Gen1 assessments are not all ready.", "Review partial/unknown evidence and every reason. --all/--force do not bypass planning limits; a ready assessment or guarded preview is not approval to apply an irreversible upgrade.", HintType::SemanticCorrection).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(status: &str, reasons: Option<Value>) -> Value {
        let mut row =
            json!({"id":Uuid::new_v4().to_string(),"displayName":"private","status":status});
        if let Some(reasons) = reasons {
            row["reasons"] = reasons;
        }
        row
    }
    fn reason(status: &str, code: &str) -> Value {
        json!({"status":status,"code":code,"message":"private endpoint credential"})
    }

    #[test]
    fn ready_and_recognized_attention_are_assessment_evidence_only() {
        let workspace = Uuid::new_v4();
        assert_eq!(
            parse_assessment(&row("ReadyToMigrate", None), workspace).status,
            Status::Ready
        );
        let evidence = parse_assessment(
            &row(
                "NeedsAttention",
                Some(json!([reason("NeedsAttention", "HasByolAsStorage")])),
            ),
            workspace,
        );
        assert_eq!(evidence.status, Status::NeedsAction);
        let safe = serde_json::to_string(&evidence.reasons).unwrap();
        assert!(!safe.contains("private"));
        assert!(safe.contains("storage_review"));
    }

    #[test]
    fn future_malformed_and_conflicting_assessments_remain_unknown() {
        for row in [
            row("FutureStatus", None),
            row("ReadyToMigrate", Some(Value::Null)),
            row(
                "ReadyToMigrate",
                Some(json!([reason(
                    "UpgradeUnavailable",
                    "HasUnresolvedConnections"
                )])),
            ),
            row(
                "NeedsAttention",
                Some(json!([reason(
                    "UpgradeUnavailable",
                    "HasUnresolvedConnections"
                )])),
            ),
            row(
                "NeedsAttention",
                Some(json!([reason("NeedsAttention", "FutureReason")])),
            ),
            row("NeedsAttention", Some(json!([]))),
            row("Unknown", None),
        ] {
            assert_eq!(
                parse_assessment(&row, Uuid::new_v4()).status,
                Status::Unknown
            );
        }
    }

    #[test]
    fn authoritative_unavailable_and_unknown_reasons_are_not_force_bypasses() {
        let workspace = Uuid::new_v4();
        let evidence = parse_assessment(
            &row(
                "UpgradeUnavailable",
                Some(json!([reason(
                    "UpgradeUnavailable",
                    "HasUnresolvedConnections"
                )])),
            ),
            workspace,
        );
        assert_eq!(evidence.status, Status::Blocked);
        let evidence = parse_assessment(
            &row(
                "Unknown",
                Some(json!([reason("Unknown", "ServiceThrottled")])),
            ),
            workspace,
        );
        assert_eq!(evidence.status, Status::Unknown);
        assert_eq!(evidence.reason, "service_assessment_unavailable");
        // The service can stop before finishing an assessment after discovering
        // a known issue. Unknown is not derived from those reason severities.
        let evidence = parse_assessment(
            &row(
                "Unknown",
                Some(json!([reason("NeedsAttention", "HasLinkedEntities")])),
            ),
            workspace,
        );
        assert_eq!(evidence.status, Status::Unknown);
        assert_eq!(evidence.reason, "service_assessment_unavailable");
    }

    #[test]
    fn all_documented_codes_have_review_guidance_and_unknown_codes_are_not_echoed() {
        for code in [
            "MightBeAccessedByDirectQuery",
            "HasByolAsStorage",
            "HasIncrementalRefresh",
            "HasLinkedEntities",
            "IsReferencedByLinkedEntities",
            "ServiceThrottled",
            "HasUnsupportedNameCharacters",
            "DisplayNameAlreadyInUse",
            "ExceedsGen2QueryLimit",
            "HasUnresolvedConnections",
            "AssessmentUnavailable",
        ] {
            assert!(guidance(code).is_some());
        }
        assert!(guidance("private-untrusted-code").is_none());
    }
}
