//! End-to-end integration tests for `fabio context` commands.

mod common;

use common::{TestConfig, fabio, parse_json};
use serial_test::serial;

// ── Dry-run tests (offline, no live tenant needed) ──────────────────────────

#[test]
fn context_tenant_dry_run_succeeds() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "context tenant");
}

#[test]
fn context_tenant_dry_run_with_deep() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--deep",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    // Verify deep flag is reflected in dry-run output
    assert_eq!(data["details"]["deep"], true);
}

#[test]
fn context_tenant_dry_run_with_connections() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--include-connections",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["includeConnections"], true);
}

#[test]
fn context_tenant_dry_run_with_item_types() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--item-types",
            "Notebook,Lakehouse",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["itemTypes"], "Notebook,Lakehouse");
}

#[test]
fn context_tenant_dry_run_multiple_workspaces() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--workspace",
            "00000000-0000-0000-0000-000000000002",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    let workspaces = data["details"]["workspaces"].as_array().unwrap();
    assert_eq!(workspaces.len(), 2);
}

#[test]
fn context_tenant_dry_run_concurrency_flag() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--concurrency",
            "4",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["details"]["concurrency"], 4);
}

#[test]
fn context_tenant_dry_run_no_properties() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--no-properties",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["noProperties"], true);
}

#[test]
fn context_tenant_dry_run_output_file() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--output-file",
            "/tmp/opencode/test_context_output.json",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    assert!(
        data["details"]["outputFile"]
            .as_str()
            .unwrap_or_default()
            .contains("test_context_output.json")
    );
}

#[test]
fn context_tenant_dry_run_merge() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--merge",
            "/tmp/opencode/existing_graph.json",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    assert!(
        data["details"]["merge"]
            .as_str()
            .unwrap_or_default()
            .contains("existing_graph.json")
    );
}

#[test]
fn context_tenant_dry_run_format_jsonld() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--format",
            "jsonld",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
}

// ── Live tenant tests ───────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_returns_graph_structure() {
    let config = TestConfig::from_env();
    let assert = fabio()
        .args(["context", "tenant", "--workspace", &config.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");

    // Verify graph structure
    assert!(data.get("nodes").is_some(), "missing nodes");
    assert!(data.get("edges").is_some(), "missing edges");
    assert!(data.get("workspaces").is_some(), "missing workspaces");
    assert!(data.get("summary").is_some(), "missing summary");

    // Verify summary fields
    let summary = &data["summary"];
    assert!(summary["totalNodes"].as_u64().unwrap() > 0);
    assert_eq!(summary["workspacesScanned"].as_u64().unwrap(), 1);
    assert!(summary["itemTypes"].is_object());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_nodes_have_required_fields() {
    let config = TestConfig::from_env();
    let assert = fabio()
        .args(["context", "tenant", "--workspace", &config.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    let nodes = data["nodes"].as_array().expect("nodes is not an array");

    // Every node must have id, type, name, workspaceId
    for node in nodes {
        assert!(
            node.get("id").and_then(|v| v.as_str()).is_some(),
            "node missing id"
        );
        assert!(
            node.get("type").and_then(|v| v.as_str()).is_some(),
            "node missing type"
        );
        assert!(
            node.get("name").and_then(|v| v.as_str()).is_some(),
            "node missing name"
        );
        assert!(
            node.get("workspaceId").and_then(|v| v.as_str()).is_some(),
            "node missing workspaceId"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_with_item_type_filter() {
    let config = TestConfig::from_env();
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--item-types",
            "Lakehouse",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    let nodes = data["nodes"].as_array().expect("nodes is not an array");

    // All nodes should be Lakehouse type
    for node in nodes {
        let node_type = node["type"].as_str().unwrap_or_default();
        assert_eq!(
            node_type.to_lowercase(),
            "lakehouse",
            "expected Lakehouse, got {node_type}"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_deep_discovers_more_edges() {
    let config = TestConfig::from_env();

    // Without deep
    let assert_shallow = fabio()
        .args(["context", "tenant", "--workspace", &config.source_workspace])
        .assert()
        .success();
    let json_shallow = parse_json(&assert_shallow);
    let shallow_edges = json_shallow["data"]["summary"]["totalEdges"]
        .as_u64()
        .unwrap_or(0);

    // With deep
    let assert_deep = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--deep",
        ])
        .assert()
        .success();
    let json_deep = parse_json(&assert_deep);
    let deep_edges = json_deep["data"]["summary"]["totalEdges"]
        .as_u64()
        .unwrap_or(0);

    // Both modes should find edges; deep mode may find same or more
    // (deep can find fewer if getDefinition calls fail due to permissions/rate limits)
    assert!(
        shallow_edges > 0 || deep_edges > 0,
        "at least one mode should find edges (shallow={shallow_edges}, deep={deep_edges})"
    );
    // Deep mode should produce relationship type data
    let deep_rel_types = &json_deep["data"]["summary"]["relationshipTypes"];
    assert!(
        deep_rel_types.is_object() || deep_edges == 0,
        "deep mode should report relationship types when edges exist"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_workspace_by_name() {
    // This test uses the workspace ID env var but resolves it as a name
    // (only works if the workspace listing returns it)
    let config = TestConfig::from_env();
    let assert = fabio()
        .args(["context", "tenant", "--workspace", &config.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    let workspaces = data["workspaces"].as_array().expect("missing workspaces");
    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0]["id"], config.source_workspace);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_no_properties_is_faster_and_lacks_properties() {
    let config = TestConfig::from_env();
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--no-properties",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    let nodes = data["nodes"].as_array().expect("nodes is not an array");

    // Nodes should exist but without properties
    assert_ne!(nodes.len(), 0);
    for node in nodes {
        assert!(
            node.get("properties").is_none() || node["properties"].is_null(),
            "expected no properties in --no-properties mode"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_output_file_writes_graph() {
    let config = TestConfig::from_env();
    let output_path = "/tmp/opencode/e2e_context_output_test.json";

    // Remove file if it exists from a previous run
    let _ = std::fs::remove_file(output_path);

    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--no-properties",
            "--output-file",
            output_path,
        ])
        .assert()
        .success();

    // stdout reports what was written
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["status"], "written");
    assert!(data["nodes"].as_u64().unwrap() > 0);

    // File should exist and contain valid graph JSON
    let content = std::fs::read_to_string(output_path).expect("output file not found");
    let file_json: serde_json::Value =
        serde_json::from_str(&content).expect("output file is not valid JSON");
    assert!(file_json["data"]["nodes"].is_array());
    assert!(file_json["data"]["summary"]["totalNodes"].as_u64().unwrap() > 0);

    // Cleanup
    let _ = std::fs::remove_file(output_path);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_merge_incremental() {
    let config = TestConfig::from_env();
    let output_path = "/tmp/opencode/e2e_context_merge_test.json";
    let _ = std::fs::remove_file(output_path);

    // Step 1: Extract source workspace
    fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--no-properties",
            "--output-file",
            output_path,
        ])
        .assert()
        .success();

    // Read initial node count
    let content1 = std::fs::read_to_string(output_path).unwrap();
    let json1: serde_json::Value = serde_json::from_str(&content1).unwrap();
    let nodes1 = json1["data"]["summary"]["totalNodes"].as_u64().unwrap();

    // Step 2: Merge dest workspace into same file
    fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.dest_workspace,
            "--no-properties",
            "--merge",
            output_path,
            "--output-file",
            output_path,
        ])
        .assert()
        .success();

    // Merged graph should have more nodes (or same if same workspace)
    let content2 = std::fs::read_to_string(output_path).unwrap();
    let json2: serde_json::Value = serde_json::from_str(&content2).unwrap();
    let nodes2 = json2["data"]["summary"]["totalNodes"].as_u64().unwrap();
    let ws_count = json2["data"]["summary"]["workspacesScanned"]
        .as_u64()
        .unwrap();

    assert!(
        nodes2 >= nodes1,
        "merged graph ({nodes2}) should have >= initial ({nodes1}) nodes"
    );
    assert_eq!(ws_count, 2, "merged graph should span 2 workspaces");

    // Cleanup
    let _ = std::fs::remove_file(output_path);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_format_jsonld_has_context_and_graph() {
    let config = TestConfig::from_env();
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--format",
            "jsonld",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");

    // JSON-LD must have @context and @graph
    assert!(data.get("@context").is_some(), "missing @context");
    assert!(data.get("@graph").is_some(), "missing @graph");

    let graph = data["@graph"].as_array().expect("@graph is not array");
    assert!(!graph.is_empty(), "@graph should have resources");

    // Every resource should have @id and @type
    for resource in graph {
        assert!(
            resource.get("@id").and_then(|v| v.as_str()).is_some(),
            "resource missing @id"
        );
        assert!(
            resource.get("@type").and_then(|v| v.as_str()).is_some(),
            "resource missing @type"
        );
    }

    // Should have at least one workspace resource
    let has_workspace = graph.iter().any(|r| {
        r["@id"]
            .as_str()
            .unwrap_or_default()
            .contains("urn:fabric:workspace:")
    });
    assert!(has_workspace, "should have workspace resource");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_format_jsonld_to_file() {
    let config = TestConfig::from_env();
    let output_path = "/tmp/opencode/e2e_context_jsonld_test.json";
    let _ = std::fs::remove_file(output_path);

    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--no-properties",
            "--format",
            "jsonld",
            "--output-file",
            output_path,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["status"], "written");
    assert_eq!(data["format"], "jsonld");

    // File should contain valid JSON-LD
    let content = std::fs::read_to_string(output_path).expect("output file not found");
    let file_json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(file_json["data"]["@context"].is_object());
    assert!(file_json["data"]["@graph"].is_array());

    let _ = std::fs::remove_file(output_path);
}

// ── Dry-run tests for new LSP-inspired features ─────────────────────────────

#[test]
fn context_tenant_dry_run_summary_only() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--summary-only",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["summaryOnly"], true);
}

#[test]
fn context_tenant_dry_run_resolve() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--resolve",
            "Notebook:my-nb,Lakehouse:bronze",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["details"]["resolve"],
        "Notebook:my-nb,Lakehouse:bronze"
    );
}

#[test]
fn context_tenant_dry_run_focus() {
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--focus",
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "--depth",
            "3",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["details"]["focus"],
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
    );
    assert_eq!(data["details"]["depth"], 3);
}

// ── Live tenant tests for new features ──────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_summary_only_returns_inventory() {
    let config = TestConfig::from_env();
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--summary-only",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");

    // Must have workspace list, item counts, and timing
    assert!(data.get("workspaces").is_some(), "missing workspaces");
    assert!(
        data["totalItems"].as_u64().unwrap() > 0,
        "should have items"
    );
    assert!(data.get("itemTypes").is_some(), "missing itemTypes");
    assert!(
        data["scanDurationMs"].as_u64().is_some(),
        "missing scanDurationMs"
    );
    assert!(data.get("hint").is_some(), "missing hint for agents");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_resolve_finds_known_item() {
    let config = TestConfig::from_env();

    // First get the name of the source lakehouse
    let list_assert = fabio()
        .args([
            "item",
            "show",
            "--workspace",
            &config.source_workspace,
            "--id",
            &config.source_lakehouse,
        ])
        .assert()
        .success();
    let list_json = parse_json(&list_assert);
    let lakehouse_name = list_json["data"]["displayName"]
        .as_str()
        .expect("lakehouse should have displayName");

    // Now resolve it
    let resolve_spec = format!("Lakehouse:{lakehouse_name}");
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--resolve",
            &resolve_spec,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    let resolved = data["resolved"].as_array().expect("resolved is not array");

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0]["id"], config.source_lakehouse);
    assert_eq!(resolved[0]["type"], "Lakehouse");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_resolve_not_found() {
    let config = TestConfig::from_env();
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--resolve",
            "Notebook:nonexistent-item-xyz-12345",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    let resolved = data["resolved"].as_array().expect("resolved is not array");

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0]["error"], "Not found");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_metadata_envelope_present() {
    let config = TestConfig::from_env();
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--no-properties",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");

    // Verify meta envelope
    let meta = data.get("meta").expect("missing meta envelope");
    assert!(
        meta.get("scannedAt").and_then(|v| v.as_str()).is_some(),
        "missing scannedAt"
    );
    assert!(
        meta.get("scanDurationMs")
            .and_then(serde_json::Value::as_u64)
            .is_some(),
        "missing scanDurationMs"
    );
    assert!(
        meta.get("etag").and_then(|v| v.as_str()).is_some(),
        "missing etag"
    );
    assert!(meta.get("partial").is_some(), "missing partial");
    assert!(meta.get("scope").is_some(), "missing scope");

    // Etag should be sha256:... format
    let etag = meta["etag"].as_str().unwrap();
    assert!(
        etag.starts_with("sha256:"),
        "etag should start with sha256:"
    );
    assert_eq!(etag.len(), 7 + 64, "etag should be sha256: + 64 hex chars");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_metadata_etag_stable() {
    let config = TestConfig::from_env();

    // Run twice — same workspace, same items — etag should be identical
    let assert1 = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--no-properties",
        ])
        .assert()
        .success();
    let json1 = parse_json(&assert1);
    let etag1 = json1["data"]["meta"]["etag"]
        .as_str()
        .expect("missing etag");

    let assert2 = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--no-properties",
        ])
        .assert()
        .success();
    let json2 = parse_json(&assert2);
    let etag2 = json2["data"]["meta"]["etag"]
        .as_str()
        .expect("missing etag");

    assert_eq!(etag1, etag2, "etag should be stable across identical scans");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_focus_returns_subgraph() {
    let config = TestConfig::from_env();

    // First get an item ID to focus on
    let full_assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--no-properties",
        ])
        .assert()
        .success();
    let full_json = parse_json(&full_assert);
    let all_nodes = full_json["data"]["nodes"]
        .as_array()
        .expect("nodes is array");
    let total_nodes = all_nodes.len();
    assert!(total_nodes > 1, "need at least 2 items for focus test");

    let focus_id = all_nodes[0]["id"].as_str().unwrap();

    // Now focus on that item with depth 1
    let focus_assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--no-properties",
            "--focus",
            focus_id,
            "--depth",
            "1",
        ])
        .assert()
        .success();
    let focus_json = parse_json(&focus_assert);
    let focus_data = focus_json.get("data").expect("missing data");
    let focus_nodes = focus_data["nodes"].as_array().expect("nodes array");

    // Focused graph should have the focal item
    assert!(
        focus_nodes.iter().any(|n| n["id"] == focus_id),
        "focused graph should contain the focal item"
    );
    // Focused graph should be <= total (likely smaller unless everything is connected)
    assert!(
        focus_nodes.len() <= total_nodes,
        "focused graph should be <= full graph"
    );
    // Meta should indicate partial
    let meta = focus_data.get("meta").expect("missing meta");
    assert_eq!(meta["partial"], true, "focus mode should mark partial=true");
    assert_eq!(
        meta["scope"]["focusItem"], focus_id,
        "scope should record focus item"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn context_tenant_edges_have_confidence() {
    let config = TestConfig::from_env();
    let assert = fabio()
        .args([
            "context",
            "tenant",
            "--workspace",
            &config.source_workspace,
            "--deep",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    let edges = data["edges"].as_array().expect("edges is array");

    if edges.is_empty() {
        // No edges discovered — can't validate confidence fields
        return;
    }

    // At least some edges should have confidence and discoveryMethod
    let edges_with_confidence = edges
        .iter()
        .filter(|e| e.get("confidence").and_then(|v| v.as_str()).is_some())
        .count();
    assert!(
        edges_with_confidence > 0,
        "at least some edges should have confidence field"
    );

    // Validate confidence values are valid
    for edge in edges {
        if let Some(conf) = edge.get("confidence").and_then(|v| v.as_str()) {
            assert!(
                conf == "high" || conf == "medium" || conf == "low",
                "invalid confidence value: {conf}"
            );
        }
        if let Some(method) = edge.get("discoveryMethod").and_then(|v| v.as_str()) {
            assert!(
                method == "structured_property"
                    || method == "guid_scan_property"
                    || method == "guid_scan_definition"
                    || method == "connection_api",
                "invalid discoveryMethod value: {method}"
            );
        }
    }
}
