//! GET-only Spark/environment triage: unavailable evidence is never a reason
//! to launch, publish, cancel, or change pool policy.
mod common;

use common::{mock_fabio, parse_json};
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const ENV: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const ITEM: &str = "cccccccc-1111-2222-3333-444444444444";
const LIVY: &str = "dddddddd-1111-2222-3333-444444444444";
const ACTIVATION: &str = "eeeeeeee-1111-2222-3333-444444444444";
const APP: &str = "application_1_1";

fn diag(server: &MockServer) -> assert_cmd::Command {
    let mut command = mock_fabio(server);
    command.args([
        "--readonly",
        "environment",
        "diagnostics",
        "--workspace",
        WS,
    ]);
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
async fn environment(server: &MockServer) {
    get(
        server,
        &format!("/workspaces/{WS}/environments/{ENV}"),
        json!({"id":ENV,"workspaceId":WS,"type":"Environment",
        "displayName":"secret-name","properties":{"publishDetails":{"state":"Success"}}}),
    )
    .await;
    get(
        server,
        &format!("/workspaces/{WS}/environments/{ENV}/sparkcompute"),
        json!({"runtimeVersion":"2.0","driverCores":4,
        "sparkProperties":{"secret-token":"never-print-this"}}),
    )
    .await;
}
async fn empty_history(server: &MockServer) {
    get(
        server,
        &format!("/workspaces/{WS}/environments/{ENV}/poolActivations"),
        json!({"value":[]}),
    )
    .await;
}
fn session(state: &str) -> Value {
    json!({"livyId":LIVY,"state":state,"itemType":"notebook","item":{"referenceType":"ById","workspaceId":WS,"itemId":ITEM},
        "sparkApplicationId":APP,"attemptNumber":1,"livyName":"secret-name","cancellationReason":"never-print-this"})
}
async fn session_get(server: &MockServer, body: Value) {
    get(
        server,
        &format!("/workspaces/{WS}/notebooks/{ITEM}/livySessions/{LIVY}"),
        body,
    )
    .await;
}
async fn defaults(server: &MockServer) {
    get(
        server,
        &format!("/workspaces/{WS}/spark/settings"),
        json!({"environment":{"runtimeVersion":"2.0"},"secret":"never-print-this"}),
    )
    .await;
}
fn check<'a>(data: &'a Value, key: &str) -> &'a Value {
    data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["checkId"] == key)
        .unwrap()
}
async fn only_gets(server: &MockServer, count: usize) {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), count);
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET"
                && !request.url.path().contains("livyApi")
                && !request.url.path().starts_with("/admin")
                && !request.url.path().contains("publish"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_preview_validates_scope_and_incompatible_flags_before_reads() {
    let server = MockServer::start().await;
    let output = diag(&server)
        .args(["--id", ENV, "--dry-run"])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "environment diagnostics");
    assert_eq!(data["details"]["scope"]["maxLogicalReads"], 5);
    for args in [
        vec!["--id", "../escape"],
        vec!["--id", "00000000-0000-0000-0000-000000000000"],
        vec!["--item-id", ITEM],
        vec!["--id", ENV, "--include-logs"],
        vec!["--id", ENV, "--continuation-token", "page"],
    ] {
        diag(&server)
            .args(["--dry-run"])
            .args(args)
            .assert()
            .failure();
    }
    only_gets(&server, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_snapshot_is_configuration_only_and_does_not_expose_properties_or_names() {
    let server = MockServer::start().await;
    environment(&server).await;
    empty_history(&server).await;
    let output = diag(&server)
        .args(["--id", ENV, "--strict"])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "ready");
    assert_eq!(data["activationHistory"]["complete"], true);
    let text = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(!text.contains("secret-name"));
    assert!(!text.contains("never-print-this"));
    assert_eq!(data["scope"]["sessionEnvironmentBinding"], "not_verified");
    only_gets(&server, 3).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publish_unknown_and_staging_differences_are_review_signals_not_publish_requests() {
    let server = MockServer::start().await;
    environment(&server).await;
    empty_history(&server).await;
    get(
        &server,
        &format!("/workspaces/{WS}/environments/{ENV}/staging/sparkcompute"),
        json!({"runtimeVersion":"1.3"}),
    )
    .await;
    let output = diag(&server)
        .args(["--id", ENV, "--include-staging", "--strict"])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "needs_action");
    assert_eq!(
        check(data, "staging_comparison")["evidence"]["different"],
        true
    );
    only_gets(&server, 4).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unavailable_environment_does_not_probe_dependent_state_or_admin_permissions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/environments/{ENV}")))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(
                json!({"errorCode":"FeatureNotAvailable","message":"never-print-this"}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = diag(&server)
        .args(["--id", ENV, "--strict"])
        .assert()
        .failure();
    assert_eq!(parse_json(&output)["data"]["status"], "unknown");
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("never-print-this"));
    only_gets(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_activation_failed_state_and_resource_breakdown_are_separate_observations() {
    let server = MockServer::start().await;
    environment(&server).await;
    get(&server,&format!("/workspaces/{WS}/environments/{ENV}/poolActivations/{ACTIVATION}"),json!({"jobInstanceId":ACTIVATION,"state":"Failed",
        "name":"secret-name","sparkConfiguration":{"driverCores":4,"secret":"never-print-this"},"resourceStatusSummary":{
            "starting":0,"ready":0,"acquired":0,"stopping":0,"deactivated":0,"expired":0,"cancelled":0,"failed":2}})).await;
    let output = diag(&server)
        .args(["--id", ENV, "--activation-id", ACTIVATION, "--strict"])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(check(data, "pool_activation")["status"], "blocked");
    assert_eq!(
        check(data, "activation_resources")["evidence"]["counts"]["failed"],
        2
    );
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("secret-name"));
    only_gets(&server, 3).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_history_and_future_states_never_imply_absence_or_newest_activation() {
    let server = MockServer::start().await;
    environment(&server).await;
    get(&server,&format!("/workspaces/{WS}/environments/{ENV}/poolActivations"),json!({"value":[{"jobInstanceId":ACTIVATION,"state":"FutureState","name":"secret-name"}],"continuationToken":"next"})).await;
    let output = diag(&server)
        .args(["--id", ENV, "--page-limit", "1", "--all"])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "unknown");
    assert_eq!(data["activationHistory"]["complete"], false);
    assert_eq!(data["activationHistory"]["reason"], "page_limit");
    assert_eq!(data["activationId"], Value::Null);
    only_gets(&server, 3).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_only_mode_never_invents_an_environment_or_reads_raw_logs() {
    let server = MockServer::start().await;
    defaults(&server).await;
    session_get(&server, session("Succeeded")).await;
    let base = format!("/workspaces/{WS}/notebooks/{ITEM}/livySessions/{LIVY}/applications/{APP}");
    get(&server, &format!("{base}/advice"), json!([])).await;
    get(
        &server,
        &format!("{base}/resourceUsage"),
        json!({"coreEfficiency":0.7,"data":{"timestamps":[1,2]},"secret":"never-print-this"}),
    )
    .await;
    Mock::given(method("GET")).and(path(format!("{base}/logs"))).and(query_param("meta","true")).and(query_param("type","driver")).and(query_param("fileName","stderr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"containerLogMeta":{"fileName":"secret-path","length":10},"content":"never-print-this"}))).expect(1).mount(&server).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/workspaces/{WS}/notebooks/{ITEM}/livySessions/{LIVY}/applications/none/logs"
        )))
        .and(query_param("meta", "true"))
        .and(query_param("type", "livy"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"fileName":"secret-path"})))
        .expect(1)
        .mount(&server)
        .await;
    let output = diag(&server)
        .args([
            "--item-id",
            ITEM,
            "--item-type",
            "notebook",
            "--livy-id",
            LIVY,
            "--include-logs",
            "--strict",
        ])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["environmentId"], Value::Null);
    assert_eq!(data["scope"]["environmentSource"], "workspace_defaults");
    assert_eq!(check(data, "spark_session")["status"], "ready");
    assert_eq!(
        check(data, "spark_resource_usage")["evidence"]["sampleCount"],
        2
    );
    assert_eq!(
        check(data, "spark_resource_usage")["evidence"]["attemptPinned"],
        false
    );
    let text = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(!text.contains("secret-path"));
    assert!(!text.contains("never-print-this"));
    only_gets(&server, 6).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_scope_mismatch_or_unsafe_application_id_prevents_application_reads() {
    for wrong_scope in [true, false] {
        let server = MockServer::start().await;
        defaults(&server).await;
        let mut body = session("Succeeded");
        if wrong_scope {
            body["item"]["itemId"] = json!(ENV);
        } else {
            body["sparkApplicationId"] = json!("../private?token=secret");
        }
        session_get(&server, body).await;
        let output = diag(&server)
            .args([
                "--item-id",
                ITEM,
                "--item-type",
                "notebook",
                "--livy-id",
                LIVY,
            ])
            .assert()
            .success();
        assert_eq!(parse_json(&output)["data"]["status"], "unknown");
        only_gets(&server, 2).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn total_deadline_stops_further_probes_without_cancelling_a_workload() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/spark/settings")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_json(json!({})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = diag(&server)
        .args([
            "--item-id",
            ITEM,
            "--item-type",
            "notebook",
            "--livy-id",
            LIVY,
            "--timeout",
            "1",
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["status"], "unknown");
    only_gets(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_activation_state_is_encoded_and_conflicts_with_raw_filter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/workspaces/{WS}/environments/{ENV}/poolActivations"
        )))
        .and(query_param("filter", "state eq 'Failed'"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value":[]})))
        .expect(1)
        .mount(&server)
        .await;
    mock_fabio(&server)
        .args([
            "--readonly",
            "environment",
            "list-pool-activations",
            "--workspace",
            WS,
            "--id",
            ENV,
            "--state",
            "Failed",
        ])
        .assert()
        .success();
    mock_fabio(&server)
        .args([
            "environment",
            "list-pool-activations",
            "--workspace",
            WS,
            "--id",
            ENV,
            "--state",
            "Failed",
            "--filter",
            "state eq 'Succeeded'",
        ])
        .assert()
        .failure();
    mock_fabio(&server)
        .args([
            "environment",
            "list-pool-activations",
            "--workspace",
            WS,
            "--id",
            ENV,
            "--state",
            "idle",
        ])
        .assert()
        .failure();
    only_gets(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_session_filter_is_client_side_and_preserves_a_more_pages_token() {
    let server = MockServer::start().await;
    get(&server,&format!("/workspaces/{WS}/spark/livySessions"),json!({"value":[{"livyId":LIVY,"state":"Succeeded"},{"livyId":ITEM,"state":"Failed"}],"continuationToken":"more"})).await;
    let output = mock_fabio(&server)
        .args([
            "spark",
            "list-livy-sessions",
            "--workspace",
            WS,
            "--state",
            "Failed",
        ])
        .assert()
        .success();
    let data = parse_json(&output);
    assert_eq!(data["count"], 1);
    assert_eq!(data["continuationToken"], "more");
    assert_eq!(data["data"][0]["state"], "Failed");
    only_gets(&server, 1).await;
}

fn events(output: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(output)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_follow_stops_on_failure_and_does_not_expose_raw_activation_data() {
    let server = MockServer::start().await;
    get(&server,&format!("/workspaces/{WS}/environments/{ENV}/poolActivations/{ACTIVATION}"),json!({"jobInstanceId":ACTIVATION,"state":"Failed","name":"secret-name","cancellationReason":"never-print-this"})).await;
    let output = mock_fabio(&server)
        .args([
            "--readonly",
            "environment",
            "show-pool-activation",
            "--workspace",
            WS,
            "--id",
            ENV,
            "--activation-id",
            ACTIVATION,
            "--follow",
            "--max-duration",
            "3",
        ])
        .assert()
        .success();
    let rows = events(&output.get_output().stdout);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["rows"][0]["state"], "Failed");
    assert_eq!(rows[1]["status"], "follow_complete");
    assert_eq!(rows[1]["reason"], "complete");
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("secret-name"));
    only_gets(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn follow_bounds_an_inflight_read_and_row_limit_without_fetching_extra_data() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/workspaces/{WS}/environments/{ENV}/poolActivations/{ACTIVATION}"
        )))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(4))
                .set_body_json(json!({"jobInstanceId":ACTIVATION,"state":"Succeeded"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .timeout(Duration::from_secs(8))
        .args([
            "environment",
            "show-pool-activation",
            "--workspace",
            WS,
            "--id",
            ENV,
            "--activation-id",
            ACTIVATION,
            "--follow",
            "--max-duration",
            "1",
        ])
        .assert()
        .success();
    let rows = events(&output.get_output().stdout);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["rows_emitted"], 0);
    assert_eq!(rows[0]["reason"], "max_duration");
    only_gets(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn follow_option_and_scope_validation_precede_offline_preview() {
    let server = MockServer::start().await;
    for args in [
        vec!["--max-duration", "1"],
        vec!["--follow", "--query", "state"],
        vec!["--follow", "--output", "table"],
    ] {
        mock_fabio(&server)
            .args([
                "environment",
                "show-pool-activation",
                "--workspace",
                WS,
                "--id",
                ENV,
                "--activation-id",
                ACTIVATION,
                "--dry-run",
            ])
            .args(args)
            .assert()
            .failure();
    }
    let output = mock_fabio(&server)
        .args([
            "environment",
            "show-pool-activation",
            "--workspace",
            WS,
            "--id",
            ENV,
            "--activation-id",
            ACTIVATION,
            "--follow",
            "--dry-run",
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["dry_run"], true);
    only_gets(&server, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_follow_row_limit_does_not_fetch_and_driver_metadata_requires_an_encoded_file_name() {
    let server = MockServer::start().await;
    let output = mock_fabio(&server)
        .args([
            "environment",
            "show-pool-activation",
            "--workspace",
            WS,
            "--id",
            ENV,
            "--activation-id",
            ACTIVATION,
            "--follow",
            "--limit",
            "0",
        ])
        .assert()
        .success();
    let rows = events(&output.get_output().stdout);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["reason"], "limit");
    assert_eq!(rows[0]["rows_emitted"], 0);
    mock_fabio(&server)
        .args([
            "spark",
            "get-logs",
            "--workspace",
            WS,
            "--item-type",
            "notebook",
            "--item-id",
            ITEM,
            "--livy-id",
            LIVY,
            "--app-id",
            APP,
            "--type",
            "driver",
            "--meta",
        ])
        .assert()
        .failure();
    only_gets(&server, 0).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/workspaces/{WS}/notebooks/{ITEM}/livySessions/{LIVY}/applications/{APP}/logs"
        )))
        .and(query_param("meta", "true"))
        .and(query_param("fileName", "stderr&meta=false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"fileName":"fixture"})))
        .expect(1)
        .mount(&server)
        .await;
    mock_fabio(&server)
        .args([
            "spark",
            "get-logs",
            "--workspace",
            WS,
            "--item-type",
            "notebook",
            "--item-id",
            ITEM,
            "--livy-id",
            LIVY,
            "--app-id",
            APP,
            "--type",
            "driver",
            "--meta",
            "--file-name",
            "stderr&meta=false",
        ])
        .assert()
        .success();
    only_gets(&server, 1).await;
}
