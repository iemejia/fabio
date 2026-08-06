//! E2E integration tests for the `fabio spark` command group.
//!
//! Tests workspace-level Spark settings and custom pool operations.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
fn spark_get_settings() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "spark",
            "get-settings",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    // Should return an object with Spark settings data
    assert!(json.get("data").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn spark_update_settings_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "spark",
            "update-settings",
            "--workspace",
            &cfg.source_workspace,
            "--settings",
            r#"{"automaticLog":{"enabled":true}}"#,
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn spark_list_pools() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args(["spark", "list-pools", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&output);
    // Should return a list (possibly empty)
    assert!(json.get("data").is_some());
    assert!(json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn spark_create_pool_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "spark",
            "create-pool",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-pool",
            "--node-size",
            "Small",
            "--max-node-count",
            "3",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn spark_delete_pool_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "spark",
            "delete-pool",
            "--workspace",
            &cfg.source_workspace,
            "--pool-id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

// ─── Workspace runtime version (typed --runtime-version) ─────────────────────

#[test]
fn spark_update_settings_runtime_version_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "spark",
            "update-settings",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--runtime-version",
            "2.0",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "spark update-settings");
    assert_eq!(data["details"]["runtimeVersion"], "2.0");
}

#[test]
fn spark_update_settings_requires_input() {
    // Neither --settings nor --runtime-version provided.
    let assert = fabio()
        .args([
            "spark",
            "update-settings",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("--runtime-version"));
}

#[test]
fn spark_update_settings_conflicts_with_runtime_version() {
    let assert = fabio()
        .args([
            "spark",
            "update-settings",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--settings",
            "{}",
            "--runtime-version",
            "2.0",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("cannot be used with"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn spark_update_settings_runtime_version_lifecycle() {
    let cfg = TestConfig::from_env();

    // Capture the current workspace default runtime version so we can restore it.
    let get = fabio()
        .args([
            "spark",
            "get-settings",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let json = parse_json(&get);
    let original = extract_data(&json)["environment"]["runtimeVersion"]
        .as_str()
        .unwrap_or("1.3")
        .to_string();

    // Switch to Runtime 2.0 (read-merge-write); other settings must be preserved.
    let set = fabio()
        .args([
            "spark",
            "update-settings",
            "--workspace",
            &cfg.source_workspace,
            "--runtime-version",
            "2.0",
        ])
        .assert()
        .success();
    let json = parse_json(&set);
    let data = extract_data(&json);
    assert_eq!(data["environment"]["runtimeVersion"], "2.0");
    // A sibling top-level setting should still be present (not wiped by the PATCH).
    assert!(
        data.get("automaticLog").is_some() || data.get("job").is_some(),
        "expected sibling settings to be preserved"
    );

    // Restore the original runtime version.
    let restore = fabio()
        .args([
            "spark",
            "update-settings",
            "--workspace",
            &cfg.source_workspace,
            "--runtime-version",
            &original,
        ])
        .assert()
        .success();
    let json = parse_json(&restore);
    assert_eq!(
        extract_data(&json)["environment"]["runtimeVersion"],
        original
    );
}

// ---------------------------------------------------------------------------
// Spark monitoring APIs (advice / resource usage / logs)
// ---------------------------------------------------------------------------

#[test]
fn spark_get_advice_requires_flags() {
    // Missing required --item-type/--item-id/--livy-id/--app-id -> parse failure.
    fabio()
        .args([
            "spark",
            "get-advice",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

#[test]
fn spark_get_advice_rejects_bad_item_type() {
    // --item-type is restricted to notebook/spark-job-definition/lakehouse.
    fabio()
        .args([
            "spark",
            "get-advice",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--item-type",
            "warehouse",
            "--item-id",
            "x",
            "--livy-id",
            "y",
            "--app-id",
            "z",
        ])
        .assert()
        .failure();
}

#[test]
fn spark_get_logs_driver_requires_app_id() {
    // Driver logs need --app-id; the error is a clean INVALID_INPUT.
    let assert = fabio()
        .args([
            "spark",
            "get-logs",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--item-type",
            "notebook",
            "--item-id",
            "x",
            "--livy-id",
            "y",
            "--type",
            "driver",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant with a completed Spark application"]
#[serial]
fn spark_monitoring_lifecycle() {
    // Requires FABIO_TEST_SPARK_ITEM_ID / _LIVY_ID / _APP_ID for a notebook that
    // has a completed Spark application.
    let cfg = TestConfig::from_env();
    let (Ok(item_id), Ok(livy_id), Ok(app_id)) = (
        std::env::var("FABIO_TEST_SPARK_ITEM_ID"),
        std::env::var("FABIO_TEST_SPARK_LIVY_ID"),
        std::env::var("FABIO_TEST_SPARK_APP_ID"),
    ) else {
        return; // skip when not configured
    };

    for cmd in ["get-advice", "get-resource-usage"] {
        fabio()
            .args([
                "spark",
                cmd,
                "--workspace",
                &cfg.source_workspace,
                "--item-type",
                "notebook",
                "--item-id",
                &item_id,
                "--livy-id",
                &livy_id,
                "--app-id",
                &app_id,
            ])
            .timeout(std::time::Duration::from_mins(1))
            .assert()
            .success();
    }

    // Livy log metadata (app id is "none" for livy).
    fabio()
        .args([
            "spark",
            "get-logs",
            "--workspace",
            &cfg.source_workspace,
            "--item-type",
            "notebook",
            "--item-id",
            &item_id,
            "--livy-id",
            &livy_id,
            "--type",
            "livy",
            "--meta",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}
