//! End-to-end integration tests for `fabio app-backend` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn app_backend_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["app-backend", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn app_backend_create_show_update_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("appbackend_test");
    let updated_name = format!("{name}_updated");
    let description = "created by e2e test";
    let updated_description = "updated by e2e test";

    // Create
    let assert = fabio()
        .args([
            "app-backend",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--description",
            description,
        ])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["description"], description);
    let id = data["id"].as_str().unwrap().to_string();

    // Show
    let assert = fabio()
        .args([
            "app-backend",
            "show",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"], id);
    assert_eq!(data["displayName"], name);

    // Update
    let assert = fabio()
        .args([
            "app-backend",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
            "--name",
            &updated_name,
            "--description",
            updated_description,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"], id);
    assert_eq!(data["displayName"], updated_name);
    assert_eq!(data["description"], updated_description);

    // Delete
    let assert = fabio()
        .args([
            "app-backend",
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
fn app_backend_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "app-backend",
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
fn app_backend_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "app-backend",
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
fn app_backend_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "app-backend",
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
    assert_eq!(json["data"]["would_execute"], "app-backend create");
}
