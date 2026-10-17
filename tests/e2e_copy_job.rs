//! End-to-end integration tests for `fabio copy-job` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
    let cfg = TestConfig::from_env();
    let copy_job_id =
        std::env::var("FABIO_TEST_COPY_JOB_ID").expect("FABIO_TEST_COPY_JOB_ID required");

    // Trigger on demand and wait for the instance to reach a terminal state.
    let assert = fabio()
        .args([
            "copy-job",
            "run",
            "--workspace",
            &cfg.source_workspace,
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
    assert!(data.get("jobInstanceId").is_some());
}

#[test]
#[serial]
fn copy_job_run_uses_dedicated_execute_endpoint() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let workspace = "aaaaaaaa-1111-2222-3333-444444444444";
    let copy_job = "bbbbbbbb-1111-2222-3333-444444444444";
    let instance = "cccccccc-1111-2222-3333-444444444444";
    let route = format!("/workspaces/{workspace}/copyJobs/{copy_job}/jobs/execute/instances");
    let (server_uri, server) = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(&route))
            .respond_with(
                ResponseTemplate::new(202)
                    .insert_header(
                        "Location",
                        format!(
                            "{}/workspaces/{workspace}/items/{copy_job}/jobs/instances/{instance}",
                            server.uri()
                        ),
                    )
                    .insert_header("Retry-After", "60"),
            )
            .expect(1)
            .mount(&server)
            .await;
        (server.uri(), server)
    });

    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server_uri)
        .args([
            "copy-job",
            "run",
            "--workspace",
            workspace,
            "--id",
            copy_job,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    assert_eq!(data["jobInstanceId"], instance);
    assert_eq!(data["status"], "started");
    let requests = runtime
        .block_on(server.received_requests())
        .expect("request recording enabled");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body.len(), 0);
}

#[test]
#[serial]
fn copy_job_list_instances_uses_paginated_endpoint() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let workspace = "aaaaaaaa-1111-2222-3333-444444444444";
    let copy_job = "bbbbbbbb-1111-2222-3333-444444444444";
    let route = format!("/workspaces/{workspace}/copyJobs/{copy_job}/jobs/execute/instances");
    let (server_uri, _server) = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(&route))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [{
                    "id": "cccccccc-1111-2222-3333-444444444444",
                    "itemId": copy_job,
                    "jobType": "execute",
                    "invokeType": "Manual",
                    "status": "Completed",
                    "rootActivityId": "dddddddd-1111-2222-3333-444444444444",
                    "startTimeUtc": "2024-06-22T06:35:00.7812154",
                    "endTimeUtc": "2024-06-22T06:35:00.8033333",
                    "failureReason": null
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        (server.uri(), server)
    });

    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server_uri)
        .args([
            "copy-job",
            "list-instances",
            "--workspace",
            workspace,
            "--id",
            copy_job,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(json["count"], 1);
    assert_eq!(json["data"][0]["status"], "Completed");
    assert_eq!(json["data"][0]["invokeType"], "Manual");
}
