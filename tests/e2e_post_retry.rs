//! Hermetic POST retry policy and acknowledgement-evidence regression tests.
mod common;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use common::mock_fabio;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const ITEM: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const OP: &str = "cccccccc-1111-2222-3333-444444444444";

fn command(server: &MockServer, home: &tempfile::TempDir) -> assert_cmd::Command {
    let mut command = inspect(server, home);
    command.arg("--quiet");
    command
}

fn inspect(server: &MockServer, home: &tempfile::TempDir) -> assert_cmd::Command {
    let mut command = mock_fabio(server);
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("FABIO_POWERBI_ENDPOINT", server.uri())
        .env("FABIO_NO_VERSION_CHECK", "1");
    command
}

fn error(output: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice::<Value>(&output.get_output().stderr).unwrap()["error"].clone()
}

async fn first_failure_then_success(
    server: &MockServer,
    initial: ResponseTemplate,
    success: Value,
) {
    let count = Arc::new(AtomicUsize::new(0));
    Mock::given(method("POST"))
        .respond_with(move |_: &wiremock::Request| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                initial.clone()
            } else {
                ResponseTemplate::new(200).set_body_json(&success)
            }
        })
        .mount(server)
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ambiguous_server_responses_never_authorize_or_duplicate_a_create() {
    for status in [302, 408, 500, 502, 503, 504, 600] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        // The first request could have created work despite a gateway failure.
        // A replay would misleadingly succeed here; the CLI must not send it.
        first_failure_then_success(&server, ResponseTemplate::new(status).set_body_json(json!({
            "error":{"message":"unavailable","isRetriable":true,"parameters":[{"name":"mock","value":"preserved"}]}
        })), json!({"id":WS})).await;
        let output = command(&server, &home)
            .args(["workspace", "create", "--name", "Mock"])
            .assert()
            .failure();
        let error = error(&output);
        assert_eq!(error["retriable"], false);
        assert_eq!(error["hintType"], "semantic_correction");
        assert_eq!(error["submission"]["state"], "unknown");
        assert_eq!(error["submission"]["httpStatus"], status);
        assert_eq!(error["submission"]["serverRetriable"], true);
        assert_eq!(error["parameters"][0]["value"], "preserved");
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn success_headers_survive_malformed_bodies_without_inventing_completion_or_identity() {
    for status in [200, 201, 202] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("x-ms-operation-id", OP)
                    .insert_header("x-ms-request-id", WS)
                    .set_body_string("not-json"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let output = command(&server, &home)
            .args(["workspace", "create", "--name", "Mock"])
            .assert()
            .failure();
        let error = error(&output);
        assert_eq!(
            error["submission"]["state"],
            if status == 202 {
                "accepted"
            } else {
                "acknowledged"
            }
        );
        assert_eq!(error["submission"]["operationId"], OP);
        assert_eq!(error["submission"]["httpStatus"], status);
        assert_eq!(error["retriable"], false);
        assert!(error["verifyAfter"].is_null());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn known_auth_and_throttle_rejections_remain_bounded_and_retryable() {
    for status in [401, 429, 430] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        first_failure_then_success(
            &server,
            ResponseTemplate::new(status).insert_header("retry-after", "0"),
            json!({"id":WS}),
        )
        .await;
        command(&server, &home)
            .args(["workspace", "create", "--name", "Mock"])
            .assert()
            .success();
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn contradictory_acknowledgement_markers_prevent_retries_even_on_rejection_statuses() {
    for status in [401, 429, 430] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        first_failure_then_success(
            &server,
            ResponseTemplate::new(status)
                .insert_header("retry-after", "0")
                .insert_header("x-ms-operation-id", OP),
            json!({"id":WS}),
        )
        .await;
        let output = command(&server, &home)
            .args(["workspace", "create", "--name", "Mock"])
            .assert()
            .failure();
        let error = error(&output);
        assert_eq!(error["hintType"], "semantic_correction");
        assert_eq!(error["retriable"], false);
        assert_eq!(error["submission"]["state"], "unknown");
        assert_eq!(error["submission"]["operationId"], OP);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_dry_run_and_readonly_guards_precede_all_network_requests() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    command(&server, &home)
        .args(["workspace", "create", "--name", "Mock", "--dry-run"])
        .assert()
        .success();
    let output = command(&server, &home)
        .args(["workspace", "create", "--name", "Mock", "--readonly"])
        .assert()
        .failure();
    assert_eq!(error(&output)["code"], "READONLY_MODE");
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_only_post_search_and_definition_retrieval_keep_gateway_retries() {
    for (args, body) in [
        (
            vec!["catalog", "search", "--search", "mock"],
            json!({"value":[]}),
        ),
        (
            vec!["map", "get-definition", "--workspace", WS, "--id", ITEM],
            json!({"definition":{"parts":[]}}),
        ),
    ] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        first_failure_then_success(&server, ResponseTemplate::new(503), body).await;
        command(&server, &home)
            .arg("--readonly")
            .args(args)
            .assert()
            .success();
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_only_definition_classification_cannot_turn_user_ids_into_mutating_routes() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    for (workspace, item) in [
        (WS.to_string(), format!("{ITEM}/updateDefinition?ignored=")),
        (WS.to_string(), format!("{ITEM}%2fupdateDefinition")),
        (format!("{WS}/items?ignored="), ITEM.to_string()),
    ] {
        let output = command(&server, &home)
            .args([
                "--readonly",
                "map",
                "get-definition",
                "--workspace",
                &workspace,
                "--id",
                &item,
            ])
            .assert()
            .failure();
        assert_eq!(error(&output)["code"], "INVALID_INPUT");
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_job_and_refresh_errors_preserve_requested_route_scope_without_replay() {
    for args in [
        vec!["data-pipeline", "run", "--workspace", WS, "--id", ITEM],
        vec![
            "monitoring",
            "enable-data-collection",
            "--workspace",
            WS,
            "--id",
            ITEM,
        ],
        vec!["semantic-model", "refresh", "--workspace", WS, "--id", ITEM],
    ] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(503)
                    .set_body_json(json!({"error":{"message":"unavailable","isRetriable":true}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let output = command(&server, &home).args(args).assert().failure();
        let error = error(&output);
        assert_eq!(error["submission"]["state"], "unknown");
        assert_eq!(error["submission"]["workspaceId"], WS);
        assert_eq!(error["submission"]["targetId"], ITEM);
        assert_eq!(error["retriable"], false);
        assert_eq!(error["hintType"], "semantic_correction");
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_job_without_identity_is_not_an_invitation_to_trigger_another_job() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(202).insert_header("x-ms-operation-id", OP))
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&server, &home)
        .args(["data-pipeline", "run", "--workspace", WS, "--id", ITEM])
        .assert()
        .failure();
    let error = error(&output);
    assert_eq!(error["submission"]["state"], "accepted");
    assert_eq!(error["submission"]["operationId"], OP);
    assert_eq!(error["retriable"], false);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn safe_gets_still_retry_transient_gateway_failures() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}")))
        .respond_with(move |_: &wiremock::Request| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200).set_body_json(json!({"id":WS}))
            }
        })
        .mount(&server)
        .await;
    let output = command(&server, &home)
        .args(["--readonly", "workspace", "show", "--id", WS])
        .assert()
        .success();
    // --quiet suppresses successful data, not transport execution.
    assert_eq!(output.get_output().stdout.len(), 0);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_level_named_rejection_loop_cannot_replay_ambiguous_definition_submission() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WS}/mirroredDatabases")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":ITEM})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/workspaces/{WS}/mirroredDatabases/{ITEM}/updateDefinition"
        )))
        .respond_with(
            ResponseTemplate::new(503).set_body_json(json!({"message":"MirroredDatabaseNotReady"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&server, &home)
        .args([
            "mirrored-database",
            "create",
            "--workspace",
            WS,
            "--name",
            "Mock",
            "--open-mirroring",
        ])
        .assert()
        .failure();
    assert_eq!(error(&output)["submission"]["state"], "unknown");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_lro_observation_failure_retains_the_original_ack_without_retry_advice() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(202)
                .insert_header("x-ms-operation-id", OP)
                .insert_header(
                    "location",
                    format!("https://example.org/operations/{OP}?private=secret"),
                ),
        )
        .expect(1)
        .mount(&server)
        .await;
    // The returned origin fails validation before any GET. We must not suggest
    // repeating the accepted creation just because observation cannot continue.
    let output = command(&server, &home)
        .args(["map", "create", "--workspace", WS, "--name", "Mock"])
        .assert()
        .failure();
    let error = error(&output);
    assert_eq!(error["submission"]["state"], "accepted");
    assert_eq!(error["submission"]["operationId"], OP);
    assert_eq!(error["submission"]["workspaceId"], WS);
    assert_eq!(error["retriable"], false);
    assert_eq!(error["hintType"], "semantic_correction");
    assert!(!error["submission"].to_string().contains("secret"));
    assert!(!error["hint"].as_str().unwrap().contains("secret"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnect_after_request_arrival_is_unknown_not_retry_safe() {
    use std::io::Read as _;
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let peer = std::thread::spawn(move || {
        let started = std::time::Instant::now();
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                        .unwrap();
                    let mut bytes = [0; 4096];
                    let read = stream.read(&mut bytes).unwrap();
                    assert!(read > 0);
                    assert!(bytes[..read].starts_with(b"POST "));
                    stream.shutdown(std::net::Shutdown::Both).unwrap();
                    return;
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && started.elapsed() < std::time::Duration::from_secs(5) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("mock peer did not receive request: {}", error.kind()),
            }
        }
    });
    let output = command(&server, &home)
        .env(
            "FABIO_FABRIC_API_ENDPOINT",
            format!("{}://{address}", "http"),
        )
        .args(["workspace", "create", "--name", "Mock"])
        .assert()
        .failure();
    peer.join().unwrap();
    let error = error(&output);
    assert_eq!(error["code"], "NETWORK_ERROR");
    assert_eq!(error["submission"]["state"], "unknown");
    assert!(error["submission"]["httpStatus"].is_null());
    assert_eq!(error["retriable"], false);
    assert_eq!(error["hintType"], "semantic_correction");
    assert!(
        !error["message"]
            .as_str()
            .unwrap()
            .contains(&address.to_string())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_policy_and_error_shape_are_discoverable_without_network_requests() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let output = inspect(&server, &home)
        .args([
            "--readonly",
            "context",
            "best-practices",
            "submission-retries",
        ])
        .assert()
        .success();
    assert_eq!(
        common::parse_json(&output)["data"]["topic"],
        "submission-retries"
    );
    let output = inspect(&server, &home)
        .args(["--readonly", "context", "agent"])
        .assert()
        .success();
    assert!(
        common::parse_json(&output)["data"]["output_contract"]["mutation_retry"]
            .as_str()
            .unwrap()
            .contains("retriable:false")
    );
    let output = inspect(&server, &home)
        .args(["--readonly", "context", "examples", "rest", "call"])
        .assert()
        .success();
    assert_eq!(
        common::parse_json(&output)["data"]["mutation_error_response"]["error"]["submission"]["state"],
        "unknown"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
