//! End-to-end integration tests for `fabio kql-dashboard` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kql_dashboard_create_does_not_require_location_header() {
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let workspace = "aaaaaaaa-1111-2222-3333-444444444444";
    let id = "bbbbbbbb-1111-2222-3333-444444444444";
    let body =
        serde_json::json!({"id": id, "displayName": "Mock dashboard", "type": "KQLDashboard"});
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{workspace}/kqlDashboards")))
        .and(body_json(
            serde_json::json!({"displayName": "Mock dashboard"}),
        ))
        // The October spec removed Location from the synchronous 201 response.
        .respond_with(ResponseTemplate::new(201).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;
    let output = common::mock_fabio(&server)
        .args([
            "kql-dashboard",
            "create",
            "--workspace",
            workspace,
            "--name",
            "Mock dashboard",
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"], body);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn kql_dashboard_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "kql-dashboard",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn kql_dashboard_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("kd_test");

    // Create
    let assert = fabio()
        .args([
            "kql-dashboard",
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
            "kql-dashboard",
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
fn kql_dashboard_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "kql-dashboard",
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
fn kql_dashboard_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "kql-dashboard",
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
fn kql_dashboard_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "kql-dashboard",
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
    assert_eq!(json["data"]["would_execute"], "kql-dashboard create");
}
