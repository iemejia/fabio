//! End-to-end integration tests for `fabio item` commands.

mod common;

use common::{TestConfig, extract_count, extract_data, fabio, parse_json};
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_returns_items() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["item", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let count = extract_count(&json);

    assert!(count > 0);
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty());

    // Each item should have id, displayName, type
    let first = &arr[0];
    assert!(first.get("id").is_some());
    assert!(first.get("displayName").is_some());
    assert!(first.get("type").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_with_type_filter() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--type",
            "Lakehouse",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();

    // All returned items should be Lakehouses
    for item in arr {
        assert_eq!(item["type"], "Lakehouse");
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_without_type_filter_returns_all() {
    let cfg = TestConfig::from_env();

    // Without type filter, should return items of multiple types
    let assert = fabio()
        .args(["item", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();

    // Collect unique types
    let types: std::collections::HashSet<&str> =
        arr.iter().filter_map(|i| i["type"].as_str()).collect();

    // Should have at least Lakehouse and Notebook
    assert!(
        types.len() >= 2,
        "expected multiple item types, got: {types:?}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_show_returns_details() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    assert_eq!(data["id"], cfg.source_lakehouse);
    assert_eq!(data["type"], "Lakehouse");
    assert!(data.get("displayName").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("item_test");

    // Create a Lakehouse item
    let assert = fabio()
        .args([
            "item",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--type",
            "Lakehouse",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["type"], "Lakehouse");
    let new_id = data["id"].as_str().unwrap().to_string();

    // Delete the item
    let assert = fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &new_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
    assert_eq!(data["id"], new_id);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_copy_with_custom_name_and_delete() {
    let cfg = TestConfig::from_env();
    let copy_name = common::unique_name("test_copy");

    // Copy the notebook to dest workspace with explicit name
    let assert = fabio()
        .args([
            "item",
            "copy",
            "--source-workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--dest-workspace",
            &cfg.dest_workspace,
            "--name",
            &copy_name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    assert_eq!(data["displayName"], copy_name);
    assert_eq!(data["type"], "Notebook");

    let new_id = data["id"].as_str().unwrap();

    // Delete the copy
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            new_id,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("deleted"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_copy_without_name_inherits_source_name() {
    let cfg = TestConfig::from_env();

    // First create a uniquely named notebook in source to avoid conflicts
    let src_name = common::unique_name("cp_src_nb");
    fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &src_name,
            "--content",
            "print('copy inherit name test')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Find the notebook ID
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let nb = items.iter().find(|i| i["displayName"] == src_name).unwrap();
    let nb_id = nb["id"].as_str().unwrap().to_string();

    // Copy without specifying --name (should inherit from source)
    let assert = fabio()
        .args([
            "item",
            "copy",
            "--source-workspace",
            &cfg.source_workspace,
            "--id",
            &nb_id,
            "--dest-workspace",
            &cfg.dest_workspace,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Name should be the source notebook's name
    assert_eq!(data["displayName"], src_name);
    assert_eq!(data["type"], "Notebook");

    let new_id = data["id"].as_str().unwrap();

    // Cleanup: delete copy from dest
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            new_id,
        ])
        .assert()
        .success();

    // Cleanup: delete source
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &nb_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_move_to_dest_workspace() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("move_test");

    // First create a notebook in dest workspace (we'll move it back to source)
    fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--content",
            "print('move test')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Find the notebook ID
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let nb = items
        .iter()
        .find(|i| i["displayName"] == name)
        .expect("created notebook not found");
    let nb_id = nb["id"].as_str().unwrap().to_string();

    // Move from dest to source workspace
    let move_name = common::unique_name("moved_nb");
    let assert = fabio()
        .args([
            "item",
            "move",
            "--source-workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
            "--dest-workspace",
            &cfg.source_workspace,
            "--name",
            &move_name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "moved");
    assert_eq!(data["displayName"], move_name);
    assert_eq!(data["type"], "Notebook");

    let moved_id = data["id"].as_str().unwrap();

    // Verify original is gone (should fail)
    fabio()
        .args([
            "item",
            "show",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
        ])
        .assert()
        .failure();

    // Cleanup: delete from source workspace
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            moved_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_move_without_name() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("move_noname");

    // Create notebook
    fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--content",
            "print('move no rename')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Find ID
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let nb = items.iter().find(|i| i["displayName"] == name).unwrap();
    let nb_id = nb["id"].as_str().unwrap().to_string();

    // Move without --name (should keep original name)
    let assert = fabio()
        .args([
            "item",
            "move",
            "--source-workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
            "--dest-workspace",
            &cfg.source_workspace,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "moved");
    assert_eq!(data["displayName"], name);

    let moved_id = data["id"].as_str().unwrap();

    // Cleanup
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            moved_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// item list with --limit
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_with_limit() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--limit",
            "2",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().expect("Expected array");
    assert!(
        arr.len() <= 2,
        "Expected at most 2 items with --limit 2, got {}",
        arr.len()
    );
}

// ---------------------------------------------------------------------------
// item show not found — error with hint
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_show_not_found_with_hint() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let error = &err_json["error"];
    assert_eq!(error["code"], "NOT_FOUND");
    // Should have a hint telling you how to list items
    let hint = error["hint"].as_str().expect("Expected hint field");
    assert!(
        hint.contains("fabio item list"),
        "Hint should suggest listing items: {hint}"
    );
    assert!(
        hint.contains(&cfg.source_workspace),
        "Hint should include workspace ID: {hint}"
    );
}

// ---------------------------------------------------------------------------
// item create with invalid type — error with valid types hint
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_create_invalid_type_with_hint() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test_invalid",
            "--type",
            "FakeItemType",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let error = &err_json["error"];
    assert_eq!(error["code"], "INVALID_INPUT");
    // Should have a hint with valid item types
    let hint = error["hint"].as_str().expect("Expected hint field");
    assert!(
        hint.contains("Lakehouse"),
        "Hint should list valid types: {hint}"
    );
    assert!(
        hint.contains("Notebook"),
        "Hint should list valid types: {hint}"
    );
    assert!(
        hint.contains("Warehouse"),
        "Hint should list valid types: {hint}"
    );
}

// ---------------------------------------------------------------------------
// item delete not found — error
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_delete_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR",
        "Expected NOT_FOUND or API_ERROR, got: {code}"
    );
}

// ===========================================================================
// item update — rename/redescribe
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_update_name() {
    let cfg = TestConfig::from_env();
    let original_name = common::unique_name("upd_orig");
    let updated_name = common::unique_name("upd_new");

    // Create a Lakehouse
    let assert = fabio()
        .args([
            "item",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &original_name,
            "--type",
            "Lakehouse",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let item_id = data["id"].as_str().unwrap().to_string();

    // Update name
    let assert = fabio()
        .args([
            "item",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &item_id,
            "--name",
            &updated_name,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], updated_name);

    // Cleanup
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &item_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_update_description() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("upd_desc");

    // Create a Lakehouse
    let assert = fabio()
        .args([
            "item",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--type",
            "Lakehouse",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let item_id = data["id"].as_str().unwrap().to_string();

    // Update description
    let assert = fabio()
        .args([
            "item",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &item_id,
            "--description",
            "Test description from e2e",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["description"], "Test description from e2e");

    // Cleanup
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &item_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_update_requires_at_least_one_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "INVALID_INPUT");
    let hint = err_json["error"]["hint"].as_str().unwrap();
    assert!(
        hint.contains("--name"),
        "Hint should mention --name: {hint}"
    );
}

// ===========================================================================
// item get-definition
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_get_definition_notebook() {
    let cfg = TestConfig::from_env();

    // get-definition for a notebook (known to support definitions)
    let assert = fabio()
        .args([
            "item",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Should have a definition with parts
    let definition = &data["definition"];
    assert!(
        definition.get("parts").is_some(),
        "Expected definition.parts: {data}"
    );
}

// ===========================================================================
// item list-connections
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_connections_returns_array() {
    let cfg = TestConfig::from_env();

    // list-connections for the lakehouse (may be empty but should succeed)
    let assert = fabio()
        .args([
            "item",
            "list-connections",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // data should be an array (possibly empty)
    assert!(data.is_array(), "Expected array, got: {data}");
}

// ===========================================================================
// item update-definition (create notebook, update definition, verify)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_update_definition_inline() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("upd_def");

    // Create a notebook
    let assert = fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--content",
            "print('original')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let item_id = data["id"].as_str().unwrap().to_string();

    // Build an updated .ipynb definition inline
    let notebook_json = serde_json::json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": {},
        "cells": [{
            "cell_type": "code",
            "source": ["print('updated definition')"],
            "metadata": {},
            "outputs": []
        }]
    });
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        serde_json::to_string(&notebook_json).unwrap().as_bytes(),
    );

    let definition_payload = serde_json::json!({
        "definition": {
            "parts": [{
                "path": "notebook-content.py",
                "payload": encoded,
                "payloadType": "InlineBase64"
            }]
        }
    });

    let assert = fabio()
        .args([
            "item",
            "update-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &item_id,
            "--definition",
            &serde_json::to_string(&definition_payload).unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "definition_updated");

    // Cleanup
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &item_id,
        ])
        .assert()
        .success();
}

// ===========================================================================
// item update-definition requires file or definition
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_update_definition_requires_input() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "INVALID_INPUT");
    let hint = err_json["error"]["hint"].as_str().unwrap();
    assert!(
        hint.contains("--file"),
        "Hint should mention --file: {hint}"
    );
}

// ===========================================================================
// item exists — returns {exists: true/false}, never errors on 404
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_exists_returns_true_for_existing_item() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "exists",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["exists"], true);
    assert_eq!(data["id"], cfg.source_lakehouse);
    assert_eq!(data["workspaceId"], cfg.source_workspace);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_exists_returns_false_for_nonexistent_item() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "exists",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000099",
        ])
        .assert()
        .success(); // Never errors — returns {exists: false}

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["exists"], false);
    assert_eq!(data["id"], "00000000-0000-0000-0000-000000000099");
}

// ===========================================================================
// item url — returns Fabric portal URL (no API call)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_url_returns_portal_url_with_type() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "url",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--type",
            "Lakehouse",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let url = data["url"].as_str().unwrap();

    assert!(
        url.starts_with("https://app.fabric.microsoft.com/groups/"),
        "URL should start with portal base: {url}"
    );
    assert!(
        url.contains("/lakehouses/"),
        "Lakehouse type should map to /lakehouses/ path: {url}"
    );
    assert!(
        url.contains(&cfg.source_lakehouse),
        "URL should contain item ID: {url}"
    );
    assert_eq!(data["itemId"], cfg.source_lakehouse);
    assert_eq!(data["workspaceId"], cfg.source_workspace);
    assert_eq!(data["itemType"], "Lakehouse");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_url_returns_generic_path_without_type() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "url",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let url = data["url"].as_str().unwrap();

    assert!(
        url.contains("/items/"),
        "Without --type, should use generic /items/ path: {url}"
    );
    // No itemType field when --type is not provided
    assert!(
        data.get("itemType").is_none(),
        "itemType should not be present without --type flag"
    );
}

// ===========================================================================
// item inspect — aggregated view (metadata + definition + connections)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_inspect_returns_metadata() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "inspect",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Metadata is always present
    assert!(data.get("metadata").is_some(), "inspect must have metadata");
    let metadata = &data["metadata"];
    assert_eq!(metadata["id"], cfg.source_lakehouse);
    assert!(metadata.get("displayName").is_some());
    assert!(metadata.get("type").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_inspect_notebook_includes_definition() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "inspect",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Notebooks support definitions
    assert!(data.get("metadata").is_some());
    assert!(
        data.get("definition").is_some(),
        "Notebook inspect should include definition"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_inspect_nonexistent_item_fails() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "item",
            "inspect",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000099",
        ])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// item list --folder filter
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_with_folder_filter_nonexistent() {
    let cfg = TestConfig::from_env();

    // A folder ID that doesn't exist should return zero results
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--folder",
            "00000000-0000-0000-0000-000000000099",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();
    assert!(
        arr.is_empty(),
        "expected no items for nonexistent folder, got {}",
        arr.len()
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_folder_combined_with_type() {
    let cfg = TestConfig::from_env();

    // Both filters together — should succeed even with no matches
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--type",
            "Notebook",
            "--folder",
            "00000000-0000-0000-0000-000000000099",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();
    assert!(
        arr.is_empty(),
        "expected no items for nonexistent folder + type combo"
    );
}

// ─── Bulk Create ─────────────────────────────────────────────────────────────

#[test]
fn item_bulk_create_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "item",
            "bulk-create",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--content",
            r#"[{"displayName":"BulkItem1","type":"Lakehouse"},{"displayName":"BulkItem2","type":"Lakehouse"}]"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["count"], 2);
}

#[test]
fn item_bulk_create_empty_array_fails() {
    let assert = fabio()
        .args([
            "item",
            "bulk-create",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--content",
            "[]",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(err["error"]["message"].as_str().unwrap().contains("empty"));
}

#[test]
fn item_bulk_create_non_array_fails() {
    let assert = fabio()
        .args([
            "item",
            "bulk-create",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--content",
            r#"{"displayName":"Single","type":"Lakehouse"}"#,
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(err["error"]["message"].as_str().unwrap().contains("array"));
}

// ─── Bulk Delete ─────────────────────────────────────────────────────────────

#[test]
fn item_bulk_delete_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "item",
            "bulk-delete",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--ids",
            "id1,id2,id3",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["count"], 3);
}

#[test]
fn item_bulk_delete_no_ids_fails() {
    let assert = fabio()
        .args([
            "item",
            "bulk-delete",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--ids",
            "",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_bulk_create_and_delete_live() {
    let cfg = TestConfig::from_env();

    // Bulk-create 2 lakehouses
    let items_json = serde_json::json!([
        {"displayName": "BulkTest1E2e", "type": "Lakehouse"},
        {"displayName": "BulkTest2E2e", "type": "Lakehouse"},
    ])
    .to_string();

    let assert = fabio()
        .args([
            "item",
            "bulk-create",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &items_json,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["succeeded"], 2);
    assert_eq!(data["failed"], 0);
    let items = data["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    // Extract IDs for cleanup
    let id1 = items[0]["id"].as_str().unwrap();
    let id2 = items[1]["id"].as_str().unwrap();
    let ids = format!("{id1},{id2}");

    // Bulk-delete both
    let assert = fabio()
        .args([
            "item",
            "bulk-delete",
            "--workspace",
            &cfg.source_workspace,
            "--ids",
            &ids,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["succeeded"], 2);
    assert_eq!(data["failed"], 0);
}

// ===========================================================================
// item delete --hard-delete (dry-run, no live tenant required)
// ===========================================================================

#[test]
fn item_delete_hard_delete_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "item",
            "delete",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--hard-delete",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["hardDelete"], true);
}

#[test]
fn item_delete_without_hard_delete_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "item",
            "delete",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["hardDelete"], false);
}

// ===========================================================================
// item move-to-folder (dry-run + live)
// ===========================================================================

#[test]
fn item_move_to_folder_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "item",
            "move-to-folder",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--folder-id",
            "cccccccc-1111-2222-3333-444444444444",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["details"]["targetFolderId"],
        "cccccccc-1111-2222-3333-444444444444"
    );
}

#[test]
fn item_move_to_folder_root_dry_run() {
    // Omitting --folder-id moves to workspace root (null)
    let assert = fabio()
        .args([
            "--dry-run",
            "item",
            "move-to-folder",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert!(data["details"]["targetFolderId"].is_null());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_move_to_folder_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "move-to-folder",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--folder-id",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR",
        "Expected NOT_FOUND or API_ERROR, got: {code}"
    );
}

// ─── Item List with folder/recursive/include flags ──────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_with_recursive_flag() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--recursive",
            "true",
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
fn item_list_with_include_flag() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--include",
            "description",
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
fn item_list_with_type_and_recursive() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--type",
            "Lakehouse",
            "--recursive",
            "true",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

// ===========================================================================
// item list-downstream-relations / list-upstream-relations (beta API)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_downstream_relations_returns_object() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "list-downstream-relations",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Response is an object with items/relations/workspaces arrays (possibly empty).
    assert!(data.is_object(), "Expected object, got: {data}");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_upstream_relations_returns_object() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "item",
            "list-upstream-relations",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_object(), "Expected object, got: {data}");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn item_list_downstream_relations_invalid_item_fails() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "item",
            "list-downstream-relations",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}
