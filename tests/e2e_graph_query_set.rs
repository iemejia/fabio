//! End-to-end integration tests for `fabio graph-query-set` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_query_set_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "graph-query-set",
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
fn graph_query_set_create_show_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("gqs_test");

    // Create
    let assert = fabio()
        .args([
            "graph-query-set",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--description",
            "E2E test graph query set",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["type"], "GraphQuerySet");
    let gqs_id = data["id"].as_str().unwrap().to_string();

    // Show
    let assert = fabio()
        .args([
            "graph-query-set",
            "show",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gqs_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["id"], gqs_id);

    // Delete
    let assert = fabio()
        .args([
            "graph-query-set",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gqs_id,
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
fn graph_query_set_get_definition() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("gqs_def");

    // Create
    let assert = fabio()
        .args([
            "graph-query-set",
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
    let gqs_id = data["id"].as_str().unwrap().to_string();

    // Get definition
    let assert = fabio()
        .args([
            "graph-query-set",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gqs_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();
    assert_ne!(parts.len(), 0);
    // Should have exportedDefinition.json and .platform
    let paths: Vec<&str> = parts.iter().map(|p| p["path"].as_str().unwrap()).collect();
    assert!(paths.contains(&"exportedDefinition.json"));
    assert!(paths.contains(&".platform"));

    // Cleanup
    fabio()
        .args([
            "graph-query-set",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gqs_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_query_set_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "graph-query-set",
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
fn graph_query_set_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "graph-query-set",
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
    assert_eq!(json["data"]["would_execute"], "graph-query-set create");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_query_set_update_definition() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("gqs_updef");

    // Create
    let assert = fabio()
        .args([
            "graph-query-set",
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
    let gqs_id = data["id"].as_str().unwrap().to_string();

    // Update definition (server accepts but strips ArtifactContents)
    let assert = fabio()
        .args([
            "graph-query-set",
            "update-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gqs_id,
            "--content",
            r#"{"dependencies":[],"indirectDependencies":[],"ArtifactContents":[],"ConfigurationCategories":[]}"#,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // update-definition returns status or the object
    assert!(data["status"] == "definition_updated" || data["displayName"] == name);

    // Cleanup
    fabio()
        .args([
            "graph-query-set",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gqs_id,
        ])
        .assert()
        .success();
}
