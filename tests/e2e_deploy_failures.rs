//! Hermetic deployment aggregation tests: never replay uncertain batch members.
mod common;

use std::path::PathBuf;

use common::mock_fabio;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path},
};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const ITEM: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const OP: &str = "cccccccc-1111-2222-3333-444444444444";

struct Fixture {
    home: tempfile::TempDir,
    source: PathBuf,
    plan: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::TempDir::new().unwrap();
        let source = home.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let plan = home.path().join("plan.json");
        Self { home, source, plan }
    }

    fn item(&self, name: &str, kind: &str) -> PathBuf {
        let dir = self.source.join(format!("{name}.{kind}"));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join(".platform"),
            json!({"metadata":{"type":kind,"displayName":name}}).to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("definition.json"), "{}").unwrap();
        dir
    }

    fn plan(&self, changes: &[Value]) {
        std::fs::write(&self.plan, json!({"workspace_id":WS,"source_path":self.source,"changeset":{"changes":changes,"warnings":[],"errors":[]}}).to_string()).unwrap();
    }

    fn command(&self, server: &MockServer, strategy: &str) -> assert_cmd::Command {
        let mut command = mock_fabio(server);
        command
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env("FABIO_NO_VERSION_CHECK", "1")
            .env("FABIO_POWERBI_ENDPOINT", server.uri())
            .args(["deploy", "apply", "--plan"])
            .arg(&self.plan)
            .args(["--strategy", strategy, "--concurrency", "4", "--no-folders"]);
        command
    }
}

fn change(name: &str, kind: &str) -> Value {
    json!({"name":name,"item_type":kind,"action":"create","reason":"mock"})
}

fn data(output: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice::<Value>(&output.get_output().stdout).unwrap_or_else(|error| {
        panic!("Invalid stdout: {error}; output: {:?}", output.get_output())
    })["data"]
        .clone()
}

fn uncertain() -> ResponseTemplate {
    ResponseTemplate::new(503)
        .insert_header("x-ms-operation-id", OP)
        .set_body_json(json!({"error":{"errorCode":"MockFailure","message":"DependenciesCouldNotBeResolved ActiveCiCdOperationInProgress","requestId":"mock-trace","isRetriable":true,"parameters":[{"name":"mock","value":"preserved"}]}}))
}

fn assert_no_replay(summary: &Value) {
    assert_eq!(summary["retryAllowed"], false);
    assert!(
        summary["hint"]
            .as_str()
            .unwrap()
            .contains("Do not automatically retry")
    );
}

fn assert_terminal_error(output: &assert_cmd::assert::Assert, failures: usize) {
    assert_eq!(output.get_output().status.code(), Some(1));
    let error = String::from_utf8_lossy(&output.get_output().stderr)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|value| value.get("error").cloned())
        .unwrap();
    assert_eq!(error["code"], "UNKNOWN");
    assert_eq!(
        error["message"],
        format!("Deployment completed with {failures} failures")
    );
    assert_eq!(error["retriable"], false);
    assert!(
        error["hint"]
            .as_str()
            .unwrap()
            .contains("Do not automatically retry")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sequential_parallel_and_ordered_failures_preserve_uncertainty_without_replaying() {
    for (strategy, kind) in [
        ("sequential", "Notebook"),
        ("default", "Notebook"),
        ("default", "DataPipeline"),
    ] {
        let server = MockServer::start().await;
        let fixture = Fixture::new();
        for name in ["Good", "Uncertain"] {
            fixture.item(name, kind);
        }
        fixture.plan(&[change("Good", kind), change("Uncertain", kind)]);
        let route = format!("/workspaces/{WS}/items");
        Mock::given(method("POST"))
            .and(path(&route))
            .and(body_partial_json(json!({"displayName":"Good"})))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":ITEM})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(&route))
            .and(body_partial_json(json!({"displayName":"Uncertain"})))
            .respond_with(uncertain())
            .expect(1)
            .mount(&server)
            .await;
        let output = fixture
            .command(&server, strategy)
            .arg("--no-post-hooks")
            .assert()
            .failure();
        let summary = data(&output);
        assert_eq!(summary["status"], "partial_failure");
        assert_eq!(summary["succeeded"], 1);
        assert_eq!(summary["failed"], 1);
        assert_eq!(summary["skipped"], 0);
        assert_no_replay(&summary);
        let failure = &summary["failures"][0];
        assert_eq!(failure["change"]["name"], "Uncertain");
        assert!(failure["error"].is_string());
        assert_eq!(failure["code"], "ApiError");
        assert_eq!(failure["retryAllowed"], false);
        let details = &failure["errorDetails"];
        assert_eq!(details["code"], "API_ERROR");
        assert_eq!(details["retriable"], false);
        assert_eq!(details["requestId"], "mock-trace");
        assert_eq!(details["parameters"][0]["value"], "preserved");
        assert_eq!(details["submission"]["state"], "unknown");
        assert_eq!(details["submission"]["operationId"], OP);
        assert_eq!(details["submission"]["serverRetriable"], true);
        assert_terminal_error(&output, 1);
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_bulk_request_retains_shared_evidence_and_never_suggests_strategy_switching() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    for name in ["First", "Second"] {
        fixture.item(name, "Notebook");
    }
    fixture.plan(&[
        change("First", "Notebook"),
        change("Second", "Notebook"),
        change("Missing", "Notebook"),
    ]);
    Mock::given(method("POST"))
        .and(path(format!(
            "/workspaces/{WS}/items/bulkImportDefinitions"
        )))
        .respond_with(uncertain())
        .expect(1)
        .mount(&server)
        .await;
    let output = fixture
        .command(&server, "bulk")
        .arg("--no-post-hooks")
        .assert()
        .failure();
    let summary = data(&output);
    assert_no_replay(&summary);
    assert_eq!(summary["succeeded"], 0);
    assert_eq!(summary["failed"], 3);
    let failures = summary["failures"].as_array().unwrap();
    assert_eq!(failures[0]["code"], "SOURCE_NOT_FOUND");
    assert!(failures[0].get("errorDetails").is_none());
    for failure in &failures[1..] {
        assert_eq!(failure["code"], "BULK_IMPORT_FAILED");
        assert_eq!(failure["retryAllowed"], false);
        assert_eq!(failure["errorDetails"]["submission"]["state"], "unknown");
        assert_eq!(failure["errorDetails"]["submission"]["operationId"], OP);
        assert!(
            failure["errorDetails"]["submission"]
                .get("targetId")
                .is_none()
        );
        assert_eq!(
            failure["errorDetails"]["parameters"][0]["value"],
            "preserved"
        );
        assert!(
            !failure["error"]
                .as_str()
                .unwrap()
                .contains("Use --strategy")
        );
        assert!(
            !failure["error"]
                .as_str()
                .unwrap()
                .contains("git disconnect")
        );
    }
    assert!(summary.get("diagnosticHint").is_none());
    assert_terminal_error(&output, 3);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: Value = requests[0].body_json().unwrap();
    assert_eq!(body["definitionParts"].as_array().unwrap().len(), 4);
    assert!(!body.to_string().contains("Missing"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bulk_mixed_documented_results_do_not_count_partial_or_missing_members_as_success() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    let names = ["Good", "Failed", "Partial", "MissingResult"];
    for name in names {
        fixture.item(name, "Notebook");
    }
    let mut changes = names.map(|name| change(name, "Notebook")).to_vec();
    changes.push(change("MissingSource", "Notebook"));
    fixture.plan(&changes);
    Mock::given(method("POST")).and(path(format!("/workspaces/{WS}/items/bulkImportDefinitions")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"importItemDefinitionsDetails":[
            {"itemDisplayName":"Good","itemType":"Notebook","itemId":ITEM,"operationStatus":"Succeeded"},
            {"itemDisplayName":"Failed","itemType":"Notebook","operationStatus":"Failed","errorDetails":{"errorCode":"MockFailure","message":"mock error","isRetriable":true,"parameters":[{"name":"mock","value":"preserved"}]}},
            {"itemDisplayName":"Partial","itemType":"Notebook","itemId":OP,"operationStatus":"SucceededDespiteFailures"}
        ]}))).expect(1).mount(&server).await;
    let output = fixture
        .command(&server, "bulk")
        .arg("--no-post-hooks")
        .assert()
        .failure();
    let summary = data(&output);
    assert_eq!(summary["succeeded"], 1);
    assert_eq!(summary["failed"], 4);
    assert_eq!(summary["failures"][0]["code"], "SOURCE_NOT_FOUND");
    assert_eq!(
        summary["failures"][1]["errorDetails"]["parameters"][0]["value"],
        "preserved"
    );
    assert_eq!(summary["failures"][1]["errorDetails"]["retriable"], true);
    assert_eq!(summary["failures"][2]["code"], "BULK_IMPORT_FAILED");
    assert_eq!(summary["failures"][3]["code"], "BULK_RESULT_UNVERIFIED");
    for failure in summary["failures"].as_array().unwrap() {
        assert_eq!(failure["retryAllowed"], false);
        assert!(failure["errorDetails"].get("submission").is_none());
    }
    assert_no_replay(&summary);
    assert_terminal_error(&output, 4);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_bulk_payload_is_sent_when_all_source_members_are_missing() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    fixture.plan(&[change("Missing", "Notebook")]);
    let output = fixture
        .command(&server, "bulk")
        .arg("--no-post-hooks")
        .assert()
        .failure();
    let summary = data(&output);
    assert_eq!(summary["failed"], 1);
    assert_eq!(summary["failures"][0]["code"], "SOURCE_NOT_FOUND");
    assert_no_replay(&summary);
    assert_terminal_error(&output, 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bulk_unusable_success_and_acknowledged_body_failure_never_imply_member_success() {
    for (template, expected_code) in [
        (ResponseTemplate::new(200).set_body_json(json!({})), "BULK_RESULT_UNVERIFIED"),
        (ResponseTemplate::new(200).set_body_json(json!({"importItemDefinitionsDetails":[{"itemDisplayName":"Mock","itemType":"Notebook","itemId":ITEM,"operationStatus":"FutureStatus"}]})), "BULK_RESULT_UNVERIFIED"),
        (ResponseTemplate::new(201).insert_header("x-ms-operation-id",OP).set_body_string("not-json"), "BULK_IMPORT_FAILED"),
    ] {
        let server = MockServer::start().await;
        let fixture = Fixture::new();
        fixture.item("Mock","Notebook");
        fixture.plan(&[change("Mock","Notebook")]);
        Mock::given(method("POST")).respond_with(template).expect(1).mount(&server).await;
        let output = fixture.command(&server,"bulk").arg("--no-post-hooks").assert().failure();
        let summary = data(&output);
        assert_eq!(summary["succeeded"],0);
        assert_eq!(summary["failed"],1);
        assert_no_replay(&summary);
        let failure = &summary["failures"][0];
        assert_eq!(failure["code"],expected_code);
        if expected_code == "BULK_IMPORT_FAILED" {
            assert_eq!(failure["errorDetails"]["submission"]["state"],"acknowledged");
            assert_eq!(failure["errorDetails"]["submission"]["operationId"],OP);
        } else {
            assert_eq!(failure["code"],"BULK_RESULT_UNVERIFIED");
            assert!(failure["errorDetails"].get("submission").is_none());
        }
        assert_terminal_error(&output,1);
        assert_eq!(server.received_requests().await.unwrap().len(),1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bulk_success_keeps_legacy_summary_without_extra_failure_signals() {
    for legacy in [false, true] {
        let server = MockServer::start().await;
        let fixture = Fixture::new();
        fixture.item("Mock", "Notebook");
        fixture.plan(&[change("Mock", "Notebook")]);
        let row = if legacy {
            json!({"displayName":"Mock","type":"Notebook","id":ITEM,"status":"Succeeded"})
        } else {
            json!({"itemDisplayName":"Mock","itemType":"Notebook","itemId":ITEM,"operationStatus":"Succeeded"})
        };
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"importItemDefinitionsDetails":[row]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let output = fixture
            .command(&server, "bulk")
            .arg("--no-post-hooks")
            .assert()
            .success();
        let summary = data(&output);
        assert_eq!(summary["status"], "succeeded");
        assert_eq!(summary["succeeded"], 1);
        assert_eq!(summary["failed"], 0);
        assert_eq!(summary["failures"], json!([]));
        assert!(summary.get("retryAllowed").is_none());
        assert!(summary.get("hint").is_none());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bulk_legacy_override_codes_preserve_underlying_typed_errors() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    fixture.item("Mock", "Notebook");
    let mut rename = change("Renamed", "Notebook");
    rename["action"] = json!("rename");
    rename["deployed_id"] = json!(ITEM);
    let mut delete = change("Deleted", "Notebook");
    delete["action"] = json!("delete");
    delete["deployed_id"] = json!(OP);
    fixture.plan(&[rename, change("Mock", "Notebook"), delete]);
    Mock::given(method("PATCH"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"errorCode":"MockRenameFailure","message":"rename failed"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(400).set_body_json(json!({"errorCode":"ActiveCiCdOperationInProgress","message":"ActiveCiCdOperationInProgress"}))).expect(1).mount(&server).await;
    Mock::given(method("DELETE"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"errorCode":"MockDeleteFailure","message":"delete failed"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = fixture
        .command(&server, "bulk")
        .arg("--no-post-hooks")
        .assert()
        .failure();
    let summary = data(&output);
    for (failure, code) in summary["failures"].as_array().unwrap().iter().zip([
        "RENAME_FAILED",
        "BULK_GIT_BLOCKED",
        "DELETE_FAILED",
    ]) {
        assert_eq!(failure["code"], code);
        assert_eq!(failure["errorDetails"]["code"], "API_ERROR");
        assert_eq!(failure["retryAllowed"], false);
    }
    assert_eq!(summary["failed"], 3);
    assert_no_replay(&summary);
    assert_terminal_error(&output, 3);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nonfatal_schedule_hook_keeps_submission_evidence_and_blocks_whole_batch_replay() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    let dir = fixture.item("Scheduled", "Notebook");
    std::fs::write(
        dir.join("schedules.metadata.json"),
        json!([{"jobType":"RunNotebook","enabled":true,"configuration":{"type":"Daily"}}, {"jobType":"RunNotebook","enabled":false,"configuration":{"type":"Daily"}}])
            .to_string(),
    )
    .unwrap();
    fixture.plan(&[change("Scheduled", "Notebook")]);
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WS}/items")))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":ITEM})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/workspaces/{WS}/items/{ITEM}/jobs/RunNotebook/schedules"
        )))
        .respond_with(uncertain())
        .expect(1)
        .mount(&server)
        .await;
    let output = fixture.command(&server, "default").assert().success();
    let summary = data(&output);
    assert_eq!(summary["status"], "succeeded");
    assert_eq!(summary["succeeded"], 1);
    assert_eq!(summary["failed"], 0);
    assert_no_replay(&summary);
    let hook = &summary["post_hooks"][0];
    assert_eq!(hook["hook"], "create_schedule");
    assert_eq!(hook["status"], "failed");
    assert!(hook["error"].is_string());
    assert_eq!(hook["retryAllowed"], false);
    assert_eq!(hook["errorDetails"]["submission"]["state"], "unknown");
    assert_eq!(hook["errorDetails"]["submission"]["operationId"], OP);
    assert_eq!(hook["errorDetails"]["parameters"][0]["value"], "preserved");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_takeover_halts_dependent_definitions_without_refresh_or_later_hooks() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    fixture.item("Model", "SemanticModel");
    fixture.item("Report", "Report");
    fixture.plan(&[change("Model", "SemanticModel"), change("Report", "Report")]);
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WS}/items")))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":ITEM})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/groups/{WS}/datasets/{ITEM}/Default.TakeOver"
        )))
        .respond_with(uncertain())
        .expect(1)
        .mount(&server)
        .await;
    let output = fixture.command(&server, "default").assert().failure();
    let summary = data(&output);
    assert_eq!(summary["halted"], true);
    assert_eq!(summary["notExecuted"][0]["name"], "Report");
    assert_eq!(
        summary["post_hooks"][0]["errorDetails"]["submission"]["state"],
        "unknown"
    );
    assert_no_replay(&summary);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn successful_intertier_refresh_is_not_submitted_again_by_post_hooks() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    fixture.item("Model", "SemanticModel");
    fixture.plan(&[change("Model", "SemanticModel")]);
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WS}/items")))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":ITEM})))
        .expect(1)
        .mount(&server)
        .await;
    for action in ["Default.TakeOver", "refreshes"] {
        Mock::given(method("POST"))
            .and(path(format!("/groups/{WS}/datasets/{ITEM}/{action}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
    }
    let output = fixture.command(&server, "default").assert().success();
    assert_eq!(data(&output)["post_hooks"].as_array().unwrap().len(), 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shortcut_reconciliation_stops_on_bad_inventory_or_first_mutation_failure() {
    for inventory in [
        json!({}),
        json!({"value":[],"continuationToken":"more"}),
        json!({"value":[{"path":"Files","name":"../unsafe"}]}),
        json!({"value":[{"path":"Files","name":"Orphan"}]}),
    ] {
        let server = MockServer::start().await;
        let fixture = Fixture::new();
        let dir = fixture.item("Lake", "Lakehouse");
        std::fs::write(dir.join("shortcuts.metadata.json"), json!([{"path":"Files","name":"New","target":{"oneLake":{"workspaceId":WS,"itemId":ITEM,"path":"Files"}}}]).to_string()).unwrap();
        let mut update = change("Lake", "Lakehouse");
        update["action"] = json!("update");
        update["deployed_id"] = json!(ITEM);
        fixture.plan(&[update]);
        Mock::given(method("POST"))
            .and(path(format!(
                "/workspaces/{WS}/items/{ITEM}/updateDefinition"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/workspaces/{WS}/items/{ITEM}/shortcuts")))
            .respond_with(ResponseTemplate::new(200).set_body_json(&inventory))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(uncertain())
            .mount(&server)
            .await;
        let output = fixture.command(&server, "default").assert().success();
        let summary = data(&output);
        assert_eq!(summary["post_hooks"][0]["status"], "failed");
        assert_no_replay(&summary);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests.len(),
            if inventory["value"][0]["name"] == "Orphan" {
                3
            } else {
                2
            }
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method.as_str() == "POST")
                .count(),
            1
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn batch_dry_run_and_readonly_never_submit() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    fixture.item("Mock", "Notebook");
    fixture.plan(&[change("Mock", "Notebook")]);
    for strategy in ["default", "bulk"] {
        let output = fixture
            .command(&server, strategy)
            .args(["--dry-run", "--no-post-hooks"])
            .assert()
            .success();
        assert_eq!(data(&output)["status"], "dry_run");
        let output = fixture
            .command(&server, strategy)
            .args(["--readonly", "--no-post-hooks"])
            .assert()
            .failure();
        let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
        assert_eq!(error["error"]["code"], "READONLY_MODE");
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
