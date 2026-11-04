//! Evidence-only environment/runtime triage. Selection is explicit; co-located
//! observations never prove a session uses this environment or activation.

use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::diagnostics::{
    Check, PageCursor, ReadBudget, Status, encode_id, error_evidence, id, summary, uuid_target,
};
use crate::commands::remediation::RecoveryHint;
use crate::commands::spark_monitoring::{JobState, applications_base, spark_item_segment, version};
use crate::errors::{ErrorCode, FabioError, HintType};
use crate::output;

use super::pool_activations::{observation, resource_counts};

fn inspect_environment(workspace: Uuid, environment: Uuid, command: &str) -> String {
    format!(
        "fabio --wrap-untrusted --readonly environment {command} --workspace {workspace} --id {environment}"
    )
}

const fn phase_status(state: Option<JobState>) -> Status {
    match state {
        Some(JobState::Succeeded) => Status::Ready,
        Some(JobState::Failed) => Status::Blocked,
        Some(JobState::NotStarted | JobState::InProgress | JobState::Cancelled) => {
            Status::NeedsAction
        }
        _ => Status::Unknown,
    }
}

fn publish_check(body: &Value, next: String) -> Check {
    let reported = body["properties"]["publishDetails"]["state"].as_str();
    let reported = reported.and_then(|state| {
        [
            "Success",
            "Failed",
            "Waiting",
            "Running",
            "Cancelling",
            "Cancelled",
        ]
        .into_iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(state))
    });
    let (state, status) = match reported {
        Some("Success") => (reported, Status::Ready),
        Some("Failed") => (reported, Status::Blocked),
        Some("Waiting" | "Running" | "Cancelling" | "Cancelled") => (reported, Status::NeedsAction),
        _ => (None, Status::Unknown),
    };
    let mut check = Check::new(
        "publish_state",
        status,
        "Only the reported publish operation state is observed. A missing state is not proof of an unpublished environment; no publish was triggered.",
        json!({"source":"environment.properties.publishDetails", "state":state}),
        next,
    );
    if status != Status::Ready {
        check.next_steps.push(RecoveryHint::review("Review environment publish/component state before dependent work. Do not publish, cancel, or change libraries/settings automatically."));
    }
    check
}

fn small_number(body: &Value, key: &str) -> Option<u64> {
    body[key]
        .as_u64()
        .filter(|value| i32::try_from(*value).is_ok())
}

fn compute_check(body: &Value, next: String) -> Check {
    let runtime = version(&body["runtimeVersion"]);
    Check::new(
        "published_compute",
        if runtime.is_some() {
            Status::Ready
        } else {
            Status::Unknown
        },
        "Reported published compute is configuration evidence, not proof of library compatibility, available capacity, or runnable Spark.",
        json!({"source":"environment.sparkcompute", "runtimeVersion":runtime,"poolId":encode_id(id(&body["instancePool"]["id"])),
            "driverCores":small_number(body,"driverCores"),"executorCores":small_number(body,"executorCores")}),
        next,
    )
}

fn staging_check(published: &Result<Value>, staging: &Result<Value>, next: String) -> Check {
    if let Err(error) = staging {
        return Check::error("staging_comparison", error, "staging compute", next);
    }
    let (Ok(published), Ok(staging)) = (published, staging) else {
        return Check::new(
            "staging_comparison",
            Status::Unknown,
            "Both published and staging compute are needed for comparison.",
            json!({"reason":"prerequisite_unavailable"}),
            next,
        );
    };
    let mut compared = 0;
    let mut different = false;
    for key in [
        "runtimeVersion",
        "driverCores",
        "driverMemory",
        "executorCores",
        "executorMemory",
        "dynamicExecutorAllocation",
    ] {
        if let (Some(a), Some(b)) = (published.get(key), staging.get(key)) {
            compared += 1;
            different |= a != b;
        }
    }
    let status = if compared == 0 {
        Status::Unknown
    } else if different {
        Status::NeedsAction
    } else {
        Status::Ready
    };
    let mut check = Check::new(
        "staging_comparison",
        status,
        "Only mutually reported compute fields were compared. Equality does not establish library/Spark-property parity or session attachment; differences require review, not automatic publishing.",
        json!({"comparedFields":compared,"different":if compared>0 {Some(different)} else {None}}),
        next,
    );
    if different {
        check.next_steps.push(RecoveryHint::review("Review intended staging differences and library/runtime compatibility. No environment publish or configuration update was performed."));
    }
    check
}

fn activation_checks(body: &Value, activation: Uuid, next: String) -> Vec<Check> {
    let row = observation(body, activation);
    let mut checks = vec![Check::new(
        "pool_activation",
        phase_status(JobState::parse(&row["state"])),
        "This is the selected activation's historical lifecycle, not current warm-cluster availability or proof of a workload failure cause.",
        row,
        next.clone(),
    )];
    if id(&body["jobInstanceId"]) != Some(activation) {
        checks[0].status = Status::Unknown;
        return checks;
    }
    let counts = resource_counts(&body["resourceStatusSummary"]);
    let complete = counts
        .as_object()
        .is_some_and(|object| object.values().all(Value::is_u64))
        && body["resourceStatusSummary"]
            .as_object()
            .is_some_and(|object| object.len() == 8);
    let status = if !complete {
        Status::Unknown
    } else if counts["failed"].as_u64().is_some_and(|count| count > 0) {
        Status::NeedsAction
    } else {
        Status::Ready
    };
    checks.push(Check::new("activation_resources",status,
        "Resource breakdown is an observation of this activation. Zero ready resources or historical success does not prove current capacity health.",
        json!({"counts":counts,"complete":complete,"driverCores":small_number(&body["sparkConfiguration"],"driverCores"),
            "executorCores":small_number(&body["sparkConfiguration"],"executorCores"),"runtimeVersion":version(&body["runtimeVersion"])}),next));
    checks
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct History {
    pages_scanned: u8,
    rows_scanned: usize,
    complete: bool,
    reason: Option<&'static str>,
    error: Option<Value>,
    observations: Vec<Value>,
    #[serde(skip)]
    cursor: PageCursor,
    #[serde(skip)]
    seen: HashMap<Uuid, (Option<JobState>, Option<String>)>,
}

impl History {
    fn page(&mut self, body: &Value, limit: u8) -> Option<String> {
        self.pages_scanned += 1;
        let Some(rows) = body["value"].as_array() else {
            self.reason = Some("malformed_page");
            return None;
        };
        for row in rows {
            if self.rows_scanned >= usize::from(limit) {
                self.reason = Some("activation_limit");
                return None;
            }
            self.rows_scanned += 1;
            let Some(activation) = id(&row["jobInstanceId"]) else {
                self.reason = Some("malformed_activation");
                return None;
            };
            let state = JobState::parse(&row["state"]);
            let runtime = version(&row["runtimeVersion"]);
            if let Some((previous_state, previous_runtime)) = self.seen.get(&activation) {
                if *previous_state != state || previous_runtime.as_deref() != runtime {
                    self.reason = Some("conflicting_activation");
                    return None;
                }
            } else {
                self.seen
                    .insert(activation, (state, runtime.map(str::to_string)));
                self.observations.push(json!({"jobInstanceId":activation.to_string(),"state":state,"runtimeVersion":runtime}));
            }
        }
        match self.cursor.next(body) {
            Ok(token) => {
                if token.is_some() && self.rows_scanned >= usize::from(limit) {
                    self.reason = Some("activation_limit");
                    return None;
                }
                self.complete = token.is_none();
                token
            }
            Err(reason) => {
                self.reason = Some(reason);
                None
            }
        }
    }
}

async fn history(
    client: &FabricClient,
    budget: &ReadBudget,
    path: &str,
    pages: u8,
    limit: u8,
) -> History {
    let mut history = History::default();
    let path = format!("{path}?pageSize={limit}");
    let mut token: Option<String> = None;
    for _ in 0..pages {
        let url = token.as_ref().map_or_else(
            || path.clone(),
            |token| format!("{path}&continuationToken={}", urlencoding::encode(token)),
        );
        match budget.get(client, &url).await {
            Ok(body) => {
                token = history.page(&body, limit);
                if token.is_none() {
                    return history;
                }
            }
            Err(error) => {
                history.reason = Some("read_failed");
                history.error = Some(error_evidence(&error));
                return history;
            }
        }
    }
    history.reason = Some("page_limit");
    history
}

fn application_id(value: &Value) -> Option<&str> {
    let value = value.as_str().filter(|value| value.len() <= 128)?;
    let rest = value.strip_prefix("application_")?;
    let (time, number) = rest.split_once('_')?;
    (!time.is_empty()
        && !number.is_empty()
        && time.bytes().all(|byte| byte.is_ascii_digit())
        && number.bytes().all(|byte| byte.is_ascii_digit()))
    .then_some(value)
}

fn session_check(
    body: &Value,
    workspace: Uuid,
    item: Uuid,
    livy: Uuid,
    item_type: &str,
    next: String,
) -> (Check, bool) {
    let reference = &body["item"];
    let expected = match item_type {
        "notebook" => "Notebook",
        "lakehouse" => "Lakehouse",
        _ => "SparkJobDefinition",
    };
    let scope_valid = id(&body["livyId"]) == Some(livy)
        && reference["referenceType"]
            .as_str()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("ById"))
        && id(&reference["workspaceId"]) == Some(workspace)
        && id(&reference["itemId"]) == Some(item)
        && body["itemType"]
            .as_str()
            .is_some_and(|kind| kind.eq_ignore_ascii_case(expected));
    let state = scope_valid
        .then(|| JobState::parse(&body["state"]))
        .flatten();
    let check = Check::new(
        "spark_session",
        phase_status(state),
        "Only the explicitly selected monitoring session was inspected. Monitoring states are not interactive Livy idle/busy states; environment/activation attachment and failure causality are not inferred.",
        json!({"scopeMatched":scope_valid,"state":state,"livyId":livy.to_string(),"runtimeVersion":if scope_valid {version(&body["runtimeVersion"])} else {None}}),
        next,
    );
    (check, scope_valid)
}

fn named_log(body: &Value) -> bool {
    body["fileName"]
        .as_str()
        .is_some_and(|name| !name.is_empty() && name.len() <= 256)
}

fn log_count(body: &Value) -> Option<usize> {
    if named_log(body) {
        return Some(1);
    }
    if let Some(rows) = body
        .as_array()
        .or_else(|| body["containerLogMeta"].as_array())
    {
        let count = rows.iter().take(200).filter(|row| named_log(row)).count();
        return (count > 0).then_some(count);
    }
    named_log(&body["containerLogMeta"]).then_some(1)
}

fn runtime_check(key: &'static str, body: &Value, next: String) -> Check {
    let (known, evidence) = match key {
        "spark_advice" => {
            let rows = body.as_array().or_else(|| body["advices"].as_array());
            (
                rows.is_some(),
                json!({"adviceCount":rows.map(Vec::len),"descriptionsIncluded":false}),
            )
        }
        "spark_resource_usage" => {
            let numeric = |key: &str| {
                body[key]
                    .as_f64()
                    .filter(|value| value.is_finite() && *value >= 0.0)
            };
            let samples = body["data"]["timestamps"].as_array().map(Vec::len);
            let known = samples.is_some()
                || numeric("coreEfficiency").is_some()
                || numeric("idleTime").is_some()
                || body["capacityExceeded"].is_boolean();
            (
                known,
                json!({"sampleCount":samples,"coreEfficiency":numeric("coreEfficiency"),"idleTime":numeric("idleTime"),"capacityExceeded":body["capacityExceeded"].as_bool()}),
            )
        }
        _ => {
            let count = log_count(body);
            (
                count.is_some(),
                json!({"filesObservedUpTo200":count,"contentIncluded":false,"fileNamesIncluded":false}),
            )
        }
    };
    Check::new(
        key,
        if known {
            Status::Ready
        } else {
            Status::Unknown
        },
        "Only bounded telemetry/log-metadata observations are summarized. Empty advice or readable logs do not prove application health. Raw logs and arbitrary Spark properties are never included.",
        evidence,
        next,
    )
}

#[allow(clippy::too_many_arguments)]
async fn runtime_checks(
    client: &FabricClient,
    budget: &ReadBudget,
    workspace: Uuid,
    item: Uuid,
    kind: &str,
    livy: Uuid,
    logs: bool,
) -> Vec<Check> {
    let segment = spark_item_segment(kind);
    let session_path = format!("/workspaces/{workspace}/{segment}/{item}/livySessions/{livy}");
    let next = format!(
        "fabio --wrap-untrusted --readonly {kind} get-livy-session --workspace {workspace} --id {item} --livy-id {livy}"
    );
    let body = match budget.get(client, &session_path).await {
        Ok(body) => body,
        Err(error) => {
            return vec![Check::error(
                "spark_session",
                &error,
                "monitoring session",
                next,
            )];
        }
    };
    let (check, matched) = session_check(&body, workspace, item, livy, kind, next.clone());
    let mut checks = vec![check];
    if !matched {
        return checks;
    }
    let app = application_id(&body["sparkApplicationId"]);
    let attempt = body["attemptNumber"]
        .as_u64()
        .filter(|attempt| i32::try_from(*attempt).is_ok());
    if let Some(app) = app {
        let base = applications_base(
            &workspace.to_string(),
            segment,
            &item.to_string(),
            &livy.to_string(),
            app,
            None,
        );
        let args = format!(
            "--workspace {workspace} --item-type {kind} --item-id {item} --livy-id {livy} --app-id {app}"
        );
        let mut probes = vec![
            (
                "spark_advice",
                format!("{base}/advice"),
                format!("fabio --wrap-untrusted --readonly spark get-advice {args}"),
            ),
            (
                "spark_resource_usage",
                format!("{base}/resourceUsage"),
                format!("fabio --wrap-untrusted --readonly spark get-resource-usage {args}"),
            ),
        ];
        if logs {
            probes.push((
                "driver_log_metadata",
                format!("{base}/logs?type=driver&meta=true&fileName=stderr"),
                format!(
                    "fabio --wrap-untrusted --readonly spark get-logs {args} --type driver --meta --file-name stderr"
                ),
            ));
        }
        for (key, path, next) in probes {
            let mut check = match budget.get(client, &path).await {
                Ok(body) => runtime_check(key, &body, next),
                Err(error) => Check::error(key, &error, "Spark monitoring", next),
            };
            check.evidence["reportedAttemptNumber"] = json!(attempt);
            check.evidence["attemptPinned"] = json!(false);
            checks.push(check);
        }
    } else {
        checks.push(Check::new("spark_application",Status::Unknown,
            "Application metadata was unavailable or malformed. No application ID or alternate application/attempt was guessed.",json!({"reason":"application_reference_unavailable"}),next));
    }
    if logs {
        let base = applications_base(
            &workspace.to_string(),
            segment,
            &item.to_string(),
            &livy.to_string(),
            "none",
            None,
        );
        let next = format!(
            "fabio --wrap-untrusted --readonly spark get-logs --workspace {workspace} --item-type {kind} --item-id {item} --livy-id {livy} --type livy --meta"
        );
        checks.push(
            match budget
                .get(client, &format!("{base}/logs?type=livy&meta=true"))
                .await
            {
                Ok(body) => runtime_check("livy_log_metadata", &body, next),
                Err(error) => Check::error("livy_log_metadata", &error, "Spark monitoring", next),
            },
        );
    }
    checks
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn execute(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    environment: Option<&str>,
    activation: Option<&str>,
    staging: bool,
    item: Option<&str>,
    kind: Option<&str>,
    livy: Option<&str>,
    logs: bool,
    pages: u8,
    limit: u8,
    timeout: u64,
    strict: bool,
) -> Result<()> {
    let workspace = uuid_target(workspace, "--workspace")?;
    let environment = environment
        .map(|value| uuid_target(value, "--id"))
        .transpose()?;
    let activation = activation
        .map(|value| uuid_target(value, "--activation-id"))
        .transpose()?;
    let session =
        match (item, kind, livy) {
            (None, None, None) => None,
            (
                Some(item),
                Some(kind @ ("notebook" | "spark-job-definition" | "lakehouse")),
                Some(livy),
            ) => Some((
                uuid_target(item, "--item-id")?,
                kind,
                uuid_target(livy, "--livy-id")?,
            )),
            _ => return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Session diagnostics require --item-id, --item-type, and --livy-id together.",
                "Select one explicit monitoring session; no workspace-wide discovery is performed.",
            )
            .into()),
        };
    if (logs && session.is_none())
        || cli.continuation_token.is_some()
        || (environment.is_none() && (session.is_none() || activation.is_some() || staging))
    {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,"Log metadata requires an explicit session, and aggregate diagnostics cannot resume one list cursor.","Select the session explicitly; use environment list-pool-activations to resume a history page.").into());
    }
    let environment_reads = if environment.is_some() {
        2 + usize::from(staging)
            + if activation.is_some() {
                1
            } else {
                usize::from(pages)
            }
    } else {
        1
    };
    let session_reads = if session.is_some() {
        3 + if logs { 2 } else { 0 }
    } else {
        0
    };
    let scope = json!({"area":"environment_spark_observations","mutates":false,"includeStaging":staging,"includeLogMetadata":logs,
        "historyPageLimit":pages,"historyActivationLimit":limit,"timeoutSeconds":timeout,
        "environmentSource":if environment.is_some(){"explicit_item"}else{"workspace_defaults"},
        "sessionEnvironmentBinding":"not_verified",
        "applicationAttemptSelection":if session.is_some(){"server_default_unpinned"}else{"not_requested"},
        "maxLogicalReads":environment_reads+session_reads});
    let selected=session.map(|(item,kind,livy)|json!({"itemId":item.to_string(),"itemType":kind,"livyId":livy.to_string()}));
    if output::dry_run_guard(
        cli,
        "environment diagnostics",
        &json!({"workspaceId":workspace.to_string(),"environmentId":encode_id(environment),"activationId":encode_id(activation),"session":selected,"scope":scope}),
    ) {
        return Ok(());
    }
    let budget = ReadBudget::new(timeout);
    let mut history_data = None;
    let mut checks = Vec::new();
    if let Some(environment) = environment {
        let next = inspect_environment(workspace, environment, "show");
        let metadata = budget
            .get(
                client,
                &format!("/workspaces/{workspace}/environments/{environment}"),
            )
            .await;
        let matched = metadata.as_ref().is_ok_and(|body| {
            id(&body["id"]) == Some(environment)
                && (body["workspaceId"].is_null() || id(&body["workspaceId"]) == Some(workspace))
                && (body["type"].is_null() || body["type"] == "Environment")
        });
        checks.push(match &metadata {
            Ok(_) if matched => Check::new(
                "environment_access",
                Status::Ready,
                "The requested environment metadata is visible.",
                json!({"source":"environment"}),
                next.clone(),
            ),
            Ok(_) => Check::new(
                "environment_access",
                Status::Unknown,
                "Environment metadata did not match the requested scope.",
                json!({"reason":"scope_mismatch"}),
                next.clone(),
            ),
            Err(error) => Check::error("environment_access", error, "environment", next.clone()),
        });
        if matched {
            checks.push(publish_check(
                metadata.as_ref().expect("matched successful metadata"),
                next.clone(),
            ));
            let published = budget
                .get(
                    client,
                    &format!(
                        "/workspaces/{workspace}/environments/{environment}/sparkcompute?beta=true"
                    ),
                )
                .await;
            let compute_next = inspect_environment(workspace, environment, "get-spark-settings");
            checks.push(match &published {
                Ok(body) => compute_check(body, compute_next),
                Err(error) => Check::error(
                    "published_compute",
                    error,
                    "published compute",
                    compute_next,
                ),
            });
            if staging {
                let staged=budget.get(client,&format!("/workspaces/{workspace}/environments/{environment}/staging/sparkcompute?beta=true")).await;
                checks.push(staging_check(
                    &published,
                    &staged,
                    inspect_environment(workspace, environment, "get-staging-spark-settings"),
                ));
            }
            if let Some(activation) = activation {
                let path = format!(
                    "/workspaces/{workspace}/environments/{environment}/poolActivations/{activation}"
                );
                let next = format!(
                    "{} --activation-id {activation}",
                    inspect_environment(workspace, environment, "show-pool-activation")
                );
                match budget.get(client, &path).await {
                    Ok(body) => checks.extend(activation_checks(&body, activation, next)),
                    Err(error) => checks.push(Check::error(
                        "pool_activation",
                        &error,
                        "pool activation",
                        next,
                    )),
                }
            } else {
                let path =
                    format!("/workspaces/{workspace}/environments/{environment}/poolActivations");
                let history = history(client, &budget, &path, pages, limit).await;
                let known = history.observations.iter().all(|row| {
                    JobState::parse(&row["state"]).is_some_and(|state| state != JobState::Unknown)
                });
                checks.push(Check::new("activation_history",if history.complete && known {Status::Ready}else{Status::Unknown},
                "Only bounded history was inspected; no newest-first ordering, current warm capacity, or workload failure causality is inferred. Select an activation ID for detailed counts.",
                serde_json::to_value(&history)?,inspect_environment(workspace,environment,"list-pool-activations")));
                history_data = Some(history);
            }
        } else {
            checks.push(Check::new("environment_probes",Status::Unknown,"Environment-dependent probes were not attempted because environment scope was not established.",json!({"reason":"prerequisite_unavailable"}),next));
        }
    } else {
        let next =
            format!("fabio --wrap-untrusted --readonly spark get-settings --workspace {workspace}");
        checks.push(match budget.get(client,&format!("/workspaces/{workspace}/spark/settings")).await {
            Ok(body)=> {
                let mut check=compute_check(&body["environment"],next);
                check.key="workspace_spark_defaults";
                check.message="Workspace runtime defaults were observed, not proof this session uses them. No environment ID was guessed and no publish/history probes were attempted.".to_string();
                check.evidence["source"]=json!("workspace.spark.settings.environment");
                check
            },
            Err(error)=>Check::error("workspace_spark_defaults",&error,"workspace Spark settings",next)
        });
    }
    if let Some((item, kind, livy)) = session {
        checks.extend(runtime_checks(client, &budget, workspace, item, kind, livy, logs).await);
    }
    let status = summary(&checks);
    let partial = checks.iter().any(|check| check.status == Status::Unknown);
    let report = json!({"workspaceId":workspace.to_string(),"environmentId":encode_id(environment),"activationId":encode_id(activation),"session":selected,
        "observedAt":Utc::now().to_rfc3339(),"scope":scope,"status":status,"partial":partial,"checks":checks,"activationHistory":history_data,
        "notChecked":["session/environment/activation binding","exact application attempt pinning","raw log contents","library compatibility and staged library parity","arbitrary Spark properties","source authorization","live capacity headroom","workload execution"],
        "limitations":"These are explicitly selected configuration/historical/runtime observations, not end-to-end readiness or a causal diagnosis. No jobs, sessions, activations, publishes, cancellations, configuration changes, or source probes are started."});
    output::render_object(cli, &report, "status");
    if strict && status != Status::Ready {
        return Err(FabioError::with_typed_hint(ErrorCode::ApiError,"Environment/Spark diagnostics did not establish every requested observation.",
            "Inspect the scoped checks and read-only nextSteps. Do not publish, restart, cancel, or change pools based on incomplete or historical evidence.",HintType::SemanticCorrection).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const ITEM: &str = "bbbbbbbb-1111-2222-3333-444444444444";
    const LIVY: &str = "cccccccc-1111-2222-3333-444444444444";

    #[test]
    fn states_and_telemetry_do_not_turn_missing_evidence_into_ready() {
        assert_eq!(phase_status(None), Status::Unknown);
        assert_eq!(phase_status(Some(JobState::Unknown)), Status::Unknown);
        assert_eq!(phase_status(Some(JobState::Failed)), Status::Blocked);
        assert_eq!(
            publish_check(&json!({}), "inspect".to_string()).status,
            Status::Unknown
        );
        assert_eq!(
            compute_check(&json!({}), "inspect".to_string()).status,
            Status::Unknown
        );
        assert_eq!(
            runtime_check("spark_advice", &json!([]), "inspect".to_string()).status,
            Status::Ready
        );
        assert_eq!(
            runtime_check("spark_resource_usage", &json!({}), "inspect".to_string()).status,
            Status::Unknown
        );
    }

    #[test]
    fn history_is_bounded_and_never_uses_untrusted_urls_or_names() {
        let mut history = History::default();
        history.page(&json!({"value":[{"jobInstanceId":LIVY,"name":"private","state":"Failed"}],"continuationUri":"https://example.org/private"}),10);
        assert_eq!(history.reason, Some("uri_only_continuation"));
        assert!(!history.complete);
        assert!(!serde_json::to_string(&history).unwrap().contains("private"));
        let mut history = History::default();
        history.page(&json!({"value":[{"jobInstanceId":LIVY,"state":"Succeeded"},{"jobInstanceId":LIVY,"state":"FutureState"}]}),10);
        assert_eq!(history.reason, Some("conflicting_activation"));
        assert!(!history.complete);
    }

    #[test]
    fn application_and_session_scope_are_validated_before_application_probes() {
        assert_eq!(
            application_id(&json!("application_123_456")),
            Some("application_123_456")
        );
        for value in [
            "../escape",
            "application_1_2/evil",
            "https://example.org",
            "none",
        ] {
            assert_eq!(application_id(&json!(value)), None);
        }
        let mut body = json!({"livyId":LIVY,"state":"Succeeded","itemType":"notebook","item":{"referenceType":"ById","workspaceId":WS,"itemId":ITEM}});
        assert!(
            session_check(
                &body,
                WS.parse().unwrap(),
                ITEM.parse().unwrap(),
                LIVY.parse().unwrap(),
                "notebook",
                "inspect".to_string()
            )
            .1
        );
        body["item"]["workspaceId"] = json!(ITEM);
        assert!(
            !session_check(
                &body,
                WS.parse().unwrap(),
                ITEM.parse().unwrap(),
                LIVY.parse().unwrap(),
                "notebook",
                "inspect".to_string()
            )
            .1
        );
    }

    #[test]
    fn metadata_and_resource_summaries_exclude_arbitrary_log_content() {
        let row = runtime_check(
            "driver_log_metadata",
            &json!({"containerLogMeta":{"fileName":"private.log","length":10},"content":"secret"}),
            "inspect".to_string(),
        );
        assert_eq!(row.status, Status::Ready);
        assert!(!row.evidence.to_string().contains("private"));
        assert!(!row.evidence.to_string().contains("secret"));
        let checks = activation_checks(
            &json!({"jobInstanceId":LIVY,"state":"Succeeded","resourceStatusSummary":{"ready":2}}),
            LIVY.parse().unwrap(),
            "inspect".to_string(),
        );
        assert_eq!(checks[1].status, Status::Unknown);
        let checks = activation_checks(
            &json!({"jobInstanceId":LIVY,"state":"Succeeded","resourceStatusSummary":{
            "starting":0,"ready":1,"acquired":0,"stopping":0,"deactivated":0,"expired":0,"cancelled":0,"failed":0,"futureState":1}}),
            LIVY.parse().unwrap(),
            "inspect".to_string(),
        );
        assert_eq!(checks[1].status, Status::Unknown);
    }
}
