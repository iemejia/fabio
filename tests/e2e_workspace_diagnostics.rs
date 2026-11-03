//! Bounded GET-only diagnostics: missing/partial evidence must not turn green.
mod common;

use std::time::Duration;

use assert_cmd::Command;
use common::{mock_fabio, parse_json};
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const CAP: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const ACTOR: &str = "cccccccc-1111-2222-3333-444444444444";
const OTHER: &str = "dddddddd-1111-2222-3333-444444444444";

fn command(server: &MockServer) -> Command {
    let mut command = mock_fabio(server);
    command.args(["--readonly", "workspace", "diagnostics", "--id", WS]);
    command
}

async fn get(server: &MockServer, route: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

async fn workspace(server: &MockServer, progress: &str) {
    get(
        server,
        &format!("/workspaces/{WS}"),
        json!({"id": WS, "capacityId": CAP, "capacityAssignmentProgress": progress}),
    )
    .await;
}

async fn capacity(server: &MockServer, state: &str) {
    get(
        server,
        &format!("/capacities/{CAP}"),
        json!({"id": CAP, "state": state, "sku": "F2"}),
    )
    .await;
}

async fn settings(server: &MockServer, state: &str) {
    get(
        server,
        &format!("/capacities/{CAP}/surgeProtection"),
        json!({"state": state}),
    )
    .await;
}

async fn actors(server: &MockServer, collection: &str, body: Value) {
    get(
        server,
        &format!("/capacities/{CAP}/surgeProtection/{collection}"),
        body,
    )
    .await;
}

fn actor(workspace: &str, policy: Value) -> Value {
    let mut value =
        json!({"id": ACTOR, "actorDetails": {"actorType": "Workspace", "workspaceId": workspace}});
    value["expirationPolicy"] = policy;
    value
}

fn check<'a>(data: &'a Value, name: &str) -> &'a Value {
    data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["checkId"] == name)
        .unwrap()
}

async fn only_gets(server: &MockServer, count: usize) {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), count);
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dry_run_is_offline_and_scope_validation_precedes_the_preview() {
    let server = MockServer::start().await;
    let output = command(&server)
        .args(["--include-surge-protection", "--dry-run"])
        .assert()
        .success();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace diagnostics");
    assert_eq!(data["details"]["workspaceId"], WS);
    assert_eq!(data["details"]["scope"]["mutates"], false);
    assert_eq!(data["details"]["scope"]["maxLogicalReads"], 9);
    for args in [
        vec![
            "--readonly",
            "--dry-run",
            "workspace",
            "diagnostics",
            "--id",
            "invalid",
        ],
        vec![
            "--readonly",
            "--dry-run",
            "workspace",
            "diagnostics",
            "--id",
            WS,
            "--continuation-token",
            "opaque",
        ],
        vec![
            "--readonly",
            "--dry-run",
            "workspace",
            "diagnostics",
            "--id",
            WS,
            "--actor-page-limit",
            "0",
        ],
        vec![
            "--readonly",
            "--dry-run",
            "workspace",
            "diagnostics",
            "--id",
            WS,
            "--timeout",
            "301",
        ],
    ] {
        mock_fabio(&server).args(args).assert().failure();
    }
    only_gets(&server, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_checks_only_probe_workspace_and_capacity_metadata() {
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Active").await;
    let output = command(&server)
        .args(["--strict", "--wrap-untrusted"])
        .assert()
        .success();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["status"], "ready");
    assert_eq!(data["partial"], false);
    assert_eq!(data["scope"]["includeSurgeProtection"], false);
    assert_eq!(data["checks"].as_array().unwrap().len(), 2);
    assert_eq!(
        check(&data, "capacity_state")["evidence"]["state"],
        "Active"
    );
    assert!(
        data["limitations"]
            .as_str()
            .unwrap()
            .contains("not end-to-end")
    );
    for check in data["checks"].as_array().unwrap() {
        assert!(
            check["nextSteps"][0]["command"]
                .as_str()
                .unwrap()
                .contains("--readonly")
        );
    }
    only_gets(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsettled_assignment_never_queries_a_previous_capacity() {
    for progress in ["InProgress", "Failed", "FutureState"] {
        let server = MockServer::start().await;
        workspace(&server, progress).await;
        let output = command(&server)
            .arg("--include-surge-protection")
            .assert()
            .success();
        let data = parse_json(&output)["data"].clone();
        assert_eq!(
            data["status"],
            if progress == "FutureState" {
                "unknown"
            } else {
                "needs_action"
            }
        );
        assert_eq!(data["partial"], true);
        assert_eq!(
            check(&data, "capacity_state")["evidence"]["reason"],
            "assignment_not_settled"
        );
        only_gets(&server, 1).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_or_forbidden_workspace_is_unknown_not_a_successful_empty_report() {
    for status in [403, 404] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/workspaces/{WS}")))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(
                    json!({"errorCode": "Unavailable", "message": "external details"}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        let output = command(&server).arg("--strict").assert().failure();
        let data = parse_json(&output)["data"].clone();
        assert_eq!(data["status"], "unknown");
        assert_eq!(
            check(&data, "workspace_access")["evidence"]["error"]["code"],
            if status == 403 {
                "FORBIDDEN"
            } else {
                "NOT_FOUND"
            }
        );
        assert!(!data.to_string().contains("external details"));
        let error = String::from_utf8_lossy(&output.get_output().stderr)
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str::<Value>(line).ok())
            .unwrap();
        assert_eq!(error["error"]["code"], "API_ERROR");
        only_gets(&server, 1).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inactive_capacity_is_a_blocker_even_if_surge_permissions_are_missing() {
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Inactive").await;
    for collection in ["", "/blockedActors", "/missionCriticalActors"] {
        Mock::given(method("GET"))
            .and(path(format!(
                "/capacities/{CAP}/surgeProtection{collection}"
            )))
            .respond_with(ResponseTemplate::new(403).set_body_json(
                json!({"error":{"errorCode":"Forbidden","requestId":"fixture-request"}}),
            ))
            .expect(1)
            .mount(&server)
            .await;
    }
    let output = command(&server)
        .args(["--include-surge-protection", "--force", "--strict"])
        .assert()
        .failure();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["status"], "blocked");
    assert_eq!(data["partial"], true);
    assert_eq!(
        check(&data, "surge_configuration")["evidence"]["error"]["code"],
        "FORBIDDEN"
    );
    assert_eq!(check(&data, "workspace_block")["status"], "unknown");
    assert_eq!(
        check(&data, "mission_critical")["evidence"]["scan"]["error"]["requestId"],
        "fixture-request"
    );
    only_gets(&server, 5).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn block_expiration_and_conflicting_classification_are_not_guessed() {
    for (policy, critical, expected) in [
        (json!({"policyType":"NeverExpire"}), false, "blocked"),
        (
            json!({"policyType":"ExpireAfterDate","expireAfterDateTime":"2000-01-01T00:00:00Z"}),
            false,
            "ready",
        ),
        (json!({"policyType":"FuturePolicy"}), false, "unknown"),
        (json!({"policyType":"NeverExpire"}), true, "unknown"),
    ] {
        let server = MockServer::start().await;
        workspace(&server, "Completed").await;
        capacity(&server, "Active").await;
        settings(&server, "Disabled").await;
        actors(
            &server,
            "blockedActors",
            json!({"value":[actor(WS, policy)]}),
        )
        .await;
        let rows = if critical {
            json!([actor(WS, Value::Null)])
        } else {
            json!([])
        };
        actors(&server, "missionCriticalActors", json!({"value": rows})).await;
        let output = command(&server)
            .arg("--include-surge-protection")
            .assert()
            .success();
        let data = parse_json(&output)["data"].clone();
        assert_eq!(data["status"], expected);
        assert_eq!(
            check(&data, "mission_critical")["evidence"]["classified"],
            critical
        );
        only_gets(&server, 5).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enabled_surge_configuration_is_not_evidence_of_current_rejection_or_readiness() {
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Active").await;
    settings(&server, "Enabled").await;
    for collection in ["blockedActors", "missionCriticalActors"] {
        actors(&server, collection, json!({"value": []})).await;
    }
    let output = command(&server)
        .args(["--include-surge-protection", "--strict"])
        .assert()
        .failure();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["status"], "unknown");
    assert_eq!(
        check(&data, "surge_configuration")["evidence"]["state"],
        "Enabled"
    );
    assert_eq!(check(&data, "workspace_block")["status"], "ready");
    only_gets(&server, 5).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actor_page_limit_is_not_bypassed_by_global_all_or_limit_flags() {
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Active").await;
    settings(&server, "Disabled").await;
    actors(
        &server,
        "blockedActors",
        json!({"value": [], "continuationToken":"opaque"}),
    )
    .await;
    actors(&server, "missionCriticalActors", json!({"value": []})).await;
    let output = command(&server)
        .args([
            "--include-surge-protection",
            "--actor-page-limit",
            "1",
            "--all",
            "--limit",
            "1",
        ])
        .assert()
        .success();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["status"], "unknown");
    assert_eq!(data["checks"].as_array().unwrap().len(), 5);
    assert_eq!(
        check(&data, "workspace_block")["evidence"]["scan"]["incompleteReason"],
        "page_limit"
    );
    only_gets(&server, 5).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paginated_actor_evidence_stays_on_the_scoped_route_and_filters_other_workspaces() {
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Active").await;
    settings(&server, "Disabled").await;
    actors(&server, "missionCriticalActors", json!({"value": []})).await;
    let route = format!("/capacities/{CAP}/surgeProtection/blockedActors");
    Mock::given(method("GET")).and(path(&route)).and(query_param_is_missing("continuationToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [actor(OTHER, json!({"policyType":"NeverExpire"}))],
            "continuationToken": "a+b &?", "continuationUri": "https://example.com/must-not-follow"})))
        .expect(1).mount(&server).await;
    Mock::given(method("GET"))
        .and(path(&route))
        .and(query_param("continuationToken", "a+b &?"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"value": [actor(&WS.to_uppercase(), json!({"policyType":"NeverExpire"}))]}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&server)
        .arg("--include-surge-protection")
        .assert()
        .success();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["status"], "blocked");
    let scan = &check(&data, "workspace_block")["evidence"]["scan"];
    assert_eq!(scan["pagesScanned"], 2);
    assert_eq!(scan["actorsScanned"], 2);
    assert_eq!(scan["matches"].as_array().unwrap().len(), 1);
    assert!(!data.to_string().contains(OTHER));
    only_gets(&server, 6).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_and_uri_only_pages_are_unknown_not_complete_empty_scans() {
    for body in [
        json!({"value": {}}),
        json!({"value": [], "continuationUri":"https://example.com/must-not-follow"}),
    ] {
        let server = MockServer::start().await;
        workspace(&server, "Completed").await;
        capacity(&server, "Active").await;
        settings(&server, "Disabled").await;
        actors(&server, "blockedActors", body).await;
        actors(&server, "missionCriticalActors", json!({"value": []})).await;
        let output = command(&server)
            .arg("--include-surge-protection")
            .assert()
            .success();
        assert_eq!(parse_json(&output)["data"]["status"], "unknown");
        only_gets(&server, 5).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actor_scan_deadline_preserves_other_checks_and_never_mutates() {
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Active").await;
    settings(&server, "Disabled").await;
    actors(&server, "missionCriticalActors", json!({"value": []})).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/capacities/{CAP}/surgeProtection/blockedActors"
        )))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"value": []}))
                .set_delay(Duration::from_secs(2)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&server)
        .args(["--include-surge-protection", "--timeout", "1"])
        .timeout(Duration::from_secs(10))
        .assert()
        .success();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["status"], "unknown");
    assert_eq!(check(&data, "capacity_state")["status"], "ready");
    assert_eq!(
        check(&data, "workspace_block")["evidence"]["scan"]["error"]["code"],
        "TIMEOUT"
    );
    only_gets(&server, 5).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_continuations_stop_early_instead_of_spending_the_whole_page_budget() {
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Active").await;
    settings(&server, "Disabled").await;
    actors(&server, "missionCriticalActors", json!({"value": []})).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/capacities/{CAP}/surgeProtection/blockedActors"
        )))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"value": [], "continuationToken":"same-token"})),
        )
        .expect(2)
        .mount(&server)
        .await;
    let output = command(&server)
        .arg("--include-surge-protection")
        .assert()
        .success();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["status"], "unknown");
    assert_eq!(
        check(&data, "workspace_block")["evidence"]["scan"]["incompleteReason"],
        "continuation_cycle"
    );
    only_gets(&server, 6).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovery_and_the_published_example_match_the_actual_report_contract() {
    let server = MockServer::start().await;
    let output = mock_fabio(&server)
        .args(["context", "agent", "--full"])
        .assert()
        .success();
    let schema = parse_json(&output);
    let metadata = &schema["data"]["commands"]["workspace"]["subcommands"]["diagnostics"];
    assert_eq!(metadata["mutates"], false);
    assert_eq!(metadata["returns"], "object");
    assert_ne!(metadata["destructive"], true);
    assert_eq!(metadata["flags"]["--id"]["required"], true);
    let output = mock_fabio(&server)
        .args(["context", "examples", "workspace", "diagnostics"])
        .assert()
        .success();
    let mut example = parse_json(&output)["data"]["response"]["data"].clone();
    workspace(&server, "Completed").await;
    capacity(&server, "Active").await;
    // GET-only by implementation, not merely because --readonly blocks writes.
    let output = mock_fabio(&server)
        .args(["workspace", "diagnostics", "--id", WS])
        .assert()
        .success();
    let mut actual = parse_json(&output)["data"].clone();
    example.as_object_mut().unwrap().remove("observedAt");
    actual.as_object_mut().unwrap().remove("observedAt");
    assert_eq!(actual, example);
    let output = mock_fabio(&server)
        .args(["context", "examples", "workspace", "show"])
        .assert()
        .success();
    let guidance = parse_json(&output);
    let notes = guidance["data"]["notes"].as_str().unwrap();
    assert!(notes.contains("last assignment"));
    assert!(notes.contains("workspace diagnostics"));
    assert!(!notes.contains("when capacity is fully active"));
    only_gets(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn confirmed_block_produces_the_same_guarded_preview_in_report_and_error_hint() {
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Active").await;
    settings(&server, "Disabled").await;
    actors(
        &server,
        "blockedActors",
        json!({"value":[actor(WS, json!({"policyType":"NeverExpire"}))]}),
    )
    .await;
    actors(&server, "missionCriticalActors", json!({"value":[]})).await;
    let output = command(&server)
        .args(["--include-surge-protection", "--strict", "--wrap-untrusted"])
        .assert()
        .failure();
    let data = parse_json(&output)["data"].clone();
    let hint = check(&data, "workspace_block")["nextSteps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hint| hint["kind"] == "remediation")
        .unwrap();
    assert_eq!(hint["requiresApproval"], true);
    assert_eq!(hint["destructive"], true);
    assert_eq!(hint["hintType"], "semantic_correction");
    let preview = hint["command"].as_str().unwrap();
    assert!(preview.contains("--readonly"));
    assert!(preview.contains("--dry-run"));
    let error = String::from_utf8_lossy(&output.get_output().stderr)
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line).ok())
        .unwrap();
    assert_eq!(error["error"]["hintType"], "semantic_correction");
    assert!(error["error"]["hint"].as_str().unwrap().contains(preview));
    assert_eq!(error["error"]["verifyAfter"], hint["verifyAfter"]);
    let output = mock_fabio(&server)
        .env("OPENCODE_AGENT", "1")
        .args(shlex::split(preview).unwrap().into_iter().skip(1))
        .assert()
        .success();
    let preview = parse_json(&output)["data"].clone();
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["destructive"], true);
    assert!(
        preview["agentNotice"]
            .as_str()
            .unwrap()
            .contains("Confirm with the user")
    );
    assert_eq!(preview["details"]["capacityId"], CAP);
    assert_eq!(preview["details"]["actorId"], ACTOR);
    only_gets(&server, 5).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_actor_evidence_and_enabled_settings_do_not_offer_policy_mutation_previews() {
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Active").await;
    settings(&server, "Enabled").await;
    actors(&server, "blockedActors", json!({"value":[actor(WS, json!({"policyType":"NeverExpire"}))],"continuationToken":"more"})).await;
    actors(&server, "missionCriticalActors", json!({"value":[]})).await;
    let output = command(&server)
        .args([
            "--include-surge-protection",
            "--actor-page-limit",
            "1",
            "--strict",
        ])
        .assert()
        .failure();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["status"], "unknown");
    assert!(data["checks"].as_array().unwrap().iter().all(|check| {
        check["nextSteps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hint| hint["kind"] != "remediation")
    }));
    assert!(!String::from_utf8_lossy(&output.get_output().stderr).contains("delete-blocked-actor"));
    only_gets(&server, 5).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unassigned_workspace_has_a_template_not_a_guessed_target_capacity() {
    let server = MockServer::start().await;
    get(
        &server,
        &format!("/workspaces/{WS}"),
        json!({"id":WS,"capacityId":null,"capacityAssignmentProgress":"Completed"}),
    )
    .await;
    let output = command(&server).arg("--strict").assert().failure();
    let data = parse_json(&output)["data"].clone();
    let hint = check(&data, "capacity_assignment")["nextSteps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hint| hint["kind"] == "remediation")
        .unwrap();
    assert_eq!(hint["command"], Value::Null);
    assert_eq!(hint["missingInputs"], json!(["targetCapacityId"]));
    assert!(
        hint["commandTemplate"]
            .as_str()
            .unwrap()
            .contains("$TARGET_CAPACITY_ID")
    );
    assert_eq!(hint["requiresApproval"], true);
    only_gets(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workload_capacity_errors_offer_diagnosis_without_hidden_probes_and_keep_metadata() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path(format!("/workspaces/{WS}/items/{ACTOR}/jobs/instances")))
        .and(query_param("jobType", "Pipeline"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": {
            "errorCode":"CapacityNotActive","message":"CapacityNotActive","requestId":"fixture-request","isRetriable":false,
            "relatedResource":{"resourceId":CAP,"resourceType":"Capacity"},
            "parameters":[{"name":"workspaceId","value":WS}]
        }}))).expect(1).mount(&server).await;
    let output = mock_fabio(&server)
        .args(["data-pipeline", "run", "--workspace", WS, "--id", ACTOR])
        .assert()
        .failure();
    let error = String::from_utf8_lossy(&output.get_output().stderr)
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line).ok())
        .unwrap();
    assert_eq!(error["error"]["requestId"], "fixture-request");
    assert_eq!(error["error"]["retriable"], false);
    assert_eq!(error["error"]["relatedResource"]["resourceId"], CAP);
    assert_eq!(error["error"]["parameters"][0]["value"], WS);
    assert_eq!(error["error"]["hintType"], "semantic_correction");
    let text = error["error"]["hint"].as_str().unwrap();
    assert!(text.contains(&format!("workspace diagnostics --id {WS}")));
    assert!(!text.contains("delete-blocked-actor"));
    assert!(!text.contains("capacity resume"));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "POST");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inactive_capacity_resume_template_needs_explicit_inputs_and_previews_the_exact_arm_scope()
{
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Inactive").await;
    let output = command(&server).arg("--strict").assert().failure();
    let data = parse_json(&output)["data"].clone();
    let hint = check(&data, "capacity_state")["nextSteps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hint| hint["kind"] == "remediation")
        .unwrap();
    assert!(hint["command"].is_null());
    assert_eq!(
        hint["missingInputs"],
        json!(["subscription", "resourceGroup", "capacityName"])
    );
    let args = shlex::split(hint["commandTemplate"].as_str().unwrap())
        .unwrap()
        .into_iter()
        .skip(1)
        .map(|arg| match arg.as_str() {
            "$SUBSCRIPTION_ID" => WS.to_string(),
            "$RESOURCE_GROUP" => "rg-example".to_string(),
            "$CAPACITY_NAME" => "examplecapacity".to_string(),
            _ => arg,
        });
    let output = mock_fabio(&server).args(args).assert().success();
    let preview = parse_json(&output)["data"].clone();
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["would_execute"], "capacity resume");
    assert_eq!(preview["details"]["subscription"], WS);
    assert_eq!(preview["details"]["resourceGroup"], "rg-example");
    assert_eq!(
        preview["details"]["name"],
        "<<<UNTRUSTED>>>examplecapacity<<<END_UNTRUSTED>>>"
    );
    only_gets(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_known_and_unknown_block_records_do_not_offer_removal_previews() {
    let server = MockServer::start().await;
    workspace(&server, "Completed").await;
    capacity(&server, "Active").await;
    settings(&server, "Disabled").await;
    actors(
        &server,
        "blockedActors",
        json!({"value": [
            actor(WS, json!({"policyType":"NeverExpire"})),
            actor(WS, json!({"policyType":"FuturePolicy"}))
        ]}),
    )
    .await;
    actors(&server, "missionCriticalActors", json!({"value":[]})).await;
    let output = command(&server)
        .args(["--include-surge-protection", "--strict"])
        .assert()
        .failure();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["status"], "unknown");
    assert_eq!(check(&data, "workspace_block")["status"], "unknown");
    assert!(
        check(&data, "workspace_block")["nextSteps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hint| hint["kind"] != "remediation")
    );
    assert!(!String::from_utf8_lossy(&output.get_output().stderr).contains("delete-blocked-actor"));
    only_gets(&server, 5).await;
}
