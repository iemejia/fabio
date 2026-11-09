//! Submit once, persist scope before the POST, and observe only a verified
//! operation. Migration completion is distinct from Gen2 refresh/consumer health.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use reqwest::header::HeaderMap;
use reqwest::{StatusCode, Url};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::{FabricClient, is_secure_or_loopback};
use crate::commands::diagnostics::{
    ReadBudget, Status, error_evidence, has_error as error_body, id, uuid_target,
};
use crate::commands::jobs::{JobEntry, JobLedger};
use crate::errors::{ErrorCode, FabioError, HintType};
use crate::output;

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<Result<&'a str, &'static str>> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?;
    Some(if values.next().is_some() {
        Err("duplicate_identity_header")
    } else {
        first
            .to_str()
            .ok()
            .filter(|value| !value.is_empty() && value.len() <= 8192)
            .ok_or("malformed_identity_header")
    })
}

fn operation(headers: &HeaderMap, endpoint: &str) -> (Option<Uuid>, &'static str) {
    let direct = match header(headers, "x-ms-operation-id") {
        None => None,
        Some(Ok(value)) if value.len() == 36 => {
            match Uuid::parse_str(value).ok().filter(|id| !id.is_nil()) {
                Some(id) => Some(id),
                None => return (None, "invalid_operation_header"),
            }
        }
        _ => return (None, "invalid_operation_header"),
    };
    let location = match header(headers, "location") {
        None => None,
        Some(Ok(value)) => {
            if value.trim() != value
                || value.contains(['%', '\\'])
                || value.chars().any(char::is_control)
                || value.split('/').any(|part| matches!(part, "." | ".."))
            {
                return (None, "invalid_location");
            }
            let Ok(base) = Url::parse(endpoint) else {
                return (None, "invalid_endpoint");
            };
            let Ok(url) = base.join(value) else {
                return (None, "invalid_location");
            };
            // The operation header is independently authoritative. A different-
            // origin Location never selects transport or erases this identity.
            if url.origin() != base.origin() {
                return direct.map_or((None, "invalid_location"), |id| {
                    (Some(id), "verified_header_location_ignored")
                });
            }
            let prefix = format!("{}/operations/", base.path().trim_end_matches('/'));
            let Some(tail) = url.path().strip_prefix(&prefix) else {
                return (None, "wrong_operation_route");
            };
            if !is_secure_or_loopback(url.as_str())
                || url.origin() != base.origin()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || tail.len() != 36
            {
                return (None, "invalid_location");
            }
            match Uuid::parse_str(tail).ok().filter(|id| !id.is_nil()) {
                Some(id) => Some(id),
                None => return (None, "invalid_location"),
            }
        }
        _ => return (None, "invalid_location"),
    };
    if direct.is_some() && location.is_some() && direct != location {
        return (None, "conflicting_operation_identity");
    }
    direct
        .or(location)
        .map_or((None, "identity_unavailable"), |id| (Some(id), "verified"))
}

fn correlation_evidence(headers: &HeaderMap, endpoint: &str, reason: &str) -> Value {
    let operation = header(headers, "x-ms-operation-id");
    // Retain only bounded path-token syntax, never URLs, userinfo or query strings.
    // This candidate is DIAGNOSTIC, not verified operation identity or approval.
    let candidate = operation
        .as_ref()
        .and_then(|value| value.as_ref().ok())
        .copied()
        .filter(|value| {
            value.len() <= 4096
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-+/=".contains(&byte))
        });
    let location = header(headers, "location");
    let location_url = location
        .as_ref()
        .and_then(|value| value.as_ref().ok())
        .and_then(|value| Url::parse(endpoint).ok()?.join(value).ok());
    json!({"reason":reason,"operationHeaderPresent":operation.is_some(),"operationHeaderLength":operation.as_ref().and_then(|value|value.as_ref().ok()).map(|value|value.len()),
        "unverifiedOperationCandidate":candidate,"locationHeaderPresent":location.is_some(),
        "locationSameOrigin":location_url.as_ref().zip(Url::parse(endpoint).ok().as_ref()).map(|(location,base)|location.origin()==base.origin()),
        "locationHasEncodedPath":location_url.as_ref().map(|url|url.path().contains('%'))})
}

fn failure(message: &str) -> FabioError {
    FabioError::with_typed_hint(ErrorCode::ApiError, message,
        "Preserve the receipt. Some dataflows may already be upgraded. Do not resubmit the batch, retry unknown/accepted work, or use isRetriable as approval or a cooldown bypass. Inspect this same local receipt with dataflow follow-upgrade.",
        HintType::SemanticCorrection).set_retriable(Some(false))
}

fn scope(entry: &JobEntry) -> Result<(Uuid, Vec<Uuid>, Option<Uuid>)> {
    if entry.kind != "gen1-upgrade" {
        return Err(
            FabioError::new(ErrorCode::InvalidInput, "Receipt kind is not Gen1 upgrade.").into(),
        );
    }
    let workspace = uuid_target(&entry.workspace, "receipt workspace")?;
    super::build_upgrade_gen1_body(&entry.target_ids)?;
    let targets = entry
        .target_ids
        .iter()
        .map(|id| uuid_target(id, "receipt target"))
        .collect::<Result<Vec<_>>>()?;
    let operation = entry
        .operation_id
        .as_deref()
        .map(|id| uuid_target(id, "receipt operation"))
        .transpose()?;
    Ok((workspace, targets, operation))
}

fn action_code(value: &Value) -> Value {
    match value.as_str() {
        Some(code)
            if matches!(
                code,
                "MightBeAccessedByDirectQuery"
                    | "HasByolAsStorage"
                    | "HasIncrementalRefresh"
                    | "HasLinkedEntities"
                    | "IsReferencedByLinkedEntities"
                    | "ServiceThrottled"
                    | "HasUnsupportedNameCharacters"
                    | "DisplayNameAlreadyInUse"
                    | "ExceedsGen2QueryLimit"
                    | "HasUnresolvedConnections"
                    | "AssessmentUnavailable"
            ) =>
        {
            Value::from(code)
        }
        _ => Value::from("UnrecognizedRequiredAction"),
    }
}

#[allow(clippy::too_many_lines)]
fn result(
    body: &Value,
    workspace: Uuid,
    targets: &[Uuid],
    operation: Option<Uuid>,
    cached: bool,
) -> std::result::Result<Value, &'static str> {
    if error_body(body) {
        return Err("error_bearing_result");
    }
    let Some(rows) = body["migrations"]
        .as_array()
        .filter(|rows| rows.len() == targets.len())
    else {
        return Err("incomplete_migration_result");
    };
    let selected: HashSet<_> = targets.iter().copied().collect();
    let mut found = HashMap::new();
    let mut succeeded = 0u64;
    let mut failed = 0u64;
    for row in rows {
        let Some(target) = id(&row["dataflowId"]) else {
            return Err("invalid_result_identity");
        };
        if !selected.contains(&target)
            || id(&row["workspaceId"]) != Some(workspace)
            || found.contains_key(&target)
        {
            return Err("wrong_or_duplicate_result_scope");
        }
        if !cached
            && ["dataflowName", "targetDataflowName", "workspaceName"]
                .iter()
                .any(|key| !row[*key].is_string())
        {
            return Err("malformed_result");
        }
        let assessment = match row["assessmentStatus"].as_str() {
            Some("ReadyToMigrate") => "ReadyToMigrate",
            Some("NeedsAttention") => "NeedsAttention",
            Some("Unknown") => "Unknown",
            _ => return Err("invalid_assessment_status"),
        };
        let status = match row["migrationStatus"].as_str() {
            Some("Succeeded") => {
                succeeded += 1;
                "Succeeded"
            }
            Some("Failed") => {
                failed += 1;
                "Failed"
            }
            Some("Unknown") => "Unknown",
            _ => return Err("invalid_migration_status"),
        };
        let error = if status == "Failed" {
            let error = &row["migrationError"];
            if !error["isRetriable"].is_boolean() || !error["errorCode"].is_string() {
                return Err("malformed_migration_error");
            }
            if let Some(value) = error.get("requestId")
                && (id(value).is_none()
                    || operation.is_some_and(|expected| id(value) != Some(expected)))
            {
                return Err("conflicting_migration_operation");
            }
            let code = match error["errorCode"].as_str() {
                Some("InsufficientPrivileges") => "InsufficientPrivileges",
                Some("DataflowNotFound") => "DataflowNotFound",
                Some("DataflowBusy") => "DataflowBusy",
                Some("DataflowRequiresChanges") => "DataflowRequiresChanges",
                Some("CapacityOrLicenseNotSupported") => "CapacityOrLicenseNotSupported",
                Some("UpgradeFailed") => "UpgradeFailed",
                _ => "UnrecognizedMigrationError",
            };
            Some(json!({"errorCode":code,"isRetriable":error["isRetriable"],"retryAllowed":false}))
        } else {
            if row
                .get("migrationError")
                .is_some_and(|value| !value.is_null())
            {
                return Err("conflicting_migration_error");
            }
            None
        };
        let actions = if cached {
            row["requiredActionCodes"]
                .as_array()
                .filter(|rows| rows.len() <= 32)
                .map(|rows| rows.iter().map(action_code).collect::<Vec<_>>())
        } else {
            row.get("requiredActions")
                .map_or(Some(Vec::new()), |value| {
                    value
                        .as_array()
                        .filter(|rows| rows.len() <= 32)
                        .map(|rows| {
                            rows.iter()
                                .map(|row| action_code(&row["code"]))
                                .collect::<Vec<_>>()
                        })
                })
        };
        if actions.is_none() {
            return Err("malformed_required_actions");
        }
        found.insert(target, json!({"dataflowId":target.to_string(),"workspaceId":workspace.to_string(),"assessmentStatus":assessment,"migrationStatus":status,"migrationError":error,"requiredActionCodes":actions.unwrap()}));
    }
    let summary = &body["summary"];
    if summary["totalCount"].as_u64() != Some(targets.len() as u64)
        || summary["succeededCount"].as_u64() != Some(succeeded)
        || summary["failedCount"].as_u64() != Some(failed)
    {
        return Err("conflicting_result_counts");
    }
    let unknown = targets.len() as u64 - succeeded - failed;
    Ok(
        json!({"summary":{"totalCount":targets.len(),"succeededCount":succeeded,"failedCount":failed,"unknownCount":unknown},
        "migrations":targets.iter().map(|target|found.remove(target).unwrap()).collect::<Vec<_>>(),
        "completionStatus":if unknown>0{"unknown"}else if failed==0{"succeeded"}else if succeeded>0{"partial_success"}else{"failed"}}),
    )
}

fn keep(entry: &JobEntry, warnings: &mut Vec<&'static str>) {
    if JobLedger::replace(entry).is_err() {
        warnings.push(
            "Local receipt persistence failed AFTER submission; retain stdout and do not resubmit.",
        );
    }
}

fn complete(entry: &mut JobEntry, value: Value) {
    entry.status = value["completionStatus"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    if entry.status != "unknown" {
        entry.completed_at = Some(Utc::now().to_rfc3339());
    }
    entry.result = Some(value);
}

fn render(
    cli: &Cli,
    entry: &JobEntry,
    correlation: &str,
    reason: Option<&str>,
    error: Option<&Value>,
    warnings: &[&str],
    source: &str,
) {
    output::render_object(
        cli,
        &json!({"receiptId":entry.id,"workspaceId":entry.workspace,"dataflowIds":entry.target_ids,
        "submission":{"state":entry.submission_state,"httpStatus":entry.http_status,"operationId":entry.operation_id,"correlationStatus":correlation,"responseBodyStatus":entry.response_body_status,"error":error},
        "completionStatus":if entry.result.is_some(){entry.status.as_str()}else if entry.status=="accepted"{"not_checked"}else{"unknown"},
        "result":entry.result,"observationReason":reason,"source":source,"destructive":true,"retryAllowed":false,"cancellationSupported":false,"warnings":warnings,
        "notChecked":["Gen2 first refresh, loaded/fresh data, consumers and visuals","source credentials, grants, storage/security modes and naming parity"],
        "limitations":"Upgrade results are not data or consumer readiness. No refresh, takeover, grants, source/consumer rewrite or automatic resubmission occurred."}),
        "completionStatus",
    );
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn submit(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    ids: &[String],
    approved: bool,
    wait: bool,
    timeout: u64,
    page_limit: u8,
    row_limit: u16,
) -> Result<()> {
    let workspace = uuid_target(workspace, "--workspace")?;
    let body = super::build_upgrade_gen1_body(ids)?;
    let targets: Vec<_> = ids
        .iter()
        .map(|id| uuid_target(id, "--id"))
        .collect::<Result<_>>()?;
    if output::dry_run_guard(
        cli,
        "dataflow upgrade-gen1",
        &json!({"workspace":workspace.to_string(),"dataflows":body["dataflows"],"count":targets.len(),"requiresApproval":true,"approvalProvided":approved,"readinessNotChecked":true,"singleSubmission":true,"timeoutSeconds":timeout}),
    ) {
        return Ok(());
    }
    if cli.readonly {
        return Err(FabioError::new(
            ErrorCode::ReadonlyMode,
            "An irreversible upgrade is blocked by --readonly; preview or observe only.",
        )
        .into());
    }
    if !approved {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,"Gen1 replacement is irreversible and cached data is not preserved.","Only after explicit approval and backup/consumer/security review, use --allow-in-place-upgrade for this exact selected scope. It does not bypass readiness.").into());
    }
    let budget = ReadBudget::new(timeout);
    let selected: HashSet<_> = targets.iter().copied().collect();
    let scan = super::gen1::assess(
        client, &budget, workspace, &selected, 30, page_limit, row_limit,
    )
    .await;
    let ready = scan.complete
        && targets.iter().all(|target| {
            scan.selected.get(target).is_some_and(|row| {
                super::gen1::parse_assessment(row, workspace).status == Status::Ready
            })
        });
    if !ready {
        output::render_object(
            cli,
            &json!({"submission":{"state":"not_submitted"},"workspaceId":workspace.to_string(),"dataflowIds":targets.iter().map(Uuid::to_string).collect::<Vec<_>>(),"readinessScan":scan,"destructive":true,"applied":false}),
            "submission",
        );
        return Err(failure(
            "No POST submitted: complete all-ready Gen1 assessment evidence is required.",
        )
        .into());
    }
    let mut entry = JobEntry::new(
        &Uuid::new_v4().to_string(),
        "gen1-upgrade",
        &workspace.to_string(),
        &targets[0].to_string(),
    );
    entry.target_ids = targets.iter().map(Uuid::to_string).collect();
    entry.fabric_endpoint = Some(client.fabric_endpoint());
    entry.status = "preparing".to_string();
    // A failed pre-submission write fails closed, BEFORE the irreversible POST.
    JobLedger::append(&entry).map_err(|_| {
        FabioError::with_typed_hint(
            ErrorCode::InvalidInput,
            "No upgrade submitted: the local receipt ledger could not be persisted.",
            "Fix local receipt storage before any irreversible submission. No POST was attempted.",
            HintType::SemanticCorrection,
        )
    })?;
    let mut warnings = Vec::new();
    let mut correlation = "unavailable";
    let mut reason = None;
    let mut error = None;
    match client
        .post_fabric_once(
            &format!("/workspaces/{workspace}/dataflows/gen1Upgrade"),
            &body,
            budget.remaining(),
        )
        .await
    {
        Ok(response) => {
            entry.http_status = Some(response.status.as_u16());
            entry.response_body_status = Some(
                match &response.body {
                    Some(Value::Null) => "empty",
                    Some(_) => "json",
                    None if response.status.is_success() => "unreadable",
                    None => "unavailable",
                }
                .to_string(),
            );
            entry.submission_state = Some(
                if response.status == StatusCode::ACCEPTED {
                    "accepted"
                } else if response.status.is_success() {
                    "acknowledged"
                } else if response.status.is_client_error() {
                    "rejected"
                } else {
                    "unknown"
                }
                .to_string(),
            );
            entry.status = if response.status.is_success() {
                "accepted"
            } else {
                "unknown"
            }
            .to_string();
            let (op, status) = operation(&response.headers, &client.fabric_endpoint());
            entry.correlation_evidence = Some(correlation_evidence(
                &response.headers,
                &client.fabric_endpoint(),
                status,
            ));
            correlation = status;
            entry.operation_id = op.map(|id| id.to_string());
            entry.poll_interval_secs = response
                .headers
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(|value| value.min(60));
            if let Some(err) = response.error.as_ref() {
                error = Some(error_evidence(err));
            }
            if response.status == StatusCode::OK {
                let body = if matches!(
                    status,
                    "verified" | "verified_header_location_ignored" | "identity_unavailable"
                ) {
                    response.body.as_ref().ok_or("success_body_unreadable")
                } else {
                    Err("invalid_operation_correlation")
                };
                match body.and_then(|body| result(body, workspace, &targets, op, false)) {
                    Ok(value) => complete(&mut entry, value),
                    Err(why) => {
                        entry.status = "unknown".to_string();
                        reason = Some(why);
                    }
                }
            } else if !response.status.is_success() {
                reason = Some("submission_rejected_or_unknown");
            }
            if response.body.is_none() && response.status.is_success() {
                warnings.push("Success acknowledgement survived an unreadable response body; do not resubmit.");
            }
        }
        Err(err) => {
            entry.response_body_status = Some("unavailable".to_string());
            entry.submission_state = Some(
                if err.downcast_ref::<FabioError>().is_some_and(|error| {
                    matches!(
                        error.code,
                        ErrorCode::ReadonlyMode | ErrorCode::AuthRequired
                    )
                }) {
                    "not_submitted"
                } else {
                    "unknown"
                }
                .to_string(),
            );
            entry.status = "unknown".to_string();
            reason = Some("submission_outcome_unknown");
            error = Some(error_evidence(&err));
        }
    }
    keep(&entry, &mut warnings);
    if wait && entry.status == "accepted" && entry.operation_id.is_some() {
        return observe(cli, client, entry, &budget, true).await;
    }
    render(
        cli,
        &entry,
        correlation,
        reason,
        error.as_ref(),
        &warnings,
        "submission",
    );
    if entry.status == "succeeded"
        || (!wait && entry.submission_state.as_deref() == Some("accepted"))
    {
        Ok(())
    } else {
        Err(failure("Upgrade is not fully verified successful; retained receipt must be reviewed without resubmission.").into())
    }
}

pub(super) async fn follow(
    cli: &Cli,
    client: &FabricClient,
    receipt: &str,
    wait: bool,
    timeout: u64,
) -> Result<()> {
    let receipt = uuid_target(receipt, "--receipt-id")?.to_string();
    let mut entry = JobLedger::find(&receipt)?.ok_or_else(||FabioError::with_hint(ErrorCode::NotFound,"Local Gen1 receipt was not found.","Inspect jobs list/get. A transport trace or invented operation ID is not a local receipt; never resubmit to obtain an ID."))?;
    scope(&entry)?;
    if entry.fabric_endpoint.as_deref() != Some(client.fabric_endpoint().as_str()) {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            "Receipt belongs to a different configured Fabric endpoint; no request attempted.",
        )
        .into());
    }
    // Recover a retained original HEADER rejected solely for Location origin.
    // This is never a transport trace, guessed ID or latest-history heuristic.
    if entry.operation_id.is_none()
        && entry.submission_state.as_deref() == Some("accepted")
        && entry.http_status == Some(202)
        && let Some(evidence) = entry.correlation_evidence.as_ref()
        && evidence["reason"] == "invalid_location"
        && evidence["operationHeaderPresent"] == true
        && evidence["operationHeaderLength"] == 36
        && evidence["locationSameOrigin"] == false
        && let Some(candidate) = evidence["unverifiedOperationCandidate"].as_str()
        && candidate.len() == 36
    {
        entry.operation_id = Some(uuid_target(candidate, "retained operation header")?.to_string());
    }
    if output::dry_run_guard(
        cli,
        "dataflow follow-upgrade",
        &json!({"receiptId":receipt,"workspace":entry.workspace,"dataflowIds":entry.target_ids,"mutates":false,"timeoutSeconds":timeout}),
    ) {
        return Ok(());
    }
    observe(cli, client, entry, &ReadBudget::new(timeout), wait).await
}

#[allow(clippy::too_many_lines)]
async fn observe(
    cli: &Cli,
    client: &FabricClient,
    mut entry: JobEntry,
    budget: &ReadBudget,
    wait: bool,
) -> Result<()> {
    let (workspace, targets, op) = scope(&entry)?;
    let mut warnings = Vec::new();
    let mut reason = None;
    // A ledger status string is not result evidence. Revalidate every cached
    // projection before exposing it or allowing an offline success response.
    if let Some(cached) = entry.result.take() {
        if let Ok(value) = result(&cached, workspace, &targets, op, true) {
            complete(&mut entry, value);
        } else {
            entry.status = "unknown".to_string();
            entry.completed_at = None;
            warnings.push("Cached result failed scope/count validation; it was not treated as migration success.");
        }
    } else if matches!(
        entry.status.as_str(),
        "succeeded" | "failed" | "partial_success"
    ) {
        entry.status = "unknown".to_string();
        entry.completed_at = None;
    }
    if op.is_none() {
        if let Some(cached) = entry.result.as_ref()
            && let Ok(value) = result(cached, workspace, &targets, None, true)
        {
            complete(&mut entry, value);
        }
        render(
            cli,
            &entry,
            "unavailable",
            Some("no_verified_operation_identity"),
            None,
            &warnings,
            "local_receipt",
        );
        return if entry.status == "succeeded" {
            Ok(())
        } else {
            Err(failure(
                "No verified operation identity; no network request or resubmission was attempted.",
            )
            .into())
        };
    }
    let op = op.unwrap();
    let delay = Duration::from_secs(entry.poll_interval_secs.unwrap_or(2));
    let mut observed = false;
    for attempt in 0..120 {
        if wait && (attempt > 0 || entry.poll_interval_secs.is_some()) {
            tokio::time::sleep(delay.min(budget.remaining())).await;
        }
        let Ok(state) = budget.get(client, &format!("/operations/{op}")).await else {
            reason = Some("operation_read_unavailable");
            break;
        };
        if error_body(&state)
            || state
                .get("id")
                .is_some_and(|id_value| id(id_value) != Some(op))
            || state
                .get("workspaceId")
                .is_some_and(|value| id(value) != Some(workspace))
        {
            reason = Some("operation_scope_mismatch");
            break;
        }
        match state["status"].as_str() {
            Some("Succeeded") => {
                match budget
                    .get(client, &format!("/operations/{op}/result"))
                    .await
                {
                    Ok(value) => match result(&value, workspace, &targets, Some(op), false) {
                        Ok(value) => {
                            complete(&mut entry, value);
                            observed = true;
                        }
                        Err(why) => reason = Some(why),
                    },
                    Err(_) => reason = Some("operation_result_unavailable"),
                }
                break;
            }
            Some("Running" | "NotStarted") => {
                observed = true;
                reason = Some("operation_running");
                if !wait {
                    break;
                }
            }
            Some("Failed" | "Cancelled") => {
                reason = Some("terminal_operation_without_verified_per_item_results");
                break;
            }
            _ => {
                reason = Some("unrecognized_operation_state");
                break;
            }
        }
        if budget.expired() {
            reason = Some("observation_deadline");
            break;
        }
    }
    if !observed || entry.result.is_none() {
        warnings.push("Per-item completion remains unverified; prior verified results are retained. Do not replay this irreversible batch.");
    }
    keep(&entry, &mut warnings);
    render(
        cli,
        &entry,
        "verified",
        reason,
        None,
        &warnings,
        "operation_read",
    );
    if entry.status == "succeeded" && observed {
        Ok(())
    } else {
        Err(
            failure("Selected per-item upgrade completion is not fully verified successful.")
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const A: &str = "bbbbbbbb-1111-2222-3333-444444444444";
    const OP: &str = "cccccccc-1111-2222-3333-444444444444";

    #[test]
    fn operation_identity_is_exact_and_trace_headers_are_never_promoted() {
        let endpoint = "https://api.fabric.microsoft.com/v1";
        let mut headers = HeaderMap::new();
        headers.insert("x-ms-request-id", OP.parse().unwrap());
        assert_eq!(
            operation(&headers, endpoint),
            (None, "identity_unavailable")
        );
        headers.insert("x-ms-operation-id", OP.parse().unwrap());
        assert_eq!(
            operation(&headers, endpoint),
            (Some(OP.parse().unwrap()), "verified")
        );
        headers.insert(
            "location",
            format!("{endpoint}/operations/{OP}").parse().unwrap(),
        );
        assert_eq!(operation(&headers, endpoint).1, "verified");
        headers.insert(
            "location",
            format!("https://example.org/v1/operations/{OP}")
                .parse()
                .unwrap(),
        );
        assert_eq!(
            operation(&headers, endpoint),
            (
                Some(OP.parse().unwrap()),
                "verified_header_location_ignored"
            )
        );
        let mut location_only = headers.clone();
        location_only.remove("x-ms-operation-id");
        assert!(operation(&location_only, endpoint).0.is_none());
        for location in [
            format!("{endpoint}/workspaces/{WS}/operations/{OP}"),
            format!("{endpoint}/operations/{OP}?workspace={WS}"),
            format!("{endpoint}/operations/%2e%2e/{OP}"),
        ] {
            headers.insert("location", location.parse().unwrap());
            assert!(operation(&headers, endpoint).0.is_none());
        }
        headers.remove("location");
        headers.append("x-ms-operation-id", OP.parse().unwrap());
        assert_eq!(operation(&headers, endpoint).1, "invalid_operation_header");
    }

    #[test]
    fn result_validation_checks_scope_counts_and_redacts_free_text() {
        let body = json!({"summary":{"totalCount":1,"succeededCount":0,"failedCount":1},"migrations":[{
            "dataflowId":A,"workspaceId":WS,"dataflowName":"private","targetDataflowName":"private","workspaceName":"private",
            "assessmentStatus":"NeedsAttention","migrationStatus":"Failed","migrationError":{"errorCode":"UpgradeFailed","message":"private secret endpoint","isRetriable":true,"requestId":OP}
        }]});
        let target = A.parse().unwrap();
        let parsed = result(
            &body,
            WS.parse().unwrap(),
            &[target],
            Some(OP.parse().unwrap()),
            false,
        )
        .unwrap();
        assert_eq!(parsed["completionStatus"], "failed");
        assert_eq!(
            parsed["migrations"][0]["migrationError"]["retryAllowed"],
            false
        );
        assert!(!parsed.to_string().contains("private"));
        assert!(
            result(
                &parsed,
                WS.parse().unwrap(),
                &[target],
                Some(OP.parse().unwrap()),
                true
            )
            .is_ok()
        );
        let mut conflicting = body;
        conflicting["summary"]["failedCount"] = json!(0);
        assert_eq!(
            result(&conflicting, WS.parse().unwrap(), &[target], None, false).unwrap_err(),
            "conflicting_result_counts"
        );
    }

    #[test]
    fn cached_receipt_scope_rejects_nil_duplicate_and_wrong_kind_targets() {
        let mut entry = JobEntry::new(OP, "gen1-upgrade", WS, A);
        entry.target_ids = vec![A.to_string()];
        assert!(scope(&entry).is_ok());
        entry.target_ids.push(A.to_string());
        assert!(scope(&entry).is_err());
        entry.target_ids = vec![Uuid::nil().to_string()];
        assert!(scope(&entry).is_err());
        entry.target_ids = vec![A.to_string()];
        entry.kind = "notebook".to_string();
        assert!(scope(&entry).is_err());
    }
}
