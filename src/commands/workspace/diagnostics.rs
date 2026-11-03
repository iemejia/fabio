//! Evidence-based, bounded GET-only diagnostics. "Ready" applies only to the
//! requested configuration checks, never to end-to-end workload readiness.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::commands::remediation::{self, Kind, RecoveryHint as NextStep};
use crate::errors::{ErrorCode, FabioError, HintType};
use crate::output;

const MAX_ACTORS: usize = 1_000;
const MAX_MATCHES: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Ready,
    Blocked,
    NeedsAction,
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Check {
    #[serde(rename = "checkId")]
    key: &'static str,
    status: Status,
    message: String,
    evidence: Value,
    next_steps: Vec<NextStep>,
}

impl Check {
    fn new(
        name: &'static str,
        status: Status,
        message: impl Into<String>,
        evidence: Value,
        next: String,
    ) -> Self {
        Self {
            key: name,
            status,
            message: message.into(),
            evidence,
            next_steps: vec![NextStep::inspection(
                "Inspect the same scoped resource without changing it.",
                next,
            )],
        }
    }

    fn error(name: &'static str, error: &anyhow::Error, source: &str, next: String) -> Self {
        let error = error_evidence(error);
        let mut check = Self::new(
            name,
            Status::Unknown,
            "The requested evidence could not be read; this is not proof of readiness or resource absence.",
            json!({"source": source, "error": error}),
            next,
        );
        if matches!(
            check.evidence["error"]["code"].as_str(),
            Some("AUTH_REQUIRED" | "FORBIDDEN")
        ) {
            let permission = match name {
                "workspace_access" => {
                    "Use the intended identity with workspace read access. No authentication configuration, role, or policy was changed."
                }
                "capacity_state" => {
                    "Reading capacity metadata requires administrator or contributor permissions on the capacity; workspace access alone may be insufficient. No permissions were changed."
                }
                _ => {
                    "Surge-protection reads require capacity-admin access; workspace access alone is insufficient. Ask an authorized administrator to review; no permissions were changed."
                }
            };
            check.next_steps.push(NextStep::review(permission));
        }
        check
    }
}

fn error_evidence(error: &anyhow::Error) -> Value {
    // Keep machine-readable provenance without copying an unbounded API body,
    // credential-bearing URL, or misleading generic permission-recovery hint.
    error.downcast_ref::<FabioError>().map_or_else(
        || json!({"code": "UNKNOWN"}),
        |error| json!({"code": error.code, "requestId": error.request_id.as_deref().filter(|value| value.len() <= 128), "retriable": error.retriable}),
    )
}

fn summary(checks: &[Check]) -> Status {
    checks
        .iter()
        .map(|check| check.status)
        .max_by_key(|status| match status {
            Status::Ready => 0,
            Status::Unknown => 1,
            Status::NeedsAction => 2,
            Status::Blocked => 3,
        })
        .unwrap_or(Status::Unknown)
}

fn id(value: &Value) -> Option<Uuid> {
    value
        .as_str()
        .and_then(|value| Uuid::parse_str(value).ok())
        .filter(|id| !id.is_nil())
}

fn text(value: &Value, max_bytes: usize) -> Option<&str> {
    value.as_str().filter(|value| value.len() <= max_bytes)
}

fn readonly(group: &str, command: &str, target: &str) -> String {
    format!("fabio --wrap-untrusted --readonly {group} {command} --id {target}")
}

async fn read(client: &FabricClient, path: &str, timeout: Duration) -> Result<Value> {
    tokio::time::timeout(timeout, client.get(path))
        .await
        .map_err(|_| FabioError::new(ErrorCode::Timeout, "Diagnostic read reached its deadline"))?
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActorScan {
    pages_scanned: usize,
    actors_scanned: usize,
    complete: bool,
    matches: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    incomplete_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

impl ActorScan {
    const fn fail(&mut self, reason: &'static str) {
        self.complete = false;
        self.incomplete_reason = Some(reason);
    }

    fn page(&mut self, body: &Value, workspace: Uuid) -> Option<String> {
        self.pages_scanned += 1;
        let Some(rows) = body["value"].as_array() else {
            self.fail("malformed_page");
            return None;
        };
        for row in rows {
            if self.actors_scanned == MAX_ACTORS {
                self.fail("actor_limit");
                return None;
            }
            self.actors_scanned += 1;
            let details = &row["actorDetails"];
            let Some(actor_workspace) = id(&details["workspaceId"]) else {
                self.fail("malformed_actor");
                return None;
            };
            if details["actorType"] != "Workspace" || id(&row["id"]).is_none() {
                self.fail("unsupported_or_malformed_actor");
                return None;
            }
            if actor_workspace == workspace {
                if self.matches.len() == MAX_MATCHES {
                    self.fail("matching_actor_limit");
                    return None;
                }
                // Never include other workspaces' rows or unbounded extra fields.
                self.matches.push(json!({"id": id(&row["id"]).map(|id| id.to_string()), "expirationPolicy": {
                    "policyType": text(&row["expirationPolicy"]["policyType"], 64),
                    "expireAfterDateTime": text(&row["expirationPolicy"]["expireAfterDateTime"], 128)
                }}));
            }
        }
        let token = &body["continuationToken"];
        let uri = &body["continuationUri"];
        if (!token.is_null() && !token.is_string()) || (!uri.is_null() && !uri.is_string()) {
            self.fail("malformed_continuation");
            return None;
        }
        if let Some(token) = token.as_str().filter(|token| !token.is_empty()) {
            if token.len() > 16_384 {
                self.fail("continuation_token_limit");
                return None;
            }
            self.complete = false;
            return Some(token.to_string());
        }
        if uri.as_str().is_some_and(|uri| !uri.is_empty()) {
            // Stay on the validated capacity route; never follow an arbitrary
            // API-returned URL or silently interpret a URI-only page as empty.
            self.fail("uri_only_continuation");
        } else {
            self.complete = true;
        }
        None
    }
}

async fn scan_actors(
    client: &FabricClient,
    path: &str,
    workspace: Uuid,
    page_limit: u8,
    timeout: Duration,
) -> ActorScan {
    let mut result = ActorScan::default();
    let scan = async {
        let mut token: Option<String> = None;
        let mut seen = HashSet::new();
        for _ in 0..page_limit {
            let url = token.as_ref().map_or_else(
                || path.to_string(),
                |token| format!("{path}?continuationToken={}", urlencoding::encode(token)),
            );
            // get_list intentionally tolerates missing arrays and hides a URI-
            // only continuation in single-page mode. Diagnostics must instead
            // distinguish malformed/partial evidence from a complete empty list.
            let body = match client.get(&url).await {
                Ok(body) => body,
                Err(error) => {
                    result.error = Some(error_evidence(&error));
                    result.fail("read_failed");
                    return;
                }
            };
            token = result.page(&body, workspace);
            let Some(next) = &token else {
                return;
            };
            if !seen.insert(next.clone()) {
                result.fail("continuation_cycle");
                return;
            }
        }
        result.fail("page_limit");
    };
    if tokio::time::timeout(timeout, scan).await.is_err() {
        result.fail("timeout");
        result.error = Some(json!({"code": "TIMEOUT"}));
    }
    result
}

fn assignment(workspace: &Value, target: &str) -> Check {
    let capacity = id(&workspace["capacityId"]);
    let progress = workspace["capacityAssignmentProgress"].as_str();
    let malformed = !workspace["capacityId"].is_null()
        && workspace["capacityId"] != "00000000-0000-0000-0000-000000000000"
        && capacity.is_none();
    let (status, message) = if malformed {
        (
            Status::Unknown,
            "The workspace capacity reference is malformed; dependent requests were not attempted.",
        )
    } else {
        match (capacity, progress) {
            (_, Some("InProgress")) => (
                Status::NeedsAction,
                "Capacity assignment is still in progress. Wait and recheck before dependent operations.",
            ),
            (_, Some("Failed")) => (
                Status::NeedsAction,
                "The last capacity assignment failed. Review it with a workspace administrator; the previous capacity may still be attached.",
            ),
            (None, _) => (
                Status::NeedsAction,
                "No valid capacity assignment was observed. Review capacity/license requirements for the intended Fabric workload.",
            ),
            (Some(_), Some("Completed")) => (
                Status::Ready,
                "A capacity ID and a completed assignment were observed.",
            ),
            _ => (
                Status::Unknown,
                "The assignment progress is missing or unrecognized; a capacity ID alone does not establish completion.",
            ),
        }
    };
    let mut check = Check::new(
        "capacity_assignment",
        status,
        message,
        json!({"source": "workspace", "capacityId": capacity.map(|id| id.to_string()), "capacityAssignmentProgress": text(&workspace["capacityAssignmentProgress"], 64)}),
        readonly("workspace", "show", target),
    );
    if status == Status::NeedsAction
        && capacity.is_none()
        && !malformed
        && progress.is_none_or(|progress| progress == "Completed")
        && let Ok(workspace) = Uuid::parse_str(target)
        && !workspace.is_nil()
    {
        check
            .next_steps
            .push(remediation::assign_capacity(workspace));
    }
    check
}

fn capacity_state(body: Result<Value>, capacity: &str) -> Check {
    let next = readonly("capacity", "show", capacity);
    match body {
        Err(error) => Check::error("capacity_state", &error, "capacity", next),
        Ok(body) => {
            let (status, message) = if id(&body["id"]).is_none_or(|id| id.to_string() != capacity) {
                (
                    Status::Unknown,
                    "Capacity metadata did not identify the requested capacity.",
                )
            } else {
                match body["state"].as_str() {
                    Some("Active") => (
                        Status::Ready,
                        "Capacity metadata reports Active. This is not a measurement of current headroom.",
                    ),
                    Some("Inactive") => (
                        Status::Blocked,
                        "Capacity metadata reports Inactive. Ask its administrator to review availability; no resume was attempted.",
                    ),
                    _ => (
                        Status::Unknown,
                        "Capacity state is missing or unrecognized; readiness cannot be inferred.",
                    ),
                }
            };
            let mut check = Check::new(
                "capacity_state",
                status,
                message,
                json!({"source": "capacity", "id": id(&body["id"]).map(|id| id.to_string()), "state": text(&body["state"], 64), "sku": text(&body["sku"], 64)}),
                next,
            );
            if status == Status::Blocked
                && let Some(capacity) = id(&body["id"])
            {
                check.next_steps.push(remediation::inactive_capacity(
                    capacity,
                    text(&body["sku"], 64),
                ));
            }
            check
        }
    }
}

fn surge_configuration(body: Result<Value>, capacity: &str) -> Check {
    let next = readonly("capacity", "get-surge-protection", capacity);
    match body {
        Err(error) => Check::error(
            "surge_configuration",
            &error,
            "capacity surgeProtection",
            next,
        ),
        Ok(body) => {
            let (status, message) = match body["state"].as_str() {
                Some("Disabled") => (
                    Status::Ready,
                    "Capacity-level surge protection is configured Disabled. Explicit workspace blocks are checked separately.",
                ),
                Some("Enabled") => (
                    Status::Unknown,
                    "Surge protection is configured Enabled; these settings do not reveal whether rejection is currently active. Review live capacity telemetry.",
                ),
                _ => (
                    Status::Unknown,
                    "Surge-protection state is missing or unrecognized.",
                ),
            };
            let mut check = Check::new(
                "surge_configuration",
                status,
                message,
                json!({"source": "capacity surgeProtection", "state": text(&body["state"], 64), "rejectionThreshold": body["rejectionThreshold"].as_u64(), "recoveryThreshold": body["recoveryThreshold"].as_u64()}),
                next,
            );
            if body["state"] == "Enabled" {
                check.next_steps.insert(0, NextStep::review("Inspect live capacity telemetry to determine current rejection. Re-reading enabled settings alone cannot establish activity; do not disable protection as a diagnostic workaround."));
            }
            check
        }
    }
}

fn block_active(record: &Value, now: DateTime<Utc>) -> Option<bool> {
    let policy = &record["expirationPolicy"];
    match policy["policyType"].as_str()? {
        "NeverExpire" => Some(true),
        "ExpireAfterDate" => DateTime::parse_from_rfc3339(policy["expireAfterDateTime"].as_str()?)
            .ok()
            .map(|expiration| expiration > now),
        _ => None,
    }
}

fn actor_checks(
    blocked: &ActorScan,
    critical: &ActorScan,
    capacity: &str,
    workspace: Uuid,
    now: DateTime<Utc>,
) -> [Check; 2] {
    let has_critical = !critical.matches.is_empty();
    let enforcement: HashSet<_> = blocked
        .matches
        .iter()
        .map(|record| block_active(record, now))
        .collect();
    let has_active = enforcement.contains(&Some(true));
    let (status, message) = if !blocked.complete || !critical.complete {
        (
            Status::Unknown,
            "Actor evidence is incomplete. Missing permissions, scan limits, or read failures must not be interpreted as no block.",
        )
    } else if has_active && has_critical {
        (
            Status::Unknown,
            "Both an unexpired block and mission-critical classification were observed. Enforcement precedence is not inferred; ask the capacity administrator to review the records.",
        )
    } else if enforcement.contains(&None) {
        (
            Status::Unknown,
            "A matching block has an unsupported expiration policy or an invalid date; enforcement cannot be determined.",
        )
    } else if has_active {
        (
            Status::Blocked,
            "An unexpired workspace block was observed. This is policy evidence, not a job cancellation or an automatic request to unblock it.",
        )
    } else {
        (
            Status::Ready,
            "No unexpired workspace block was observed in the complete actor scan. Expired records may remain listed until background cleanup.",
        )
    };
    let critical_status = if critical.complete {
        Status::Ready
    } else {
        Status::Unknown
    };
    let mut block = Check::new(
        "workspace_block",
        status,
        message,
        json!({"source": "capacity blockedActors", "scan": blocked, "evaluatedAt": now.to_rfc3339()}),
        readonly("capacity", "list-blocked-actors", capacity),
    );
    if status != Status::Ready {
        block.next_steps.push(NextStep::review("Incomplete or conflicting evidence requires review by an authorized capacity administrator. No unblock, exception, or access grant was attempted."));
    }
    if status == Status::Blocked
        && let Ok(capacity) = Uuid::parse_str(capacity)
    {
        let mut suggested = HashSet::new();
        for record in &blocked.matches {
            if block_active(record, now) == Some(true)
                && let Some(actor) = id(&record["id"])
                && suggested.insert(actor)
            {
                block
                    .next_steps
                    .push(remediation::remove_block(workspace, capacity, actor));
            }
        }
    }
    [
        block,
        Check::new(
            "mission_critical",
            critical_status,
            "Classification is evidence only, not reserved compute or immunity from general capacity throttling. No classification was changed.",
            json!({"source": "capacity missionCriticalActors", "classified": if critical.complete { Some(has_critical) } else { None }, "scan": critical}),
            readonly("capacity", "list-mission-critical-actors", capacity),
        ),
    ]
}

async fn capacity_checks(
    client: &FabricClient,
    capacity: &str,
    workspace: Uuid,
    include_surge: bool,
    page_limit: u8,
    timeout: Duration,
) -> Vec<Check> {
    let path = format!("/capacities/{capacity}");
    if !include_surge {
        return vec![capacity_state(read(client, &path, timeout).await, capacity)];
    }
    let surge_path = format!("{path}/surgeProtection");
    let blocked_path = format!("{surge_path}/blockedActors");
    let critical_path = format!("{surge_path}/missionCriticalActors");
    let (state, surge, blocked, critical) = tokio::join!(
        Box::pin(read(client, &path, timeout)),
        Box::pin(read(client, &surge_path, timeout)),
        Box::pin(scan_actors(
            client,
            &blocked_path,
            workspace,
            page_limit,
            timeout
        )),
        Box::pin(scan_actors(
            client,
            &critical_path,
            workspace,
            page_limit,
            timeout
        )),
    );
    let mut checks = vec![
        capacity_state(state, capacity),
        surge_configuration(surge, capacity),
    ];
    checks.extend(actor_checks(
        &blocked,
        &critical,
        capacity,
        workspace,
        Utc::now(),
    ));
    checks
}

fn mark_unattempted(checks: &mut Vec<Check>, include_surge: bool, workspace: &str) {
    let names = [
        "capacity_assignment",
        "capacity_state",
        "surge_configuration",
        "workspace_block",
        "mission_critical",
    ];
    let observed: HashSet<_> = checks.iter().map(|check| check.key).collect();
    for name in names.into_iter().take(if include_surge { 5 } else { 2 }) {
        if !observed.contains(name) {
            checks.push(Check::new(name, Status::Unknown,
                "This requested check was not attempted because its workspace/capacity prerequisite was not established.",
                json!({"reason": "prerequisite_unavailable"}), readonly("workspace", "show", workspace)));
        }
    }
}

fn omissions(include_surge: bool) -> Value {
    let mut omitted = json!([
        "workload execution",
        "live utilization/headroom",
        "SKU/license compatibility",
        "source permissions",
        "connections/credentials",
        "workspace identity authorization",
        "Spark/environment readiness"
    ]);
    if !include_surge {
        omitted
            .as_array_mut()
            .expect("static array")
            .push(Value::from("surge/actor policy (not requested)"));
    }
    omitted
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute(
    cli: &Cli,
    client: &FabricClient,
    target: &str,
    include_surge: bool,
    page_limit: u8,
    timeout_secs: u64,
    strict: bool,
) -> Result<()> {
    validate_uuid(target, "--id")?;
    if cli.continuation_token.is_some() {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,
            "An aggregate diagnostic cannot resume a single list's continuation token.",
            "Use the corresponding capacity list-blocked-actors/list-mission-critical-actors command to inspect a particular list.").into());
    }
    let workspace_uuid = Uuid::parse_str(target)?;
    let workspace_id = workspace_uuid.to_string();
    let scope = json!({
        "area": "workspace_capacity_configuration", "includeSurgeProtection": include_surge,
        "actorPageLimit": page_limit, "actorItemLimit": MAX_ACTORS, "matchingActorLimit": MAX_MATCHES,
        "probeTimeoutSeconds": timeout_secs, "maxLogicalReads": if include_surge { 3 + 2 * u32::from(page_limit) } else { 2 },
        "mutates": false
    });
    if output::dry_run_guard(
        cli,
        "workspace diagnostics",
        &json!({"workspaceId": workspace_id, "scope": scope}),
    ) {
        return Ok(());
    }
    let timeout = Duration::from_secs(timeout_secs);
    let mut checks = Vec::new();
    let mut capacity_id = None;
    match read(client, &format!("/workspaces/{workspace_id}"), timeout).await {
        Err(error) => checks.push(Check::error(
            "workspace_access",
            &error,
            "workspace",
            readonly("workspace", "show", &workspace_id),
        )),
        Ok(workspace) if id(&workspace["id"]).is_none_or(|id| id.to_string() != workspace_id) => {
            checks.push(Check::new(
                "workspace_access",
                Status::Unknown,
                "Workspace metadata did not identify the requested workspace.",
                json!({"source": "workspace"}),
                readonly("workspace", "show", &workspace_id),
            ));
        }
        Ok(workspace) => {
            let assignment = assignment(&workspace, &workspace_id);
            capacity_id = id(&workspace["capacityId"]).map(|id| id.to_string());
            let settled = assignment.status == Status::Ready;
            checks.push(assignment);
            if let Some(capacity) = capacity_id.as_deref().filter(|_| settled) {
                checks.extend(
                    capacity_checks(
                        client,
                        capacity,
                        workspace_uuid,
                        include_surge,
                        page_limit,
                        timeout,
                    )
                    .await,
                );
            } else {
                checks.push(Check::new("capacity_state", Status::Unknown,
                    "Dependent capacity probes were not attempted because assignment is not settled.",
                    json!({"reason": "assignment_not_settled"}), readonly("workspace", "show", &workspace_id)));
            }
        }
    }
    mark_unattempted(&mut checks, include_surge, &workspace_id);
    let status = summary(&checks);
    let partial = checks.iter().any(|check| check.status == Status::Unknown);
    let report = json!({
        "workspaceId": workspace_id, "capacityId": capacity_id, "observedAt": Utc::now().to_rfc3339(),
        "scope": scope, "status": status, "partial": partial, "checks": checks,
        "notChecked": omissions(include_surge),
        "limitations": "Ready applies only to requested configuration observations. This is not end-to-end workload readiness. Unrequested surge checks and unobserved evidence are not inferred. No mutations or automatic remediation are performed."
    });
    output::render_object(cli, &report, "status");
    if strict && status != Status::Ready {
        let error = FabioError::with_typed_hint(
            ErrorCode::ApiError,
            "Workspace diagnostics did not establish readiness for every requested check.",
            "Inspect the report's checks and read-only nextSteps. No remediation was performed.",
            HintType::SemanticCorrection,
        );
        let candidates = || {
            checks
                .iter()
                .flat_map(|check| &check.next_steps)
                .filter(|hint| hint.kind == Kind::Remediation)
        };
        let hint = candidates()
            .find(|hint| hint.command.is_some())
            .or_else(|| candidates().next());
        return Err(if let Some(hint) = hint {
            hint.apply(error)
        } else {
            error
        }
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const CAP: &str = "bbbbbbbb-1111-2222-3333-444444444444";
    const ACTOR: &str = "cccccccc-1111-2222-3333-444444444444";

    fn now() -> DateTime<Utc> {
        "2026-11-02T12:00:00Z".parse().unwrap()
    }

    fn actor(workspace: &str, policy: Value) -> Value {
        let mut value = json!({"id": ACTOR, "actorDetails": {"actorType": "Workspace", "workspaceId": workspace}});
        value["expirationPolicy"] = policy;
        value
    }

    fn scanned(rows: Value) -> ActorScan {
        let mut scan = ActorScan::default();
        let mut body = json!({});
        body["value"] = rows;
        assert_eq!(scan.page(&body, WS.parse().unwrap()), None);
        scan
    }

    fn status(status: Status) -> Check {
        Check::new(
            "fixture",
            status,
            "fixture",
            Value::Null,
            readonly("workspace", "show", WS),
        )
    }

    #[test]
    fn summary_never_turns_missing_evidence_into_ready() {
        assert_eq!(summary(&[]), Status::Unknown);
        assert_eq!(summary(&[status(Status::Ready)]), Status::Ready);
        assert_eq!(
            summary(&[status(Status::Ready), status(Status::Unknown)]),
            Status::Unknown
        );
        assert_eq!(
            summary(&[status(Status::Unknown), status(Status::NeedsAction)]),
            Status::NeedsAction
        );
        assert_eq!(
            summary(&[
                status(Status::Unknown),
                status(Status::NeedsAction),
                status(Status::Blocked)
            ]),
            Status::Blocked
        );
        assert_eq!(
            serde_json::to_value(Status::NeedsAction).unwrap(),
            "needs_action"
        );
    }

    #[test]
    fn skipped_checks_are_explicit_and_machine_ids_are_not_free_text_names() {
        let mut checks = vec![Check::new(
            "capacity_assignment",
            Status::NeedsAction,
            "pending",
            Value::Null,
            readonly("workspace", "show", WS),
        )];
        mark_unattempted(&mut checks, true, WS);
        assert_eq!(checks.len(), 5);
        assert_eq!(summary(&checks), Status::NeedsAction);
        for check in &checks {
            let value = serde_json::to_value(check).unwrap();
            assert!(value["checkId"].is_string());
            assert!(value.get("name").is_none());
        }
        assert_eq!(
            checks
                .iter()
                .filter(|check| check.status == Status::Unknown)
                .count(),
            4
        );
        mark_unattempted(&mut checks, true, WS);
        assert_eq!(checks.len(), 5);
    }

    #[test]
    fn assignment_requires_both_valid_capacity_and_completed_progress() {
        for (capacity, progress, expected) in [
            (Value::from(CAP), Value::from("Completed"), Status::Ready),
            (
                Value::from(CAP),
                Value::from("InProgress"),
                Status::NeedsAction,
            ),
            (Value::Null, Value::from("InProgress"), Status::NeedsAction),
            (Value::from(CAP), Value::from("Failed"), Status::NeedsAction),
            (Value::Null, Value::from("Completed"), Status::NeedsAction),
            (
                Value::from("00000000-0000-0000-0000-000000000000"),
                Value::from("Completed"),
                Status::NeedsAction,
            ),
            (Value::from(CAP), Value::Null, Status::Unknown),
            (
                Value::from(CAP),
                Value::from("FutureState"),
                Status::Unknown,
            ),
            (
                Value::from("not-a-uuid"),
                Value::from("Completed"),
                Status::Unknown,
            ),
            (
                json!({"unexpected": true}),
                Value::from("Completed"),
                Status::Unknown,
            ),
        ] {
            let check = assignment(
                &json!({"capacityId": capacity, "capacityAssignmentProgress": progress}),
                WS,
            );
            assert_eq!(check.status, expected);
            assert!(
                check.next_steps[0]
                    .command
                    .as_deref()
                    .unwrap()
                    .contains("--readonly workspace show")
            );
        }
    }

    #[test]
    fn capacity_and_surge_settings_do_not_invent_live_utilization() {
        for (state, expected) in [
            ("Active", Status::Ready),
            ("Inactive", Status::Blocked),
            ("FutureState", Status::Unknown),
        ] {
            assert_eq!(
                capacity_state(Ok(json!({"id": CAP, "state": state})), CAP).status,
                expected
            );
        }
        assert_eq!(
            capacity_state(Ok(json!({"id": WS, "state": "Active"})), CAP).status,
            Status::Unknown
        );
        assert_eq!(
            capacity_state(Ok(json!({"state": "Active"})), CAP).status,
            Status::Unknown
        );
        for (state, expected) in [
            ("Enabled", Status::Unknown),
            ("Disabled", Status::Ready),
            ("FutureState", Status::Unknown),
        ] {
            assert_eq!(
                surge_configuration(Ok(json!({"state": state})), CAP).status,
                expected
            );
        }
    }

    #[test]
    fn scoped_errors_keep_codes_without_copying_response_bodies_or_mutation_hints() {
        for code in [
            ErrorCode::Forbidden,
            ErrorCode::AuthRequired,
            ErrorCode::NotFound,
            ErrorCode::RateLimited,
            ErrorCode::Timeout,
        ] {
            let error: anyhow::Error =
                FabioError::with_hint(code, "unbounded external body", "mutating workaround")
                    .set_request_id(Some("fixture-request".to_string()))
                    .set_retriable(Some(false))
                    .into();
            let check = capacity_state(Err(error), CAP);
            assert_eq!(check.status, Status::Unknown);
            assert_eq!(check.evidence["error"]["code"], code.as_str());
            assert_eq!(check.evidence["error"]["requestId"], "fixture-request");
            let rendered = serde_json::to_string(&check).unwrap();
            assert!(!rendered.contains("unbounded external body"));
            assert!(!rendered.contains("mutating workaround"));
        }
        assert_eq!(
            error_evidence(&anyhow::anyhow!("unstructured detail"))["code"],
            "UNKNOWN"
        );
    }

    #[test]
    fn expired_block_records_are_not_active_even_before_garbage_collection() {
        assert_eq!(
            block_active(
                &json!({"expirationPolicy": {"policyType": "NeverExpire"}}),
                now()
            ),
            Some(true)
        );
        for (date, active) in [
            ("2026-11-02T11:59:59Z", false),
            ("2026-11-02T12:00:00Z", false),
            ("2026-11-02T12:00:01Z", true),
            ("2026-11-02T13:00:00+01:00", false),
        ] {
            assert_eq!(
                block_active(
                    &json!({"expirationPolicy": {"policyType": "ExpireAfterDate", "expireAfterDateTime": date}}),
                    now()
                ),
                Some(active)
            );
        }
        for policy in [
            Value::Null,
            json!({"policyType": "FuturePolicy"}),
            json!({"policyType": "ExpireAfterDate", "expireAfterDateTime": "invalid"}),
        ] {
            assert_eq!(
                block_active(&json!({"expirationPolicy": policy}), now()),
                None
            );
        }
    }

    #[test]
    fn actor_scans_filter_other_workspaces_and_detect_incomplete_pages() {
        let rows = json!([
            actor(CAP, json!({"policyType": "NeverExpire"})),
            actor(&WS.to_uppercase(), json!({"policyType": "NeverExpire"}))
        ]);
        let scan = scanned(rows);
        assert!(scan.complete);
        assert_eq!(scan.actors_scanned, 2);
        assert_eq!(scan.matches.len(), 1);
        assert!(!serde_json::to_string(&scan).unwrap().contains(CAP));
        for body in [
            json!({}),
            json!({"value": {}}),
            json!({"value": [null]}),
            json!({"value": [], "continuationUri": "https://example.com/untrusted"}),
            json!({"value": [], "continuationToken": 123}),
        ] {
            let mut scan = ActorScan::default();
            scan.page(&body, WS.parse().unwrap());
            assert!(!scan.complete);
            assert!(scan.incomplete_reason.is_some());
        }
        let mut scan = ActorScan::default();
        assert_eq!(
            scan.page(
                &json!({"value": [], "continuationToken": "a+b &?"}),
                WS.parse().unwrap()
            ),
            Some("a+b &?".to_string())
        );
        assert!(!scan.complete);
    }

    #[test]
    fn actor_counts_and_matching_output_are_bounded() {
        let other = actor(CAP, Value::Null);
        let scan = scanned(json!(vec![other.clone(); MAX_ACTORS]));
        assert!(scan.complete);
        let scan = scanned(json!(vec![other; MAX_ACTORS + 1]));
        assert!(!scan.complete);
        assert_eq!(scan.actors_scanned, MAX_ACTORS);
        assert_eq!(scan.incomplete_reason, Some("actor_limit"));
        let scan = scanned(json!(vec![
            actor(WS, json!({"policyType": "NeverExpire"}));
            MAX_MATCHES + 1
        ]));
        assert_eq!(scan.matches.len(), MAX_MATCHES);
        assert_eq!(scan.incomplete_reason, Some("matching_actor_limit"));
        let mut scan = ActorScan::default();
        scan.page(
            &json!({"value": [], "continuationToken": "x".repeat(16_385)}),
            WS.parse().unwrap(),
        );
        assert_eq!(scan.incomplete_reason, Some("continuation_token_limit"));
        let scan = scanned(json!([actor(
            WS,
            json!({"policyType": "x".repeat(1000), "extra": "secret metadata"})
        )]));
        assert!(scan.complete);
        assert_eq!(
            scan.matches[0]["expirationPolicy"]["policyType"],
            Value::Null
        );
        assert!(
            !serde_json::to_string(&scan)
                .unwrap()
                .contains("secret metadata")
        );
        assert_eq!(
            actor_checks(&scan, &scanned(json!([])), CAP, WS.parse().unwrap(), now())[0].status,
            Status::Unknown
        );
    }

    #[test]
    fn actor_classification_requires_complete_evidence_and_does_not_guess_precedence() {
        let active = || scanned(json!([actor(WS, json!({"policyType": "NeverExpire"}))]));
        let empty = || scanned(json!([]));
        assert_eq!(
            actor_checks(&active(), &empty(), CAP, WS.parse().unwrap(), now())[0].status,
            Status::Blocked
        );
        assert_eq!(
            actor_checks(&empty(), &empty(), CAP, WS.parse().unwrap(), now())[0].status,
            Status::Ready
        );
        let checks = actor_checks(&active(), &active(), CAP, WS.parse().unwrap(), now());
        assert_eq!(checks[0].status, Status::Unknown);
        assert_eq!(checks[1].evidence["classified"], true);
        assert_eq!(
            actor_checks(
                &active(),
                &ActorScan::default(),
                CAP,
                WS.parse().unwrap(),
                now()
            )[0]
            .status,
            Status::Unknown
        );
        let expired = scanned(json!([actor(
            WS,
            json!({"policyType": "ExpireAfterDate", "expireAfterDateTime": "2026-11-01T00:00:00Z"})
        )]));
        assert_eq!(
            actor_checks(&expired, &empty(), CAP, WS.parse().unwrap(), now())[0].status,
            Status::Ready
        );
        let malformed = scanned(json!([actor(WS, json!({"policyType": "FuturePolicy"}))]));
        assert_eq!(
            actor_checks(&malformed, &empty(), CAP, WS.parse().unwrap(), now())[0].status,
            Status::Unknown
        );
    }

    #[test]
    fn confirmed_block_previews_are_deduplicated_and_unknown_evidence_is_not_remediated() {
        let row = actor(WS, json!({"policyType": "NeverExpire"}));
        let observed = scanned(json!([row, row]));
        let empty = scanned(json!([]));
        let checks = actor_checks(&observed, &empty, CAP, WS.parse().unwrap(), now());
        let hints: Vec<_> = checks[0]
            .next_steps
            .iter()
            .filter(|hint| hint.kind == Kind::Remediation)
            .collect();
        assert_eq!(hints.len(), 1);
        assert!(hints[0].requires_approval);
        let unknown = actor_checks(
            &observed,
            &ActorScan::default(),
            CAP,
            WS.parse().unwrap(),
            now(),
        );
        assert_eq!(
            unknown[0]
                .next_steps
                .iter()
                .filter(|hint| hint.kind == Kind::Remediation)
                .count(),
            0
        );
        let mixed = scanned(json!([
            actor(WS, json!({"policyType": "NeverExpire"})),
            actor(WS, json!({"policyType": "FuturePolicy"}))
        ]));
        let unknown = actor_checks(&mixed, &empty, CAP, WS.parse().unwrap(), now());
        assert_eq!(unknown[0].status, Status::Unknown);
        assert_eq!(
            unknown[0]
                .next_steps
                .iter()
                .filter(|hint| hint.kind == Kind::Remediation)
                .count(),
            0
        );
    }
}
