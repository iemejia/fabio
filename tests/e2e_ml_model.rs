//! End-to-end integration tests for `fabio ml-model` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ml_model_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["ml-model", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ml_model_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("mlm_test");

    // Create
    let assert = fabio()
        .args([
            "ml-model",
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
            "ml-model",
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
fn ml_model_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "ml-model",
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
fn ml_model_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "ml-model",
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
fn ml_model_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "ml-model",
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
    assert_eq!(json["data"]["would_execute"], "ml-model create");
}

// ---------------------------------------------------------------------------
// MLflow model registry versions
// ---------------------------------------------------------------------------

#[test]
fn ml_model_get_registry_version_requires_version() {
    // --version is required for get-registry-version.
    fabio()
        .args([
            "ml-model",
            "get-registry-version",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant with a registered ML model version"]
#[serial]
fn ml_model_registry_versions_lifecycle() {
    let cfg = TestConfig::from_env();
    let Ok(model_id) = std::env::var("FABIO_TEST_ML_MODEL_ID") else {
        return; // skip when not configured
    };

    // list-registry-versions -> proper list envelope
    let assert = fabio()
        .args([
            "ml-model",
            "list-registry-versions",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &model_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(extract_data(&json).is_array());

    // get-registry-version --version 1 — assert the `{model_version:{…}}` envelope
    // is UNWRAPPED so `version` is at the top level of `data` (regression for 0c8a1c4).
    let out = fabio()
        .args([
            "ml-model",
            "get-registry-version",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &model_id,
            "--version",
            "1",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
    assert!(
        v["data"].get("model_version").is_none(),
        "model_version envelope must be unwrapped, got: {}",
        v["data"]
    );
    assert_eq!(
        v["data"]["version"].as_str(),
        Some("1"),
        "version field must be at the top level of data"
    );
}
