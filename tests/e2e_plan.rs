//! End-to-end integration tests for `fabio plan` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn plan_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["plan", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn plan_create_show_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("plan_crud");

    // Create
    let assert = fabio()
        .args([
            "plan",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--description",
            "Test plan for e2e",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["type"], "Plan");
    let plan_id = data["id"].as_str().unwrap().to_string();

    // Show
    let assert = fabio()
        .args([
            "plan",
            "show",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &plan_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["description"], "Test plan for e2e");

    // Delete
    let assert = fabio()
        .args([
            "plan",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &plan_id,
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
fn plan_update_name_and_description() {
    let cfg = TestConfig::from_env();
    let original = common::unique_name("plan_upd_o");
    let updated = common::unique_name("plan_upd_n");

    // Create
    let assert = fabio()
        .args([
            "plan",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &original,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let plan_id = data["id"].as_str().unwrap().to_string();

    // Update
    let assert = fabio()
        .args([
            "plan",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &plan_id,
            "--name",
            &updated,
            "--description",
            "Updated description",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], updated);
    assert_eq!(data["description"], "Updated description");

    // Cleanup
    fabio()
        .args([
            "plan",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &plan_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn plan_get_definition_returns_infobridge_json() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("plan_def");

    // Create
    let assert = fabio()
        .args([
            "plan",
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
    let plan_id = data["id"].as_str().unwrap().to_string();

    // Update definition with a minimal payload so getDefinition returns parts
    let dir = tempfile::tempdir().unwrap();
    let def_path = dir.path().join("infobridge.json");
    std::fs::write(&def_path, r#"{"connectionId": "test-connection"}"#).unwrap();

    fabio()
        .args([
            "plan",
            "update-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &plan_id,
            "--file",
            def_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Get definition
    let assert = fabio()
        .args([
            "plan",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &plan_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();
    assert!(
        parts
            .iter()
            .any(|p| p["path"] == "connectedPlanning/infobridge.json"),
        "Expected connectedPlanning/infobridge.json part"
    );

    // Get definition with an explicit --format (spec's getDefinition optional format query param)
    let assert = fabio()
        .args([
            "plan",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &plan_id,
            "--format",
            "PlanV1",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();
    assert!(
        parts
            .iter()
            .any(|p| p["path"] == "connectedPlanning/infobridge.json"),
        "Expected connectedPlanning/infobridge.json part with --format PlanV1"
    );

    // Cleanup
    fabio()
        .args([
            "plan",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &plan_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn plan_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "plan",
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
fn plan_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "plan",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-dry-run",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "plan create");
    assert!(data["dry_run"].as_bool().unwrap());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn plan_dry_run_delete() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "plan",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "plan delete");
    assert!(data["dry_run"].as_bool().unwrap());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn plan_dry_run_update_definition() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::tempdir().unwrap();
    let def_path = dir.path().join("infobridge.json");
    std::fs::write(&def_path, r#"{"connectionId": "test-connection"}"#).unwrap();

    let assert = fabio()
        .args([
            "plan",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--file",
            def_path.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "plan update-definition");
    assert!(data["dry_run"].as_bool().unwrap());
    assert_eq!(
        data["details"]["id"],
        "00000000-0000-0000-0000-000000000000"
    );
    assert_eq!(data["details"]["workspace"], cfg.source_workspace);
    assert!(data["details"]["contentLength"].as_u64().unwrap() > 0);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn plan_list_with_folder_scoping_flags() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "plan",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--no-recursive",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}
