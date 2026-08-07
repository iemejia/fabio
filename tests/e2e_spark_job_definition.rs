//! End-to-end integration tests for `fabio spark-job-definition` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn spark_job_definition_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "spark-job-definition",
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
fn spark_job_definition_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("sjd_test");

    // Create
    let assert = fabio()
        .args([
            "spark-job-definition",
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
            "spark-job-definition",
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
fn spark_job_definition_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "spark-job-definition",
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
fn spark_job_definition_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "spark-job-definition",
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
fn spark_job_definition_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "spark-job-definition",
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
    assert_eq!(json["data"]["would_execute"], "spark-job-definition create");
}

// ---------------------------------------------------------------------------
// Livy sessions (spec parity with notebook/lakehouse/spark)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant with a Spark job definition"]
#[serial]
fn spark_job_definition_list_livy_sessions() {
    let cfg = TestConfig::from_env();
    let Ok(sjd_id) = std::env::var("FABIO_TEST_SPARK_JOB_DEFINITION_ID") else {
        return; // skip when not configured
    };
    let assert = fabio()
        .args([
            "spark-job-definition",
            "list-livy-sessions",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &sjd_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(extract_data(&json).is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn spark_job_definition_get_livy_session_not_found() {
    let cfg = TestConfig::from_env();
    let Ok(sjd_id) = std::env::var("FABIO_TEST_SPARK_JOB_DEFINITION_ID") else {
        return;
    };
    fabio()
        .args([
            "spark-job-definition",
            "get-livy-session",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &sjd_id,
            "--livy-id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .failure();
}
