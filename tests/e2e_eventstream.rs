//! End-to-end integration tests for `fabio eventstream` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["eventstream", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("es_test");

    // Create
    let assert = fabio()
        .args([
            "eventstream",
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
    let es_id = data["id"].as_str().unwrap().to_string();

    // Delete
    let assert = fabio()
        .args([
            "eventstream",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &es_id,
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
fn eventstream_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "eventstream",
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
fn eventstream_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "eventstream",
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
fn eventstream_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "eventstream",
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
    assert_eq!(json["data"]["would_execute"], "eventstream create");
}

// ─── Topology tests ──────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_get_topology_returns_structure() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("es_topo");

    // Create eventstream
    let assert = fabio()
        .args([
            "eventstream",
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
    let es_id = data["id"].as_str().unwrap().to_string();

    // Get topology
    let assert = fabio()
        .args([
            "eventstream",
            "get-topology",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &es_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data["sources"].is_array());
    assert!(data["destinations"].is_array());
    assert!(data["streams"].is_array());
    assert!(data["operators"].is_array());
    assert!(data["compatibilityLevel"].is_string());

    // Cleanup
    fabio()
        .args([
            "eventstream",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &es_id,
        ])
        .assert()
        .success();
}

// ─── add-source / add-destination tests ──────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_add_source_custom_endpoint() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("es_addsrc");

    // Create eventstream
    let assert = fabio()
        .args([
            "eventstream",
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
    let es_id = data["id"].as_str().unwrap().to_string();

    // Add source
    let assert = fabio()
        .args([
            "eventstream",
            "add-source",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &es_id,
            "--name",
            "my-source",
            "--source-type",
            "CustomEndpoint",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Topology should contain the new source
    let sources = data["sources"].as_array().unwrap();
    assert!(!sources.is_empty());
    assert_eq!(sources[0]["name"], "my-source");
    assert_eq!(sources[0]["type"], "CustomEndpoint");
    // Default stream should have been auto-created
    let streams = data["streams"].as_array().unwrap();
    assert!(!streams.is_empty());

    // Cleanup
    fabio()
        .args([
            "eventstream",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &es_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_add_source_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "eventstream",
            "add-source",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "test-source",
            "--source-type",
            "CustomEndpoint",
            "--dry-run",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "eventstream add-source");
    assert_eq!(json["data"]["details"]["source"]["name"], "test-source");
    assert_eq!(json["data"]["details"]["source"]["type"], "CustomEndpoint");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_add_destination_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "eventstream",
            "add-destination",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "test-dest",
            "--destination-type",
            "Eventhouse",
            "--input-node",
            "my-stream",
            "--properties",
            r#"{"dataIngestionMode":"DirectIngestion","workspaceId":"ws-id","itemId":"item-id","tableName":"T1","connectionName":"conn","mappingRuleName":"map"}"#,
            "--dry-run",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "eventstream add-destination");
    assert_eq!(json["data"]["details"]["destination"]["name"], "test-dest");
    assert_eq!(json["data"]["details"]["destination"]["type"], "Eventhouse");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_add_destination_eventhouse_wrong_mode_teaches() {
    let cfg = TestConfig::from_env();

    // DirectIngestion mode but supplying the ProcessedIngestion fields
    // (tableName/inputSerialization) and omitting connectionName/mappingRuleName
    // — the silent-Warning trap. Validation fires BEFORE the network call, so
    // this fails fast with a teaching error even with --dry-run.
    let assert = fabio()
        .args([
            "eventstream",
            "add-destination",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "bad-dest",
            "--destination-type",
            "Eventhouse",
            "--input-node",
            "my-stream",
            "--properties",
            r#"{"dataIngestionMode":"DirectIngestion","workspaceId":"ws","itemId":"kql","tableName":"T1","inputSerialization":{"type":"Json"}}"#,
            "--dry-run",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("connectionName") && stderr.contains("mappingRuleName"),
        "expected teaching error naming the missing DirectIngestion fields, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_add_source_invalid_properties_json() {
    let cfg = TestConfig::from_env();

    // Invalid JSON in --properties should fail
    fabio()
        .args([
            "eventstream",
            "add-source",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "test-source",
            "--source-type",
            "CustomEndpoint",
            "--properties",
            "not-valid-json",
        ])
        .assert()
        .failure();
}

// ─── get-definition / update-definition tests ────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_get_definition_returns_parts() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("es_def");

    // Create eventstream
    let assert = fabio()
        .args([
            "eventstream",
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
    let es_id = data["id"].as_str().unwrap().to_string();

    // Get definition
    let assert = fabio()
        .args([
            "eventstream",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &es_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();
    assert!(!parts.is_empty());
    // Should contain eventstream.json
    let has_eventstream_json = parts
        .iter()
        .any(|p| p["path"].as_str() == Some("eventstream.json"));
    assert!(has_eventstream_json);

    // Cleanup
    fabio()
        .args([
            "eventstream",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &es_id,
        ])
        .assert()
        .success();
}

// ─── pause / resume tests ────────────────────────────────────────────────────

// The resume API requires a `startType` body field (Now/WhenLastStopped/
// CustomTime); the old empty {} body failed with "Required property
// 'startType' not found". Offline dry-run regression.
#[test]
fn eventstream_resume_includes_start_type() {
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "resume",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["details"]["startType"], "Now");
}

#[test]
fn eventstream_resume_custom_time_requires_timestamp() {
    fabio()
        .args([
            "eventstream",
            "resume",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--start-type",
            "CustomTime",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_pause_resume_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "eventstream",
            "pause",
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
    assert_eq!(json["data"]["would_execute"], "eventstream pause");

    let assert = fabio()
        .args([
            "eventstream",
            "resume",
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
    assert_eq!(json["data"]["would_execute"], "eventstream resume");
}

// ─── Phase 2: Builder Improvements Tests ─────────────────────────────────────

#[test]
fn eventstream_list_components_all() {
    let output = fabio()
        .args(["eventstream", "list-components"])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let items = data.as_array().expect("should be array");
    assert!(items.len() >= 18, "Should have sources + destinations");
    // Check has both categories
    let has_source = items.iter().any(|i| i["category"] == "source");
    let has_dest = items.iter().any(|i| i["category"] == "destination");
    assert!(has_source, "Should include sources");
    assert!(has_dest, "Should include destinations");
}

#[test]
fn eventstream_list_components_filter_source() {
    let output = fabio()
        .args(["eventstream", "list-components", "--category", "source"])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let items = data.as_array().expect("should be array");
    assert_eq!(items.len(), 37, "Should match the complete SourceType enum");
    let types: Vec<&str> = items
        .iter()
        .filter_map(|item| item["type"].as_str())
        .collect();
    for new_type in [
        "Cribl",
        "FabricCapacityOperationEvents",
        "ReferenceLakehouse",
        "SAPDatasphere",
    ] {
        assert!(types.contains(&new_type), "missing source type {new_type}");
    }
    for item in items {
        assert_eq!(item["category"], "source");
    }
}

#[test]
fn eventstream_list_components_filter_destination() {
    let output = fabio()
        .args([
            "eventstream",
            "list-components",
            "--category",
            "destination",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let items = data.as_array().expect("should be array");
    assert_eq!(items.len(), 5, "Should have 5 destination types");
    for item in items {
        assert_eq!(item["category"], "destination");
    }
}

#[test]
fn eventstream_list_components_filter_operator() {
    let output = fabio()
        .args(["eventstream", "list-components", "--category", "operator"])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let items = data.as_array().expect("should be array");
    assert_eq!(items.len(), 7, "Should have 7 event-processor operators");
    let types: Vec<&str> = items.iter().filter_map(|i| i["type"].as_str()).collect();
    for expected in [
        "Filter",
        "ManageFields",
        "Aggregate",
        "GroupBy",
        "Join",
        "Union",
        "Expand",
    ] {
        assert!(types.contains(&expected), "missing operator {expected}");
    }
    for item in items {
        assert_eq!(item["category"], "operator");
    }
}

#[test]
fn eventstream_add_operator_unknown_type_fails() {
    // Unknown operator type is rejected client-side with the valid enum in the hint.
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "add-operator",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "op1",
            "--type",
            "NotAnOperator",
            "--input-node",
            "s1",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("Filter") && stderr.contains("ManageFields"));
}

#[test]
fn eventstream_add_operator_dry_run_builds_node() {
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "add-operator",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "KeepMSFT",
            "--type",
            "filter",
            "--input-node",
            "src-stream",
            "--properties",
            r#"{"conditions":[]}"#,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "eventstream add-operator");
    let op = &data["details"]["operator"];
    assert_eq!(op["type"], "Filter"); // case-normalized
    assert_eq!(op["inputNodes"][0]["name"], "src-stream");
}

#[test]
fn eventstream_add_operator_join_accepts_reference_input() {
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "add-operator",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "EnrichOrders",
            "--type",
            "Join",
            "--input-node",
            "orders-stream",
            "--input-node",
            "customers-stream",
            "--properties",
            r#"{"joinType":"LeftOuter","joinOn":[{"left":{"node":"orders-stream","columnName":"customerId","columnPath":[]},"right":{"node":"customers-stream","columnName":"id","columnPath":[]}}]}"#,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let inputs = &extract_data(&json)["details"]["operator"]["inputNodes"];
    assert_eq!(inputs[0]["name"], "orders-stream");
    assert_eq!(inputs[1]["name"], "customers-stream");
}

#[test]
fn eventstream_add_operator_join_rejects_three_inputs() {
    // Join has exactly two sides; a third --input-node must be rejected client-side.
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "add-operator",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "EnrichOrders",
            "--type",
            "Join",
            "--input-node",
            "orders-stream",
            "--input-node",
            "customers-stream",
            "--input-node",
            "extra-stream",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("exactly two"));
}

#[test]
fn eventstream_add_operator_filter_rejects_multiple_inputs() {
    // Filter is a unary operator; more than one --input-node must be rejected client-side.
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "add-operator",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "KeepMSFT",
            "--type",
            "Filter",
            "--input-node",
            "src-stream",
            "--input-node",
            "other-stream",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("exactly one"));
}

#[test]
fn eventstream_add_operator_union_requires_two_inputs() {
    // Union supports an arbitrary multi-input set but needs at least two.
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "add-operator",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "MergedStream",
            "--type",
            "Union",
            "--input-node",
            "src-stream",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("at least two"));
}

#[test]
fn eventstream_add_reference_lakehouse_source_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "add-source",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "customers",
            "--source-type",
            "ReferenceLakehouse",
            "--properties",
            r#"{"workspaceId":"cfafbeb1-8037-4d0c-896e-a46fb27ff229","itemId":"11111111-2222-3333-4444-555555555555","absoluteOneLakePath":"https://onelake.dfs.fabric.microsoft.com/cfafbeb1-8037-4d0c-896e-a46fb27ff229/11111111-2222-3333-4444-555555555555/Tables/dbo/customers","referencedColumns":["id","name"],"refreshRate":"00:05:00"}"#,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let source = &extract_data(&json)["details"]["source"];
    assert_eq!(source["type"], "ReferenceLakehouse");
    assert_eq!(source["properties"]["refreshRate"], "00:05:00");
}

#[test]
fn eventstream_add_reference_lakehouse_source_rejects_mismatched_path() {
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "add-source",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "customers",
            "--source-type",
            "ReferenceLakehouse",
            "--properties",
            r#"{"workspaceId":"cfafbeb1-8037-4d0c-896e-a46fb27ff229","itemId":"11111111-2222-3333-4444-555555555555","absoluteOneLakePath":"https://onelake.dfs.fabric.microsoft.com/cfafbeb1-8037-4d0c-896e-a46fb27ff229/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/Tables/dbo/customers"}"#,
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("must match"), "got: {stderr}");
}

#[test]
fn eventstream_add_reference_lakehouse_source_rejects_extra_path_segments() {
    // Exactly five path segments identify a Delta table; anything below a table
    // (e.g. a Files/ path appended after the table name) must be rejected.
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "add-source",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "customers",
            "--source-type",
            "ReferenceLakehouse",
            "--properties",
            r#"{"workspaceId":"cfafbeb1-8037-4d0c-896e-a46fb27ff229","itemId":"11111111-2222-3333-4444-555555555555","absoluteOneLakePath":"https://onelake.dfs.fabric.microsoft.com/cfafbeb1-8037-4d0c-896e-a46fb27ff229/11111111-2222-3333-4444-555555555555/Tables/dbo/customers/Files/x"}"#,
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("must identify a Delta table"),
        "got: {stderr}"
    );
}

#[test]
fn eventstream_add_capacity_operation_source_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "eventstream",
            "add-source",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "capacity-operations",
            "--source-type",
            "FabricCapacityOperationEvents",
            "--properties",
            r#"{"eventScope":"Capacity","capacityId":"0b6d8e27-0b7b-4e18-9e5b-2e6b9d9d7e3a","includedEventTypes":["Microsoft.Fabric.Capacity.OperationStarted","Microsoft.Fabric.Capacity.OperationCompleted"],"filters":[{"operatorType":"StringIn","key":"data.operationType","values":["ScaleUp","ScaleDown"]}]}"#,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let properties = &extract_data(&json)["details"]["source"]["properties"];
    assert_eq!(properties["eventScope"], "Capacity");
    assert_eq!(properties["filters"][0]["operatorType"], "StringIn");
}

#[test]
fn eventstream_validate_valid_definition() {
    let def = serde_json::json!({
        "sources": [{"name": "src1", "type": "CustomEndpoint"}],
        "streams": [{"name": "str1", "inputNodes": [{"name": "src1"}]}],
        "destinations": [{"name": "dst1", "type": "Eventhouse", "inputNodes": [{"name": "str1"}]}],
        "compatibilityLevel": "1.1"
    });
    let tmp = std::env::temp_dir().join("fabio_es_valid.json");
    std::fs::write(&tmp, serde_json::to_string(&def).unwrap()).unwrap();

    let output = fabio()
        .args([
            "eventstream",
            "validate",
            "--file",
            &tmp.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["valid"], true);
    assert!(data["errors"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn eventstream_validate_invalid_references() {
    let def = serde_json::json!({
        "sources": [{"name": "src1", "type": "CustomEndpoint"}],
        "streams": [{"name": "str1", "inputNodes": [{"name": "missing_node"}]}],
        "destinations": []
    });
    let tmp = std::env::temp_dir().join("fabio_es_invalid.json");
    std::fs::write(&tmp, serde_json::to_string(&def).unwrap()).unwrap();

    let output = fabio()
        .args([
            "eventstream",
            "validate",
            "--file",
            &tmp.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["valid"], false);
    let errors = data["errors"].as_array().unwrap();
    assert!(!errors.is_empty());
    assert!(
        errors[0]
            .as_str()
            .unwrap()
            .contains("non-existent inputNode")
    );
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn eventstream_validate_duplicate_names() {
    let def = serde_json::json!({
        "sources": [
            {"name": "node1", "type": "CustomEndpoint"},
            {"name": "node1", "type": "SampleData"}
        ],
        "streams": [],
        "destinations": []
    });
    let tmp = std::env::temp_dir().join("fabio_es_dup.json");
    std::fs::write(&tmp, serde_json::to_string(&def).unwrap()).unwrap();

    let output = fabio()
        .args([
            "eventstream",
            "validate",
            "--file",
            &tmp.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["valid"], false);
    let errors = data["errors"].as_array().unwrap();
    let has_dup = errors
        .iter()
        .any(|e| e.as_str().unwrap().contains("Duplicate"));
    assert!(has_dup, "Should detect duplicate node names");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn eventstream_validate_missing_file() {
    fabio()
        .args([
            "eventstream",
            "validate",
            "--file",
            "/nonexistent/path.json",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_add_sample_source_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "eventstream",
            "add-sample-source",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "test-sample",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "eventstream add-sample-source");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_add_derived_stream_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "eventstream",
            "add-derived-stream",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "filtered-stream",
            "--input-node",
            "source-stream",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "eventstream add-derived-stream");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventstream_validate_from_server() {
    let cfg = TestConfig::from_env();

    // List eventstreams and pick the first one
    let output = fabio()
        .args(["eventstream", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let items = data.as_array().expect("should be array");
    if items.is_empty() {
        // No eventstreams in workspace — skip gracefully
        return;
    }

    let es_id = items[0]["id"].as_str().expect("should have id");

    // Validate the live definition fetched from server
    let output = fabio()
        .args([
            "eventstream",
            "validate",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            es_id,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    // Should return a valid/invalid result (not an error)
    assert!(
        data.get("valid").is_some(),
        "validate should return 'valid' field, got: {data}"
    );
}
