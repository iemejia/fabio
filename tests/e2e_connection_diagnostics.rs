//! Scoped diagnostics never validate stored credentials or change ownership.
mod common;

use common::{mock_fabio, parse_json};
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const CONNECTION: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const ITEM: &str = "cccccccc-1111-2222-3333-444444444444";
const OTHER: &str = "dddddddd-1111-2222-3333-444444444444";

fn metadata(role: &str, credential: &str) -> Value {
    json!({"id":CONNECTION,"myRole":role,"connectivityType":"ShareableCloud",
        "displayName":"untrusted name", "credentialDetails":{"credentialType":credential,
            "skipTestConnection":false,"password":"never-print-this"},
        "connectionDetails":{"path":"https://example.org/?sig=never-print-this"},"extra":"never-print-this"})
}

async fn get(server: &MockServer, route: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

async fn only_gets(server: &MockServer, count: usize) {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), count);
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
    assert!(
        requests
            .iter()
            .all(|request| !request.url.path().starts_with("/admin")
                && request.url.path() != "/connections")
    );
}

fn check<'a>(checks: &'a Value, key: &str) -> &'a Value {
    checks
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["checkId"] == key)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dry_run_is_offline_and_all_scope_validation_precedes_preview() {
    let server = MockServer::start().await;
    let result = mock_fabio(&server)
        .args([
            "--readonly",
            "connection",
            "diagnostics",
            "--id",
            CONNECTION,
            "--dry-run",
        ])
        .assert()
        .success();
    let data = &parse_json(&result)["data"];
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "connection diagnostics");
    assert_eq!(data["details"]["scope"]["maxLogicalReads"], 1);
    for args in [
        vec!["--id", "../other"],
        vec!["--id", "00000000-0000-0000-0000-000000000000"],
        vec!["--item", ITEM],
        vec!["--id", CONNECTION, "--item", ITEM, "--workspace", WS],
        vec!["--id", CONNECTION, "--workspace", "name"],
        vec!["--id", CONNECTION, "--continuation-token", "page"],
    ] {
        mock_fabio(&server)
            .args(["connection", "diagnostics", "--dry-run"])
            .args(args)
            .assert()
            .failure();
    }
    only_gets(&server, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn usable_role_is_configuration_only_and_never_leaks_credential_or_endpoint_fields() {
    let server = MockServer::start().await;
    get(
        &server,
        &format!("/connections/{CONNECTION}"),
        metadata("Owner", "Anonymous"),
    )
    .await;
    let result = mock_fabio(&server)
        .args([
            "--readonly",
            "connection",
            "diagnostics",
            "--id",
            CONNECTION,
            "--strict",
        ])
        .assert()
        .success();
    let data = &parse_json(&result)["data"];
    assert_eq!(data["status"], "ready");
    assert_eq!(data["connections"][0]["credentialHealth"], "not_checked");
    assert_eq!(
        check(&data["connections"][0]["checks"], "effective_role")["status"],
        "ready"
    );
    let stdout = String::from_utf8_lossy(&result.get_output().stdout);
    assert!(!stdout.contains("never-print-this"));
    assert!(!stdout.contains("untrusted name"));
    assert!(!stdout.contains("test-connection"));
    only_gets(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_owner_is_not_usable_ownership_and_strict_preserves_the_report() {
    let server = MockServer::start().await;
    get(
        &server,
        &format!("/connections/{CONNECTION}"),
        metadata("PendingOwner", "OAuth2"),
    )
    .await;
    let result = mock_fabio(&server)
        .args(["connection", "diagnostics", "--id", CONNECTION, "--strict"])
        .assert()
        .failure();
    let data = &parse_json(&result)["data"];
    assert_eq!(data["status"], "needs_action");
    let steps = &check(&data["connections"][0]["checks"], "effective_role")["nextSteps"];
    assert!(
        steps
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step["kind"] != "remediation")
    );
    assert!(!String::from_utf8_lossy(&result.get_output().stdout).contains("--credentials"));
    only_gets(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forbidden_and_not_found_are_unknown_without_admin_fallback_or_raw_error_body() {
    for status in [403, 404] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/connections/{CONNECTION}")))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(json!({"errorCode":"EntityNotFound",
                "message":"never-print-this","requestId":"fixture-request"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let result = mock_fabio(&server)
            .args(["connection", "diagnostics", "--id", CONNECTION])
            .assert()
            .success();
        let data = &parse_json(&result)["data"];
        assert_eq!(data["status"], "unknown");
        assert_eq!(
            check(&data["connections"][0]["checks"], "connection_access")["evidence"]["error"]["requestId"],
            "fixture-request"
        );
        assert!(!String::from_utf8_lossy(&result.get_output().stdout).contains("never-print-this"));
        only_gets(&server, 1).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_or_future_role_and_mismatched_metadata_do_not_become_ready() {
    for body in [
        json!({"id":CONNECTION,"credentialDetails":{"credentialType":"Anonymous"}}),
        metadata("FutureRole", "Anonymous"),
        json!({"id":OTHER,"myRole":"Owner"}),
        metadata("Owner", "FutureCredential"),
    ] {
        let server = MockServer::start().await;
        get(&server, &format!("/connections/{CONNECTION}"), body).await;
        let result = mock_fabio(&server)
            .args(["connection", "diagnostics", "--id", CONNECTION])
            .assert()
            .success();
        assert_eq!(parse_json(&result)["data"]["status"], "unknown");
        only_gets(&server, 1).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_identity_configuration_is_separate_from_source_authorization() {
    let server = MockServer::start().await;
    get(
        &server,
        &format!("/workspaces/{WS}"),
        json!({"id":WS,"workspaceIdentity":{"applicationId":OTHER,"servicePrincipalId":OTHER}}),
    )
    .await;
    get(
        &server,
        &format!("/connections/{CONNECTION}"),
        metadata("User", "WorkspaceIdentity"),
    )
    .await;
    let result = mock_fabio(&server)
        .args([
            "--readonly",
            "connection",
            "diagnostics",
            "--id",
            CONNECTION,
            "--workspace",
            WS,
        ])
        .assert()
        .success();
    let data = &parse_json(&result)["data"];
    let identity = check(&data["connections"][0]["checks"], "workspace_identity");
    assert_eq!(identity["status"], "ready");
    assert_eq!(identity["evidence"]["authorization"], "not_checked");
    assert_eq!(data["scope"]["workspaceContextIsNotBindingProof"], true);
    only_gets(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_identity_without_context_does_not_guess_a_workspace() {
    let server = MockServer::start().await;
    get(
        &server,
        &format!("/connections/{CONNECTION}"),
        metadata("Owner", "WorkspaceIdentity"),
    )
    .await;
    let result = mock_fabio(&server)
        .args(["connection", "diagnostics", "--id", CONNECTION])
        .assert()
        .success();
    let data = &parse_json(&result)["data"];
    assert_eq!(data["status"], "unknown");
    assert_eq!(
        check(&data["connections"][0]["checks"], "workspace_identity")["evidence"]["missingInputs"],
        json!(["workspace"])
    );
    only_gets(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn item_references_deduplicate_explicit_ids_and_do_not_invent_automatic_or_unbound_connections()
 {
    let server = MockServer::start().await;
    get(&server, &format!("/workspaces/{WS}"), json!({"id":WS})).await;
    let row = json!({"id":CONNECTION,"connectivityType":"ShareableCloud"});
    get(&server,&format!("/workspaces/{WS}/items/{ITEM}/connections"),json!({"value":[row,row,
        {"connectivityType":"Automatic","connectionDetails":{"path":"never-print-this"}},{"connectivityType":"None"}]})).await;
    get(
        &server,
        &format!("/connections/{CONNECTION}"),
        metadata("Owner", "Anonymous"),
    )
    .await;
    let result = mock_fabio(&server)
        .args([
            "--readonly",
            "connection",
            "diagnostics",
            "--workspace",
            WS,
            "--item",
            ITEM,
        ])
        .assert()
        .success();
    let data = &parse_json(&result)["data"];
    assert_eq!(data["status"], "needs_action");
    assert_eq!(data["references"]["complete"], true);
    assert_eq!(data["connections"].as_array().unwrap().len(), 3);
    assert_eq!(data["connections"][1]["connectionId"], Value::Null);
    assert_eq!(data["connections"][1]["checks"][0]["status"], "unknown");
    assert_eq!(
        data["connections"][2]["checks"][0]["status"],
        "needs_action"
    );
    only_gets(&server, 3).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn limits_are_not_overridden_by_all_and_empty_complete_references_are_not_workload_readiness()
{
    let server = MockServer::start().await;
    get(&server, &format!("/workspaces/{WS}"), json!({"id":WS})).await;
    get(
        &server,
        &format!("/workspaces/{WS}/items/{ITEM}/connections"),
        json!({"value":[],"continuationToken":"more"}),
    )
    .await;
    let result = mock_fabio(&server)
        .args([
            "connection",
            "diagnostics",
            "--workspace",
            WS,
            "--item",
            ITEM,
            "--all",
            "--page-limit",
            "1",
        ])
        .assert()
        .success();
    let data = &parse_json(&result)["data"];
    assert_eq!(data["status"], "unknown");
    assert_eq!(data["references"]["reason"], "page_limit");
    only_gets(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn item_scan_follows_only_encoded_tokens_on_the_original_scope_and_has_a_complete_empty_case()
{
    let server = MockServer::start().await;
    get(&server, &format!("/workspaces/{WS}"), json!({"id":WS})).await;
    Mock::given(method("GET")).and(path(format!("/workspaces/{WS}/items/{ITEM}/connections")))
        .and(query_param_is_missing("continuationToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value":[],"continuationToken":"opaque/?&value","continuationUri":"https://example.org/private"})))
        .expect(1).mount(&server).await;
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/items/{ITEM}/connections")))
        .and(query_param("continuationToken", "opaque/?&value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value":[]})))
        .expect(1)
        .mount(&server)
        .await;
    let result = mock_fabio(&server)
        .args([
            "connection",
            "diagnostics",
            "--workspace",
            WS,
            "--item",
            ITEM,
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&result)["data"]["references"]["complete"], true);
    assert_eq!(parse_json(&result)["data"]["connections"], json!([]));
    only_gets(&server, 3).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deadline_prevents_further_probes_after_a_slow_workspace_read() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(2))
                .set_body_json(json!({"id":WS})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let result = mock_fabio(&server)
        .args([
            "connection",
            "diagnostics",
            "--workspace",
            WS,
            "--id",
            CONNECTION,
            "--timeout",
            "1",
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&result)["data"]["status"], "unknown");
    only_gets(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovery_and_published_output_keep_the_configuration_only_contract() {
    let server = MockServer::start().await;
    let schema = mock_fabio(&server)
        .args(["context", "agent", "--group", "connection"])
        .assert()
        .success();
    let data = parse_json(&schema)["data"].clone();
    let text = serde_json::to_string(&data).unwrap();
    assert!(text.contains("diagnostics"));
    let example = mock_fabio(&server)
        .args(["context", "examples", "connection", "diagnostics"])
        .assert()
        .success();
    let sample = parse_json(&example)["data"]["response"]["data"].clone();
    assert_eq!(sample["scope"]["mutates"], false);
    assert_eq!(sample["connections"][0]["credentialHealth"], "not_checked");
    let help = mock_fabio(&server)
        .args(["context", "best-practices", "connection-diagnostics"])
        .assert()
        .success();
    assert!(String::from_utf8_lossy(&help.get_output().stdout).contains("PendingOwner"));
    only_gets(&server, 0).await;
}
