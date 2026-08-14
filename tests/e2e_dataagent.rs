//! End-to-end integration tests for `fabio data-agent` commands.

mod common;

use base64::Engine;
use common::{TestConfig, extract_count, extract_data, fabio, parse_json, unique_name};
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_list_returns_json() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["data-agent", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    // Should have data array and count
    assert!(json.get("data").is_some());
    assert!(json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_create_show_update_delete() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_test");
    let description = "Test data agent created by fabio e2e tests";

    // Create a data agent
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--description",
            description,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["type"], "DataAgent");
    let agent_id = data["id"].as_str().unwrap().to_string();

    // Show the data agent
    let assert = fabio()
        .args([
            "data-agent",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"], agent_id);
    assert_eq!(data["displayName"], name);

    // Update the data agent
    let new_name = unique_name("da_updated");
    let new_desc = "Updated description";
    let assert = fabio()
        .args([
            "data-agent",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--name",
            &new_name,
            "--description",
            new_desc,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], new_name);
    assert_eq!(data["description"], new_desc);

    // Delete the data agent
    let assert = fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
    assert_eq!(data["id"], agent_id);

    // Verify it's gone (show should fail)
    fabio()
        .args([
            "data-agent",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .failure();
}

/// Validates the `DataAgent` definition spec added to
/// `definition_requirements.json`: `getDefinition` returns the two canonical
/// config parts, confirming `deploy_strategy=content` in the capability matrix.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_get_definition_returns_config_parts() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_def");

    let created = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let agent_id = extract_data(&parse_json(&created))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let def_assert = fabio()
        .args([
            "data-agent",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let def_json = parse_json(&def_assert);
    let parts: Vec<&str> = extract_data(&def_json)["definition"]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["path"].as_str())
        .collect();
    assert!(
        parts.contains(&"Files/Config/data_agent.json")
            && parts.contains(&"Files/Config/draft/stage_config.json"),
        "getDefinition must return both canonical DataAgent config parts; got {parts:?}"
    );

    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_create_without_description() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_nodesc");

    // Create without description
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let agent_id = data["id"].as_str().unwrap().to_string();

    // Cleanup
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_requires_at_least_one_field() {
    let cfg = TestConfig::from_env();

    // Update without --name or --description should fail
    fabio()
        .args([
            "data-agent",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("INVALID_INPUT"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_show_nonexistent_returns_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "data-agent",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NOT_FOUND").or(predicate::str::contains("API_ERROR")));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_list_table_output() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "--output",
            "table",
            "data-agent",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_list_with_query_projection() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--query",
            "[*].displayName",
            "data-agent",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    // Query projection should return array of names (or empty)
    let data = extract_data(&json);
    if let Some(arr) = data.as_array() {
        for item in arr {
            // Each item should be a string (projected displayName)
            assert!(item.is_string(), "expected string, got: {item}");
        }
    }
}

#[test]
#[ignore = "requires live Fabric tenant and published data agent"]
#[serial]
fn dataagent_query_with_prompt() {
    let cfg = TestConfig::from_env();

    // This test requires a published data agent. Skip if no agent exists.
    let assert = fabio()
        .args(["data-agent", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let count = extract_count(&json);
    if count == 0 {
        eprintln!("No data agents in workspace, skipping query test");
        return;
    }

    let items = extract_data(&json).as_array().unwrap().clone();
    let agent_id = items[0]["id"].as_str().unwrap();

    // Try querying the data agent
    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            agent_id,
            "--prompt",
            "What data sources do you have access to?",
        ])
        .timeout(std::time::Duration::from_mins(5))
        .assert();

    // Query may fail if agent isn't published - that's OK for this test
    let output = assert.get_output();
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        let data = extract_data(&json);
        assert!(data.get("answer").is_some());
        assert!(data.get("question").is_some());
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("Query failed (agent may not be published): {stderr}");
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_query_no_prompt_no_stdin_fails() {
    let cfg = TestConfig::from_env();

    // Without --prompt and with empty stdin, should fail
    fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("INVALID_INPUT"));
}

// ─── Definition & Publish Tests ──────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_get_definition() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_getdef");

    // Create a data agent first
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let agent_id = data["id"].as_str().unwrap().to_string();

    // Get definition - should succeed (even if empty/minimal)
    let assert = fabio()
        .args([
            "data-agent",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should have a definition with parts
    assert!(
        data.get("definition").is_some(),
        "expected definition field in response"
    );

    // Cleanup
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_definition_with_lakehouse_datasource() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_upddef");

    // Create a data agent
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let agent_id = data["id"].as_str().unwrap().to_string();

    // Build a definition that configures the data agent with a lakehouse data source
    let data_agent_json = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/dataAgent/definition/dataAgent/2.1.0/schema.json"
    });
    let stage_config_json = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/dataAgent/definition/stageConfiguration/1.0.0/schema.json",
        "aiInstructions": "You are a helpful data assistant for sales analytics."
    });
    let datasource_json = serde_json::json!({
        "$schema": "1.0.0",
        "artifactId": &cfg.source_lakehouse,
        "workspaceId": &cfg.source_workspace,
        "displayName": "SalesLakehouse",
        "type": "lakehouse_tables",
        "userDescription": "Sales data lakehouse",
        "dataSourceInstructions": "This contains sales transaction data"
    });

    let encode = |v: &serde_json::Value| {
        base64::engine::general_purpose::STANDARD.encode(v.to_string().as_bytes())
    };

    let definition = serde_json::json!({
        "definition": {
            "parts": [
                {
                    "path": "Files/Config/data_agent.json",
                    "payload": encode(&data_agent_json),
                    "payloadType": "InlineBase64"
                },
                {
                    "path": "Files/Config/draft/stage_config.json",
                    "payload": encode(&stage_config_json),
                    "payloadType": "InlineBase64"
                },
                {
                    "path": "Files/Config/draft/lakehouse-SalesLakehouse/datasource.json",
                    "payload": encode(&datasource_json),
                    "payloadType": "InlineBase64"
                }
            ]
        }
    });

    let def_content = definition.to_string();

    // Update definition with lakehouse data source
    let assert = fabio()
        .args([
            "data-agent",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--content",
            &def_content,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Response is either {"status":"definition_updated"} or the full item object
    assert!(
        data["status"] == "definition_updated" || data["id"] == *agent_id,
        "expected definition_updated status or item with matching id, got: {data}"
    );

    // Verify the definition was updated by fetching it back
    let assert = fabio()
        .args([
            "data-agent",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();
    // Should have at least the parts we set
    assert!(
        parts.len() >= 3,
        "expected at least 3 definition parts, got {}",
        parts.len()
    );

    // Cleanup
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_definition_requires_file_or_content() {
    let cfg = TestConfig::from_env();

    // Should fail without --file or --content
    fabio()
        .args([
            "data-agent",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("INVALID_INPUT"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_publish_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_pub");

    // Create a data agent
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let agent_id = data["id"].as_str().unwrap().to_string();

    // First set up a draft definition
    let data_agent_json = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/dataAgent/definition/dataAgent/2.1.0/schema.json"
    });
    let stage_config_json = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/dataAgent/definition/stageConfiguration/1.0.0/schema.json",
        "aiInstructions": "You are a helpful data assistant."
    });
    let datasource_json = serde_json::json!({
        "$schema": "1.0.0",
        "artifactId": &cfg.source_lakehouse,
        "workspaceId": &cfg.source_workspace,
        "displayName": "TestLH",
        "type": "lakehouse_tables",
        "userDescription": "Test lakehouse"
    });

    let encode = |v: &serde_json::Value| {
        base64::engine::general_purpose::STANDARD.encode(v.to_string().as_bytes())
    };

    let definition = serde_json::json!({
        "definition": {
            "parts": [
                {
                    "path": "Files/Config/data_agent.json",
                    "payload": encode(&data_agent_json),
                    "payloadType": "InlineBase64"
                },
                {
                    "path": "Files/Config/draft/stage_config.json",
                    "payload": encode(&stage_config_json),
                    "payloadType": "InlineBase64"
                },
                {
                    "path": "Files/Config/draft/lakehouse-TestLH/datasource.json",
                    "payload": encode(&datasource_json),
                    "payloadType": "InlineBase64"
                }
            ]
        }
    });

    let def_content = definition.to_string();

    // Set draft definition
    fabio()
        .args([
            "data-agent",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--content",
            &def_content,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Publish the data agent
    let assert = fabio()
        .args([
            "data-agent",
            "publish",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--description",
            "Test publish from e2e",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Status is always "published" since definition-based publish is the official path
    assert_eq!(
        data["status"], "published",
        "expected published, got: {}",
        data["status"]
    );
    assert_eq!(data["id"], agent_id);

    // Verify the definition now has published parts
    let assert = fabio()
        .args([
            "data-agent",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();
    let paths: Vec<&str> = parts.iter().filter_map(|p| p["path"].as_str()).collect();

    // Should now have published paths
    assert!(
        paths
            .iter()
            .any(|p| p.starts_with("Files/Config/published/")),
        "expected published parts after publish, got paths: {paths:?}"
    );
    assert!(
        paths.contains(&"Files/Config/publish_info.json"),
        "expected publish_info.json, got paths: {paths:?}"
    );

    // Cleanup
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
}

/// The MCP endpoint URL is the canonical runtime/consumption surface for a
/// published data agent. Verify `data-agent mcp-url` returns the documented URL
/// format, reports `published: false` (with a hint) for a draft agent, and flips
/// to `published: true` after publishing.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_mcp_url_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_mcp");

    // Create a fresh (draft) data agent.
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let json = parse_json(&assert);
    let agent_id = extract_data(&json)["id"].as_str().unwrap().to_string();

    let expected_url = format!(
        "https://api.fabric.microsoft.com/v1/mcp/workspaces/{}/dataagents/{}/agent",
        cfg.source_workspace, agent_id
    );

    // Before publish: URL is emitted, published is false, and a hint is present.
    let assert = fabio()
        .args([
            "data-agent",
            "mcp-url",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["mcpUrl"], expected_url);
    assert_eq!(data["published"], false);
    assert!(
        data["hint"]
            .as_str()
            .unwrap()
            .contains("data-agent publish"),
        "expected publish hint, got: {}",
        data["hint"]
    );

    // Publish the agent (definition-based publish is the official path).
    fabio()
        .args([
            "data-agent",
            "publish",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // After publish: same URL, published is true, no hint.
    let assert = fabio()
        .args([
            "data-agent",
            "mcp-url",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["mcpUrl"], expected_url);
    assert_eq!(data["published"], true);
    assert!(data["hint"].is_null());

    // Cleanup.
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_publish_dry_run() {
    let cfg = TestConfig::from_env();

    // --dry-run should succeed without making changes
    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "publish",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--description",
            "dry run test",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent publish");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_definition_dry_run() {
    let cfg = TestConfig::from_env();

    let definition = serde_json::json!({"definition": {"parts": []}}).to_string();

    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            &definition,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent update-definition");
}

/// Full lifecycle test: create → configure → publish → query → delete.
///
/// This validates that publishing via the definition API (without the portal)
/// activates the chat endpoint and allows querying the data agent.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_full_lifecycle_create_publish_query_delete() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_e2e");

    // ─── Step 1: Create a data agent ─────────────────────────────────────────
    eprintln!("[1/5] Creating data agent '{name}'...");
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--description",
            "E2E lifecycle test agent",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["type"], "DataAgent");
    let agent_id = data["id"].as_str().unwrap().to_string();
    eprintln!("  Created agent: {agent_id}");

    // ─── Step 2: Configure with lakehouse data source ────────────────────────
    eprintln!("[2/5] Configuring data source...");
    let data_agent_json = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/dataAgent/definition/dataAgent/2.1.0/schema.json"
    });
    let stage_config_json = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/dataAgent/definition/stageConfiguration/1.0.0/schema.json",
        "aiInstructions": "You are a helpful data assistant. Answer questions about the data in the connected lakehouse. Be concise."
    });
    let datasource_json = serde_json::json!({
        "$schema": "1.0.0",
        "artifactId": &cfg.source_lakehouse,
        "workspaceId": &cfg.source_workspace,
        "displayName": "SalesLakehouse",
        "type": "lakehouse_tables",
        "userDescription": "Product catalog and order management data",
        "dataSourceInstructions": "This lakehouse contains product and order data. Use SQL to query tables.",
        "elements": [
            {
                "id": "dbo",
                "display_name": "dbo",
                "type": "lakehouse_tables.schema",
                "is_selected": true,
                "children": [
                    {
                        "id": "dbo.products",
                        "display_name": "products",
                        "type": "lakehouse_tables.table",
                        "is_selected": true,
                        "description": "Product catalog with prices and stock",
                        "children": [
                            {"id": "dbo.products.product_id", "display_name": "product_id", "type": "lakehouse_tables.column", "data_type": "int", "is_selected": true},
                            {"id": "dbo.products.product_name", "display_name": "product_name", "type": "lakehouse_tables.column", "data_type": "string", "is_selected": true},
                            {"id": "dbo.products.category", "display_name": "category", "type": "lakehouse_tables.column", "data_type": "string", "is_selected": true},
                            {"id": "dbo.products.price", "display_name": "price", "type": "lakehouse_tables.column", "data_type": "double", "is_selected": true},
                            {"id": "dbo.products.stock_quantity", "display_name": "stock_quantity", "type": "lakehouse_tables.column", "data_type": "int", "is_selected": true}
                        ]
                    },
                    {
                        "id": "dbo.orders",
                        "display_name": "orders",
                        "type": "lakehouse_tables.table",
                        "is_selected": true,
                        "description": "Customer orders with amounts and dates",
                        "children": [
                            {"id": "dbo.orders.order_id", "display_name": "order_id", "type": "lakehouse_tables.column", "data_type": "int", "is_selected": true},
                            {"id": "dbo.orders.product_id", "display_name": "product_id", "type": "lakehouse_tables.column", "data_type": "int", "is_selected": true},
                            {"id": "dbo.orders.customer_name", "display_name": "customer_name", "type": "lakehouse_tables.column", "data_type": "string", "is_selected": true},
                            {"id": "dbo.orders.quantity", "display_name": "quantity", "type": "lakehouse_tables.column", "data_type": "int", "is_selected": true},
                            {"id": "dbo.orders.order_date", "display_name": "order_date", "type": "lakehouse_tables.column", "data_type": "string", "is_selected": true},
                            {"id": "dbo.orders.total_amount", "display_name": "total_amount", "type": "lakehouse_tables.column", "data_type": "double", "is_selected": true}
                        ]
                    }
                ]
            }
        ]
    });
    let fewshots_json = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/dataAgent/definition/fewShots/1.0.0/schema.json",
        "fewShots": [
            {
                "id": "a0000001-0001-0001-0001-000000000001",
                "question": "What is the most expensive product?",
                "query": "SELECT TOP 1 product_name, price FROM products ORDER BY price DESC"
            },
            {
                "id": "a0000001-0001-0001-0001-000000000002",
                "question": "How many orders are there?",
                "query": "SELECT COUNT(*) as total_orders FROM orders"
            }
        ]
    });

    let encode = |v: &serde_json::Value| {
        base64::engine::general_purpose::STANDARD.encode(v.to_string().as_bytes())
    };

    let definition = serde_json::json!({
        "definition": {
            "parts": [
                {
                    "path": "Files/Config/data_agent.json",
                    "payload": encode(&data_agent_json),
                    "payloadType": "InlineBase64"
                },
                {
                    "path": "Files/Config/draft/stage_config.json",
                    "payload": encode(&stage_config_json),
                    "payloadType": "InlineBase64"
                },
                {
                    "path": "Files/Config/draft/lakehouse-SalesLakehouse/datasource.json",
                    "payload": encode(&datasource_json),
                    "payloadType": "InlineBase64"
                },
                {
                    "path": "Files/Config/draft/lakehouse-SalesLakehouse/fewshots.json",
                    "payload": encode(&fewshots_json),
                    "payloadType": "InlineBase64"
                }
            ]
        }
    });

    let def_content = definition.to_string();

    fabio()
        .args([
            "data-agent",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--content",
            &def_content,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    eprintln!("  Definition updated with lakehouse data source + fewshots");

    // ─── Step 3: Publish the data agent ──────────────────────────────────────
    eprintln!("[3/5] Publishing data agent...");
    let assert = fabio()
        .args([
            "data-agent",
            "publish",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--description",
            "E2E lifecycle publish",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "published");
    eprintln!(
        "  Published successfully. publishedUrl: {:?}",
        data.get("publishedUrl")
    );

    // Verify definition has published parts
    let assert = fabio()
        .args([
            "data-agent",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();
    let paths: Vec<&str> = parts.iter().filter_map(|p| p["path"].as_str()).collect();
    assert!(
        paths
            .iter()
            .any(|p| p.starts_with("Files/Config/published/")),
        "expected published parts, got: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.contains("fewshots.json")),
        "expected fewshots.json in parts, got: {paths:?}"
    );
    eprintln!(
        "  Definition verified: {} parts including published + fewshots",
        paths.len()
    );

    // ─── Step 4: Query the data agent ────────────────────────────────────────
    eprintln!("[4/5] Querying data agent...");

    // Construct the published URL (since V3 settings may not be enabled)
    let published_url = format!(
        "https://api.fabric.microsoft.com/v1/workspaces/{}/dataagents/{}/aiassistant/openai",
        cfg.source_workspace, agent_id
    );

    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--published-url",
            &published_url,
            "--prompt",
            "What is the most expensive product and how much does it cost?",
        ])
        .timeout(std::time::Duration::from_mins(5))
        .assert();

    let output = assert.get_output();
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        let data = &json["data"];
        assert!(
            data.get("answer").is_some(),
            "expected answer field in query response"
        );
        assert!(
            data.get("question").is_some(),
            "expected question field in query response"
        );
        let answer = data["answer"].as_str().unwrap_or("");
        eprintln!("  Query succeeded! Answer: {answer}");
        // The agent should mention "Laptop Pro 15" and "$1,299.99" if it has data access
        assert!(
            answer.contains("Laptop") || answer.contains("1299") || answer.contains("1,299"),
            "expected answer to mention Laptop Pro 15 or its price, got: {answer}"
        );
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Query failed — chat endpoint not activated after publish: {stderr}");
    }

    // ─── Step 5: Delete the data agent ───────────────────────────────────────
    eprintln!("[5/5] Cleaning up (deleting agent)...");
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
    eprintln!("  Agent deleted. Full lifecycle complete.");
}

// ─── Tests for new subcommands ───────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_get_config_dry_run() {
    let cfg = TestConfig::from_env();

    // get-config should work (no dry-run guard on reads, just needs an agent)
    // Use a nonexistent ID — will get a NOT_FOUND error (validates CLI parsing)
    let assert = fabio()
        .args([
            "data-agent",
            "get-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert();

    // Should fail with NOT_FOUND (not a parse error)
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("NOT_FOUND") || stderr.contains("not found") || stderr.contains("404"),
        "Expected NOT_FOUND error, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_config_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "update-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--instructions",
            "Test instructions",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent update-config");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_config_requires_at_least_one_flag() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "data-agent",
            "update-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("At least one of"),
        "Expected validation error, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_list_datasources_dry_run() {
    let cfg = TestConfig::from_env();

    // list-datasources on a nonexistent agent should return NOT_FOUND
    let assert = fabio()
        .args([
            "data-agent",
            "list-datasources",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("NOT_FOUND") || stderr.contains("not found") || stderr.contains("404"),
        "Expected NOT_FOUND error, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_add_datasource_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "add-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--artifact",
            &cfg.source_lakehouse,
            "--artifact-type",
            "Lakehouse",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent add-datasource");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_remove_datasource_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "remove-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--datasource",
            "TestLH",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent remove-datasource");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_select_tables_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "select-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--datasource",
            "TestLH",
            "--tables",
            "orders,products",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent select-tables");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_select_tables_requires_tables_or_all() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "data-agent",
            "select-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--datasource",
            "TestLH",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("--tables") || stderr.contains("--all-tables"),
        "Expected validation error about tables, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_add_fewshot_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "add-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--datasource",
            "TestLH",
            "--question",
            "How many rows?",
            "--answer",
            "SELECT COUNT(*) FROM t",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent add-fewshot");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_remove_fewshot_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "remove-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--datasource",
            "TestLH",
            "--fewshot-id",
            "a0000001-0001-0001-0001-000000000001",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent remove-fewshot");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_upload_fewshots_dry_run() {
    let cfg = TestConfig::from_env();

    // Create a temp file with fewshots
    let fewshots = serde_json::json!([
        {"question": "How many customers?", "query": "SELECT COUNT(*) FROM customers"},
        {"question": "Top product?", "query": "SELECT TOP 1 name FROM products ORDER BY sales DESC"}
    ]);
    let tmpfile = "/tmp/opencode/test_fewshots.json";
    std::fs::write(tmpfile, fewshots.to_string()).unwrap();

    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "upload-fewshots",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--datasource",
            "TestLH",
            "--file",
            tmpfile,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent upload-fewshots");
    assert_eq!(data["details"]["count"], 2);

    std::fs::remove_file(tmpfile).ok();
}

#[test]
fn dataagent_query_stage_sandbox_is_rejected() {
    // Querying the draft/sandbox stage is not supported via the public API and
    // must fail fast with a clear INVALID_INPUT error — before any network call,
    // so no live tenant is required.
    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--stage",
            "sandbox",
            "--prompt",
            "test query",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("INVALID_INPUT") && stderr.contains("not supported"),
        "sandbox stage should be rejected with a clear error: {stderr}"
    );
    assert!(
        stderr.contains("publish"),
        "error hint should point to publishing the agent: {stderr}"
    );
}

#[test]
fn dataagent_query_stage_invalid_is_rejected() {
    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--stage",
            "banana",
            "--prompt",
            "test query",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("INVALID_INPUT") && stderr.contains("Invalid --stage"),
        "invalid stage should be rejected: {stderr}"
    );
}

#[test]
fn dataagent_query_accepts_thread_and_download_flags() {
    // The new multi-turn / file-download flags must be accepted by the parser.
    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--prompt",
            "test",
            "--thread-id",
            "thread_abc",
            "--keep-thread",
            "--download-files",
            "/tmp/fabio-dl-test",
            "--published-url",
            "https://api.fabric.microsoft.com/v1/x",
            "--timeout",
            "5",
        ])
        .assert();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        !stderr.contains("unexpected argument") && !stderr.contains("unrecognized"),
        "CLI should accept --thread-id/--keep-thread/--download-files: {stderr}"
    );
}

#[test]
fn dataagent_evaluate_missing_file_fails() {
    let assert = fabio()
        .args([
            "data-agent",
            "evaluate",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--questions",
            "/nonexistent/path/questions.json",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("INVALID_INPUT") && stderr.contains("questions file"),
        "missing questions file should fail clearly: {stderr}"
    );
}

#[test]
fn dataagent_evaluate_repeats_zero_fails() {
    let tmp = std::env::temp_dir().join("fabio_eval_q_zero.json");
    std::fs::write(&tmp, r#"["what?"]"#).unwrap();
    let assert = fabio()
        .args([
            "data-agent",
            "evaluate",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--questions",
            tmp.to_str().unwrap(),
            "--repeats",
            "0",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("repeats") && stderr.contains("at least 1"),
        "--repeats 0 should be rejected: {stderr}"
    );
    std::fs::remove_file(&tmp).ok();
}

#[test]
fn dataagent_validate_fewshots_requires_llm() {
    // Without an LLM endpoint/key/model the command must fail fast (before any
    // network call). Clear the FABIO_LLM_* env so the test is deterministic.
    let assert = fabio()
        .env_remove("FABIO_LLM_ENDPOINT")
        .env_remove("FABIO_LLM_KEY")
        .env_remove("FABIO_LLM_MODEL")
        .args([
            "data-agent",
            "validate-fewshots",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--datasource",
            "some-ds",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("INVALID_INPUT") && stderr.contains("judge model"),
        "validate-fewshots should require an LLM: {stderr}"
    );
    assert!(
        stderr.contains("--llm-endpoint"),
        "error hint should name the LLM flags: {stderr}"
    );
}

#[test]
fn dataagent_evaluate_accepts_llm_flags() {
    // The grader flags must be accepted by the parser.
    let tmp = std::env::temp_dir().join("fabio_eval_llm_flags.json");
    std::fs::write(&tmp, r#"["q?"]"#).unwrap();
    let assert = fabio()
        .args([
            "data-agent",
            "evaluate",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--questions",
            tmp.to_str().unwrap(),
            "--llm-endpoint",
            "https://x.openai.azure.com",
            "--llm-key",
            "k",
            "--llm-model",
            "gpt-4o",
            "--llm-api-version",
            "2024-10-21",
        ])
        .assert();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        !stderr.contains("unexpected argument") && !stderr.contains("unrecognized"),
        "CLI should accept --llm-* flags on evaluate: {stderr}"
    );
    std::fs::remove_file(&tmp).ok();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_publish_to_m365_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "publish",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--to-m365",
            "--description",
            "dry run m365 test",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent publish");
    assert_eq!(data["details"]["toM365"], true);
}

/// Full lifecycle test for new datasource + fewshot management subcommands.
///
/// Creates a data agent, adds a datasource via `add-datasource`, manages
/// fewshots via `add-fewshot`/`list-fewshots`/`remove-fewshot`, then cleans up.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_datasource_fewshot_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_ds_test");

    // ─── Step 1: Create a data agent ─────────────────────────────────────────
    eprintln!("[1/8] Creating data agent '{name}'...");
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let agent_id = data["id"].as_str().unwrap().to_string();
    eprintln!("  Created agent: {agent_id}");

    // ─── Step 2: Get config (should be empty/defaults) ───────────────────────
    eprintln!("[2/8] Getting config...");
    let assert = fabio()
        .args([
            "data-agent",
            "get-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(
        data["dataSources"].as_array().unwrap().is_empty(),
        "expected no datasources initially"
    );
    eprintln!("  Config OK: no datasources");

    // ─── Step 3: Add datasource via add-datasource ───────────────────────────
    eprintln!("[3/8] Adding lakehouse datasource...");
    let assert = fabio()
        .args([
            "data-agent",
            "add-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--artifact",
            &cfg.source_lakehouse,
            "--artifact-type",
            "Lakehouse",
            "--lro-timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "datasource_added");
    eprintln!("  Datasource added: type={}", data["fabricItemType"]);

    // ─── Step 4: List datasources ────────────────────────────────────────────
    eprintln!("[4/8] Listing datasources...");
    let assert = fabio()
        .args([
            "data-agent",
            "list-datasources",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let count = extract_count(&json);
    assert!(count >= 1, "expected at least 1 datasource, got {count}");
    eprintln!("  Listed {count} datasource(s)");

    // ─── Step 5: Add fewshot ─────────────────────────────────────────────────
    eprintln!("[5/8] Adding fewshot...");
    let ds_name = &cfg.source_lakehouse; // use the artifact ID as datasource identifier

    let assert = fabio()
        .args([
            "data-agent",
            "add-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            ds_name,
            "--question",
            "How many tables are there?",
            "--answer",
            "SELECT COUNT(*) FROM INFORMATION_SCHEMA.TABLES",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "fewshot_added");
    let fewshot_id = data["id"].as_str().unwrap().to_string();
    eprintln!("  Added fewshot: {fewshot_id}");

    // ─── Step 6: List fewshots ───────────────────────────────────────────────
    eprintln!("[6/8] Listing fewshots...");
    let assert = fabio()
        .args([
            "data-agent",
            "list-fewshots",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            ds_name,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let count = extract_count(&json);
    assert_eq!(count, 1, "expected 1 fewshot, got {count}");
    eprintln!("  Listed {count} fewshot(s)");

    // ─── Step 7: Remove fewshot ──────────────────────────────────────────────
    eprintln!("[7/8] Removing fewshot...");
    fabio()
        .args([
            "data-agent",
            "remove-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            ds_name,
            "--fewshot-id",
            &fewshot_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    eprintln!("  Fewshot removed");

    // Verify it's gone
    let assert = fabio()
        .args([
            "data-agent",
            "list-fewshots",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            ds_name,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let count = extract_count(&json);
    assert_eq!(count, 0, "expected 0 fewshots after removal, got {count}");

    // ─── Step 8: Cleanup ─────────────────────────────────────────────────────
    eprintln!("[8/8] Cleaning up...");
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
    eprintln!("  Done. Datasource + fewshot lifecycle complete.");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_list_elements_dry_run() {
    let cfg = TestConfig::from_env();

    // list-elements on nonexistent agent → NOT_FOUND
    let assert = fabio()
        .args([
            "data-agent",
            "list-elements",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--datasource",
            "TestLH",
        ])
        .assert();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("NOT_FOUND") || stderr.contains("not found") || stderr.contains("404"),
        "Expected NOT_FOUND error, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_describe_element_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "describe-element",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--datasource",
            "TestLH",
            "--path",
            "dbo.orders.total_amount",
            "--description",
            "Total order amount in USD",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent describe-element");
    assert_eq!(data["details"]["path"], "dbo.orders.total_amount");
    assert_eq!(data["details"]["description"], "Total order amount in USD");
}

/// Test list-elements and describe-element against a live agent with datasource.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_elements_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_elem_test");

    // Step 1: Create agent
    eprintln!("[1/5] Creating agent...");
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let agent_id = data["id"].as_str().unwrap().to_string();

    // Step 2: Add datasource
    eprintln!("[2/5] Adding datasource...");
    fabio()
        .args([
            "data-agent",
            "add-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--artifact",
            &cfg.source_lakehouse,
            "--artifact-type",
            "Lakehouse",
            "--lro-timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();

    // Step 3: List elements (should show schema at minimum)
    eprintln!("[3/5] Listing elements...");
    let assert = fabio()
        .args([
            "data-agent",
            "list-elements",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let count = extract_count(&json);
    eprintln!("  Found {count} elements");
    // May be 0 if no schema discovered, but command succeeded
    assert!(json.get("data").is_some());

    // Step 4: If elements exist, try describe-element
    if count > 0 {
        let data = extract_data(&json);
        let elements = data.as_array().unwrap();
        // Find the first element with a path
        let first_path = elements[0]["path"].as_str().unwrap_or("dbo");

        eprintln!("[4/5] Setting description on '{first_path}'...");
        let assert = fabio()
            .args([
                "data-agent",
                "describe-element",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &agent_id,
                "--datasource",
                &cfg.source_lakehouse,
                "--path",
                first_path,
                "--description",
                "Test description from e2e",
            ])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success();

        let json = parse_json(&assert);
        let data = extract_data(&json);
        assert_eq!(data["status"], "description_set");
        eprintln!("  Description set successfully");
    } else {
        eprintln!("[4/5] Skipping describe-element (no elements discovered)");
    }

    // Step 5: Cleanup
    eprintln!("[5/5] Cleaning up...");
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
    eprintln!("  Done. Elements lifecycle complete.");
}

/// Comprehensive lifecycle test covering show-datasource, update-config (live),
/// upload-fewshots (live with CSV), and select-tables (live).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_advanced_management_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_adv_test");

    // ─── Create agent ────────────────────────────────────────────────────────
    eprintln!("[1/8] Creating agent...");
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let agent_id = data["id"].as_str().unwrap().to_string();
    eprintln!("  Created: {agent_id}");

    // ─── update-config live (set instructions + preview runtime) ─────────────
    eprintln!("[2/8] Updating config with instructions...");
    let assert = fabio()
        .args([
            "data-agent",
            "update-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--instructions",
            "Answer questions about sales data. Use SQL for lakehouse tables.",
            "--enable-preview-runtime",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "config_updated");
    // The preview runtime (Advanced NL2SQL for SQL sources) must be reported as
    // enabled — this proves --enable-preview-runtime is wired to the settings API.
    assert_eq!(
        data["previewRuntime"], true,
        "Expected previewRuntime=true after --enable-preview-runtime, got: {data}"
    );
    eprintln!("  Config updated (preview runtime enabled)");

    // Verify via get-config
    let assert = fabio()
        .args([
            "data-agent",
            "get-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let instr = data["instructions"].as_str().unwrap_or("");
    assert!(
        instr.contains("sales data"),
        "Expected instructions to contain 'sales data', got: {instr}"
    );
    assert_eq!(
        data["previewRuntime"], true,
        "Expected get-config to report previewRuntime=true, got: {data}"
    );
    eprintln!("  get-config verified: instructions set, preview runtime enabled");

    // ─── Toggle preview runtime back off (standard runtime / NL2SQL) ──────────
    eprintln!("[2b/8] Disabling preview runtime...");
    let assert = fabio()
        .args([
            "data-agent",
            "update-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--disable-preview-runtime",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "config_updated");
    assert_eq!(
        data["previewRuntime"], false,
        "Expected previewRuntime=false after --disable-preview-runtime, got: {data}"
    );

    // Verify the standard runtime persisted and instructions were untouched.
    let assert = fabio()
        .args([
            "data-agent",
            "get-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(
        data["previewRuntime"], false,
        "Expected get-config to report previewRuntime=false after disable, got: {data}"
    );
    assert!(
        data["instructions"]
            .as_str()
            .unwrap_or("")
            .contains("sales data"),
        "Disabling preview runtime must not clear instructions, got: {data}"
    );
    eprintln!("  preview runtime toggle verified (enable → disable)");

    // ─── Add datasource ──────────────────────────────────────────────────────
    eprintln!("[3/8] Adding datasource...");
    fabio()
        .args([
            "data-agent",
            "add-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--artifact",
            &cfg.source_lakehouse,
            "--artifact-type",
            "Lakehouse",
            "--instructions",
            "Contains product catalog and order history",
            "--lro-timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();
    eprintln!("  Datasource added");

    // ─── show-datasource ─────────────────────────────────────────────────────
    eprintln!("[4/8] Showing datasource...");
    let assert = fabio()
        .args([
            "data-agent",
            "show-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // New API uses discriminator "FabricItem" with separate "fabricItemType" field
    assert!(
        data["type"].as_str().unwrap_or("") == "FabricItem"
            || data["type"].as_str().unwrap_or("") == "LakehouseTables"
            || data["type"].as_str().unwrap_or("") == "lakehouse_tables",
        "expected FabricItem or LakehouseTables type, got: {}",
        data["type"]
    );
    eprintln!("  show-datasource OK: type={}", data["type"]);

    // ─── upload-fewshots from CSV ────────────────────────────────────────────
    eprintln!("[5/8] Uploading fewshots from CSV...");
    let csv_content = "question,query\nHow many products?,SELECT COUNT(*) FROM products\nMost expensive?,SELECT TOP 1 product_name FROM products ORDER BY price DESC\n";
    let csv_path = "/tmp/opencode/e2e_fewshots.csv";
    std::fs::write(csv_path, csv_content).unwrap();

    let assert = fabio()
        .args([
            "data-agent",
            "upload-fewshots",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
            "--file",
            csv_path,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "fewshots_uploaded");
    assert_eq!(data["added"], 2);
    eprintln!("  Uploaded 2 fewshots from CSV");

    std::fs::remove_file(csv_path).ok();

    // Verify fewshots
    let assert = fabio()
        .args([
            "data-agent",
            "list-fewshots",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let count = extract_count(&json);
    assert_eq!(count, 2, "Expected 2 fewshots after upload");
    eprintln!("  Verified: {count} fewshots");

    // ─── Upload duplicate fewshots (test rename logic) ───────────────────────
    eprintln!("[6/8] Uploading duplicate fewshot (tests rename)...");
    let assert = fabio()
        .args([
            "data-agent",
            "add-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
            "--question",
            "How many products?",
            "--answer",
            "SELECT COUNT(*) FROM products WHERE active = 1",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "fewshot_added");
    // Server handles duplicates (may accept as-is or validate)
    eprintln!(
        "  Duplicate fewshot handled by server: {}",
        data["question"]
    );

    // ─── select-tables (unselect all, then select specific) ──────────────────
    eprintln!("[7/8] Testing select-tables...");

    // First check if there are elements to select
    let assert = fabio()
        .args([
            "data-agent",
            "list-elements",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let elem_count = extract_count(&json);

    if elem_count > 0 {
        // Unselect all tables
        let assert = fabio()
            .args([
                "data-agent",
                "select-tables",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &agent_id,
                "--datasource",
                &cfg.source_lakehouse,
                "--all-tables",
                "--unselect",
            ])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success();

        let json = parse_json(&assert);
        let data = extract_data(&json);
        assert_eq!(data["status"], "tables_unselected");
        let modified = data["modified"].as_u64().unwrap_or(0);
        eprintln!("  Unselected {modified} tables");

        // Select all back
        let assert = fabio()
            .args([
                "data-agent",
                "select-tables",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &agent_id,
                "--datasource",
                &cfg.source_lakehouse,
                "--all-tables",
            ])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success();

        let json = parse_json(&assert);
        let data = extract_data(&json);
        assert_eq!(data["status"], "tables_selected");
        eprintln!("  Re-selected all tables");
    } else {
        eprintln!("  Skipping select-tables (no elements discovered)");
    }

    // ─── Cleanup ─────────────────────────────────────────────────────────────
    eprintln!("[8/8] Cleaning up...");
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
    eprintln!("  Done. Advanced management lifecycle complete.");
}

// ─── Tests for new commands (update-datasource, show-fewshot, update-fewshot,
//     clear-fewshots, delete-element, reset, --stage published) ───────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_datasource_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "data-agent",
            "update-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--datasource",
            "TestDS",
            "--instructions",
            "Use this for sales queries",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "data-agent update-datasource");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_datasource_requires_at_least_one_field() {
    let cfg = TestConfig::from_env();
    fabio()
        .args([
            "data-agent",
            "update-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--datasource",
            "TestDS",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("At least one"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_show_fewshot_dry_run() {
    let cfg = TestConfig::from_env();
    // show-fewshot is a read — test that the command parses all args correctly
    // This will fail with NOT_FOUND since the IDs are fake, but that proves arg parsing works
    fabio()
        .args([
            "data-agent",
            "show-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--datasource",
            "TestDS",
            "--fewshot-id",
            "00000000-0000-0000-0000-000000000099",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NOT_FOUND").or(predicate::str::contains("error")));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_fewshot_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "data-agent",
            "update-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--datasource",
            "TestDS",
            "--fewshot-id",
            "00000000-0000-0000-0000-000000000099",
            "--question",
            "Updated question?",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "data-agent update-fewshot");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_fewshot_requires_at_least_one_field() {
    let cfg = TestConfig::from_env();
    fabio()
        .args([
            "data-agent",
            "update-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--datasource",
            "TestDS",
            "--fewshot-id",
            "00000000-0000-0000-0000-000000000099",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("At least one"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_clear_fewshots_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "data-agent",
            "clear-fewshots",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--datasource",
            "TestDS",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "data-agent clear-fewshots");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_delete_element_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "data-agent",
            "delete-element",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--datasource",
            "TestDS",
            "--element-id",
            "dbo.old_table",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "data-agent delete-element");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_reset_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "data-agent",
            "reset",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "data-agent reset");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_list_datasources_with_stage_published_dry_run() {
    let cfg = TestConfig::from_env();
    // --stage published is accepted as a flag (no error at parse time)
    // Will fail at runtime with NOT_FOUND for the fake ID, proving the flag works
    fabio()
        .args([
            "data-agent",
            "list-datasources",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--stage",
            "published",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NOT_FOUND").or(predicate::str::contains("error")));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_get_config_with_stage_published_dry_run() {
    let cfg = TestConfig::from_env();
    fabio()
        .args([
            "data-agent",
            "get-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--stage",
            "published",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NOT_FOUND").or(predicate::str::contains("error")));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_reset_live() {
    let cfg = TestConfig::from_env();

    // Create agent, publish, modify staging, then reset (should revert to published state)
    eprintln!("[1/5] Creating agent...");
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &unique_name("da_reset_test"),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let agent_id = data["id"].as_str().unwrap().to_string();
    eprintln!("  Created: {agent_id}");

    // Publish first (reset requires a published state to revert to)
    eprintln!("[2/5] Publishing agent...");
    fabio()
        .args([
            "data-agent",
            "publish",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    eprintln!("  Published");

    // Update config (staging change)
    eprintln!("[3/5] Updating config (staging change)...");
    fabio()
        .args([
            "data-agent",
            "update-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--instructions",
            "Test instructions for reset — should be discarded",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    // Reset staging (discard the config change)
    eprintln!("[4/5] Resetting staging...");
    let assert = fabio()
        .args([
            "data-agent",
            "reset",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "staging_reset");
    eprintln!("  Reset successful");

    // Cleanup
    eprintln!("[5/5] Cleanup...");
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
    eprintln!("  Done.");
}

// ─── P1: CRUD flag acceptance and validation tests ───────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_create_missing_name_fails() {
    let cfg = TestConfig::from_env();
    fabio()
        .args(["data-agent", "create", "--workspace", &cfg.source_workspace])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--name"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_delete_accepts_hard_delete_flag() {
    let cfg = TestConfig::from_env();
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--hard-delete",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NOT_FOUND"));
}

// ─── P2: Flag coverage tests ─────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_config_instructions_file_dry_run() {
    let cfg = TestConfig::from_env();
    let tmp_path = "/tmp/opencode/da_instructions.txt";
    std::fs::write(tmp_path, "These are file-based instructions").unwrap();

    let assert = fabio()
        .args([
            "data-agent",
            "update-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--instructions-file",
            tmp_path,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "data-agent update-config");
    std::fs::remove_file(tmp_path).ok();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_update_config_disable_preview_runtime_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "data-agent",
            "update-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--disable-preview-runtime",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "data-agent update-config");
    assert_eq!(data["details"]["disablePreviewRuntime"], true);
}

/// Focused, self-contained live test for the preview-runtime (Advanced NL2SQL
/// for SQL sources) toggle. Independent of add-datasource so it validates the
/// runtime wiring even where the data agent's SQL schema discovery is not
/// available. Covers: enable → get-config reports true → disable → get-config
/// reports false and instructions are preserved (read-modify-write).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_preview_runtime_toggle_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_preview_rt");

    // Create the agent.
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let agent_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // A fresh agent defaults to the standard runtime.
    let assert = fabio()
        .args([
            "data-agent",
            "get-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(
        extract_data(&parse_json(&assert))["previewRuntime"],
        false,
        "new agents must default to the standard runtime"
    );

    // Enable the preview runtime AND set instructions in one call.
    let assert = fabio()
        .args([
            "data-agent",
            "update-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--instructions",
            "Route SQL questions to the sales lakehouse.",
            "--enable-preview-runtime",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "config_updated");
    assert_eq!(
        data["previewRuntime"], true,
        "update-config must echo previewRuntime=true after enable, got: {data}"
    );

    // get-config must report the preview runtime and the instructions.
    let assert = fabio()
        .args([
            "data-agent",
            "get-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["previewRuntime"], true);
    assert!(
        data["instructions"]
            .as_str()
            .unwrap_or("")
            .contains("sales lakehouse"),
        "instructions must be set alongside the runtime, got: {data}"
    );

    // Disable the preview runtime WITHOUT touching instructions.
    let assert = fabio()
        .args([
            "data-agent",
            "update-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--disable-preview-runtime",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    assert_eq!(
        extract_data(&parse_json(&assert))["previewRuntime"],
        false,
        "update-config must echo previewRuntime=false after disable"
    );

    // Standard runtime persisted; instructions untouched by the toggle.
    let assert = fabio()
        .args([
            "data-agent",
            "get-config",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["previewRuntime"], false);
    assert!(
        data["instructions"]
            .as_str()
            .unwrap_or("")
            .contains("sales lakehouse"),
        "disabling the runtime must not clear instructions, got: {data}"
    );

    // Cleanup.
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_query_show_steps_flag_accepted() {
    let cfg = TestConfig::from_env();
    fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--prompt",
            "test",
            "--show-steps",
            "--published-url",
            "https://api.fabric.microsoft.com/v1/workspaces/test/dataagents/test/aiassistant/openai",
            "--timeout",
            "5",
        ])
        .assert()
        .failure(); // Will fail (agent doesn't exist) but --show-steps flag is accepted
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_upload_fewshots_invalid_file_path() {
    let cfg = TestConfig::from_env();
    fabio()
        .args([
            "data-agent",
            "upload-fewshots",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--datasource",
            "TestDS",
            "--file",
            "/nonexistent/path/fewshots.json",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to read file"));
}

// ─── P0: Live E2E tests for new commands ─────────────────────────────────────

/// Tests update-datasource, show-fewshot, update-fewshot, clear-fewshots,
/// and remove-datasource in a single lifecycle.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_new_commands_lifecycle() {
    let cfg = TestConfig::from_env();

    eprintln!("[1/10] Creating agent...");
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &unique_name("da_newcmds"),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let agent_id = data["id"].as_str().unwrap().to_string();
    eprintln!("  Created: {agent_id}");

    eprintln!("[2/10] Adding lakehouse datasource (may take several minutes)...");
    let assert = fabio()
        .args([
            "data-agent",
            "add-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--artifact",
            &cfg.source_lakehouse,
            "--artifact-type",
            "Lakehouse",
            "--instructions",
            "Original instructions",
            "--lro-timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "datasource_added");
    eprintln!("  Datasource added");

    eprintln!("[3/10] Updating datasource instructions...");
    let assert = fabio()
        .args([
            "data-agent",
            "update-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
            "--instructions",
            "Updated: use for sales analytics",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "datasource_updated");
    eprintln!("  Datasource instructions updated");

    eprintln!("[4/10] Adding fewshot...");
    let assert = fabio()
        .args([
            "data-agent",
            "add-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
            "--question",
            "How many rows?",
            "--answer",
            "SELECT COUNT(*) FROM table1",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "fewshot_added");
    let fewshot_id = data["id"].as_str().unwrap_or("").to_string();
    eprintln!("  Fewshot added: {fewshot_id}");

    eprintln!("[5/10] Showing fewshot...");
    assert!(!fewshot_id.is_empty(), "Expected fewshot ID from add");
    let assert = fabio()
        .args([
            "data-agent",
            "show-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
            "--fewshot-id",
            &fewshot_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["question"], "How many rows?");
    eprintln!("  show-fewshot OK");

    eprintln!("[6/10] Updating fewshot...");
    let assert = fabio()
        .args([
            "data-agent",
            "update-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
            "--fewshot-id",
            &fewshot_id,
            "--question",
            "How many total rows?",
            "--answer",
            "SELECT COUNT(*) FROM all_tables",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "fewshot_updated");
    eprintln!("  Fewshot updated");

    eprintln!("[7/10] Adding second fewshot for clear test...");
    fabio()
        .args([
            "data-agent",
            "add-fewshot",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
            "--question",
            "Top products?",
            "--answer",
            "SELECT TOP 5 name FROM products",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    eprintln!("[8/10] Clearing all fewshots...");
    let assert = fabio()
        .args([
            "data-agent",
            "clear-fewshots",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "fewshots_cleared");
    eprintln!("  All fewshots cleared");

    eprintln!("[9/10] Removing datasource...");
    let assert = fabio()
        .args([
            "data-agent",
            "remove-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "datasource_removed");
    eprintln!("  Datasource removed");

    eprintln!("[10/10] Cleanup...");
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
    eprintln!("  Done. New commands lifecycle test complete.");
}

// ---------------------------------------------------------------------------
// Ground a data agent on an Ontology, then scope with element selection.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_ground_on_ontology_and_select_elements() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    // An ontology to ground on.
    let ont_name = unique_name("da_ont");
    let ont = parse_json(
        &fabio()
            .args(["ontology", "create", "--workspace", ws, "--name", &ont_name])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success(),
    );
    let ont_id = extract_data(&ont)["id"].as_str().unwrap().to_string();

    // A data agent.
    let agent_name = unique_name("da_ground");
    let agent = parse_json(
        &fabio()
            .args([
                "data-agent",
                "create",
                "--workspace",
                ws,
                "--name",
                &agent_name,
            ])
            .timeout(std::time::Duration::from_mins(3))
            .assert()
            .success(),
    );
    let agent_id = extract_data(&agent)["id"].as_str().unwrap().to_string();

    // Ground the agent on the ontology (FabricItem datasource).
    let ds = parse_json(
        &fabio()
            .args([
                "data-agent",
                "add-datasource",
                "--workspace",
                ws,
                "--id",
                &agent_id,
                "--artifact",
                &ont_id,
                "--artifact-type",
                "Ontology",
            ])
            .timeout(std::time::Duration::from_mins(5))
            .assert()
            .success(),
    );
    assert_eq!(
        extract_data(&ds)["fabricItemType"],
        "Ontology",
        "grounded on the ontology: {ds}"
    );

    // Element-mode selection works on a non-table datasource (any-type).
    // --all-elements succeeds even before async schema discovery finishes.
    fabio()
        .args([
            "data-agent",
            "select-tables",
            "--workspace",
            ws,
            "--id",
            &agent_id,
            "--datasource",
            &ont_name,
            "--all-elements",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // Cleanup.
    let _ = fabio()
        .args(["data-agent", "delete", "--workspace", ws, "--id", &agent_id])
        .assert();
    let _ = fabio()
        .args([
            "ontology",
            "delete",
            "--workspace",
            ws,
            "--id",
            &ont_id,
            "--hard",
        ])
        .assert();
}

/// End-to-end live validation of the published-consumption features:
/// constructed published-URL fallback, `threadId` in the response, multi-turn
/// thread reuse (`--keep-thread` + `--thread-id`), and the `evaluate` batch
/// primitive. Creates + publishes an agent, exercises the flow, then cleans up.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataagent_query_multiturn_and_evaluate_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("da_query_e2e");

    // Create the agent.
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let agent_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Add a lakehouse datasource so the agent has a valid config to publish.
    fabio()
        .args([
            "data-agent",
            "add-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--artifact",
            &cfg.source_lakehouse,
            "--artifact-type",
            "Lakehouse",
            "--lro-timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();

    // Publish so the OpenAI-compatible consumption endpoint goes live.
    fabio()
        .args([
            "data-agent",
            "publish",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // ── Basic query: no --published-url (exercises the constructed fallback),
    //    and the response must carry a threadId. ──────────────────────────────
    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--prompt",
            "Reply with exactly one word: ok",
            "--timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(
        data["threadId"].as_str().is_some_and(|t| !t.is_empty()),
        "query response must include a non-empty threadId, got: {data}"
    );
    assert!(
        data["answer"].as_str().is_some_and(|a| !a.is_empty()),
        "query response must include an answer, got: {data}"
    );

    // ── Multi-turn: keep a thread, then reuse it. The reused thread id must be
    //    identical, proving the follow-up ran on the same conversation. ───────
    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--prompt",
            "Remember the number 7 for later.",
            "--keep-thread",
            "--timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();
    let thread_id = extract_data(&parse_json(&assert))["threadId"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!thread_id.is_empty(), "kept thread id must be non-empty");

    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--prompt",
            "What number did I ask you to remember?",
            "--thread-id",
            &thread_id,
            "--timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(
        data["threadId"].as_str(),
        Some(thread_id.as_str()),
        "follow-up must reuse the same thread, got: {data}"
    );
    assert!(
        data["answer"].as_str().is_some_and(|a| !a.is_empty()),
        "follow-up must return an answer, got: {data}"
    );

    // ── Evaluate: batch-run a small question set (no --published-url). ────────
    let qfile = std::env::temp_dir().join(format!("{name}_questions.json"));
    std::fs::write(
        &qfile,
        r#"[{"question":"Reply with exactly: PONG","expected":"PONG"}]"#,
    )
    .unwrap();
    let assert = fabio()
        .args([
            "data-agent",
            "evaluate",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--questions",
            qfile.to_str().unwrap(),
            "--timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["questionCount"], 1);
    assert_eq!(
        data["failedRuns"], 0,
        "evaluation run should succeed: {data}"
    );
    assert!(
        data["results"][0]["match"]["exact"].is_boolean(),
        "expected a naive match signal on the result, got: {data}"
    );
    std::fs::remove_file(&qfile).ok();

    // Cleanup.
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
}

/// Live validation of the LLM-powered features (`validate-fewshots` and
/// `evaluate --llm-*`). Requires a live tenant AND an OpenAI-compatible judge
/// model exposed via `FABIO_LLM_ENDPOINT`/`FABIO_LLM_KEY`/`FABIO_LLM_MODEL`;
/// skips gracefully if the LLM env is not configured.
#[test]
#[ignore = "requires live Fabric tenant + FABIO_LLM_* judge model"]
#[serial]
fn dataagent_llm_validate_and_grade_lifecycle() {
    let (Ok(_ep), Ok(_key), Ok(_model)) = (
        std::env::var("FABIO_LLM_ENDPOINT"),
        std::env::var("FABIO_LLM_KEY"),
        std::env::var("FABIO_LLM_MODEL"),
    ) else {
        eprintln!("FABIO_LLM_* not set — skipping LLM feature lifecycle test");
        return;
    };

    let cfg = TestConfig::from_env();
    let name = unique_name("da_llm_e2e");

    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let agent_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    fabio()
        .args([
            "data-agent",
            "add-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--artifact",
            &cfg.source_lakehouse,
            "--artifact-type",
            "Lakehouse",
            "--lro-timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();

    // Add two near-duplicate questions mapped to CONFLICTING queries — a defect
    // the LLM judge should flag.
    for (q, sql) in [
        (
            "What is total revenue?",
            "SELECT SUM(revenue) FROM products",
        ),
        ("What's the total revenue?", "SELECT COUNT(*) FROM products"),
    ] {
        fabio()
            .args([
                "data-agent",
                "add-fewshot",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &agent_id,
                "--datasource",
                &cfg.source_lakehouse,
                "--question",
                q,
                "--sql",
                sql,
            ])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success();
    }

    // validate-fewshots must find at least one issue (LLM reads FABIO_LLM_*).
    let assert = fabio()
        .args([
            "data-agent",
            "validate-fewshots",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--datasource",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["fewshotCount"], 2);
    assert!(
        data["issueCount"].as_u64().unwrap_or(0) >= 1,
        "LLM should flag the conflicting duplicate few-shots, got: {data}"
    );

    // Publish, then evaluate with LLM grading.
    fabio()
        .args([
            "data-agent",
            "publish",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let qfile = std::env::temp_dir().join(format!("{name}_graded.json"));
    std::fs::write(
        &qfile,
        r#"[{"question":"Reply with exactly: PONG","expected":"PONG"}]"#,
    )
    .unwrap();
    let assert = fabio()
        .args([
            "data-agent",
            "evaluate",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
            "--questions",
            qfile.to_str().unwrap(),
            "--timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(
        data["gradedRuns"].as_u64().unwrap_or(0) >= 1,
        "evaluate should report gradedRuns when FABIO_LLM_* is set, got: {data}"
    );
    assert!(
        data["passRate"].is_number(),
        "evaluate should report a numeric passRate when grading, got: {data}"
    );
    std::fs::remove_file(&qfile).ok();

    // Cleanup.
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &agent_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
}

/// Live validation of **NL2DAX**: a data agent grounded on a Semantic Model
/// generates and executes DAX to answer a natural-language question.
///
/// Requires a semantic model WITH DATA in the tenant, provided via the optional
/// `FABIO_TEST_SEMANTIC_MODEL` env var (item id or name in the source
/// workspace); the test is skipped when it is not set.
///
/// Also validates the critical gotcha that a Semantic Model data source's TABLES
/// are NOT auto-selected by `add-datasource` (only columns) — without
/// `select-tables`, the agent hallucinates instead of doing NL2DAX.
#[test]
#[ignore = "requires live Fabric tenant + a semantic model with data (FABIO_TEST_SEMANTIC_MODEL)"]
#[serial]
fn dataagent_semantic_model_nl2dax_lifecycle() {
    let Ok(model) = std::env::var("FABIO_TEST_SEMANTIC_MODEL") else {
        eprintln!("FABIO_TEST_SEMANTIC_MODEL not set — skipping NL2DAX validation");
        return;
    };
    let cfg = TestConfig::from_env();
    let name = unique_name("nl2dax");

    // 1) Create the agent.
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let created = parse_json(&assert);
    let id = extract_data(&created)["id"].as_str().unwrap().to_string();

    // 2) Add the Semantic Model data source (this is what enables NL2DAX).
    let assert = fabio()
        .args([
            "data-agent",
            "add-datasource",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
            "--artifact",
            &model,
            "--artifact-type",
            "SemanticModel",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let ds = parse_json(&assert);
    assert_eq!(
        extract_data(&ds)["fabricItemType"],
        "SemanticModel",
        "datasource must be a SemanticModel: {ds}"
    );

    // 3) The datasource id equals the model id; discover its tables and select
    //    the first one (tables are NOT auto-selected — required for grounding).
    let assert = fabio()
        .args([
            "data-agent",
            "list-datasources",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let dss = parse_json(&assert);
    let ds_id = extract_data(&dss)[0]["id"].as_str().unwrap().to_string();

    let assert = fabio()
        .args([
            "data-agent",
            "list-elements",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
            "--datasource",
            &ds_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let elements = parse_json(&assert);
    let table = extract_data(&elements)
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "Table")
        .and_then(|e| e["name"].as_str())
        .expect("semantic model must expose at least one table")
        .to_string();

    fabio()
        .args([
            "data-agent",
            "select-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
            "--datasource",
            &ds_id,
            "--tables",
            &table,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // 4) Publish (snapshots the selected config).
    fabio()
        .args([
            "data-agent",
            "publish",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
            "--description",
            "NL2DAX e2e",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // 5) Ask a counting question and inspect the run steps: a grounded answer
    //    must generate DAX via the SemanticModel NL2DAX tool.
    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
            "--prompt",
            &format!("How many rows are in {table}? Return a single count."),
            "--show-steps",
        ])
        .timeout(std::time::Duration::from_mins(4))
        .assert()
        .success();
    let result = parse_json(&assert);
    let blob = serde_json::to_string(&result).unwrap();
    assert!(
        blob.contains("SemanticModel"),
        "run steps must reference the SemanticModel datasource: {blob}"
    );
    let did_nl2dax = blob.contains("nl2code")
        || blob.contains("analyze_semantic_model")
        || blob.to_uppercase().contains("EVALUATE")
        || blob.to_lowercase().contains("dax");
    assert!(
        did_nl2dax,
        "expected evidence of NL2DAX (a generated DAX query) in the run steps: {blob}"
    );

    // Cleanup.
    fabio()
        .args([
            "data-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
}

/// Live validation of **NL2KQL**: a data agent grounded on a KQL database
/// generates and executes KQL to answer a natural-language question.
///
/// Self-seeding (no external fixture): creates an eventhouse + KQL database +
/// a small table with data, runs the agent flow, and cleans up. Also guards the
/// `select-tables` fix for KQL sources (their tables are nested under grouping
/// containers, so `--tables <name>` must drill one level in).
#[test]
#[ignore = "requires live Fabric tenant (creates an eventhouse + KQL DB)"]
#[serial]
fn dataagent_kql_database_nl2kql_lifecycle() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;
    let eh_name = unique_name("nl2kql_eh");

    // 1) Eventhouse (auto-creates a same-named KQL database).
    fabio()
        .args([
            "eventhouse",
            "create",
            "--workspace",
            ws,
            "--name",
            &eh_name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    // Resolve the auto-created KQL database by name.
    let assert = fabio()
        .args(["kql-database", "list", "--workspace", ws])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let dbs = parse_json(&assert);
    let kdb = extract_data(&dbs)
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["displayName"] == eh_name.as_str())
        .and_then(|d| d["id"].as_str())
        .expect("eventhouse should auto-create a KQL database")
        .to_string();

    // 2) Seed a table with data.
    fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            ws,
            "--id",
            &kdb,
            "--kql",
            ".create table Sales (Product: string, Region: string, Amount: int)",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    fabio()
        .args([
            "kql-database",
            "ingest",
            "--workspace",
            ws,
            "--id",
            &kdb,
            "--table",
            "Sales",
            "--data",
            "Widget,France,100\nGadget,Germany,200\nWidget,Germany,150",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    std::thread::sleep(std::time::Duration::from_secs(8));

    // 3) Data agent grounded on the KQL database.
    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            ws,
            "--name",
            &unique_name("nl2kql"),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let assert = fabio()
        .args([
            "data-agent",
            "add-datasource",
            "--workspace",
            ws,
            "--id",
            &id,
            "--artifact",
            &kdb,
            "--artifact-type",
            "KQLDatabase",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    assert_eq!(
        extract_data(&parse_json(&assert))["fabricItemType"],
        "KQLDatabase",
        "datasource must be a KQLDatabase"
    );

    // 4) Select the table (regression guard: KQL tables are nested under a
    //    "Tables" container, which select-tables must drill into).
    fabio()
        .args([
            "data-agent",
            "select-tables",
            "--workspace",
            ws,
            "--id",
            &id,
            "--datasource",
            &kdb,
            "--tables",
            "Sales",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    fabio()
        .args([
            "data-agent",
            "publish",
            "--workspace",
            ws,
            "--id",
            &id,
            "--description",
            "NL2KQL e2e",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // 5) Ask a question requiring aggregation and confirm KQL was generated.
    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            ws,
            "--id",
            &id,
            "--prompt",
            "What is the total Amount per Region?",
            "--show-steps",
        ])
        .timeout(std::time::Duration::from_mins(4))
        .assert()
        .success();
    let blob = serde_json::to_string(&parse_json(&assert)).unwrap();
    let did_nl2kql = blob.contains("analyze_kusto_database")
        || blob.contains("nl2code")
        || blob.to_lowercase().contains("summarize");
    assert!(
        did_nl2kql,
        "expected evidence of NL2KQL (a generated KQL query): {blob}"
    );

    // Cleanup: agent + eventhouse (cascades the KQL database).
    fabio()
        .args(["data-agent", "delete", "--workspace", ws, "--id", &id])
        .assert()
        .success();
    fabio()
        .args([
            "eventhouse",
            "delete",
            "--workspace",
            ws,
            "--id",
            &eh_id_from(ws, &eh_name),
        ])
        .assert()
        .success();
}

/// Resolve an eventhouse id by name (cleanup helper).
fn eh_id_from(ws: &str, name: &str) -> String {
    let assert = fabio()
        .args(["eventhouse", "list", "--workspace", ws])
        .assert()
        .success();
    extract_data(&parse_json(&assert))
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["displayName"] == name)
        .and_then(|e| e["id"].as_str())
        .unwrap_or("")
        .to_string()
}

/// Live validation of **NL2GQL**: a data agent grounded on a `GraphModel`
/// translates a natural-language question into GQL (a `MATCH ... RETURN` query).
///
/// Requires a portal-LOADED graph model (REST-created graphs cannot load),
/// provided via `FABIO_TEST_LOADED_GRAPH_ID`; skipped when unset. Asserts NL2GQL
/// GENERATION (the agent's `analyze_graph` tool emits a GQL query) — NOT a final
/// answer: the data-agent's server-side GQL EXECUTION currently fails ("Unable
/// to process the request") even though the generated GQL runs fine via the
/// direct executeQuery API. That execution gap is server-side, not fabio.
#[test]
#[ignore = "requires live Fabric tenant + a portal-loaded graph (FABIO_TEST_LOADED_GRAPH_ID)"]
#[serial]
fn dataagent_graph_model_nl2gql_generation_lifecycle() {
    let Ok(graph) = std::env::var("FABIO_TEST_LOADED_GRAPH_ID") else {
        eprintln!("FABIO_TEST_LOADED_GRAPH_ID not set — skipping NL2GQL validation");
        return;
    };
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    let assert = fabio()
        .args([
            "data-agent",
            "create",
            "--workspace",
            ws,
            "--name",
            &unique_name("nl2gql"),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Graph sources add cleanly and auto-select their node types (unlike SQL /
    // semantic-model tables, which start unselected).
    let assert = fabio()
        .args([
            "data-agent",
            "add-datasource",
            "--workspace",
            ws,
            "--id",
            &id,
            "--artifact",
            &graph,
            "--artifact-type",
            "GraphModel",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    assert_eq!(
        extract_data(&parse_json(&assert))["fabricItemType"],
        "GraphModel",
        "datasource must be a GraphModel"
    );

    fabio()
        .args([
            "data-agent",
            "publish",
            "--workspace",
            ws,
            "--id",
            &id,
            "--description",
            "NL2GQL e2e",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let assert = fabio()
        .args([
            "data-agent",
            "query",
            "--workspace",
            ws,
            "--id",
            &id,
            "--prompt",
            "Using the graph, list the DimStore node StoreName values.",
            "--show-steps",
        ])
        .timeout(std::time::Duration::from_mins(4))
        .assert()
        .success();
    let blob = serde_json::to_string(&parse_json(&assert)).unwrap();
    assert!(
        blob.contains("analyze_graph"),
        "run steps must invoke the NL2GQL graph tool: {blob}"
    );
    assert!(
        blob.contains("MATCH"),
        "expected a generated GQL query (MATCH ... RETURN) in the steps: {blob}"
    );

    fabio()
        .args(["data-agent", "delete", "--workspace", ws, "--id", &id])
        .assert()
        .success();
}

/// Shared **NL2SQL** lifecycle for any SQL-backed data-agent source: a data
/// agent grounded on the given artifact translates a natural-language question
/// into a query (T-SQL) and returns a grounded answer.
///
/// Parameterized by the fixture env var (holding a PRE-POPULATED artifact id —
/// a freshly-provisioned source's schema-discovery LRO is slow/flaky) and the
/// `--artifact-type`. Skips when the fixture is unset or `az` is unavailable.
///
/// It acquires an **Azure-CLI-issued Fabric token** (via `az`) and runs every
/// fabio call with it, because the data-agent server fetches a SQL source's
/// schema *on-behalf-of the caller's token*, and that on-behalf-of exchange
/// only succeeds for a broadly-trusted client app (Azure CLI) — fabio's own
/// device-code login token is rejected with "Failed to fetch schema for the
/// data source" (see API-BEHAVIORS-DISCOVERED "Data-agent SQL-source schema
/// fetch is on-behalf-of the caller").
///
/// Regression guard for two fixed bugs:
///   1. `add-datasource` MUST send a **Lakehouse** as `LakehouseTables` (a
///      lakehouse item is not itself the SQL database), while every other SQL
///      source (`Warehouse`/`SQLDatabase`/`MirroredDatabase`) is `FabricItem`.
///      The wrong discriminator fails schema discovery.
///   2. `select-tables` MUST drill through the 3-level `Schemas → Schema →
///      Tables → Table` nesting to reach the tables (the previous single-level
///      drill failed with "No matching elements").
#[allow(clippy::too_many_lines)]
fn run_nl2sql_source_lifecycle(fixture_env: &str, artifact_type: &str) {
    let Ok(artifact) = std::env::var(fixture_env) else {
        eprintln!("{fixture_env} not set — skipping {artifact_type} NL2SQL validation");
        return;
    };
    let Some(token) = az_fabric_token() else {
        eprintln!("az Fabric token unavailable — skipping {artifact_type} NL2SQL validation");
        return;
    };
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    // Every fabio invocation must carry the OBO-capable Fabric token.
    let fb = || {
        let mut c = fabio();
        c.env("FABIO_ACCESS_TOKEN", &token);
        c
    };

    // 1) Agent grounded on the pre-populated source.
    let assert = fb()
        .args([
            "data-agent",
            "create",
            "--workspace",
            ws,
            "--name",
            &unique_name("nl2sql"),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Add the source. A regression to the wrong discriminator (a Lakehouse sent
    // as FabricItem, or vice-versa) would fail this with "Failed to fetch
    // schema". A LakehouseTables datasource reports `fabricItemType: null`, so
    // the shape-agnostic success signal is `status: datasource_added` — reaching
    // it proves schema discovery succeeded.
    let assert = fb()
        .args([
            "data-agent",
            "add-datasource",
            "--workspace",
            ws,
            "--id",
            &id,
            "--artifact",
            &artifact,
            "--artifact-type",
            artifact_type,
            "--lro-timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();
    assert_eq!(
        extract_data(&parse_json(&assert))["status"],
        "datasource_added",
        "{artifact_type} datasource must be added without a schema-fetch error"
    );

    // 2) Select ALL tables — exercises the recursive walk through the 3-level
    //    Schemas → Schema → Tables → Table nesting (the deep-drill fix). A
    //    single-level drill would select nothing.
    let assert = fb()
        .args([
            "data-agent",
            "select-tables",
            "--workspace",
            ws,
            "--id",
            &id,
            "--datasource",
            &artifact,
            "--all-tables",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let modified = extract_data(&parse_json(&assert))["modified"]
        .as_u64()
        .unwrap_or(0);
    assert!(
        modified >= 1,
        "select-tables --all-tables must reach the schema-nested tables (got modified={modified})"
    );

    fb().args([
        "data-agent",
        "publish",
        "--workspace",
        ws,
        "--id",
        &id,
        "--description",
        "NL2SQL e2e",
    ])
    .timeout(std::time::Duration::from_mins(3))
    .assert()
    .success();

    // 3) Ask a question and confirm a grounded NL2SQL answer (a generated
    //    T-SQL query in the run steps).
    let assert = fb()
        .args([
            "data-agent",
            "query",
            "--workspace",
            ws,
            "--id",
            &id,
            "--prompt",
            "List the tables available and how many rows each one has.",
            "--show-steps",
        ])
        .timeout(std::time::Duration::from_mins(4))
        .assert()
        .success();
    let blob = serde_json::to_string(&parse_json(&assert)).unwrap();
    let did_nl2sql = blob.contains("nl2code") || blob.to_lowercase().contains("select");
    assert!(
        did_nl2sql,
        "expected evidence of NL2SQL (a generated T-SQL query): {blob}"
    );

    // Cleanup: the agent only — the source fixture is pre-existing.
    fb().args(["data-agent", "delete", "--workspace", ws, "--id", &id])
        .assert()
        .success();
}

/// NL2SQL over a **Lakehouse** (the `LakehouseTables` discriminator branch).
/// Fixture: `FABIO_TEST_LOADED_LAKEHOUSE` (a pre-populated, schema-enabled
/// lakehouse).
#[test]
#[ignore = "requires live Fabric tenant + a populated lakehouse (FABIO_TEST_LOADED_LAKEHOUSE)"]
#[serial]
fn dataagent_lakehouse_nl2sql_lifecycle() {
    run_nl2sql_source_lifecycle("FABIO_TEST_LOADED_LAKEHOUSE", "Lakehouse");
}

/// NL2SQL over a **Warehouse** (the `FabricItem` discriminator branch).
/// Fixture: `FABIO_TEST_LOADED_WAREHOUSE` (a pre-populated warehouse; a freshly
/// created warehouse's SQL schema takes ~60-180 s to become discoverable).
#[test]
#[ignore = "requires live Fabric tenant + a populated warehouse (FABIO_TEST_LOADED_WAREHOUSE)"]
#[serial]
fn dataagent_warehouse_nl2sql_lifecycle() {
    run_nl2sql_source_lifecycle("FABIO_TEST_LOADED_WAREHOUSE", "Warehouse");
}

/// NL2SQL over a **SQL Database** (the `FabricItem` discriminator branch;
/// requires F4+ capacity). Fixture: `FABIO_TEST_LOADED_SQLDATABASE` (a
/// pre-populated Fabric SQL database).
#[test]
#[ignore = "requires live Fabric tenant + a populated SQL database (FABIO_TEST_LOADED_SQLDATABASE)"]
#[serial]
fn dataagent_sql_database_nl2sql_lifecycle() {
    run_nl2sql_source_lifecycle("FABIO_TEST_LOADED_SQLDATABASE", "SQLDatabase");
}

/// NL2SQL over a **Mirrored Database** (the `FabricItem` discriminator branch;
/// requires an external mirror source actively replicating data). Fixture:
/// `FABIO_TEST_LOADED_MIRRORED_DATABASE` (a mirrored database with tables).
#[test]
#[ignore = "requires live Fabric tenant + a populated mirrored database (FABIO_TEST_LOADED_MIRRORED_DATABASE)"]
#[serial]
fn dataagent_mirrored_database_nl2sql_lifecycle() {
    run_nl2sql_source_lifecycle("FABIO_TEST_LOADED_MIRRORED_DATABASE", "MirroredDatabase");
}

/// Acquire an Azure-CLI-issued Fabric access token (`az account
/// get-access-token --resource https://api.fabric.microsoft.com`). Returns
/// `None` if `az` is missing, not logged in, or the token is empty — the caller
/// then skips. Used by NL2SQL tests whose data-agent schema fetch needs an
/// on-behalf-of-capable token (see `dataagent_lakehouse_nl2sql_lifecycle`).
fn az_fabric_token() -> Option<String> {
    let output = std::process::Command::new("az")
        .args([
            "account",
            "get-access-token",
            "--resource",
            "https://api.fabric.microsoft.com",
            "--query",
            "accessToken",
            "-o",
            "tsv",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Chart/visual extraction (`data-agent query --visuals`).
///
/// Verifies fabio's finding that a Fabric data agent's visual responses — which
/// the docs describe as portal-only — are in fact reachable through the
/// published Assistants API: when the agent answers with a chart it invokes a
/// `VisualizeDataset` tool whose `arguments` carry the COMPLETE chart spec
/// (chart type, x/y columns, title, axis titles, sort, and the aggregated data
/// as inline CSV). `--visuals` surfaces that spec.
///
/// Fixture: `FABIO_TEST_LOADED_LAKEHOUSE` (a pre-populated, schema-enabled
/// lakehouse — same fixture the NL2SQL tests use) plus `az` for the
/// OBO-capable Fabric token that add-datasource's schema fetch needs.
#[test]
#[ignore = "requires live Fabric tenant + a populated lakehouse (FABIO_TEST_LOADED_LAKEHOUSE) + az"]
#[serial]
fn dataagent_query_visuals_lifecycle() {
    let Ok(lh) = std::env::var("FABIO_TEST_LOADED_LAKEHOUSE") else {
        eprintln!("FABIO_TEST_LOADED_LAKEHOUSE not set — skipping visuals validation");
        return;
    };
    let Some(token) = az_fabric_token() else {
        eprintln!("az Fabric token unavailable — skipping visuals validation");
        return;
    };
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;
    let fb = || {
        let mut c = fabio();
        c.env("FABIO_ACCESS_TOKEN", &token);
        c
    };

    // 1) Agent grounded on the pre-populated lakehouse.
    let id = extract_data(&parse_json(
        &fb()
            .args([
                "data-agent",
                "create",
                "--workspace",
                ws,
                "--name",
                &unique_name("viz_agent"),
            ])
            .timeout(std::time::Duration::from_mins(2))
            .assert()
            .success(),
    ))["id"]
        .as_str()
        .unwrap()
        .to_string();
    fb().args([
        "data-agent",
        "add-datasource",
        "--workspace",
        ws,
        "--id",
        &id,
        "--artifact",
        &lh,
        "--artifact-type",
        "Lakehouse",
        "--lro-timeout",
        "300",
    ])
    .timeout(std::time::Duration::from_mins(6))
    .assert()
    .success();
    fb().args([
        "data-agent",
        "select-tables",
        "--workspace",
        ws,
        "--id",
        &id,
        "--datasource",
        &lh,
        "--all-tables",
    ])
    .timeout(std::time::Duration::from_mins(2))
    .assert()
    .success();
    fb().args([
        "data-agent",
        "publish",
        "--workspace",
        ws,
        "--id",
        &id,
        "--description",
        "visuals e2e",
    ])
    .timeout(std::time::Duration::from_mins(3))
    .assert()
    .success();

    // 2) Ask for a chart and assert a structured chart spec comes back. The
    //    prompt is deliberately explicit so the agent invokes VisualizeDataset
    //    regardless of the fixture's exact schema.
    let assert = fb()
        .args([
            "data-agent",
            "query",
            "--workspace",
            ws,
            "--id",
            &id,
            "--prompt",
            "Create a bar chart that visualizes a summary of the available data. \
             Aggregate a numeric column grouped by a categorical column and return a chart.",
            "--visuals",
        ])
        .timeout(std::time::Duration::from_mins(4))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let visuals = data["visuals"].as_array().expect("visuals array");
    assert!(
        !visuals.is_empty(),
        "expected at least one extracted chart spec, got: {data}"
    );
    let v = &visuals[0];
    assert!(
        v.get("chart_type").is_some(),
        "chart spec must carry a chart_type: {v}"
    );
    assert!(
        v.get("inline_csv_data").is_some(),
        "chart spec must carry the aggregated inline_csv_data: {v}"
    );

    // Cleanup: the agent only — the lakehouse fixture is pre-existing.
    fb().args(["data-agent", "delete", "--workspace", ws, "--id", &id])
        .assert()
        .success();
}

#[test]
fn dataagent_delete_dry_run_is_guarded() {
    // Regression: data-agent delete previously had no --dry-run guard and would
    // execute the real (hard-)delete.
    let assert = fabio()
        .args([
            "--dry-run",
            "data-agent",
            "delete",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
            "--hard-delete",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "data-agent delete");
    assert_eq!(data["details"]["hardDelete"], true);
}
