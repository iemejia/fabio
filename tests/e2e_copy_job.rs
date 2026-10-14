//! End-to-end integration tests for `fabio copy-job` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const WORKSPACE: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const COPY_JOB: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const INSTANCE: &str = "cccccccc-1111-2222-3333-444444444444";

#[test]
fn copy_job_run_retries_with_empty_body_and_returns_location_without_waiting() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        let attempts = AtomicUsize::new(0);
        let accepted = ResponseTemplate::new(202)
            .insert_header(
                "Location",
                format!(
                    "{}/workspaces/{WORKSPACE}/items/{COPY_JOB}/jobs/instances/{INSTANCE}",
                    server.uri()
                ),
            )
            .insert_header("Retry-After", "60");
        Mock::given(method("POST"))
            .and(path(format!(
                "/workspaces/{WORKSPACE}/copyJobs/{COPY_JOB}/jobs/execute/instances"
            )))
            .and(|request: &Request| request.body.is_empty())
            .and(header("content-length", "0"))
            .respond_with(move |_: &Request| {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(401)
                } else {
                    accepted.clone()
                }
            })
            .expect(2)
            .mount(&server)
            .await;
        server
    });
    let output = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server.uri())
        .args([
            "copy-job",
            "run",
            "--workspace",
            WORKSPACE,
            "--id",
            COPY_JOB,
        ])
        .timeout(std::time::Duration::from_secs(5))
        .assert()
        .success();
    let data = extract_data(&parse_json(&output)).clone();
    assert_eq!(data["jobInstanceId"], INSTANCE);
    assert_eq!(data["status"], "started");
}

#[test]
fn copy_job_wait_respects_retry_after_before_first_poll() {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let triggered_at = Arc::new(Mutex::new(None));
    let polled_at = Arc::new(Mutex::new(None));
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        let trigger_time = Arc::clone(&triggered_at);
        let response = ResponseTemplate::new(202)
            .insert_header(
                "Location",
                format!(
                    "{}/workspaces/{WORKSPACE}/items/{COPY_JOB}/jobs/instances/{INSTANCE}",
                    server.uri()
                ),
            )
            .insert_header("Retry-After", "1");
        Mock::given(method("POST"))
            .and(path(format!(
                "/workspaces/{WORKSPACE}/copyJobs/{COPY_JOB}/jobs/execute/instances"
            )))
            .and(|request: &Request| request.body.is_empty())
            .and(header("content-length", "0"))
            .respond_with(move |_: &Request| {
                *trigger_time.lock().unwrap() = Some(Instant::now());
                response.clone()
            })
            .expect(1)
            .mount(&server)
            .await;
        let poll_time = Arc::clone(&polled_at);
        Mock::given(method("GET"))
            .and(path(format!(
                "/workspaces/{WORKSPACE}/items/{COPY_JOB}/jobs/instances/{INSTANCE}"
            )))
            .respond_with(move |_: &Request| {
                *poll_time.lock().unwrap() = Some(Instant::now());
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": INSTANCE, "status": "Completed"}))
            })
            .expect(1)
            .mount(&server)
            .await;
        server
    });
    let output = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server.uri())
        .args([
            "copy-job",
            "run",
            "--workspace",
            WORKSPACE,
            "--id",
            COPY_JOB,
            "--wait",
            "--timeout",
            "10",
        ])
        .timeout(Duration::from_secs(15))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&output))["status"], "Completed");
    let elapsed = polled_at
        .lock()
        .unwrap()
        .unwrap()
        .duration_since(triggered_at.lock().unwrap().unwrap());
    assert!(
        elapsed >= Duration::from_secs(1),
        "polled after {elapsed:?}"
    );
}

#[test]
fn copy_job_timeout_never_polls_before_retry_after_and_preserves_cancellation() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for cancel in [false, true] {
        let server = runtime.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(format!("/workspaces/{WORKSPACE}/copyJobs/{COPY_JOB}/jobs/execute/instances")))
                .respond_with(ResponseTemplate::new(202)
                    .insert_header("Location", format!("{}/workspaces/{WORKSPACE}/items/{COPY_JOB}/jobs/instances/{INSTANCE}", server.uri()))
                    .insert_header("Retry-After", "60"))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "Completed"})))
                .expect(0)
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path(format!("/workspaces/{WORKSPACE}/items/{COPY_JOB}/jobs/instances/{INSTANCE}/cancel")))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
                .expect(u64::from(cancel))
                .mount(&server)
                .await;
            server
        });
        let mut command = fabio();
        command
            .env("FABIO_ACCESS_TOKEN", "fake-test-token")
            .env("FABIO_FABRIC_API_ENDPOINT", server.uri())
            .args([
                "copy-job",
                "run",
                "--workspace",
                WORKSPACE,
                "--id",
                COPY_JOB,
                "--wait",
                "--timeout",
                "1",
            ])
            .timeout(std::time::Duration::from_secs(5));
        if cancel {
            command.arg("--cancel-on-timeout");
        }
        let output = command.assert().failure();
        let stderr = String::from_utf8_lossy(&output.get_output().stderr);
        let error: serde_json::Value = stderr
            .lines()
            .find_map(|line| serde_json::from_str(line).ok())
            .unwrap_or_else(|| panic!("expected a structured error: {stderr}"));
        assert_eq!(error["error"]["code"], "TIMEOUT");
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn copy_job_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["copy-job", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn copy_job_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("cj_test");

    // Create
    let assert = fabio()
        .args([
            "copy-job",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let id = data["id"].as_str().unwrap().to_string();

    // Delete
    let assert = fabio()
        .args([
            "copy-job",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn copy_job_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "copy-job",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn copy_job_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "copy-job",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn copy_job_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "copy-job",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-dry-run",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "copy-job create");
}

#[test]
fn copy_job_dry_run_reset_all() {
    let assert = fabio()
        .args([
            "--dry-run",
            "copy-job",
            "reset",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--all-entities",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "copy-job reset");
}

#[test]
fn copy_job_reset_requires_all_or_entity_ids() {
    fabio()
        .args([
            "copy-job",
            "reset",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn copy_job_dry_run_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "copy-job",
            "run",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "copy-job run");
}

#[test]
#[ignore = "requires live Fabric tenant with a copy job"]
#[serial]
fn copy_job_run_and_wait() {
    let workspace =
        std::env::var("FABIO_TEST_SOURCE_WORKSPACE").expect("FABIO_TEST_SOURCE_WORKSPACE required");
    let copy_job_id =
        std::env::var("FABIO_TEST_COPY_JOB_ID").expect("FABIO_TEST_COPY_JOB_ID required");

    // Trigger on demand and wait for the instance to reach a terminal state.
    let assert = fabio()
        .args([
            "copy-job",
            "run",
            "--workspace",
            &workspace,
            "--id",
            &copy_job_id,
            "--wait",
            "--timeout",
            "180",
        ])
        .timeout(std::time::Duration::from_secs(200))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "Completed");
    assert_eq!(data["itemId"], copy_job_id);
    assert!(uuid::Uuid::parse_str(data["id"].as_str().unwrap()).is_ok());
}

#[test]
#[ignore = "requires live Fabric tenant with a copy job"]
#[serial]
fn copy_job_list_runs_returns_execute_instances() {
    let workspace =
        std::env::var("FABIO_TEST_SOURCE_WORKSPACE").expect("FABIO_TEST_SOURCE_WORKSPACE required");
    let copy_job_id =
        std::env::var("FABIO_TEST_COPY_JOB_ID").expect("FABIO_TEST_COPY_JOB_ID required");
    let assert = fabio()
        .args([
            "--readonly",
            "copy-job",
            "list-runs",
            "--workspace",
            &workspace,
            "--id",
            &copy_job_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let instances = extract_data(&json).as_array().unwrap();
    for instance in instances {
        assert_eq!(instance["itemId"], copy_job_id);
        assert!(uuid::Uuid::parse_str(instance["id"].as_str().unwrap()).is_ok());
        assert!(instance["status"].is_string());
    }
}
