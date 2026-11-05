//! Observational BI readiness, independent of native/generated/authored creation.
//! Query POSTs are read-only; refresh submission, rebinding and source repair are
//! deliberately not available here. Never infer freshness from refresh history.

use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::diagnostics::{
    Check, ReadBudget, Status, encode_id, id, summary, uuid_target,
};
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::operations::run_dax_rows;

const SCHEMA_QUERY: &str =
    "EVALUATE SELECTCOLUMNS(TOPN(201, INFO.VIEW.TABLES(), [ID], ASC), \"Name\", [Name])";

struct Target {
    workspace: Uuid,
    model: Uuid,
    refresh: Option<Uuid>,
    report: Option<(Uuid, Uuid)>,
}

impl Target {
    fn inspect(&self, command: &str) -> String {
        format!(
            "fabio --wrap-untrusted --readonly semantic-model {command} --workspace {} --id {}",
            self.workspace, self.model
        )
    }
}

fn validate_tables(tables: &[String]) -> Result<()> {
    if tables.len() > 20
        || tables.iter().any(|name| {
            name.trim().is_empty()
                || name.trim() != name
                || name.len() > 256
                || name.chars().any(char::is_control)
        })
        || tables.iter().enumerate().any(|(index, name)| {
            tables[..index]
                .iter()
                .any(|other| other.eq_ignore_ascii_case(name))
        })
    {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,
            "Expected tables must be distinct, non-empty model table names (at most 20, at most 256 bytes each).",
            "Repeat --expected-table with exact model table names, not source URLs or wildcards. Do not include surrounding whitespace or control characters.").into());
    }
    Ok(())
}

fn schema_check(rows: &[Value], expected: &[String], next: String) -> Check {
    let names: Option<Vec<_>> = rows
        .iter()
        .map(|row| {
            row["[Name]"]
                .as_str()
                .filter(|name| !name.is_empty() && name.len() <= 256)
        })
        .collect();
    let valid = names.as_ref().is_some_and(|names| {
        rows.len() <= 201
            && !names.iter().enumerate().any(|(index, name)| {
                names[..index]
                    .iter()
                    .any(|other| other.eq_ignore_ascii_case(name))
            })
    });
    if !valid {
        return Check::new(
            "model_schema",
            Status::Unknown,
            "Model schema rows were malformed or conflicting; absence was not inferred.",
            json!({"reason":"malformed_schema"}),
            next,
        );
    }
    let names = names.expect("validated schema");
    let complete = rows.len() <= 200;
    let missing: Vec<_> = expected
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            (!names.iter().any(|actual| actual.eq_ignore_ascii_case(name))).then_some(index)
        })
        .collect();
    let status = if !complete {
        Status::Unknown
    } else if rows.is_empty() || !missing.is_empty() {
        Status::NeedsAction
    } else {
        Status::Ready
    };
    Check::new(
        "model_schema",
        status,
        "Model table metadata is visible, not proof of Delta compatibility, source authorization, loaded rows, relationships or measures. Missing tables require review; creation/binding modes are never changed.",
        json!({"tableCountObserved":rows.len(),"complete":complete,"schemaTableLimit":200,"expectedTableCount":expected.len(),"missingExpectedTableIndexes":missing}),
        next,
    )
}

fn count_query(tables: &[String]) -> String {
    let pairs: Vec<_> = tables
        .iter()
        .enumerate()
        .map(|(index, name)| {
            format!(
                "\"t{index}\", COALESCE(COUNTROWS('{}'), 0)",
                name.replace('\'', "''")
            )
        })
        .collect();
    format!("EVALUATE ROW({})", pairs.join(", "))
}

fn data_check(rows: &[Value], tables: usize, minimum: u64, next: String) -> Check {
    let counts: Option<Vec<_>> = if rows.len() == 1 {
        (0..tables)
            .map(|index| rows[0][format!("[t{index}]")].as_u64())
            .collect()
    } else {
        None
    };
    let status = counts.as_ref().map_or(Status::Unknown, |counts| {
        if counts.iter().all(|count| *count >= minimum) {
            Status::Ready
        } else {
            Status::NeedsAction
        }
    });
    Check::new(
        "data_query",
        status,
        "COUNTROWS observes only the selected model tables under the caller's current identity. Success does not establish freshness, Direct Lake rather than fallback, source grants for others, or Direct Lake compatibility.",
        json!({"expectedTableCount":tables,"minimumRowsPerTable":minimum,"rowCountsByExpectedTableIndex":counts,"queryResultsIncluded":false}),
        next,
    )
}

fn refresh_phase(state: &str) -> Option<Status> {
    match state {
        "Completed" => Some(Status::Ready),
        "Failed" | "Cancelled" | "Disabled" | "TimedOut" => Some(Status::Blocked),
        "InProgress" | "NotStarted" => Some(Status::NeedsAction),
        "Unknown" => Some(Status::Unknown),
        _ => None,
    }
}

fn refresh_check(body: &Value, refresh: Uuid, next: String) -> Check {
    let response_id = body.get("requestId");
    let matched = response_id.is_none_or(|value| id(value) == Some(refresh));
    let state = body["status"].as_str().filter(|state| {
        matches!(
            *state,
            "Completed"
                | "Failed"
                | "Cancelled"
                | "Disabled"
                | "InProgress"
                | "NotStarted"
                | "Unknown"
                | "TimedOut"
        )
    });
    let consistent = body.get("error").is_none_or(Value::is_null)
        && body.get("extendedStatus").is_none_or(|value| {
            value
                .as_str()
                .and_then(refresh_phase)
                .is_some_and(|phase| Some(phase) == state.and_then(refresh_phase))
        });
    let status = if matched && consistent {
        state.and_then(refresh_phase).unwrap_or(Status::Unknown)
    } else {
        Status::Unknown
    };
    Check::new(
        "selected_refresh",
        status,
        "Only this explicit request's detail endpoint was read. Accepted submission, an older success, and model queryability do not establish this refresh completed. No refresh is submitted, cancelled or retried.",
        json!({"requestId":refresh.to_string(),"correlation":"exact_request_endpoint","responseIdentityReported":response_id.is_some(),"responseIdentityMatched":response_id.map(|_|matched),"completionEvidenceConsistent":consistent,"state":if matched{state}else{None}}),
        next,
    )
}

fn binding_check(body: &Value, report: Uuid, model: Uuid, workspace: Uuid, next: String) -> Check {
    let scope = id(&body["id"]) == Some(report);
    let bound = id(&body["datasetId"]);
    let source_workspace = body.get("datasetWorkspaceId");
    let workspace_matches = source_workspace.is_none_or(|value| id(value) == Some(workspace));
    let status =
        if !scope || bound.is_none() || source_workspace.is_some_and(|value| id(value).is_none()) {
            Status::Unknown
        } else if bound == Some(model) && workspace_matches {
            Status::Ready
        } else {
            Status::NeedsAction
        };
    Check::new(
        "report_binding",
        status,
        "The report's Power BI dataset reference was compared with the selected model. Binding does not prove visuals exist/render or that viewers have access. No rebind, takeover or definition update is performed.",
        json!({"reportIdentityMatched":scope,"datasetIdMatched":scope.then_some(bound==Some(model)),"datasetWorkspaceReported":source_workspace.is_some(),"datasetWorkspaceMatched":source_workspace.map(|_|workspace_matches)}),
        next,
    )
}

#[allow(clippy::too_many_lines)] // Keep a complete, explicitly scoped observation cycle together.
async fn observe(
    client: &FabricClient,
    budget: &ReadBudget,
    target: &Target,
    tables: &[String],
    probe: bool,
    minimum: u64,
) -> Vec<Check> {
    let mut checks = Vec::new();
    let workspace = target.workspace.to_string();
    let model = target.model.to_string();
    let metadata = budget
        .get(
            client,
            &format!("/workspaces/{workspace}/semanticModels/{model}"),
        )
        .await;
    let matched = metadata.as_ref().is_ok_and(|body| {
        id(&body["id"]) == Some(target.model)
            && body
                .get("workspaceId")
                .is_none_or(|value| id(value) == Some(target.workspace))
            && body
                .get("type")
                .is_none_or(|value| value == "SemanticModel")
    });
    checks.push(match metadata {
        Ok(_) => Check::new(
            "model_access",
            if matched {
                Status::Ready
            } else {
                Status::Unknown
            },
            "Metadata visibility is not query or source readiness.",
            json!({"scopeMatched":matched}),
            target.inspect("show"),
        ),
        Err(error) => Check::error(
            "model_access",
            &error,
            "model metadata",
            target.inspect("show"),
        ),
    });
    let mut refresh_blocked = false;
    if matched && let Some(refresh) = target.refresh {
        let path = format!("/groups/{workspace}/datasets/{model}/refreshes/{refresh}");
        let next = format!(
            "{} --refresh-id {refresh}",
            target.inspect("refresh-details")
        );
        let check = match budget.run(client.get_powerbi(&path)).await {
            Ok(body) => refresh_check(&body, refresh, next),
            Err(error) => Check::error("selected_refresh", &error, "exact refresh request", next),
        };
        refresh_blocked = check.status == Status::Blocked;
        checks.push(check);
    }
    if matched && !refresh_blocked {
        let schema = match budget
            .run(run_dax_rows(client, &workspace, &model, SCHEMA_QUERY))
            .await
        {
            Ok(rows) => schema_check(&rows, tables, target.inspect("list-tables")),
            Err(error) => Check::error(
                "model_schema",
                &error,
                "DAX table metadata",
                target.inspect("list-tables"),
            ),
        };
        let schema_ready = schema.status == Status::Ready;
        checks.push(schema);
        if probe {
            let next = target.inspect("list-tables");
            checks.push(if schema_ready {
                match budget
                    .run(run_dax_rows(
                        client,
                        &workspace,
                        &model,
                        &count_query(tables),
                    ))
                    .await
                {
                    Ok(rows) => data_check(&rows, tables.len(), minimum, next),
                    Err(error) => {
                        Check::error("data_query", &error, "DAX selected-table counts", next)
                    }
                }
            } else {
                Check::new(
                    "data_query",
                    Status::Unknown,
                    "The selected schema was not established; no data query was attempted.",
                    json!({"reason":"prerequisite_unavailable"}),
                    next,
                )
            });
        }
    } else {
        checks.push(Check::new(
            "model_schema",
            Status::Unknown,
            "Query probes were skipped because model scope was unavailable or the selected refresh was explicitly blocked.",
            json!({"reason":if refresh_blocked{"selected_refresh_blocked"}else{"prerequisite_unavailable"}}),
            target.inspect("show"),
        ));
        for key in [
            probe.then_some("data_query"),
            (!matched && target.refresh.is_some()).then_some("selected_refresh"),
        ]
        .into_iter()
        .flatten()
        {
            checks.push(Check::new(
                key,
                Status::Unknown,
                "Model scope was unavailable or the selected refresh was blocked; dependent query evidence is unavailable.",
                json!({"reason":"prerequisite_unavailable"}),
                target.inspect("show"),
            ));
        }
    }
    if let Some((report_workspace, report)) = target.report {
        let path = format!("/groups/{report_workspace}/reports/{report}");
        let next = format!(
            "{} --report-id {report} --report-workspace {report_workspace}",
            target.inspect("readiness")
        );
        checks.push(match budget.run(client.get_powerbi(&path)).await {
            Ok(body) => binding_check(&body, report, target.model, target.workspace, next),
            Err(error) => Check::error("report_binding", &error, "report dataset metadata", next),
        });
    }
    checks
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn execute(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    model: &str,
    tables: &[String],
    probe: bool,
    minimum: Option<u64>,
    refresh: Option<&str>,
    report: Option<&str>,
    report_workspace: Option<&str>,
    wait: bool,
    attempts: Option<u8>,
    interval: Option<u64>,
    timeout: u64,
    strict: bool,
) -> Result<()> {
    validate_tables(tables)?;
    if (probe && tables.is_empty())
        || (minimum.is_some() && !probe)
        || (!wait && (attempts.is_some() || interval.is_some()))
        || (report_workspace.is_some() && report.is_none())
        || cli.continuation_token.is_some()
    {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,"Incompatible readiness scope or polling flags.","Data probes need --expected-table; --min-rows needs --probe-data; polling options need --wait; --report-workspace needs --report-id. Aggregate readiness cannot resume a list cursor.").into());
    }
    let workspace = uuid_target(workspace, "--workspace")?;
    let model = uuid_target(model, "--id")?;
    let refresh = refresh
        .map(|value| uuid_target(value, "--refresh-id"))
        .transpose()?;
    let report = report
        .map(|value| uuid_target(value, "--report-id"))
        .transpose()?;
    let report_workspace = report_workspace
        .map(|value| uuid_target(value, "--report-workspace"))
        .transpose()?
        .unwrap_or(workspace);
    let target = Target {
        workspace,
        model,
        refresh,
        report: report.map(|report| (report_workspace, report)),
    };
    let attempts = if wait { attempts.unwrap_or(6) } else { 1 };
    let interval = interval.unwrap_or(5);
    let scope = json!({"mutates":false,"queryTransport":"powerbi.executeQueries","expectedTableCount":tables.len(),"probeData":probe,"refreshId":encode_id(refresh),"reportId":encode_id(report),"reportWorkspaceId":report.map(|_|report_workspace.to_string()),"timeoutSeconds":timeout,"maxAttempts":attempts,"maxLogicalReads":usize::from(attempts)*(2+usize::from(probe)+usize::from(refresh.is_some())+usize::from(report.is_some()))});
    if output::dry_run_guard(
        cli,
        "semantic-model readiness",
        &json!({"workspaceId":workspace.to_string(),"modelId":model.to_string(),"scope":scope}),
    ) {
        return Ok(());
    }
    let budget = ReadBudget::new(timeout);
    let mut cycle = 0;
    let mut checks;
    let reason;
    loop {
        cycle += 1;
        checks = observe(
            client,
            &budget,
            &target,
            tables,
            probe,
            minimum.unwrap_or(0),
        )
        .await;
        let status = summary(&checks);
        if status == Status::Ready {
            reason = "ready";
            break;
        }
        if status == Status::Blocked {
            reason = "blocked";
            break;
        }
        if budget.expired() {
            reason = "deadline";
            break;
        }
        if cycle >= attempts {
            reason = if wait { "attempt_limit" } else { "snapshot" };
            break;
        }
        if let Err(error) = budget
            .run(async {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                Ok(())
            })
            .await
        {
            checks.push(Check::error(
                "read_budget",
                &error,
                "total readiness deadline",
                target.inspect("show"),
            ));
            reason = "deadline";
            break;
        }
    }
    let status = summary(&checks);
    let mut not_checked = vec![
        "source Delta schema/protocol compatibility",
        "source grants and credentials for other identities",
        "data freshness and exact storage/fallback mode",
        "report visual rendering",
        "creation/binding mode changes",
    ];
    if !probe {
        not_checked.push("data query (requires --probe-data)");
    }
    if refresh.is_none() {
        not_checked.push("refresh completion/correlation (requires --refresh-id)");
    }
    if report.is_none() {
        not_checked.push("report binding (requires --report-id)");
    }
    output::render_object(
        cli,
        &json!({"workspaceId":workspace.to_string(),"modelId":model.to_string(),"observedAt":Utc::now().to_rfc3339(),"scope":scope,"status":status,"partial":checks.iter().any(|check|check.status==Status::Unknown),"attempts":cycle,"stopReason":reason,"checks":checks,"notChecked":not_checked,"limitations":"Readiness covers only the requested observations under the current identity. No refresh submissions, source scans outside the opt-in table counts, rebinding, takeover, publishing or repairs are performed."}),
        "status",
    );
    if strict && status != Status::Ready {
        return Err(FabioError::with_hint(ErrorCode::ApiError,"Requested BI readiness checks were not all ready.","Inspect checks and notChecked. Preserve the chosen creation/binding mode; never resubmit an accepted refresh or rebind automatically.").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_and_counts_are_strict_and_do_not_leak_arbitrary_metadata() {
        let expected = vec!["Sales".to_string()];
        assert_eq!(
            schema_check(&[], &expected, "inspect".into()).status,
            Status::NeedsAction
        );
        assert_eq!(
            schema_check(&[json!({})], &expected, "inspect".into()).status,
            Status::Unknown
        );
        assert_eq!(
            schema_check(&[json!({"[Name]":"Sales"})], &expected, "inspect".into()).status,
            Status::Ready
        );
        assert_eq!(
            data_check(&[json!({"[t0]":2})], 1, 1, "inspect".into()).status,
            Status::Ready
        );
        assert_eq!(
            data_check(&[json!({"[t0]":0})], 1, 1, "inspect".into()).status,
            Status::NeedsAction
        );
        assert_eq!(
            data_check(&[json!({"[t0]":"secret"})], 1, 1, "inspect".into()).status,
            Status::Unknown
        );
        assert!(
            !data_check(&[json!({"[t0]":"secret"})], 1, 1, "inspect".into())
                .evidence
                .to_string()
                .contains("secret")
        );
        let truncated: Vec<_> = (0..201)
            .map(|index| json!({"[Name]":format!("T{index}")}))
            .collect();
        assert_eq!(
            schema_check(&truncated, &[], "inspect".into()).status,
            Status::Unknown
        );
        assert_eq!(
            schema_check(
                &[json!({"[Name]":"Sales"}), json!({"[Name]":"sales"})],
                &expected,
                "inspect".into()
            )
            .status,
            Status::Unknown
        );
    }

    #[test]
    fn selected_names_are_validated_and_dax_quotes_are_escaped() {
        for names in [
            vec![String::new()],
            vec![" T".into()],
            vec!["T".into(), "t".into()],
            vec!["x".into(); 21],
            vec!["x\ny".into()],
        ] {
            assert!(validate_tables(&names).is_err());
        }
        assert_eq!(
            count_query(&["O'Brien".into()]),
            "EVALUATE ROW(\"t0\", COALESCE(COUNTROWS('O''Brien'), 0))"
        );
    }

    #[test]
    fn refresh_mismatch_and_report_missing_metadata_are_unknown() {
        let selected = Uuid::new_v4();
        assert_eq!(
            refresh_check(&json!({"status":"Completed"}), selected, "inspect".into()).status,
            Status::Ready
        );
        assert_eq!(
            refresh_check(
                &json!({"requestId":Uuid::new_v4().to_string(),"status":"Completed"}),
                selected,
                "inspect".into()
            )
            .status,
            Status::Unknown
        );
        assert_eq!(
            refresh_check(&json!({"status":"Unknown"}), selected, "inspect".into()).status,
            Status::Unknown
        );
        for body in [
            json!({"status":"Completed","extendedStatus":"Failed"}),
            json!({"status":"Failed","extendedStatus":"Completed"}),
            json!({"status":"Completed","error":{"message":"private"}}),
        ] {
            assert_eq!(
                refresh_check(&body, selected, "inspect".into()).status,
                Status::Unknown
            );
        }
        assert_eq!(
            refresh_check(&json!({"status":"Failed"}), selected, "inspect".into()).status,
            Status::Blocked
        );
        assert_eq!(
            binding_check(
                &json!({"id":selected.to_string()}),
                selected,
                Uuid::new_v4(),
                Uuid::new_v4(),
                "inspect".into()
            )
            .status,
            Status::Unknown
        );
    }
}
