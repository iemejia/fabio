//! End-to-end integration tests for `fabio data-build-tool-job` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json, unique_name};

const FAKE_WS: &str = "00000000-0000-0000-0000-000000000000";
const FAKE_ID: &str = "00000000-0000-0000-0000-000000000001";

// ─── Offline (dry-run) tests ──────────────────────────────────────────────────

#[test]
fn data_build_tool_job_dry_run_create() {
    let assert = fabio()
        .args([
            "--dry-run",
            "data-build-tool-job",
            "create",
            "--workspace",
            FAKE_WS,
            "--name",
            "test-dbt-job",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "data-build-tool-job create");
}

#[test]
fn data_build_tool_job_dry_run_delete() {
    let assert = fabio()
        .args([
            "--dry-run",
            "data-build-tool-job",
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
    assert_eq!(data["would_execute"], "data-build-tool-job delete");
}

#[test]
fn data_build_tool_job_dry_run_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "data-build-tool-job",
            "run",
            "--workspace",
            FAKE_WS,
            "--id",
            FAKE_ID,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "data-build-tool-job run");
}

#[test]
fn data_build_tool_job_update_requires_field() {
    let assert = fabio()
        .args([
            "data-build-tool-job",
            "update",
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

#[test]
fn data_build_tool_job_update_definition_requires_file_or_content() {
    let assert = fabio()
        .args([
            "data-build-tool-job",
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
fn data_build_tool_job_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "data-build-tool-job",
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
fn data_build_tool_job_create_update_delete() {
    let cfg = TestConfig::from_env();
    let name = unique_name("dbt_test");

    let assert = fabio()
        .args([
            "data-build-tool-job",
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

    // Update
    let new_name = unique_name("dbt_upd");
    fabio()
        .args([
            "data-build-tool-job",
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

    // Delete
    fabio()
        .args([
            "data-build-tool-job",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
}
