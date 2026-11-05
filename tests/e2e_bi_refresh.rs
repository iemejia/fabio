//! Submission receipts never select latest history, follow returned URLs, or
//! turn a malformed acknowledgement into a reason to submit again.
mod common;

use common::{mock_fabio, parse_json};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const MODEL: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const REFRESH: &str = "cccccccc-1111-2222-3333-444444444444";
const OTHER: &str = "dddddddd-1111-2222-3333-444444444444";

fn command(server: &MockServer) -> assert_cmd::Command {
    let mut command = mock_fabio(server);
    command.env("FABIO_POWERBI_ENDPOINT", server.uri());
    command.args([
        "semantic-model",
        "refresh",
        "--workspace",
        WS,
        "--id",
        MODEL,
    ]);
    command
}
fn collection() -> String {
    format!("/groups/{WS}/datasets/{MODEL}/refreshes")
}
async fn respond(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path(collection()))
        .and(body_json(json!({"type":"Full"})))
        .respond_with(response)
        .expect(1)
        .mount(server)
        .await;
}
async fn submitted_once(server: &MockServer) {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "POST");
    assert_eq!(requests[0].url.path(), collection());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_receipt_exposes_an_exact_scope_identity_without_polling_or_following() {
    for absolute in [false, true] {
        let server = MockServer::start().await;
        let location = format!(
            "{}{}/{REFRESH}",
            if absolute {
                server.uri()
            } else {
                String::new()
            },
            collection()
        );
        respond(
            &server,
            ResponseTemplate::new(202)
                .insert_header("location", location.as_str())
                .insert_header("x-ms-request-id", REFRESH),
        )
        .await;
        let output = command(&server).assert().success();
        let data = &parse_json(&output)["data"];
        assert_eq!(data["status"], "refresh_triggered");
        assert_eq!(data["refreshId"], REFRESH);
        assert_eq!(data["submission"]["refreshId"], REFRESH);
        assert_eq!(data["submission"]["state"], "accepted");
        assert_eq!(data["submission"]["correlationStatus"], "verified");
        assert_eq!(data["submission"]["completionStatus"], "not_checked");
        submitted_once(&server).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trace_only_missing_or_mismatched_metadata_keeps_the_accepted_receipt_uncorrelated() {
    for (status, location, trace, expected) in [
        (202, None, None, "unavailable"),
        (202, None, Some(REFRESH), "unavailable"),
        (200, None, None, "unavailable"),
        (
            202,
            Some(format!("{}/{REFRESH}", collection())),
            Some(OTHER),
            "conflicting",
        ),
        (
            202,
            Some(format!(
                "/groups/{OTHER}/datasets/{MODEL}/refreshes/{REFRESH}"
            )),
            Some(REFRESH),
            "invalid",
        ),
        (
            202,
            Some(format!("https://example.org/private/{REFRESH}")),
            Some(REFRESH),
            "invalid",
        ),
        (
            202,
            Some(format!("{}/{REFRESH}?private=secret", collection())),
            Some(REFRESH),
            "invalid",
        ),
    ] {
        let server = MockServer::start().await;
        let mut response = ResponseTemplate::new(status);
        if let Some(location) = location {
            response = response.insert_header("location", location.as_str());
        }
        if let Some(trace) = trace {
            response = response.insert_header("x-ms-request-id", trace);
        }
        respond(&server, response).await;
        let output = command(&server).assert().success();
        let data = &parse_json(&output)["data"];
        assert_eq!(data["refreshId"], Value::Null);
        assert_eq!(data["submission"]["correlationStatus"], expected);
        assert_eq!(data["submission"]["completionStatus"], "not_checked");
        assert_ne!(data["submission"]["warnings"].as_array().unwrap().len(), 0);
        assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("secret"));
        assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("example.org"));
        submitted_once(&server).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_success_body_preserves_acknowledgement_and_refresh_identity() {
    let server = MockServer::start().await;
    let location = format!("{}/{REFRESH}", collection());
    respond(
        &server,
        ResponseTemplate::new(202)
            .insert_header("location", location.as_str())
            .set_body_string("private: not JSON"),
    )
    .await;
    let output = command(&server).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["refreshId"], REFRESH);
    assert_eq!(data["submission"]["state"], "accepted");
    assert_eq!(data["submission"]["responseBodyStatus"], "unreadable");
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("private"));
    submitted_once(&server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn error_bearing_success_is_unknown_not_retryable_success_or_a_new_submission() {
    let server = MockServer::start().await;
    let location = format!("{}/{REFRESH}", collection());
    respond(
        &server,
        ResponseTemplate::new(202)
            .insert_header("location", location.as_str())
            .set_body_json(json!({"error":{"message":"private"}})),
    )
    .await;
    let output = command(&server).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "refresh_outcome_unknown");
    assert_eq!(data["submission"]["state"], "unknown");
    assert_eq!(data["refreshId"], Value::Null);
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("private"));
    submitted_once(&server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_server_failure_is_non_retriable_and_does_not_read_history_or_resubmit() {
    let server = MockServer::start().await;
    respond(
        &server,
        ResponseTemplate::new(503).set_body_json(
            json!({"errorCode":"ServiceUnavailable","message":"unavailable","isRetriable":true}),
        ),
    )
    .await;
    let output = command(&server).assert().failure();
    let text = String::from_utf8_lossy(&output.get_output().stderr);
    let error = text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .next_back()
        .unwrap();
    assert_eq!(error["error"]["retriable"], false);
    assert!(
        error["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("Do not retry automatically")
    );
    submitted_once(&server).await;
    // A real local connection failure must override the formatter's default
    // NetworkError => retry_safe inference as well as its retriable flag.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = format!("{}://{}", "http", listener.local_addr().unwrap());
    drop(listener);
    let output = command(&server)
        .env("FABIO_POWERBI_ENDPOINT", endpoint)
        .assert()
        .failure();
    let text = String::from_utf8_lossy(&output.get_output().stderr);
    let error = text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .next_back()
        .unwrap();
    assert_eq!(error["error"]["code"], "NETWORK_ERROR");
    assert_eq!(error["error"]["retriable"], false);
    assert_eq!(error["error"]["hintType"], "semantic_correction");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dry_run_scope_validation_and_readonly_guard_precede_any_mutating_request() {
    let server = MockServer::start().await;
    let output = command(&server).arg("--dry-run").assert().success();
    assert_eq!(parse_json(&output)["data"]["dry_run"], true);
    let output = command(&server).arg("--readonly").assert().failure();
    assert!(String::from_utf8_lossy(&output.get_output().stderr).contains("READONLY_MODE"));
    for invalid in ["../refreshes", "00000000-0000-0000-0000-000000000000"] {
        let mut command = mock_fabio(&server);
        command
            .env("FABIO_POWERBI_ENDPOINT", server.uri())
            .args([
                "semantic-model",
                "refresh",
                "--workspace",
                WS,
                "--id",
                invalid,
                "--dry-run",
            ])
            .assert()
            .failure();
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
