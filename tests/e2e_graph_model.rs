//! End-to-end integration tests for `fabio graph-model` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

/// Extract the JSON error envelope from stderr, tolerating a leading
/// `[timing]` diagnostics line (emitted whenever the command is not `--quiet`).
fn stderr_error_json(assert: &assert_cmd::assert::Assert) -> serde_json::Value {
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    for line in stderr.lines() {
        if line.starts_with('{') {
            return serde_json::from_str(line).expect("parse stderr JSON error");
        }
    }
    panic!("No JSON error found in stderr: {stderr}");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_model_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["graph-model", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_model_create_show_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("gm_test");

    // Create
    let assert = fabio()
        .args([
            "graph-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--description",
            "E2E test graph model",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["type"], "GraphModel");
    let gm_id = data["id"].as_str().unwrap().to_string();

    // Show
    let assert = fabio()
        .args([
            "graph-model",
            "show",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["id"], gm_id);
    assert_eq!(data["properties"]["queryReadiness"], "None");

    // Delete
    let assert = fabio()
        .args([
            "graph-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
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
fn graph_model_create_with_ontology() {
    let cfg = TestConfig::from_env();
    let ont_name = common::unique_name("gm_ont");
    let gm_name = common::unique_name("gm_linked");

    // Create an ontology first
    let assert = fabio()
        .args([
            "ontology",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &ont_name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let ont_id = data["id"].as_str().unwrap().to_string();

    // Create graph model linked to the ontology
    let assert = fabio()
        .args([
            "graph-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &gm_name,
            "--ontology",
            &ont_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], gm_name);
    let gm_id = data["id"].as_str().unwrap().to_string();

    // Cleanup
    fabio()
        .args([
            "graph-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
        ])
        .assert()
        .success();

    fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &ont_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_model_update_name() {
    let cfg = TestConfig::from_env();
    let original = common::unique_name("gm_upd_o");
    let updated = common::unique_name("gm_upd_n");

    // Create
    let assert = fabio()
        .args([
            "graph-model",
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
    let gm_id = data["id"].as_str().unwrap().to_string();

    // Update
    let assert = fabio()
        .args([
            "graph-model",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
            "--name",
            &updated,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], updated);

    // Cleanup
    fabio()
        .args([
            "graph-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_model_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "graph-model",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let err_json = stderr_error_json(&assert);
    assert_eq!(err_json["error"]["code"], "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_model_get_definition() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("gm_def");

    // Create
    let assert = fabio()
        .args([
            "graph-model",
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
    let gm_id = data["id"].as_str().unwrap().to_string();

    // Get definition
    let assert = fabio()
        .args([
            "graph-model",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Definition should have parts with at least .platform
    let parts = data["definition"]["parts"].as_array().unwrap();
    assert!(!parts.is_empty());
    assert!(
        parts
            .iter()
            .any(|p| p["path"].as_str().unwrap() == ".platform")
    );

    // Cleanup
    fabio()
        .args([
            "graph-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_model_refresh_graph() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("gm_refresh");

    // Create
    let assert = fabio()
        .args([
            "graph-model",
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
    let gm_id = data["id"].as_str().unwrap().to_string();

    // Refresh (no --wait, just trigger)
    let assert = fabio()
        .args([
            "graph-model",
            "refresh-graph",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "refresh_triggered");

    // Wait a moment and check status
    std::thread::sleep(std::time::Duration::from_secs(5));

    let assert = fabio()
        .args([
            "graph-model",
            "show",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // lastDataLoadingStatus should exist after refresh is triggered
    assert!(data["properties"]["lastDataLoadingStatus"].is_object());

    // Cleanup
    fabio()
        .args([
            "graph-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_model_execute_query_on_unloaded_graph() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("gm_query");

    // Create
    let assert = fabio()
        .args([
            "graph-model",
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
    let gm_id = data["id"].as_str().unwrap().to_string();

    // Execute query should fail on unloaded graph
    let assert = fabio()
        .args([
            "graph-model",
            "execute-query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
            "--gql",
            "MATCH (n) RETURN n LIMIT 5",
        ])
        .assert()
        .failure();

    let err_json = stderr_error_json(&assert);
    assert_eq!(err_json["error"]["code"], "API_ERROR");
    // Error message should indicate graph is not loaded
    let msg = err_json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("GraphIsNotLoaded") || msg.contains("GraphNotQueryable"),
        "Expected graph-not-loaded error, got: {msg}"
    );

    // Cleanup
    fabio()
        .args([
            "graph-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &gm_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_model_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "graph-model",
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
    assert_eq!(json["data"]["would_execute"], "graph-model create");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graph_model_dry_run_refresh() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "graph-model",
            "refresh-graph",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "graph-model refresh-graph");
}

/// Regression test for the `--query`/global-`--query` flag clash that made
/// `execute-query` return `{"data":null}` for every real GQL query.
///
/// Requires a graph model that has been initialized/loaded through the Fabric
/// portal (REST-created graphs cannot be loaded). Provide its id via
/// `FABIO_TEST_LOADED_GRAPH_ID` (queried in `FABIO_TEST_SOURCE_WORKSPACE`);
/// the test is skipped when the env var is not set.
#[test]
#[ignore = "requires live Fabric tenant + a portal-loaded graph model"]
#[serial]
fn graph_model_execute_query_returns_results_on_loaded_graph() {
    let Ok(graph_id) = std::env::var("FABIO_TEST_LOADED_GRAPH_ID") else {
        eprintln!("FABIO_TEST_LOADED_GRAPH_ID not set — skipping loaded-graph query test");
        return;
    };
    let cfg = TestConfig::from_env();

    // A valid query must return the result envelope with a non-null data set,
    // NOT `{"data":null}` (the symptom of the JMESPath clash).
    let assert = fabio()
        .args([
            "graph-model",
            "execute-query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graph_id,
            "--gql",
            "MATCH (n) RETURN n LIMIT 1",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(
        !data.is_null(),
        "execute-query returned null data (flag-clash regression): {json}"
    );
    assert_eq!(
        data["status"]["code"].as_str(),
        Some("00000"),
        "expected successful GQL status, got: {json}"
    );
    assert!(
        data["result"]["data"].is_array(),
        "expected a tabular result set, got: {json}"
    );

    // A syntactically invalid query returns HTTP 200 with an error status
    // object; fabio must surface it as a non-zero exit, not silently succeed.
    let assert = fabio()
        .args([
            "graph-model",
            "execute-query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graph_id,
            "--gql",
            "THIS IS NOT VALID GQL @@@",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .failure();

    let err_json = stderr_error_json(&assert);
    assert_eq!(err_json["error"]["code"], "API_ERROR");
    let msg = err_json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("42000") || msg.to_lowercase().contains("syntax"),
        "expected a GQL syntax error, got: {msg}"
    );
}

/// Validates that `execute-query` returns the ACTUAL values computed by the GQL
/// engine, and that they match the expected result — the definitive regression
/// for the flag clash that silently dropped every result set.
///
/// Uses a data-independent arithmetic query (`RETURN 1 + 1 AS two`) so the
/// expected result (`2`) is deterministic on any loaded graph, regardless of
/// tenant data. Set `FABIO_TEST_LOADED_GRAPH_ID` to a portal-loaded graph.
#[test]
#[ignore = "requires live Fabric tenant + a portal-loaded graph model"]
#[serial]
fn graph_model_execute_query_returns_expected_computed_value() {
    let Ok(graph_id) = std::env::var("FABIO_TEST_LOADED_GRAPH_ID") else {
        eprintln!("FABIO_TEST_LOADED_GRAPH_ID not set — skipping computed-value query test");
        return;
    };
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "graph-model",
            "execute-query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graph_id,
            "--gql",
            "RETURN 1 + 1 AS two",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let result = &data["result"];

    // Shape: a single-column, single-row TABLE named after the RETURN alias.
    assert_eq!(result["kind"], "TABLE", "expected a TABLE result: {json}");
    assert_eq!(
        result["columns"][0]["name"], "two",
        "unexpected column name: {json}"
    );
    // Value: the engine must return the real computed value 2 — NOT null (the
    // symptom of the `--query`/JMESPath flag clash this fix removed).
    assert_eq!(
        result["data"][0]["two"],
        serde_json::json!(2),
        "expected computed value 2, got: {json}"
    );
}
