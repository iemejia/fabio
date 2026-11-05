//! Read-only BI readiness, using the real Power BI query transport and parser.
mod common;

use common::{mock_fabio, parse_json};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const MODEL: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const REPORT: &str = "cccccccc-1111-2222-3333-444444444444";
const REFRESH: &str = "dddddddd-1111-2222-3333-444444444444";
const OTHER: &str = "eeeeeeee-1111-2222-3333-444444444444";

fn command(server: &MockServer) -> assert_cmd::Command {
    let mut command = mock_fabio(server);
    command.env("FABIO_POWERBI_ENDPOINT", server.uri());
    command
}
fn readiness(server: &MockServer) -> assert_cmd::Command {
    let mut command = command(server);
    command.args([
        "--readonly",
        "semantic-model",
        "readiness",
        "--workspace",
        WS,
        "--id",
        MODEL,
    ]);
    command
}
fn query_body(dax: &str) -> Value {
    json!({"queries":[{"query":dax}],"serializerSettings":{"includeNulls":true}})
}
fn rows(rows: Value) -> Value {
    let mut table = serde_json::Map::new();
    table.insert("rows".into(), rows);
    json!({"results":[{"tables":[table]}]})
}
async fn metadata(server: &MockServer) {
    Mock::given(method("GET")).and(path(format!("/workspaces/{WS}/semanticModels/{MODEL}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":MODEL,"workspaceId":WS,"type":"SemanticModel","displayName":"private-name"})))
        .mount(server).await;
}
async fn schema(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path(format!(
            "/groups/{WS}/datasets/{MODEL}/executeQueries"
        )))
        .and(body_json(query_body(
            "EVALUATE SELECTCOLUMNS(TOPN(201, INFO.VIEW.TABLES(), [ID], ASC), \"Name\", [Name])",
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(rows(json!([{"[Name]":"Sales"}]))))
        .mount(server)
        .await;
}
fn check<'a>(data: &'a Value, name: &str) -> &'a Value {
    data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["checkId"] == name)
        .unwrap()
}
async fn only_observations(server: &MockServer) {
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET"
                || (request.method.as_str() == "POST"
                    && request.url.path()
                        == format!("/groups/{WS}/datasets/{MODEL}/executeQueries")))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn readonly_dax_works_but_refresh_takeover_and_report_creation_remain_guarded() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/groups/{WS}/datasets/{MODEL}/executeQueries"
        )))
        .and(body_json(query_body("EVALUATE ROW(\"x\", 1)")))
        .respond_with(ResponseTemplate::new(200).set_body_json(rows(json!([{"[x]":1}]))))
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&server)
        .args([
            "--readonly",
            "semantic-model",
            "query",
            "--workspace",
            WS,
            "--id",
            MODEL,
            "--dax",
            "EVALUATE ROW(\"x\", 1)",
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"][0]["[x]"], 1);
    for args in [
        vec![
            "semantic-model",
            "refresh",
            "--workspace",
            WS,
            "--id",
            MODEL,
        ],
        vec![
            "semantic-model",
            "takeover",
            "--workspace",
            WS,
            "--id",
            MODEL,
        ],
        vec![
            "report",
            "create",
            "--workspace",
            WS,
            "--name",
            "fixture",
            "--semantic-model",
            MODEL,
        ],
    ] {
        let output = command(&server)
            .arg("--readonly")
            .args(args)
            .assert()
            .failure();
        assert!(String::from_utf8_lossy(&output.get_output().stderr).contains("READONLY_MODE"));
    }
    only_observations(&server).await;
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_and_error_bearing_dax_never_render_success_or_partial_rows() {
    for response in [
        json!({}),
        json!({"results":[{"tables":[{}]}]}),
        json!({"results":[{"tables":[{"rows":[{"[x]":1}],"error":{"message":"private-error"}}]}]}),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&server)
            .await;
        let output = command(&server)
            .args([
                "--readonly",
                "semantic-model",
                "query",
                "--workspace",
                WS,
                "--id",
                MODEL,
                "--dax",
                "EVALUATE Sales",
            ])
            .assert()
            .failure();
        assert_eq!(output.get_output().stdout.len(), 0);
        assert!(!String::from_utf8_lossy(&output.get_output().stderr).contains("private-error"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn previews_validate_scope_without_network_and_cannot_turn_post_readonly_into_a_mutation() {
    let server = MockServer::start().await;
    let output = readiness(&server)
        .args([
            "--expected-table",
            "Sales",
            "--probe-data",
            "--refresh-id",
            REFRESH,
            "--report-id",
            REPORT,
            "--dry-run",
        ])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "semantic-model readiness");
    assert_eq!(data["details"]["scope"]["mutates"], false);
    for args in [
        vec!["--expected-table", ""],
        vec!["--expected-table", "Sales", "--expected-table", "sales"],
        vec!["--probe-data"],
        vec!["--refresh-id", "../refreshes"],
        vec!["--report-id", "*"],
        vec!["--interval", "1"],
        vec!["--continuation-token", "next"],
    ] {
        readiness(&server)
            .args(args)
            .arg("--dry-run")
            .assert()
            .failure();
    }
    command(&server)
        .args([
            "--readonly",
            "semantic-model",
            "query",
            "--workspace",
            WS,
            "--id",
            "../refreshes",
            "--dax",
            "EVALUATE Sales",
        ])
        .assert()
        .failure();
    command(&server)
        .args([
            "--readonly",
            "semantic-model",
            "query",
            "--workspace",
            WS,
            "--id",
            MODEL,
            "--dax",
            "",
        ])
        .arg("--dry-run")
        .assert()
        .failure();
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requested_checks_validate_counts_exact_refresh_and_report_binding_without_raw_metadata() {
    let server = MockServer::start().await;
    metadata(&server).await;
    schema(&server).await;
    Mock::given(method("POST"))
        .and(body_json(query_body(
            "EVALUATE ROW(\"t0\", COALESCE(COUNTROWS('Sales'), 0))",
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(rows(json!([{"[t0]":2}]))))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path(format!("/groups/{WS}/datasets/{MODEL}/refreshes/{REFRESH}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"requestId":REFRESH,"status":"Completed","serviceExceptionJson":"private-error"}))).expect(1).mount(&server).await;
    Mock::given(method("GET")).and(path(format!("/groups/{WS}/reports/{REPORT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":REPORT,"datasetId":MODEL,"datasetWorkspaceId":WS,"webUrl":"https://example.org/private"}))).expect(1).mount(&server).await;
    let output = readiness(&server)
        .args([
            "--expected-table",
            "Sales",
            "--probe-data",
            "--min-rows",
            "1",
            "--refresh-id",
            REFRESH,
            "--report-id",
            REPORT,
            "--strict",
        ])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "ready");
    assert_eq!(
        check(data, "data_query")["evidence"]["rowCountsByExpectedTableIndex"],
        json!([2])
    );
    assert_eq!(
        check(data, "selected_refresh")["evidence"]["correlation"],
        "exact_request_endpoint"
    );
    let text = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(!text.contains("private"));
    only_observations(&server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unrequested_evidence_is_not_checked_not_silently_inferred() {
    let server = MockServer::start().await;
    metadata(&server).await;
    schema(&server).await;
    let output = readiness(&server)
        .args(["--expected-table", "Sales", "--strict"])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "ready");
    assert!(
        data["notChecked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row.as_str().unwrap().contains("refresh completion"))
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_refresh_stops_wait_without_resubmitting_even_when_queries_work() {
    let server = MockServer::start().await;
    metadata(&server).await;
    schema(&server).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/groups/{WS}/datasets/{MODEL}/refreshes/{REFRESH}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status":"Failed"})))
        .expect(1)
        .mount(&server)
        .await;
    let output = readiness(&server)
        .args([
            "--expected-table",
            "Sales",
            "--probe-data",
            "--refresh-id",
            REFRESH,
            "--wait",
            "--strict",
        ])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "blocked");
    assert_eq!(data["stopReason"], "blocked");
    assert_eq!(data["attempts"], 1);
    only_observations(&server).await;
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn waiting_observes_one_exact_refresh_transition_to_completed_without_submitting() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let server = MockServer::start().await;
    metadata(&server).await;
    schema(&server).await;
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    Mock::given(method("GET"))
        .and(path(format!(
            "/groups/{WS}/datasets/{MODEL}/refreshes/{REFRESH}"
        )))
        .respond_with(move |_: &wiremock::Request| {
            let state = if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                "InProgress"
            } else {
                "Completed"
            };
            ResponseTemplate::new(200).set_body_json(json!({"requestId":REFRESH,"status":state}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let output = readiness(&server)
        .args([
            "--refresh-id",
            REFRESH,
            "--wait",
            "--max-attempts",
            "2",
            "--interval",
            "1",
            "--timeout",
            "10",
            "--strict",
        ])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "ready");
    assert_eq!(data["attempts"], 2);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    only_observations(&server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wait_rechecks_exact_refresh_and_is_bounded_for_unavailable_or_mismatched_evidence() {
    let server = MockServer::start().await;
    metadata(&server).await;
    schema(&server).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/groups/{WS}/datasets/{MODEL}/refreshes/{REFRESH}"
        )))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"requestId":OTHER,"status":"Completed"})),
        )
        .expect(2)
        .mount(&server)
        .await;
    let output = readiness(&server)
        .args([
            "--refresh-id",
            REFRESH,
            "--wait",
            "--interval",
            "1",
            "--max-attempts",
            "2",
            "--timeout",
            "5",
            "--strict",
        ])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "unknown");
    assert_eq!(data["stopReason"], "attempt_limit");
    assert_eq!(data["attempts"], 2);
    only_observations(&server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn absent_schema_skips_data_probe_and_does_not_read_sources_or_change_modes() {
    let server = MockServer::start().await;
    metadata(&server).await;
    schema(&server).await;
    let output = readiness(&server)
        .args(["--expected-table", "Missing", "--probe-data", "--strict"])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(check(data, "model_schema")["status"], "needs_action");
    assert_eq!(check(data, "data_query")["status"], "unknown");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    only_observations(&server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_workspace_binding_and_unavailable_metadata_remain_scoped() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/semanticModels/{MODEL}")))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"errorCode":"Forbidden","message":"private"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/groups/{OTHER}/reports/{REPORT}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id":REPORT,"datasetId":MODEL,"datasetWorkspaceId":WS})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = readiness(&server)
        .args([
            "--report-id",
            REPORT,
            "--report-workspace",
            OTHER,
            "--strict",
        ])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "unknown");
    assert_eq!(check(data, "report_binding")["status"], "ready");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    only_observations(&server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deadline_includes_an_inflight_query_and_all_later_probes() {
    let server = MockServer::start().await;
    metadata(&server).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/groups/{WS}/datasets/{MODEL}/refreshes/{REFRESH}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status":"InProgress"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(3))
                .set_body_json(rows(json!([]))),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = readiness(&server)
        .timeout(std::time::Duration::from_secs(8))
        .args([
            "--expected-table",
            "Sales",
            "--probe-data",
            "--refresh-id",
            REFRESH,
            "--report-id",
            REPORT,
            "--wait",
            "--timeout",
            "1",
            "--strict",
        ])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "needs_action");
    assert_eq!(check(data, "model_schema")["status"], "unknown");
    assert_eq!(data["partial"], true);
    assert_eq!(data["stopReason"], "deadline");
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
    only_observations(&server).await;
}
