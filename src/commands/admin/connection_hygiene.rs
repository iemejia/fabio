//! Bounded admin evidence and review-only plans. Recency is not dependency
//! coverage, an owner assignment is not account/credential health, and similar
//! target metadata is not interchangeable authentication or permission scope.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::connection::hygiene::{parse_date, parse_ts};
use crate::commands::diagnostics::{
    Check, PageCursor, ReadBudget, Status, error_evidence, has_error, id, summary, uuid_target,
};
use crate::commands::remediation::{self, RecoveryHint};
use crate::errors::{ErrorCode, FabioError, HintType, enrich_admin};
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
    rows: Vec<Value>,
}

async fn scan(
    client: &FabricClient,
    budget: &ReadBudget,
    path: &str,
    pages: u8,
    limit: usize,
    connection: Option<Uuid>,
) -> Scan {
    let mut result = Scan::default();
    let mut cursor = PageCursor::default();
    let mut token: Option<String> = None;
    for _ in 0..pages {
        let path = token.as_ref().map_or_else(
            || path.to_string(),
            |token| format!("{path}?continuationToken={}", urlencoding::encode(token)),
        );
        let body = match budget.get(client, &path).await {
            Ok(body) => body,
            Err(error) => {
                result.reason = Some("read_failed");
                result.error = Some(error_evidence(&enrich_admin(
                    error,
                    "admin connection-hygiene",
                )));
                return result;
            }
        };
        result.pages += 1;
        if has_error(&body)
            || connection.is_some_and(|expected| {
                body.get("connectionId")
                    .is_some_and(|value| id(value) != Some(expected))
            })
        {
            result.reason = Some("error_bearing_or_wrong_scope_response");
            return result;
        }
        let Some(rows) = body["value"].as_array() else {
            result.reason = Some("malformed_page");
            return result;
        };
        let remaining = limit.saturating_sub(result.rows.len());
        result.rows.extend(rows.iter().take(remaining).cloned());
        result.rows_inspected = result.rows.len();
        if rows.len() > remaining {
            result.reason = Some("row_limit");
            return result;
        }
        token = match cursor.next(&body) {
            Ok(token) => token,
            Err(reason) => {
                result.reason = Some(reason);
                return result;
            }
        };
        if token.is_none() {
            result.complete = true;
            return result;
        }
        if result.rows.len() >= limit {
            result.reason = Some("row_limit");
            return result;
        }
    }
    result.reason = Some("page_limit");
    result
}

fn show(connection: Uuid) -> String {
    format!("fabio --wrap-untrusted --readonly admin show-connection --id {connection}")
}
fn roles_show(connection: Uuid) -> String {
    format!(
        "fabio --wrap-untrusted --readonly admin list-connection-role-assignments --id {connection}"
    )
}

fn known_kind(body: &Value) -> Option<&'static str> {
    match body["connectivityType"].as_str() {
        Some("ShareableCloud") => Some("ShareableCloud"),
        Some("PersonalCloud") => Some("PersonalCloud"),
        Some("OnPremises" | "OnPremisesGateway") => Some("OnPremisesGateway"),
        Some("OnPremisesGatewayPersonal") => Some("OnPremisesGatewayPersonal"),
        Some("VirtualNetworkGateway") => Some("VirtualNetworkGateway"),
        Some("StreamingVirtualNetworkGateway") => Some("StreamingVirtualNetworkGateway"),
        _ => None,
    }
}
fn shareable(kind: Option<&str>) -> bool {
    matches!(
        kind,
        Some(
            "ShareableCloud"
                | "OnPremisesGateway"
                | "VirtualNetworkGateway"
                | "StreamingVirtualNetworkGateway"
        )
    )
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct Target {
    kind: &'static str,
    gateway: Option<Uuid>,
    connection_type: String,
    path: String,
}
fn target(body: &Value) -> Option<Target> {
    let kind = known_kind(body)?;
    let gateway = match body.get("gatewayId") {
        None | Some(Value::Null) if matches!(kind, "ShareableCloud" | "PersonalCloud") => None,
        Some(value) => Some(id(value)?),
        _ => return None,
    };
    let field = |name: &str| {
        body["connectionDetails"][name]
            .as_str()
            .filter(|value| {
                !value.is_empty() && value.len() <= 2048 && !value.chars().any(char::is_control)
            })
            .map(str::to_string)
    };
    Some(Target {
        kind,
        gateway,
        connection_type: field("type")?,
        path: field("path")?,
    })
}

fn recency(
    body: &Value,
    connection: Uuid,
    cutoff: DateTime<Utc>,
    threshold: DateTime<Utc>,
    now: DateTime<Utc>,
) -> (Check, bool) {
    let values = &body["connectionRecency"];
    let created = parse_ts(values.get("createdDateTime"));
    let bound = parse_ts(values.get("lastBoundDateTime"));
    let used = parse_ts(values.get("lastCredentialUsedDateTime"));
    let explicit_null = |name: &str| values.get(name).is_some_and(Value::is_null);
    let malformed = [
        "createdDateTime",
        "lastBoundDateTime",
        "lastCredentialUsedDateTime",
    ]
    .iter()
    .any(|name| {
        values
            .get(*name)
            .is_some_and(|value| !value.is_null() && parse_ts(Some(value)).is_none())
    });
    let (status, reason, candidate) = match created {
        None => (Status::Unknown, "creation_recency_unavailable", false),
        Some(created) if created < cutoff => {
            (Status::Unknown, "before_reliable_recency_cutoff", false)
        }
        Some(created)
            if malformed
                || created > now
                || bound.is_some_and(|time| time < created || time > now)
                || used.is_some_and(|time| time < created || time > now) =>
        {
            (Status::Unknown, "malformed_or_conflicting_recency", false)
        }
        Some(_) if bound.is_none() && used.is_some() => {
            (Status::Unknown, "conflicting_recency_events", false)
        }
        Some(_)
            if explicit_null("lastBoundDateTime")
                && explicit_null("lastCredentialUsedDateTime") =>
        {
            (
                Status::NeedsAction,
                "no_recorded_binding_or_credential_use",
                true,
            )
        }
        Some(_) if bound.is_some() && explicit_null("lastCredentialUsedDateTime") => {
            (Status::NeedsAction, "no_recorded_credential_use", true)
        }
        Some(_) if bound.is_none() || used.is_none() => {
            (Status::Unknown, "event_recency_unavailable", false)
        }
        Some(_) if used.is_some_and(|time| time < threshold) => (
            Status::NeedsAction,
            "credential_use_older_than_threshold",
            true,
        ),
        Some(_) => (Status::Ready, "recent_credential_use_reported", false),
    };
    let mut check = Check::new(
        "connection_recency",
        status,
        "Recency is a lagged historical review signal, not current binding coverage, source credential health or permission to retire a connection. Missing/malformed events remain unknown.",
        json!({"reason":reason,"creationTimeReported":created.is_some(),"bindingTimeReported":bound.is_some(),"credentialUseTimeReported":used.is_some(),"daysSinceCredentialUse":used.filter(|time|*time<=now).map(|time|(now-time).num_days()),"historicalReviewCandidate":candidate}),
        show(connection),
    );
    if status != Status::Ready {
        check.next_steps.push(RecoveryHint::review("Review seasonality and all current dependencies, including external consumers. No credential test, dependency inventory, deletion or ownership change was performed."));
    }
    (check, candidate)
}

struct Owners {
    check: Check,
    complete: bool,
    pending: bool,
    continuity: bool,
}
fn owners(rows: &Scan, body: &Value, connection: Uuid) -> Owners {
    let mut assignments: HashMap<Uuid, (Uuid, &'static str, &'static str)> = HashMap::new();
    let mut principals: HashMap<Uuid, (&'static str, &'static str)> = HashMap::new();
    let mut owners = HashSet::new();
    let mut pending = HashSet::new();
    let mut valid = true;
    for row in &rows.rows {
        if has_error(row)
            || row
                .get("connectionId")
                .is_some_and(|value| id(value) != Some(connection))
        {
            valid = false;
            continue;
        }
        let role = match row["role"].as_str() {
            Some("Owner") => "Owner",
            Some("PendingOwner") => "PendingOwner",
            Some("User") => "User",
            Some("UserWithReshare") => "UserWithReshare",
            _ => {
                valid = false;
                continue;
            }
        };
        let kind = match row["principal"]["type"].as_str() {
            Some("User") => "User",
            Some("Group") => "Group",
            Some("ServicePrincipal") => "ServicePrincipal",
            _ => {
                valid = false;
                continue;
            }
        };
        let (Some(assignment), Some(principal)) = (id(&row["id"]), id(&row["principal"]["id"]))
        else {
            valid = false;
            continue;
        };
        let value = (principal, kind, role);
        if assignments
            .get(&assignment)
            .is_some_and(|previous| *previous != value)
            || principals
                .get(&principal)
                .is_some_and(|previous| *previous != (kind, role))
        {
            valid = false;
            continue;
        }
        assignments.insert(assignment, value);
        principals.insert(principal, (kind, role));
        if role == "Owner" {
            owners.insert((principal, kind));
        } else if role == "PendingOwner" {
            pending.insert((principal, kind));
        }
    }
    if owners.iter().any(|owner| pending.contains(owner)) {
        valid = false;
    }
    let complete = rows.complete && valid;
    let pending_state = !pending.is_empty() || body["myRole"] == "PendingOwner";
    let caller_conflict = body["myRole"] == "Owner" && complete && owners.is_empty();
    let single_user =
        complete && owners.len() == 1 && owners.iter().all(|(_, kind)| *kind == "User");
    let continuity =
        complete && (owners.is_empty() || single_user) && !pending_state && !caller_conflict;
    let (status, reason) = if pending_state {
        (Status::NeedsAction, "pending_ownership_observed")
    } else if !complete || caller_conflict {
        (
            Status::Unknown,
            "ownership_evidence_incomplete_or_conflicting",
        )
    } else if owners.is_empty() {
        (Status::NeedsAction, "no_owner_assignment_reported")
    } else if single_user {
        (
            Status::NeedsAction,
            "single_individual_owner_continuity_risk",
        )
    } else {
        (Status::Ready, "owner_assignments_reported")
    };
    let mut check = Check::new(
        "connection_ownership",
        status,
        "Only admin role-assignment metadata is observed. PendingOwner is not effective ownership; a single individual is a continuity risk, not proof of departure, orphaning or failed credentials.",
        json!({"reason":reason,"scan":rows,"complete":complete && !caller_conflict,"reportedOwnerPrincipalCount":if complete && !caller_conflict{Some(owners.len())}else{None},"pendingOwnersObserved":pending.len(),"callerPendingOwner":body["myRole"]=="PendingOwner","singleIndividualOwner":if complete && !caller_conflict{Some(single_user)}else{None},"principalExistence":"not_checked"}),
        roles_show(connection),
    );
    if status != Status::Ready {
        check.next_steps.push(RecoveryHint::review("Have an authorized administrator and intended owner review continuity/handoff. An assigned Owner/PendingOwner is not a source grant or credential-health result. No principal lookup, permission grant, transfer or credential write was performed."));
    }
    Owners {
        check,
        complete: complete && !caller_conflict,
        pending: pending_state,
        continuity,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Action {
    connection_id: String,
    kind: &'static str,
    requires_approval: bool,
    destructive: bool,
    ready_to_apply: bool,
    blockers: Vec<&'static str>,
    preview: Option<RecoveryHint>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    connection_id: String,
    credential_health: &'static str,
    checks: Vec<Check>,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn execute(
    cli: &Cli,
    client: &FabricClient,
    ids: &[String],
    inventory: bool,
    connection_limit: u8,
    page_limit: u8,
    role_limit: u16,
    timeout: u64,
    unused_days: u32,
    created_after: &str,
    strict: bool,
) -> Result<()> {
    if inventory != ids.is_empty()
        || cli.continuation_token.is_some()
        || ids.len() > usize::from(connection_limit)
    {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,"Select explicit --id values OR an explicit --inventory scan, within --connection-limit. Aggregate hygiene cannot resume a list cursor.","Use admin list-connections to resume inventory pages, or select at most 20 explicit connection IDs for bounded review.").into());
    }
    let selected: Vec<_> = ids
        .iter()
        .map(|value| uuid_target(value, "--id"))
        .collect::<Result<_>>()?;
    if selected.iter().copied().collect::<HashSet<_>>().len() != selected.len() {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            "Duplicate connection IDs in hygiene scope",
        )
        .into());
    }
    let now = Utc::now();
    let cutoff = parse_date(created_after)?;
    if cutoff < parse_date("2026-05-01")? || cutoff > now {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,"Hygiene recency cutoff must be between the reliable 2026-05-01 boundary and today.","Choose a reliable --created-after date; old/null recency is not proof of unused connections.").into());
    }
    let scope = json!({"mutates":false,"source":if inventory{"bounded_admin_inventory"}else{"explicit_admin_ids"},"connectionLimit":connection_limit,"pageLimitPerList":page_limit,"roleLimitPerConnection":role_limit,"timeoutSeconds":timeout,"unusedDays":unused_days,"createdAfter":created_after,
        "maxLogicalReads":if inventory{usize::from(page_limit)*(1+usize::from(connection_limit))}else{selected.len()*(1+usize::from(page_limit))},"dependencyCoverage":"not_checked","ownershipChanges":"not_requested"});
    if output::dry_run_guard(
        cli,
        "admin connection-hygiene",
        &json!({"scope":scope,"connectionIds":selected.iter().map(Uuid::to_string).collect::<Vec<_>>()}),
    ) {
        return Ok(());
    }
    let budget = ReadBudget::new(timeout);
    let mut selection = Scan {
        complete: !inventory,
        ..Default::default()
    };
    let mut bodies: Vec<(Uuid, Result<Value>)> = Vec::new();
    if inventory {
        selection = scan(
            client,
            &budget,
            "/admin/connections",
            page_limit,
            usize::from(connection_limit),
            None,
        )
        .await;
        let mut seen = HashSet::new();
        for body in &selection.rows {
            if let Some(connection) = id(&body["id"])
                && seen.insert(connection)
            {
                bodies.push((connection, Ok(body.clone())));
            } else {
                selection.complete = false;
                selection.reason = Some("malformed_or_duplicate_connection_id");
                if let Some(duplicate) = id(&body["id"])
                    && let Some((_, value)) = bodies
                        .iter_mut()
                        .find(|(connection, _)| *connection == duplicate)
                {
                    *value = Err(FabioError::new(
                        ErrorCode::ApiError,
                        "Duplicate admin inventory identity leaves metadata uncertain",
                    )
                    .into());
                }
            }
        }
    } else {
        for connection in selected {
            let body = budget
                .get(client, &format!("/admin/connections/{connection}"))
                .await
                .map_err(|error| enrich_admin(error, "admin connection-hygiene"));
            bodies.push((connection, body));
        }
    }
    let mut checks = vec![Check::new(
        "admin_selection",
        if selection.complete {
            Status::Ready
        } else {
            Status::Unknown
        },
        "Only the explicitly selected IDs or bounded admin inventory were inspected. Partial or malformed inventory is not tenant-wide completeness or proof of absence.",
        serde_json::to_value(&selection)?,
        "fabio --wrap-untrusted --readonly admin list-connections".into(),
    )];
    let mut observations = Vec::new();
    let mut actions = Vec::new();
    let mut targets: BTreeMap<Target, Vec<String>> = BTreeMap::new();
    let mut uncomparable = 0;
    for (connection, body) in bodies {
        let mut found = Vec::new();
        match body {
            Ok(body) if id(&body["id"]) == Some(connection) && !has_error(&body) => {
                found.push(Check::new(
                    "admin_connection_access", Status::Ready,
                    "The admin endpoint authorized this metadata read, not a source credential test or permission to apply cleanup.",
                    json!({"source":"admin.connection","connectionId":connection.to_string()}),
                    show(connection),
                ));
                let key = target(&body);
                let comparable = key.is_some();
                if let Some(key) = key {
                    targets.entry(key).or_default().push(connection.to_string());
                } else {
                    uncomparable += 1;
                }
                let evaluated = Utc::now();
                let (recency_check, candidate) = recency(
                    &body,
                    connection,
                    cutoff,
                    evaluated - TimeDelta::days(i64::from(unused_days)),
                    evaluated,
                );
                found.push(recency_check);
                let roles = scan(
                    client,
                    &budget,
                    &format!("/admin/connections/{connection}/roleAssignments"),
                    page_limit,
                    usize::from(role_limit),
                    Some(connection),
                )
                .await;
                let ownership = owners(&roles, &body, connection);
                if ownership.pending {
                    actions.push(Action {
                        connection_id: connection.to_string(),
                        kind: "review_pending_ownership",
                        requires_approval: true,
                        destructive: false,
                        ready_to_apply: false,
                        blockers: vec!["handoff_and_credentials_not_verified"],
                        preview: None,
                    });
                } else if ownership.complete {
                    if candidate && comparable {
                        actions.push(Action {
                            connection_id: connection.to_string(),
                            kind: "review_retirement",
                            requires_approval: true,
                            destructive: true,
                            ready_to_apply: false,
                            blockers: vec![
                                "all_current_dependencies_not_verified",
                                "owner_and_human_approval_not_recorded",
                            ],
                            preview: Some(remediation::connection_retirement(connection)),
                        });
                    }
                    if ownership.continuity {
                        actions.push(Action {
                            connection_id: connection.to_string(),
                            kind: "review_owner_continuity",
                            requires_approval: true,
                            destructive: false,
                            ready_to_apply: false,
                            blockers: vec![
                                "durable_owner_identity_not_selected",
                                "handoff_and_credentials_not_verified",
                            ],
                            preview: shareable(known_kind(&body))
                                .then(|| remediation::connection_owner_continuity(connection)),
                        });
                    }
                }
                found.push(ownership.check);
            }
            Ok(_) => {
                uncomparable += 1;
                found.push(Check::new(
                    "admin_connection_access", Status::Unknown,
                    "Admin metadata did not match the requested connection scope or carried an error; no role reads or cleanup preview were inferred.",
                    json!({"reason":"connection_identity_mismatch_or_error"}), show(connection),
                ));
            }
            Err(error) => {
                uncomparable += 1;
                found.push(Check::error(
                    "admin_connection_access",
                    &error,
                    "admin.connection",
                    show(connection),
                ));
            }
        }
        observations.push(Observation {
            connection_id: connection.to_string(),
            credential_health: "not_checked",
            checks: found,
        });
    }
    let groups: Vec<_> = targets.into_values().filter(|ids| ids.len() > 1)
        .map(|ids| json!({"connectionIds":ids,"comparison":"target_metadata_only","credentialEquivalence":"not_checked","winner":"not_selected"})).collect();
    checks.push(Check {
        key: "target_similarity",
        status: if !groups.is_empty() { Status::NeedsAction }
            else if uncomparable > 0 || !selection.complete { Status::Unknown }
            else { Status::Ready },
        message: "Similar target metadata is only a review signal, not equivalent credentials, access or bindings. No winner or redundant connection was selected.".into(),
        evidence: json!({"groups":groups,"uncomparableConnections":uncomparable,"tenantWideCompleteness":false}),
        next_steps: vec![RecoveryHint::review("Review identity, security and all dependencies before consolidation. Recency or metadata similarity never authorizes rebinding or deletion.")],
    });
    let all = || {
        checks
            .iter()
            .chain(observations.iter().flat_map(|item| item.checks.iter()))
    };
    let status = summary(all());
    let partial = all().any(|check| check.status == Status::Unknown)
        || observations.iter().any(|item| {
            item.checks
                .iter()
                .any(|check| check.evidence["complete"] == false)
        });
    let destructive = actions.iter().any(|action| action.destructive);
    output::render_object(
        cli,
        &json!({"observedAt":Utc::now().to_rfc3339(),"scope":scope,"status":status,"partial":partial,"mutates":false,"destructive":destructive,"checks":checks,"connections":observations,
        "reviewPlan":{"readyToApply":false,"requiresApproval":true,"destructive":destructive,"actions":actions,"applied":false},
        "notChecked":["all current item/external dependencies or proof of unused connections","principal existence, employment or membership","stored credential health, source authorization and other identities","human approval, credential handoff or mutation permissions","connections outside selected/bounded inventory"],
        "limitations":"GET-only historical metadata and review previews. No credential test, grants, ownership transfer, rebinding, deletion, directory probe or automatic ordinary-connection fallback was performed."}),
        "status",
    );
    if strict && status != Status::Ready {
        return Err(FabioError::with_typed_hint(ErrorCode::ApiError,"Admin connection hygiene has review findings or unknown evidence.","Inspect checks, partial scope and reviewPlan blockers. Guarded previews do not authorize applying changes; do not delete, grant ownership or rewrite credentials automatically.",HintType::SemanticCorrection).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn times() -> (DateTime<Utc>, DateTime<Utc>, DateTime<Utc>) {
        (
            parse_date("2026-05-01").unwrap(),
            parse_date("2026-08-01").unwrap(),
            parse_date("2026-11-01").unwrap(),
        )
    }
    #[test]
    fn recency_missing_malformed_and_conflicting_fields_are_not_stale_proof() {
        let (cutoff, threshold, now) = times();
        let connection = Uuid::new_v4();
        for rec in [
            json!({}),
            json!({"createdDateTime":"2026-06-01T00:00:00Z"}),
            json!({"createdDateTime":"2026-06-01T00:00:00Z","lastBoundDateTime":"private-invalid","lastCredentialUsedDateTime":null}),
            json!({"createdDateTime":"2026-06-01T00:00:00Z","lastBoundDateTime":null,"lastCredentialUsedDateTime":"2026-10-01T00:00:00Z"}),
        ] {
            let (check, candidate) = recency(
                &json!({"connectionRecency":rec}),
                connection,
                cutoff,
                threshold,
                now,
            );
            assert_eq!(check.status, Status::Unknown);
            assert!(!candidate);
            assert!(!check.evidence.to_string().contains("private"));
        }
        let (check, candidate) = recency(
            &json!({"connectionRecency":{"createdDateTime":"2026-06-01T00:00:00Z","lastBoundDateTime":null,"lastCredentialUsedDateTime":null}}),
            connection,
            cutoff,
            threshold,
            now,
        );
        assert_eq!(check.status, Status::NeedsAction);
        assert!(candidate);
    }
    #[test]
    fn partial_or_pending_ownership_cannot_be_promoted_to_effective_single_ownership() {
        let connection = Uuid::new_v4();
        let assignment = Uuid::new_v4().to_string();
        let principal = Uuid::new_v4().to_string();
        let row =
            json!({"id":assignment,"role":"Owner","principal":{"id":principal,"type":"User"}});
        let scan = Scan {
            rows: vec![row.clone()],
            complete: false,
            ..Default::default()
        };
        let result = owners(&scan, &json!({}), connection);
        assert_eq!(result.check.status, Status::Unknown);
        assert!(!result.continuity);
        let scan = Scan {
            rows: vec![row],
            complete: true,
            ..Default::default()
        };
        let result = owners(&scan, &json!({"myRole":"PendingOwner"}), connection);
        assert!(result.pending);
        assert!(!result.continuity);
    }

    #[test]
    fn recency_age_boundaries_and_future_events_are_conservative() {
        let (cutoff, threshold, now) = times();
        for (used, expected) in [
            ("2026-07-31T23:59:59Z", Status::NeedsAction),
            ("2026-08-01T00:00:00Z", Status::Ready),
            ("2026-11-01T00:00:01Z", Status::Unknown),
            ("2026-05-01T00:00:00Z", Status::Unknown),
        ] {
            let (check, candidate) = recency(
                &json!({"connectionRecency":{
                    "createdDateTime":"2026-06-01T00:00:00Z","lastBoundDateTime":"2026-06-02T00:00:00Z","lastCredentialUsedDateTime":used
                }}),
                Uuid::new_v4(),
                cutoff,
                threshold,
                now,
            );
            assert_eq!(check.status, expected);
            assert_eq!(candidate, expected == Status::NeedsAction);
        }
    }

    #[test]
    fn comparable_targets_require_known_type_and_valid_gateway_scope() {
        let base = json!({"connectivityType":"ShareableCloud","connectionDetails":{"type":"Web","path":"https://example.org"}});
        assert!(target(&base).is_some());
        for patch in [
            json!({"connectivityType":"FutureConnectivity"}),
            json!({"gatewayId":"invalid"}),
            json!({"connectivityType":"OnPremisesGateway"}),
            json!({"connectionDetails":{"type":"Web","path":""}}),
        ] {
            let mut body = base.clone();
            for (key, value) in patch.as_object().unwrap() {
                body[key] = value.clone();
            }
            assert!(target(&body).is_none());
        }
        let mut body = base;
        body["connectivityType"] = json!("OnPremises");
        body["gatewayId"] = json!(Uuid::new_v4().to_string());
        assert!(target(&body).is_some());
    }

    #[test]
    fn inconsistent_roles_for_one_principal_stay_unknown_even_with_distinct_assignment_ids() {
        let principal = Uuid::new_v4().to_string();
        let scan = Scan {
            complete: true,
            rows: vec![
                json!({"id":Uuid::new_v4().to_string(),"principal":{"id":principal,"type":"User"},"role":"Owner"}),
                json!({"id":Uuid::new_v4().to_string(),"principal":{"id":principal,"type":"User"},"role":"UserWithReshare"}),
            ],
            ..Default::default()
        };
        let result = owners(&scan, &json!({}), Uuid::new_v4());
        assert_eq!(result.check.status, Status::Unknown);
        assert!(!result.complete);
        assert!(!result.continuity);
    }
}
