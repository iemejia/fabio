//! Read-only Gen1 planning; never upgrades or fabricates complete dependencies.
mod common;

use common::{mock_fabio, parse_json};
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const A: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const B: &str = "cccccccc-1111-2222-3333-444444444444";

fn command(server: &MockServer) -> assert_cmd::Command {
    let mut cmd = mock_fabio(server);
    cmd.args(["--readonly", "dataflow", "plan-upgrade", "--workspace", WS]);
    cmd
}
fn row(id: &str, status: &str) -> Value {
    json!({"id":id,"displayName":"private_name","status":status})
}
fn route() -> String {
    format!("/workspaces/{WS}/dataflows/gen1UpgradeReadinessResults")
}
async fn page(server: &MockServer, body: Value) {
    Mock::given(method("GET"))
        .and(path(route()))
        .and(query_param("pageSize", "30"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}
async fn reads(server: &MockServer, count: usize) {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), count);
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET" && request.url.path() == route())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_scope_and_offline_preview_refuse_unsafe_inputs_without_requests() {
    let server = MockServer::start().await;
    let result = command(&server)
        .args(["--id", A, "--dry-run"])
        .assert()
        .success();
    let data = &parse_json(&result)["data"];
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "dataflow plan-upgrade");
    assert_eq!(data["details"]["executesUpgrade"], false);
    assert_eq!(
        data["details"]["scope"]["assessmentScope"],
        "all_gen1_in_one_workspace"
    );
    for args in [
        vec![],
        vec!["--id", "invalid"],
        vec!["--id", A, "--id", A],
        vec!["--id", "00000000-0000-0000-0000-000000000000"],
        vec!["--id", A, "--continuation-token", "private"],
        vec!["--id", A, "--page-size", "31"],
        vec!["--id", A, "--timeout", "0"],
    ] {
        command(&server)
            .args(args)
            .arg("--dry-run")
            .assert()
            .failure();
    }
    let mut too_many = command(&server);
    for n in 1..=51 {
        too_many.args(["--id", &uuid::Uuid::from_u128(n).to_string()]);
    }
    too_many.arg("--dry-run").assert().failure();
    reads(&server, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ready_selected_assessments_produce_only_an_approval_blocked_batch_preview() {
    let server = MockServer::start().await;
    page(
        &server,
        json!({"value":[row(A,"ReadyToMigrate"),row(B,"ReadyToMigrate")]}),
    )
    .await;
    let output = command(&server)
        .args(["--id", A, "--id", B, "--strict"])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "ready");
    assert_eq!(data["mutates"], false);
    assert_eq!(data["reviewPlan"]["applied"], false);
    assert_eq!(data["reviewPlan"]["readyToApply"], false);
    let preview = &data["reviewPlan"]["guardedPreview"];
    assert_eq!(preview["destructive"], true);
    assert_eq!(preview["requiresApproval"], true);
    let text = preview["command"].as_str().unwrap();
    assert!(text.contains("--readonly"));
    assert!(text.contains("--dry-run"));
    let args = shlex::split(text).unwrap();
    let dry = mock_fabio(&server).args(&args[1..]).assert().success();
    assert_eq!(
        parse_json(&dry)["data"]["would_execute"],
        "dataflow upgrade-gen1"
    );
    assert_eq!(parse_json(&dry)["data"]["details"]["count"], 2);
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("private_name"));
    assert!(
        data["reviewPlan"]["blockers"]
            .as_array()
            .is_some_and(|rows| rows.len() >= 5)
    );
    reads(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attention_unavailable_future_and_malformed_reasons_are_not_apply_readiness() {
    for (status, reason_status, code, expected) in [
        (
            "NeedsAttention",
            "NeedsAttention",
            "HasIncrementalRefresh",
            "needs_action",
        ),
        (
            "UpgradeUnavailable",
            "UpgradeUnavailable",
            "HasUnresolvedConnections",
            "blocked",
        ),
        (
            "NeedsAttention",
            "NeedsAttention",
            "privateFutureReason",
            "unknown",
        ),
        (
            "NeedsAttention",
            "UpgradeUnavailable",
            "HasUnresolvedConnections",
            "unknown",
        ),
        ("Unknown", "Unknown", "AssessmentUnavailable", "unknown"),
    ] {
        let server = MockServer::start().await;
        let mut body = row(A, status);
        body["reasons"] = json!([{"status":reason_status,"code":code,"message":"https://example.org/private?credential=secret"}]);
        page(&server, json!({"value":[body]})).await;
        let output = command(&server)
            .args(["--id", A, "--strict", "--all", "--force"])
            .assert()
            .failure();
        let data = &parse_json(&output)["data"];
        assert_eq!(data["status"], expected);
        assert_eq!(data["reviewPlan"]["guardedPreview"], Value::Null);
        let text = String::from_utf8_lossy(&output.get_output().stdout);
        assert!(!text.contains("example.org"));
        assert!(!text.contains("privateFutureReason"));
        reads(&server, 1).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_ids_filter_output_not_service_assessment_and_missing_rows_stay_unknown() {
    let server = MockServer::start().await;
    page(&server, json!({"value":[row(B,"ReadyToMigrate")]})).await;
    let output = command(&server)
        .args(["--id", A, "--strict"])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "unknown");
    assert_eq!(
        data["checks"][0]["evidence"]["reason"],
        "not_in_gen1_assessment"
    );
    assert_eq!(data["checks"].as_array().unwrap().len(), 1);
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains(B));
    reads(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incomplete_duplicate_error_bearing_and_wrong_scope_evidence_suppresses_preview() {
    for body in [
        json!({"value":[row(A,"ReadyToMigrate")],"continuationToken":"more"}),
        json!({"value":[row(A,"ReadyToMigrate"),row(A,"ReadyToMigrate")]}),
        json!({"value":[row(A,"ReadyToMigrate")],"errorCode":"private"}),
        json!({"value":[row(A,"ReadyToMigrate")],"workspaceId":B}),
        json!({"value":[{"id":A,"displayName":"private","status":"ReadyToMigrate","workspaceId":B}]}),
        json!({"value":[row(A,"ReadyToMigrate")],"continuationUri":"https://example.org/private"}),
        json!({"wrong":[]}),
    ] {
        let server = MockServer::start().await;
        page(&server, body).await;
        let output = command(&server)
            .args(["--id", A, "--page-limit", "1", "--strict", "--all"])
            .assert()
            .failure();
        let data = &parse_json(&output)["data"];
        assert_eq!(data["status"], "unknown");
        assert_eq!(data["partial"], true);
        assert_eq!(data["reviewPlan"]["guardedPreview"], Value::Null);
        reads(&server, 1).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pagination_uses_only_fixed_workspace_route_and_opaque_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path(route())).and(query_param("pageSize", "30")).and(query_param_is_missing("continuationToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value":[row(B,"ReadyToMigrate")],"continuationToken":"opaque/?&private","continuationUri":"https://example.org/private"}))).expect(1).mount(&server).await;
    Mock::given(method("GET"))
        .and(path(route()))
        .and(query_param("continuationToken", "opaque/?&private"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"value":[row(A,"ReadyToMigrate")]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&server)
        .args(["--id", A, "--strict"])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["status"], "ready");
    assert_eq!(parse_json(&output)["data"]["assessmentScan"]["pages"], 2);
    reads(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn row_limit_auth_failure_and_total_deadline_do_not_create_eligibility() {
    for mode in 0..3 {
        let server = MockServer::start().await;
        let response = match mode {
            0 => ResponseTemplate::new(200)
                .set_body_json(json!({"value":[row(A,"ReadyToMigrate"),row(B,"ReadyToMigrate")]})),
            1 => ResponseTemplate::new(403)
                .set_body_json(json!({"errorCode":"Forbidden","message":"private"})),
            _ => ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(3))
                .set_body_json(json!({"value":[row(A,"ReadyToMigrate")]})),
        };
        Mock::given(method("GET"))
            .and(path(route()))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let output = command(&server)
            .args([
                "--id",
                A,
                "--assessment-limit",
                "1",
                "--timeout",
                "1",
                "--strict",
            ])
            .timeout(std::time::Duration::from_secs(8))
            .assert()
            .failure();
        assert_eq!(parse_json(&output)["data"]["status"], "unknown");
        assert_eq!(parse_json(&output)["data"]["reviewPlan"]["applied"], false);
        reads(&server, 1).await;
    }
}
