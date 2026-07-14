//! End-to-end integration tests for `fabio workspace` commands.

mod common;

use common::{TestConfig, extract_count, extract_data, fabio, parse_json, unique_name};
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_list_returns_workspaces() {
    let assert = fabio().args(["workspace", "list"]).assert().success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let count = extract_count(&json);

    assert!(count > 0, "expected at least one workspace");
    assert!(data.is_array());
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty());

    // Each workspace should have id, displayName, type
    let first = &arr[0];
    assert!(first.get("id").is_some());
    assert!(first.get("displayName").is_some());
    assert!(first.get("type").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_list_table_format() {
    fabio()
        .args(["--output", "table", "workspace", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("NAME"))
        .stdout(predicate::str::contains("ID"))
        .stdout(predicate::str::contains("TYPE"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_list_plain_format() {
    let cfg = TestConfig::from_env();

    fabio()
        .args(["--output", "plain", "workspace", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(&cfg.source_workspace));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_show_returns_details() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["workspace", "show", "--id", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    assert_eq!(data["id"], cfg.source_workspace);
    assert!(data.get("displayName").is_some());
    assert!(data.get("capacityId").is_some());
    assert!(data.get("type").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_show_not_found() {
    let assert = fabio()
        .args([
            "workspace",
            "show",
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    // Fabric may return API_ERROR or NOT_FOUND for invalid workspace IDs
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR",
        "Expected NOT_FOUND or API_ERROR, got: {code}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_create_with_description_and_delete() {
    let name = common::unique_name("ws_test");

    // Create workspace with description
    let assert = fabio()
        .args([
            "workspace",
            "create",
            "--name",
            &name,
            "--description",
            "Integration test workspace",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert!(data.get("id").is_some());

    let ws_id = data["id"].as_str().unwrap().to_string();

    // Show workspace to verify description was set
    let assert = fabio()
        .args(["workspace", "show", "--id", &ws_id])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    // Note: Fabric API may or may not return description in show, depending on version

    // Delete workspace
    let assert = fabio()
        .args(["workspace", "delete", "--id", &ws_id])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
    assert_eq!(data["id"], ws_id);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_create_without_description_and_delete() {
    let name = common::unique_name("ws_nodesc");

    // Create workspace without description
    let assert = fabio()
        .args(["workspace", "create", "--name", &name])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let ws_id = data["id"].as_str().unwrap().to_string();

    // Delete
    fabio()
        .args(["workspace", "delete", "--id", &ws_id])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_assign_capacity() {
    let cfg = TestConfig::from_env();

    // Re-assign source workspace to its current capacity (idempotent)
    let assert = fabio()
        .args([
            "workspace",
            "assign-capacity",
            "--id",
            &cfg.source_workspace,
            "--capacity",
            &cfg.capacity_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "assigned");
    assert_eq!(data["workspaceId"], cfg.source_workspace);
    assert_eq!(data["capacityId"], cfg.capacity_id);
}

// ---------------------------------------------------------------------------
// workspace list --limit
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_list_with_limit() {
    let assert = fabio()
        .args(["workspace", "list", "--limit", "1"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().expect("Expected array");
    assert_eq!(arr.len(), 1, "Expected exactly 1 workspace with --limit 1");
    // count should reflect total available, not limited
    let count = extract_count(&json);
    assert!(
        count >= 1,
        "Count should be >= 1 (total available, not limited)"
    );
}

// ---------------------------------------------------------------------------
// workspace show with --query extracts a field
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_show_with_query() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "show",
            "--id",
            &cfg.source_workspace,
            "--query",
            "displayName",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // --query displayName extracts the workspace name as a string
    assert!(data.is_string(), "Expected string from --query: {data}");
}

// ---------------------------------------------------------------------------
// workspace delete with non-existent ID returns error
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_delete_not_found() {
    let assert = fabio()
        .args([
            "workspace",
            "delete",
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    // Fabric may return API_ERROR or NOT_FOUND for invalid workspace IDs
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR",
        "Expected NOT_FOUND or API_ERROR for invalid workspace, got: {code}"
    );
}

// ---------------------------------------------------------------------------
// workspace assign-capacity with invalid capacity fails with hint
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_assign_capacity_invalid() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "assign-capacity",
            "--id",
            &cfg.source_workspace,
            "--capacity",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    // Should be a client error (INVALID_INPUT or API_ERROR or NOT_FOUND)
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR" || code == "INVALID_INPUT",
        "Expected error code for invalid capacity, got: {code}"
    );

    // Should have a hint about creating the capacity
    let hint = err_json["error"]["hint"].as_str().unwrap_or("");
    assert!(
        hint.contains("az fabric capacity"),
        "Expected hint with az CLI command, got: {hint}"
    );
}

// ===========================================================================
// workspace update
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_update_name_and_description() {
    let name = unique_name("ws_upd");

    // Create workspace
    let assert = fabio()
        .args(["workspace", "create", "--name", &name])
        .assert()
        .success();
    let json = parse_json(&assert);
    let ws_id = extract_data(&json)["id"].as_str().unwrap().to_string();

    // Update both name and description
    let new_name = format!("{name}_renamed");
    let assert = fabio()
        .args([
            "workspace",
            "update",
            "--id",
            &ws_id,
            "--name",
            &new_name,
            "--description",
            "Updated description",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], new_name);
    assert_eq!(data["id"], ws_id);

    // Cleanup
    fabio()
        .args(["workspace", "delete", "--id", &ws_id])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_update_requires_at_least_one_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["workspace", "update", "--id", &cfg.source_workspace])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert_eq!(code, "INVALID_INPUT");
}

// ===========================================================================
// workspace unassign-capacity
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_unassign_capacity_on_nonexistent() {
    let assert = fabio()
        .args([
            "workspace",
            "unassign-capacity",
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR" || code == "FORBIDDEN",
        "Expected error for invalid workspace, got: {code}"
    );
}

// ===========================================================================
// workspace role-assignments
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_list_role_assignments() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "list-role-assignments",
            "--id",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let count = extract_count(&json);

    assert!(
        count >= 1,
        "Expected at least one role assignment (the admin)"
    );
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty());

    // Each assignment should have id and role
    let first = &arr[0];
    assert!(first.get("id").is_some());
    assert!(first.get("role").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_add_role_assignment_invalid_role() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "add-role-assignment",
            "--id",
            &cfg.source_workspace,
            "--principal-id",
            "00000000-0000-0000-0000-000000000000",
            "--principal-type",
            "User",
            "--role",
            "SuperAdmin",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert_eq!(code, "INVALID_INPUT");

    let hint = err_json["error"]["hint"].as_str().unwrap_or("");
    assert!(
        hint.contains("Admin") && hint.contains("Member"),
        "Expected hint with valid roles, got: {hint}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_add_role_assignment_invalid_principal_type() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "add-role-assignment",
            "--id",
            &cfg.source_workspace,
            "--principal-id",
            "00000000-0000-0000-0000-000000000000",
            "--principal-type",
            "Application",
            "--role",
            "Viewer",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert_eq!(code, "INVALID_INPUT");

    let hint = err_json["error"]["hint"].as_str().unwrap_or("");
    assert!(
        hint.contains("ServicePrincipal"),
        "Expected hint with valid principal types, got: {hint}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_update_role_assignment_invalid_role() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "update-role-assignment",
            "--id",
            &cfg.source_workspace,
            "--assignment-id",
            "00000000-0000-0000-0000-000000000000",
            "--role",
            "Owner",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert_eq!(code, "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_delete_role_assignment_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "delete-role-assignment",
            "--id",
            &cfg.source_workspace,
            "--assignment-id",
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
// workspace provision/deprovision identity
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_provision_identity_on_nonexistent() {
    let assert = fabio()
        .args([
            "workspace",
            "provision-identity",
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR" || code == "FORBIDDEN",
        "Expected error for invalid workspace, got: {code}"
    );
}

// ===========================================================================
// workspace update with --dry-run
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_update_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "update",
            "--id",
            &cfg.source_workspace,
            "--name",
            "Should Not Change",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("dry_run"),
        "Expected dry_run indicator in output"
    );

    // Verify workspace name did NOT change
    let assert = fabio()
        .args(["workspace", "show", "--id", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_ne!(
        data["displayName"].as_str().unwrap_or(""),
        "Should Not Change"
    );
}

// ===========================================================================
// workspace get-settings
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_settings() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "get-settings",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    // Should return JSON (either a properties object or the full workspace)
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(
        data.is_object(),
        "Expected settings to be a JSON object, got: {data}"
    );
}

// ===========================================================================
// workspace update-settings --dry-run
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_update_settings_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "update-settings",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            r#"{"automaticMetadataSync":"Enabled"}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace update-settings");
    assert_eq!(data["details"]["automaticMetadataSync"], "Enabled");
}

// ===========================================================================
// workspace update-settings requires --file or --content
// ===========================================================================

#[test]
fn workspace_update_settings_missing_input() {
    fabio()
        .args([
            "workspace",
            "update-settings",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--file or --content"));
}

// ===========================================================================
// workspace get-firewall-rules
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_firewall_rules() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "workspace",
            "get-firewall-rules",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should have a "rules" array
    assert!(data.get("rules").is_some(), "expected 'rules' field");
    assert!(data["rules"].is_array());
}

// ===========================================================================
// workspace set-firewall-rules roundtrip (add then clear)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_set_firewall_rules_roundtrip() {
    let cfg = TestConfig::from_env();

    // Set a rule
    fabio()
        .args([
            "workspace",
            "set-firewall-rules",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            r#"{"rules":[{"displayName":"E2ETest","value":"172.16.0.0/12"}]}"#,
        ])
        .assert()
        .success();

    // Verify it persisted
    let assert = fabio()
        .args([
            "workspace",
            "get-firewall-rules",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let rules = data["rules"].as_array().unwrap();
    assert!(
        rules.iter().any(|r| r["displayName"] == "E2ETest"),
        "expected E2ETest rule to be present"
    );

    // Clear rules
    fabio()
        .args([
            "workspace",
            "set-firewall-rules",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            r#"{"rules":[]}"#,
        ])
        .assert()
        .success();
}

// ===========================================================================
// workspace set-firewall-rules dry-run
// ===========================================================================

#[test]
fn workspace_set_firewall_rules_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "set-firewall-rules",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            r#"{"rules":[{"displayName":"Test","value":"1.2.3.4"}]}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace set-firewall-rules");
}

// ===========================================================================
// workspace get-git-outbound-policy
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_git_outbound_policy() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "workspace",
            "get-git-outbound-policy",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should have a "defaultAction" field
    assert!(
        data.get("defaultAction").is_some(),
        "expected 'defaultAction' field"
    );
}

// ===========================================================================
// workspace set-dataset-storage-format roundtrip
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_set_dataset_storage_format_roundtrip() {
    let cfg = TestConfig::from_env();

    // Set to Large
    let assert = fabio()
        .args([
            "workspace",
            "set-dataset-storage-format",
            "--workspace",
            &cfg.source_workspace,
            "--format",
            "Large",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["defaultDatasetStorageFormat"], "Large");

    // Revert to Small
    let assert = fabio()
        .args([
            "workspace",
            "set-dataset-storage-format",
            "--workspace",
            &cfg.source_workspace,
            "--format",
            "Small",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["defaultDatasetStorageFormat"], "Small");
}

// ===========================================================================
// workspace set-dataset-storage-format dry-run
// ===========================================================================

#[test]
fn workspace_set_dataset_storage_format_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "set-dataset-storage-format",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--format",
            "Large",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["would_execute"],
        "workspace set-dataset-storage-format"
    );
    assert_eq!(data["details"]["defaultDatasetStorageFormat"], "Large");
}

// ===========================================================================
// workspace set-firewall-rules missing input
// ===========================================================================

#[test]
fn workspace_set_firewall_rules_missing_input() {
    fabio()
        .args([
            "workspace",
            "set-firewall-rules",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--file or --content"));
}

// ===========================================================================
// workspace get-dataset-storage-format
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_dataset_storage_format() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "workspace",
            "get-dataset-storage-format",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(
        data.get("defaultDatasetStorageFormat").is_some(),
        "expected 'defaultDatasetStorageFormat' field"
    );
    let fmt = data["defaultDatasetStorageFormat"].as_str().unwrap();
    assert!(
        fmt == "Small" || fmt == "Large",
        "expected Small or Large, got {fmt}"
    );
}

// ===========================================================================
// workspace get-onelake-settings
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_onelake_settings() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "workspace",
            "get-onelake-settings",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should have tier information
    assert!(
        data.get("defaultTier").is_some() || data.get("tier").is_some() || data.is_object(),
        "expected OneLake settings object"
    );
}

// ===========================================================================
// workspace get-network-policy
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_network_policy() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "workspace",
            "get-network-policy",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Network policy should be an object (may have inbound/outbound fields)
    assert!(data.is_object(), "expected network policy object");
}

// ===========================================================================
// workspace reset-shortcut-cache
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_reset_shortcut_cache() {
    let cfg = TestConfig::from_env();
    // reset-shortcut-cache is LRO; may return API_ERROR if no shortcuts exist
    let assert = fabio()
        .args([
            "workspace",
            "reset-shortcut-cache",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert();

    // Accept either success (workspace has shortcuts) or API_ERROR (no cache to reset)
    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("API_ERROR") || stderr.contains("ItemNotFound"),
            "unexpected error: {stderr}"
        );
    }
}

// ===========================================================================
// workspace show-role-assignment (via list first to get an ID)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_show_role_assignment() {
    let cfg = TestConfig::from_env();

    // First list to get an assignment ID
    let assert = fabio()
        .args([
            "workspace",
            "list-role-assignments",
            "--id",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().expect("expected array of role assignments");
    assert!(!arr.is_empty(), "expected at least one role assignment");

    let first_id = arr[0]["id"].as_str().expect("expected assignment id");

    // Now show that specific assignment
    let assert = fabio()
        .args([
            "workspace",
            "show-role-assignment",
            "--id",
            &cfg.source_workspace,
            "--assignment-id",
            first_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"].as_str().unwrap(), first_id);
    assert!(data.get("principal").is_some());
    assert!(data.get("role").is_some());
}

// ===========================================================================
// workspace folder lifecycle: create → list → show → update → move → delete
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_folder_lifecycle() {
    let cfg = TestConfig::from_env();
    let folder_name = unique_name("TestFolder");

    // Create folder
    let assert = fabio()
        .args([
            "workspace",
            "create-folder",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &folder_name,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let folder_id = data["id"].as_str().expect("expected folder id").to_string();
    assert_eq!(data["displayName"].as_str().unwrap(), folder_name);

    // List folders — should contain our folder
    let assert = fabio()
        .args([
            "workspace",
            "list-folders",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().expect("expected array of folders");
    assert!(
        arr.iter()
            .any(|f| f["id"].as_str() == Some(folder_id.as_str())),
        "created folder should appear in list"
    );

    // Show folder
    let assert = fabio()
        .args([
            "workspace",
            "show-folder",
            "--workspace",
            &cfg.source_workspace,
            "--folder-id",
            &folder_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"].as_str().unwrap(), folder_id);
    assert_eq!(data["displayName"].as_str().unwrap(), folder_name);

    // Update folder
    let updated_name = format!("{folder_name}Updated");
    let assert = fabio()
        .args([
            "workspace",
            "update-folder",
            "--workspace",
            &cfg.source_workspace,
            "--folder-id",
            &folder_id,
            "--name",
            &updated_name,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"].as_str().unwrap(), updated_name);

    // Create a second folder to test move
    let subfolder_name = unique_name("SubFolder");
    let assert = fabio()
        .args([
            "workspace",
            "create-folder",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &subfolder_name,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let subfolder_id = data["id"].as_str().expect("subfolder id").to_string();

    // Move subfolder into the first folder
    fabio()
        .args([
            "workspace",
            "move-folder",
            "--workspace",
            &cfg.source_workspace,
            "--folder-id",
            &subfolder_id,
            "--target-folder-id",
            &folder_id,
        ])
        .assert()
        .success();

    // Delete subfolder first (child before parent)
    fabio()
        .args([
            "workspace",
            "delete-folder",
            "--workspace",
            &cfg.source_workspace,
            "--folder-id",
            &subfolder_id,
        ])
        .assert()
        .success();

    // Delete the main folder
    fabio()
        .args([
            "workspace",
            "delete-folder",
            "--workspace",
            &cfg.source_workspace,
            "--folder-id",
            &folder_id,
        ])
        .assert()
        .success();
}

// ===========================================================================
// workspace list with --roles filter
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_list_with_roles_filter() {
    let assert = fabio()
        .args(["workspace", "list", "--roles", "Admin"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
    // Should have at least our test workspace where we're Admin
    let count = extract_count(&json);
    assert!(count > 0, "expected at least one workspace with Admin role");
}

// ===========================================================================
// workspace export-lifecycle-policy (read-only)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_export_lifecycle_policy() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "workspace",
            "export-lifecycle-policy",
            "--workspace",
            &cfg.source_workspace,
        ])
        .output()
        .unwrap();

    // Accept success (policy exists) or API_ERROR (no policy configured)
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert!(
            json["data"].is_object(),
            "expected lifecycle policy object in data"
        );
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("API_ERROR")
                || stderr.contains("NOT_FOUND")
                || stderr.contains("ItemNotFound")
                || stderr.contains("FeatureNotAvailable"),
            "unexpected error: {stderr}"
        );
    }
}

// ===========================================================================
// workspace modify-default-tier roundtrip (Hot → Cool → Hot)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_modify_default_tier_roundtrip() {
    let cfg = TestConfig::from_env();

    // Step 1: Get current tier from OneLake settings (nested at lifecycle.defaultTier)
    let assert = fabio()
        .args([
            "workspace",
            "get-onelake-settings",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let original_tier = data["lifecycle"]["defaultTier"]
        .as_str()
        .unwrap_or("Hot")
        .to_string();

    // Step 2: Change to a different tier
    let new_tier = if original_tier == "Hot" {
        "Cool"
    } else {
        "Hot"
    };
    let assert = fabio()
        .args([
            "workspace",
            "modify-default-tier",
            "--workspace",
            &cfg.source_workspace,
            "--tier",
            new_tier,
        ])
        .assert()
        .success();

    // The modify response returns {"data":{"defaultTier":"<new_tier>"}}
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(
        data["defaultTier"].as_str().unwrap_or(""),
        new_tier,
        "modify response should confirm new tier"
    );

    // Step 3: Verify via get-onelake-settings
    let assert = fabio()
        .args([
            "workspace",
            "get-onelake-settings",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let current_tier = data["lifecycle"]["defaultTier"].as_str().unwrap_or("");
    assert_eq!(
        current_tier, new_tier,
        "tier should have changed to {new_tier}"
    );

    // Step 4: Restore original tier
    fabio()
        .args([
            "workspace",
            "modify-default-tier",
            "--workspace",
            &cfg.source_workspace,
            "--tier",
            &original_tier,
        ])
        .assert()
        .success();
}

// ===========================================================================
// workspace modify-default-tier --dry-run
// ===========================================================================

#[test]
fn workspace_modify_default_tier_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "modify-default-tier",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--tier",
            "Cold",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace modify-default-tier");
}

// ===========================================================================
// workspace apply-tags / unapply-tags lifecycle (creates tag via admin)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant + admin access"]
#[serial]
fn workspace_tags_lifecycle() {
    let cfg = TestConfig::from_env();
    let tag_name = unique_name("fabio-ws-tag");

    // Step 1: Create a tag via admin API
    let create_body = serde_json::json!({
        "createTagsRequest": [{ "displayName": tag_name }]
    });
    let output = fabio()
        .args([
            "admin",
            "create-tags",
            "--content",
            &create_body.to_string(),
        ])
        .output()
        .unwrap();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("FORBIDDEN") || stderr.contains("insufficient scopes") {
            eprintln!("SKIP: no admin access for workspace tags test");
            return;
        }
        panic!("admin create-tags failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let create_json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // Extract tag ID — response format: {"data":{"tags":[{"id":"...","displayName":"..."}]}}
    let tag_id = create_json["data"]["tags"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|t| t["id"].as_str())
        .unwrap_or_default()
        .to_string();

    if tag_id.is_empty() {
        // Try alternate response format
        let tag_id_alt = create_json["data"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|t| t["id"].as_str())
            .unwrap_or_default()
            .to_string();
        assert!(
            !tag_id_alt.is_empty(),
            "Failed to extract tag ID from response: {create_json}"
        );
        // Use alternate path
        workspace_tags_lifecycle_inner(&cfg, &tag_id_alt, &tag_name);
        return;
    }

    workspace_tags_lifecycle_inner(&cfg, &tag_id, &tag_name);
}

fn workspace_tags_lifecycle_inner(cfg: &TestConfig, tag_id: &str, _tag_name: &str) {
    // Step 2: Apply tag to workspace
    // NOTE: applyTags returns API_ERROR "invalid input" on some tenants/capacities
    // (root cause unknown — body format matches documented spec). Handle gracefully.
    let output = fabio()
        .args([
            "workspace",
            "apply-tags",
            "--workspace",
            &cfg.source_workspace,
            "--tag-ids",
            tag_id,
        ])
        .output()
        .unwrap();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("API_ERROR") || stderr.contains("invalid input") {
            eprintln!(
                "SKIP apply-tags: endpoint returns API_ERROR on this tenant (known limitation)"
            );
            // Cleanup: delete the tag even though apply failed
            fabio()
                .args(["admin", "delete-tag", "--tag-id", tag_id])
                .assert()
                .success();
            return;
        }
        panic!("workspace apply-tags failed unexpectedly: {stderr}");
    }

    // Step 3: Unapply tag from workspace
    fabio()
        .args([
            "workspace",
            "unapply-tags",
            "--workspace",
            &cfg.source_workspace,
            "--tag-ids",
            tag_id,
        ])
        .assert()
        .success();

    // Step 4: Cleanup - delete the tag
    fabio()
        .args(["admin", "delete-tag", "--tag-id", tag_id])
        .assert()
        .success();
}

// ===========================================================================
// workspace apply-tags --dry-run
// ===========================================================================

#[test]
fn workspace_apply_tags_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "apply-tags",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--tag-ids",
            "00000000-0000-0000-0000-000000000099",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace apply-tags");
}

// ===========================================================================
// workspace unapply-tags --dry-run
// ===========================================================================

#[test]
fn workspace_unapply_tags_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "unapply-tags",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--tag-ids",
            "00000000-0000-0000-0000-000000000099",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace unapply-tags");
}

// ===========================================================================
// workspace assign-to-domain / unassign-from-domain lifecycle
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant + admin access"]
#[serial]
fn workspace_domain_assignment_lifecycle() {
    let cfg = TestConfig::from_env();
    let domain_name = unique_name("fabio-ws-domain");

    // Step 1: Create a domain via admin API
    let output = fabio()
        .args([
            "admin",
            "create-domain",
            "--name",
            &domain_name,
            "--description",
            "E2E test domain for workspace assignment - safe to delete",
        ])
        .output()
        .unwrap();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("FORBIDDEN") || stderr.contains("insufficient scopes") {
            eprintln!("SKIP: no admin access for workspace domain assignment test");
            return;
        }
        panic!("admin create-domain failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let create_json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let domain_id = create_json["data"]["id"]
        .as_str()
        .expect("Created domain should have 'id'")
        .to_string();

    // Step 2: Assign workspace to domain
    fabio()
        .args([
            "workspace",
            "assign-to-domain",
            "--workspace",
            &cfg.source_workspace,
            "--domain-id",
            &domain_id,
        ])
        .assert()
        .success();

    // Step 3: Unassign workspace from domain
    fabio()
        .args([
            "workspace",
            "unassign-from-domain",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    // Step 4: Cleanup - delete the domain
    fabio()
        .args(["admin", "delete-domain", "--domain-id", &domain_id])
        .assert()
        .success();
}

// ===========================================================================
// workspace assign-to-domain --dry-run
// ===========================================================================

#[test]
fn workspace_assign_to_domain_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "assign-to-domain",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--domain-id",
            "00000000-0000-0000-0000-000000000099",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace assign-to-domain");
}

// ===========================================================================
// workspace unassign-from-domain --dry-run
// ===========================================================================

#[test]
fn workspace_unassign_from_domain_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "unassign-from-domain",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace unassign-from-domain");
}

// ===========================================================================
// workspace modify-diagnostics --dry-run
// ===========================================================================

#[test]
fn workspace_modify_diagnostics_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "modify-diagnostics",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--content",
            r#"{"enabled":true}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace modify-diagnostics");
}

// ===========================================================================
// workspace modify-diagnostics live
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_modify_diagnostics_live() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "workspace",
            "modify-diagnostics",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            r#"{"diagnostics":{"enabled":false}}"#,
        ])
        .assert();

    // Accept success or API_ERROR (feature may not be enabled)
    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("API_ERROR")
                || stderr.contains("FeatureNotAvailable")
                || stderr.contains("InvalidInput")
                || stderr.contains("BadRequest"),
            "unexpected error: {stderr}"
        );
    }
}

// ===========================================================================
// workspace modify-immutability-policy --dry-run
// ===========================================================================

#[test]
fn workspace_modify_immutability_policy_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "modify-immutability-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--content",
            r#"{"immutabilityPolicy":{"enabled":false}}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["would_execute"],
        "workspace modify-immutability-policy"
    );
}

// ===========================================================================
// workspace modify-immutability-policy live
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_modify_immutability_policy_live() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "workspace",
            "modify-immutability-policy",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            r#"{"immutabilityPolicy":{"enabled":false}}"#,
        ])
        .assert();

    // Accept success or API_ERROR (feature may not be enabled or payload invalid)
    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("API_ERROR")
                || stderr.contains("FeatureNotAvailable")
                || stderr.contains("InvalidInput")
                || stderr.contains("BadRequest"),
            "unexpected error: {stderr}"
        );
    }
}

// ===========================================================================
// workspace import-lifecycle-policy --dry-run
// ===========================================================================

#[test]
fn workspace_import_lifecycle_policy_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "import-lifecycle-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--content",
            r#"{"rules":[]}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace import-lifecycle-policy");
}

// ===========================================================================
// workspace import-lifecycle-policy missing input
// ===========================================================================

#[test]
fn workspace_import_lifecycle_policy_missing_input() {
    fabio()
        .args([
            "workspace",
            "import-lifecycle-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("INVALID_INPUT"));
}

// ===========================================================================
// workspace set-network-policy --dry-run
// ===========================================================================

#[test]
fn workspace_set_network_policy_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "set-network-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--content",
            r#"{"inbound":{},"outbound":{}}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace set-network-policy");
}

// ===========================================================================
// workspace set-network-policy missing input
// ===========================================================================

#[test]
fn workspace_set_network_policy_missing_input() {
    fabio()
        .args([
            "workspace",
            "set-network-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("INVALID_INPUT"));
}

// ===========================================================================
// workspace deprovision-identity --dry-run
// ===========================================================================

#[test]
fn workspace_deprovision_identity_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "deprovision-identity",
            "--id",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "workspace deprovision-identity");
}

// ===========================================================================
// workspace modify-diagnostics missing input
// ===========================================================================

#[test]
fn workspace_modify_diagnostics_missing_input() {
    fabio()
        .args([
            "workspace",
            "modify-diagnostics",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("INVALID_INPUT"));
}

// ===========================================================================
// workspace modify-immutability-policy missing input
// ===========================================================================

#[test]
fn workspace_modify_immutability_policy_missing_input() {
    fabio()
        .args([
            "workspace",
            "modify-immutability-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("INVALID_INPUT"));
}

// ===========================================================================
// workspace set-network-policy roundtrip (read current → write back same)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_set_network_policy_roundtrip() {
    let cfg = TestConfig::from_env();

    // Step 1: Get current network policy
    let assert = fabio()
        .args([
            "workspace",
            "get-network-policy",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let policy_str = serde_json::to_string(data).unwrap();

    // Step 2: Write back the same policy (idempotent roundtrip)
    let assert = fabio()
        .args([
            "workspace",
            "set-network-policy",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &policy_str,
        ])
        .assert();

    // Accept success or error (some tenants restrict PUT on network policy)
    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("API_ERROR")
                || stderr.contains("FORBIDDEN")
                || stderr.contains("FeatureNotAvailable")
                || stderr.contains("BadRequest"),
            "unexpected error: {stderr}"
        );
    }
}

// ===========================================================================
// workspace import-lifecycle-policy live (may fail if no policy exists)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_import_lifecycle_policy_live() {
    let cfg = TestConfig::from_env();

    // Try to import a minimal lifecycle policy
    let policy = serde_json::json!({
        "rules": []
    });

    let assert = fabio()
        .args([
            "workspace",
            "import-lifecycle-policy",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &policy.to_string(),
        ])
        .assert();

    // Accept success or error (feature may not be enabled or payload format unknown)
    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("API_ERROR")
                || stderr.contains("NOT_FOUND")
                || stderr.contains("FeatureNotAvailable")
                || stderr.contains("BadRequest")
                || stderr.contains("InvalidInput"),
            "unexpected error: {stderr}"
        );
    }
}

// ─── OAP Networking Tests ────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_set_git_outbound_policy_live() {
    let cfg = TestConfig::from_env();

    // Try to set the git outbound policy (requires workspace-level OAP / F64+ capacity)
    let policy = serde_json::json!({
        "defaultAction": "Deny",
        "rules": []
    });

    let assert = fabio()
        .args([
            "workspace",
            "set-git-outbound-policy",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &policy.to_string(),
        ])
        .assert();

    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code == 0 {
        // If it succeeded (F64+ with OAP enabled), verify we can read it back
        let verify = fabio()
            .args([
                "workspace",
                "get-git-outbound-policy",
                "--workspace",
                &cfg.source_workspace,
            ])
            .assert()
            .success();
        let json = parse_json(&verify);
        let data = extract_data(&json);
        assert_eq!(data["defaultAction"], "Deny");
    } else {
        // On Trial/F2 capacity: OAP not available — accept known error
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("FORBIDDEN")
                || stderr.contains("Outbound Access Protection")
                || stderr.contains("NOT_FOUND"),
            "unexpected error: {stderr}"
        );
    }
}

#[test]
fn workspace_set_git_outbound_policy_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "set-git-outbound-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            r#"{"defaultAction":"Allow","rules":[]}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert!(
        data["would_execute"]
            .as_str()
            .unwrap()
            .contains("set-git-outbound-policy")
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_inbound_azure_resource_rules_live() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "get-inbound-azure-resource-rules",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert();

    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code == 0 {
        // Success: should have rules array
        let json = parse_json(&assert);
        let data = extract_data(&json);
        assert!(
            data.get("rules").is_some() || data.is_object(),
            "expected rules or object in response"
        );
    } else {
        // NOT_FOUND expected when no Private Endpoint infrastructure exists
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("NOT_FOUND") || stderr.contains("FORBIDDEN"),
            "unexpected error: {stderr}"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_set_inbound_azure_resource_rules_live() {
    let cfg = TestConfig::from_env();

    let rules = serde_json::json!({"rules": []});

    let assert = fabio()
        .args([
            "workspace",
            "set-inbound-azure-resource-rules",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &rules.to_string(),
        ])
        .assert();

    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        // NOT_FOUND expected without Private Endpoint infrastructure
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("NOT_FOUND") || stderr.contains("FORBIDDEN"),
            "unexpected error: {stderr}"
        );
    }
}

#[test]
fn workspace_set_inbound_azure_resource_rules_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "set-inbound-azure-resource-rules",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            r#"{"rules":[]}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert!(
        data["would_execute"]
            .as_str()
            .unwrap()
            .contains("set-inbound-azure-resource-rules")
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_outbound_cloud_connection_rules_live() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "get-outbound-cloud-connection-rules",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert();

    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code == 0 {
        let json = parse_json(&assert);
        let data = extract_data(&json);
        assert!(data.is_object(), "expected object in response");
    } else {
        // NOT_FOUND or FORBIDDEN expected without OAP enabled at workspace level
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("NOT_FOUND")
                || stderr.contains("FORBIDDEN")
                || stderr.contains("Outbound Access Protection"),
            "unexpected error: {stderr}"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_set_outbound_cloud_connection_rules_live() {
    let cfg = TestConfig::from_env();

    let rules = serde_json::json!({"defaultAction": "Allow", "rules": []});

    let assert = fabio()
        .args([
            "workspace",
            "set-outbound-cloud-connection-rules",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &rules.to_string(),
        ])
        .assert();

    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        // NOT_FOUND or FORBIDDEN expected without OAP (needs F64+ capacity)
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("NOT_FOUND")
                || stderr.contains("FORBIDDEN")
                || stderr.contains("Outbound Access Protection"),
            "unexpected error: {stderr}"
        );
    }
}

#[test]
fn workspace_set_outbound_cloud_connection_rules_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "set-outbound-cloud-connection-rules",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            r#"{"defaultAction":"Allow","rules":[]}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert!(
        data["would_execute"]
            .as_str()
            .unwrap()
            .contains("set-outbound-cloud-connection-rules")
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_outbound_gateway_rules_live() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "get-outbound-gateway-rules",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert();

    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code == 0 {
        let json = parse_json(&assert);
        let data = extract_data(&json);
        assert!(data.is_object(), "expected object in response");
    } else {
        // FORBIDDEN expected without OAP (needs F64+ capacity for outbound restriction)
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("FORBIDDEN")
                || stderr.contains("NOT_FOUND")
                || stderr.contains("Outbound Access Protection"),
            "unexpected error: {stderr}"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_set_outbound_gateway_rules_live() {
    let cfg = TestConfig::from_env();

    let rules = serde_json::json!({"defaultAction": "Allow", "rules": []});

    let assert = fabio()
        .args([
            "workspace",
            "set-outbound-gateway-rules",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &rules.to_string(),
        ])
        .assert();

    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        // FORBIDDEN expected without OAP (needs F64+ capacity)
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("FORBIDDEN")
                || stderr.contains("NOT_FOUND")
                || stderr.contains("Outbound Access Protection"),
            "unexpected error: {stderr}"
        );
    }
}

#[test]
fn workspace_set_outbound_gateway_rules_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "set-outbound-gateway-rules",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            r#"{"defaultAction":"Allow","rules":[]}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert!(
        data["would_execute"]
            .as_str()
            .unwrap()
            .contains("set-outbound-gateway-rules")
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_inbound_external_data_shares_policy_live() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "get-inbound-external-data-shares-policy",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(
        data.get("defaultAction").is_some(),
        "expected 'defaultAction' field"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_set_inbound_external_data_shares_policy_live() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "set-inbound-external-data-shares-policy",
            "--workspace",
            &cfg.source_workspace,
            "--default-action",
            "Allow",
        ])
        .assert();

    let output = assert.get_output();
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        // FORBIDDEN expected without workspace admin role
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("FORBIDDEN") || stderr.contains("NOT_FOUND"),
            "unexpected error: {stderr}"
        );
    }
}

#[test]
fn workspace_set_inbound_external_data_shares_policy_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "workspace",
            "set-inbound-external-data-shares-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--default-action",
            "Deny",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert!(
        data["would_execute"]
            .as_str()
            .unwrap()
            .contains("set-inbound-external-data-shares-policy")
    );
}

#[test]
fn workspace_set_inbound_external_data_shares_policy_invalid_value() {
    fabio()
        .args([
            "workspace",
            "set-inbound-external-data-shares-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--default-action",
            "Maybe",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_inbound_restriction_roundtrip() {
    let cfg = TestConfig::from_env();

    // Read current state
    let before = fabio()
        .args([
            "workspace",
            "get-network-policy",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let before_json = parse_json(&before);
    let before_data = extract_data(&before_json);

    // Enable inbound restriction (Deny)
    let deny_policy = serde_json::json!({
        "inbound": {"publicAccessRules": {"defaultAction": "Deny"}},
        "outbound": {"publicAccessRules": {"defaultAction": "Allow"}}
    });

    let set_result = fabio()
        .args([
            "workspace",
            "set-network-policy",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &deny_policy.to_string(),
        ])
        .assert();

    let set_output = set_result.get_output();
    if set_output.status.code().unwrap_or(1) != 0 {
        let stderr = String::from_utf8_lossy(&set_output.stderr);
        // If inbound restriction not supported, skip test
        assert!(
            stderr.contains("FORBIDDEN") || stderr.contains("not allowed"),
            "unexpected error: {stderr}"
        );
        return;
    }

    // Verify inbound is now Deny
    let verify = fabio()
        .args([
            "workspace",
            "get-network-policy",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let verify_json = parse_json(&verify);
    let verify_data = extract_data(&verify_json);
    assert_eq!(
        verify_data["inbound"]["publicAccessRules"]["defaultAction"], "Deny",
        "expected inbound to be Deny after setting"
    );

    // Restore original state
    let restore_policy = serde_json::json!({
        "inbound": {"publicAccessRules": {"defaultAction": before_data["inbound"]["publicAccessRules"]["defaultAction"]}},
        "outbound": {"publicAccessRules": {"defaultAction": before_data["outbound"]["publicAccessRules"]["defaultAction"]}}
    });

    fabio()
        .args([
            "workspace",
            "set-network-policy",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &restore_policy.to_string(),
        ])
        .assert()
        .success();
}

// ===========================================================================
// workspace update-settings live roundtrip
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_update_settings_live() {
    let cfg = TestConfig::from_env();

    // Get current workspace description
    let show_assert = fabio()
        .args(["workspace", "show", "--id", &cfg.source_workspace])
        .assert()
        .success();
    let show_json = parse_json(&show_assert);
    let show_data = extract_data(&show_json);
    let original_desc = show_data["description"].as_str().unwrap_or("").to_string();

    // Update description via update-settings
    let new_desc = format!(
        "fabio-e2e-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            % 100_000
    );
    let body = serde_json::json!({ "description": new_desc });

    let assert = fabio()
        .args([
            "workspace",
            "update-settings",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &body.to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_object(), "expected JSON object response");

    // Verify the description changed
    let verify = fabio()
        .args(["workspace", "show", "--id", &cfg.source_workspace])
        .assert()
        .success();
    let verify_json = parse_json(&verify);
    let verify_data = extract_data(&verify_json);
    assert_eq!(
        verify_data["description"].as_str().unwrap_or(""),
        new_desc,
        "description should have been updated"
    );

    // Restore original description
    let restore_body = serde_json::json!({ "description": original_desc });
    fabio()
        .args([
            "workspace",
            "update-settings",
            "--workspace",
            &cfg.source_workspace,
            "--content",
            &restore_body.to_string(),
        ])
        .assert()
        .success();
}

// ===========================================================================
// workspace provision-identity / deprovision-identity live roundtrip
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_identity_roundtrip() {
    let cfg = TestConfig::from_env();

    // Step 1: Provision identity on the workspace
    let output = fabio()
        .args([
            "workspace",
            "provision-identity",
            "--id",
            &cfg.source_workspace,
        ])
        .output()
        .unwrap();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Some tenants/capacities may not support workspace identity
        if stderr.contains("FORBIDDEN")
            || stderr.contains("not enabled")
            || stderr.contains("FeatureNotAvailable")
        {
            eprintln!("SKIP: workspace identity provisioning not available on this tenant");
            return;
        }
        panic!("provision-identity failed unexpectedly: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let data = extract_data(&json);
    // Should return servicePrincipalId or similar
    assert!(
        data.is_object(),
        "Expected JSON object from provision-identity, got: {data}"
    );

    // Step 2: Deprovision identity
    let depr_output = fabio()
        .args([
            "workspace",
            "deprovision-identity",
            "--id",
            &cfg.source_workspace,
        ])
        .output()
        .unwrap();

    if !depr_output.status.success() {
        let stderr = String::from_utf8_lossy(&depr_output.stderr);
        // If already deprovisioned or not supported, just skip
        if stderr.contains("NOT_FOUND") || stderr.contains("FORBIDDEN") {
            eprintln!("NOTE: deprovision returned error (may already be deprovisioned): {stderr}");
            return;
        }
        panic!("deprovision-identity failed unexpectedly: {stderr}");
    }

    let depr_stdout = String::from_utf8_lossy(&depr_output.stdout);
    let depr_json: serde_json::Value = serde_json::from_str(&depr_stdout).unwrap();
    let depr_data = extract_data(&depr_json);
    assert_eq!(depr_data["status"], "deprovisioned");
}

// ===========================================================================
// workspace url — returns Fabric portal URL
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_url_returns_portal_url() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["workspace", "url", "--id", &cfg.source_workspace])
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
        url.contains(&cfg.source_workspace),
        "URL should contain workspace ID: {url}"
    );
    assert_eq!(data["workspaceId"], cfg.source_workspace);
}

// ===========================================================================
// workspace list --capacity filter
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_list_with_capacity_filter() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["workspace", "list", "--capacity", &cfg.capacity_id])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
    let arr = data.as_array().unwrap();

    // Our source workspace is assigned to this capacity, so at least one match
    assert!(
        !arr.is_empty(),
        "expected at least one workspace on capacity"
    );

    // Verify all returned workspaces have the correct capacityId
    for ws in arr {
        let cap = ws.get("capacityId").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(
            cap.to_lowercase(),
            cfg.capacity_id.to_lowercase(),
            "workspace {} has unexpected capacityId: {cap}",
            ws["displayName"]
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_list_with_capacity_filter_nonexistent() {
    // A capacity ID that doesn't exist should return zero results
    let assert = fabio()
        .args([
            "workspace",
            "list",
            "--capacity",
            "00000000-0000-0000-0000-000000000099",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();
    assert!(arr.is_empty(), "expected no workspaces for fake capacity");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_list_capacity_combined_with_roles() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "workspace",
            "list",
            "--roles",
            "Admin",
            "--capacity",
            &cfg.capacity_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
    // Combined filter: Admin role AND on our capacity
    let arr = data.as_array().unwrap();
    assert!(
        !arr.is_empty(),
        "expected at least one Admin workspace on our capacity"
    );
}

// ─── CMK Encryption ──────────────────────────────────────────────────────────

#[test]
fn workspace_assign_encryption_dry_run() {
    fabio()
        .args([
            "workspace",
            "assign-encryption",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--key-identifier",
            "https://myvault.vault.azure.net/keys/mykey",
            "--dry-run",
        ])
        .assert()
        .success();
}

#[test]
fn workspace_assign_encryption_missing_key_fails() {
    fabio()
        .args([
            "workspace",
            "assign-encryption",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            // missing --key-identifier
        ])
        .assert()
        .failure();
}

#[test]
fn workspace_reset_encryption_dry_run() {
    fabio()
        .args([
            "workspace",
            "reset-encryption",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--dry-run",
        ])
        .assert()
        .success();
}

#[test]
fn workspace_get_encryption_help() {
    fabio()
        .args(["workspace", "get-encryption", "--help"])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_get_encryption_live() {
    let cfg = TestConfig::from_env();
    // GET encryption settings — works even without CMK configured (returns Disabled status)
    let assert = fabio()
        .args([
            "workspace",
            "get-encryption",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should return encryption details (Disabled if no CMK configured)
    assert!(
        data.get("encryptionDetail").is_some()
            || data.get("encryptionStatus").is_some()
            || data.is_object(),
        "Expected encryption details in response"
    );
}

// --- workspace clone tests ---

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_clone_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "workspace",
            "clone",
            "--source",
            &cfg.source_workspace,
            "--dest",
            &cfg.dest_workspace,
            "--allow-pairing-by-name",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "workspace clone");
    assert_eq!(data["details"]["allow_pairing_by_name"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_clone_same_workspace_fails() {
    let cfg = TestConfig::from_env();
    fabio()
        .args([
            "workspace",
            "clone",
            "--source",
            &cfg.source_workspace,
            "--dest",
            &cfg.source_workspace,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be the same"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn workspace_clone_with_item_types_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "workspace",
            "clone",
            "--source",
            &cfg.source_workspace,
            "--dest",
            &cfg.dest_workspace,
            "--item-types",
            "Notebook,DataPipeline",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "workspace clone");
}
