//! End-to-end integration tests for `fabio org-app` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json, unique_name};

const FAKE_WS: &str = "00000000-0000-0000-0000-000000000000";
const FAKE_ID: &str = "00000000-0000-0000-0000-000000000001";

// ─── Offline (dry-run) tests ──────────────────────────────────────────────────

#[test]
fn org_app_dry_run_create() {
    let assert = fabio()
        .args([
            "--dry-run",
            "org-app",
            "create",
            "--workspace",
            FAKE_WS,
            "--name",
            "test-org-app",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "org-app create");
}

#[test]
fn org_app_dry_run_delete() {
    let assert = fabio()
        .args([
            "--dry-run",
            "org-app",
            "delete",
            "--workspace",
            FAKE_WS,
            "--id",
            FAKE_ID,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "org-app delete");
}

#[test]
fn org_app_update_requires_field() {
    let assert = fabio()
        .args(["org-app", "update", "--workspace", FAKE_WS, "--id", FAKE_ID])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
}

#[test]
fn org_app_update_definition_requires_file_or_content() {
    let assert = fabio()
        .args([
            "org-app",
            "update-definition",
            "--workspace",
            FAKE_WS,
            "--id",
            FAKE_ID,
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
}

// ─── Live tests ───────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
fn org_app_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args(["org-app", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn org_app_create_update_delete() {
    let cfg = TestConfig::from_env();
    let name = unique_name("orgapp_test");

    let assert = fabio()
        .args([
            "org-app",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let id = data["id"].as_str().unwrap().to_string();

    let new_name = unique_name("orgapp_upd");
    fabio()
        .args([
            "org-app",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
            "--name",
            &new_name,
        ])
        .assert()
        .success();

    fabio()
        .args([
            "org-app",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
}
