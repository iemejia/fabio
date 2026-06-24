//! E2E integration tests for the `fabio admin` command group.
//!
//! Admin APIs require the Fabric Admin role. All tests use the dual-assertion
//! pattern: on success we validate the structured JSON response; on failure we
//! verify a well-formed FORBIDDEN error. This ensures correctness regardless of
//! whether the caller has admin privileges.
//!
//! Tests are organized by risk tier:
//! - Tier 0: Read-only commands (zero risk)
//! - Tier 1: Create + delete lifecycle (full rollback)
//! - Tier 2: Dry-run validations for destructive commands

mod common;

use common::{TestConfig, fabio, unique_name};
use serde_json::Value;
use serial_test::serial;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Assert that the output is either a successful JSON response or a known error.
/// Returns `Some(json)` on success, `None` on `FORBIDDEN`/`NOT_FOUND`/`API_ERROR`.
fn assert_admin_output(output: &std::process::Output) -> Option<Value> {
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\n{stdout}"));
        Some(json)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let json: Value = serde_json::from_str(&stderr)
            .unwrap_or_else(|e| panic!("stderr not valid JSON: {e}\n{stderr}"));
        let code = json["error"]["code"].as_str().unwrap_or("");
        assert!(
            matches!(code, "FORBIDDEN" | "NOT_FOUND" | "API_ERROR"),
            "Expected FORBIDDEN/NOT_FOUND/API_ERROR, got: {json}"
        );
        None
    }
}

/// Assert a list response has correct envelope structure.
fn assert_list_envelope(json: &Value) {
    assert!(
        json["data"].is_array(),
        "Expected 'data' to be an array, got: {json}",
    );
    assert!(
        json["count"].is_number(),
        "Expected 'count' to be a number, got: {json}",
    );
}

/// Assert a list response is non-empty.
fn assert_list_non_empty(json: &Value) {
    assert_list_envelope(json);
    let arr = json["data"].as_array().unwrap();
    assert!(!arr.is_empty(), "Expected non-empty list, got: {json}");
}

// ─── Tier 0: Read-Only Commands ──────────────────────────────────────────────
// These commands only fetch data. They are completely safe to run.

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_tenant_settings() {
    let output = fabio()
        .args(["admin", "list-tenant-settings"])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        // Tenant settings always exist in any tenant
        assert_list_non_empty(&json);
        // Verify known fields
        let first = &json["data"][0];
        assert!(
            first["settingName"].is_string(),
            "Expected 'settingName' field"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_capacities_tenant_overrides() {
    let output = fabio()
        .args(["admin", "list-capacities-tenant-overrides"])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_capacity_tenant_overrides() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "list-capacity-tenant-overrides",
            "--capacity-id",
            &cfg.capacity_id,
        ])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_domains_tenant_overrides() {
    let output = fabio()
        .args(["admin", "list-domains-tenant-overrides"])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_workspaces_tenant_overrides() {
    let output = fabio()
        .args(["admin", "list-workspaces-tenant-overrides"])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_tags() {
    let output = fabio().args(["admin", "list-tags"]).output().unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_workloads() {
    let output = fabio().args(["admin", "list-workloads"]).output().unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_workload_assignments() {
    let output = fabio()
        .args(["admin", "list-workload-assignments"])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_workspaces() {
    let output = fabio().args(["admin", "list-workspaces"]).output().unwrap();

    if let Some(json) = assert_admin_output(&output) {
        // Should always have at least one workspace (the test workspace exists)
        assert_list_non_empty(&json);
        let first = &json["data"][0];
        assert!(first["id"].is_string(), "Expected 'id' field on workspace");
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_show_workspace() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "show-workspace",
            "--workspace",
            &cfg.source_workspace,
        ])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        let data = &json["data"];
        assert!(data["id"].is_string(), "Expected 'id' field");
        assert!(data["name"].is_string(), "Expected 'name' field");
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_workspace_users() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "list-workspace-users",
            "--workspace",
            &cfg.source_workspace,
        ])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        // At least the caller should have access
        assert_list_non_empty(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_git_connections() {
    let output = fabio()
        .args(["admin", "list-git-connections"])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_network_policies() {
    let output = fabio()
        .args(["admin", "list-network-policies"])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_items() {
    let output = fabio().args(["admin", "list-items"]).output().unwrap();

    if let Some(json) = assert_admin_output(&output) {
        // Tenant always has items
        assert_list_non_empty(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_show_item() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "show-item",
            "--workspace",
            &cfg.source_workspace,
            "--item-id",
            &cfg.source_lakehouse,
        ])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        let data = &json["data"];
        assert!(data["id"].is_string(), "Expected 'id' field");
        assert!(data["type"].is_string(), "Expected 'type' field");
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_item_users() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "list-item-users",
            "--workspace",
            &cfg.source_workspace,
            "--item-id",
            &cfg.source_lakehouse,
        ])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        // At least the caller should have access
        assert_list_non_empty(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_external_data_shares() {
    let output = fabio()
        .args(["admin", "list-external-data-shares"])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_domains() {
    let output = fabio().args(["admin", "list-domains"]).output().unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

// ─── Tier 1: Tag Lifecycle (create → list → update → delete) ────────────────
// Tags can be cleanly created and deleted. This exercises the full lifecycle
// while seeding data for `list-tags` to be meaningfully non-empty.

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_tag_lifecycle() {
    let tag_name = unique_name("fabio-test-tag");

    // ── Step 1: Create tag ──
    let create_body = serde_json::json!({
        "createTagsRequest": [
            { "displayName": tag_name }
        ]
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

    let Some(create_json) = assert_admin_output(&output) else {
        // No admin access — skip rest of lifecycle
        eprintln!("SKIP: no admin access for tag lifecycle test");
        return;
    };

    // Extract the tag ID from the created response
    // The API returns the created tags; extract the ID
    let tag_id = extract_tag_id(&create_json, &tag_name);
    assert!(
        !tag_id.is_empty(),
        "Failed to extract tag ID from create response: {create_json}"
    );

    // ── Step 2: List tags and verify our tag is present ──
    let output = fabio().args(["admin", "list-tags"]).output().unwrap();
    let list_json = assert_admin_output(&output).expect("list-tags should succeed after create");
    assert_list_non_empty(&list_json);

    let found = list_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"].as_str() == Some(&tag_id));
    assert!(found, "Created tag {tag_id} not found in list-tags");

    // ── Step 3: Update tag ──
    let new_name = format!("{tag_name}-updated");
    let output = fabio()
        .args([
            "admin",
            "update-tag",
            "--tag-id",
            &tag_id,
            "--name",
            &new_name,
            "--description",
            "Updated description",
        ])
        .output()
        .unwrap();

    let update_json = assert_admin_output(&output).expect("update-tag should succeed");
    // Verify the update returned updated data
    let updated_name = update_json["data"]["displayName"]
        .as_str()
        .unwrap_or_default();
    assert_eq!(updated_name, new_name, "Tag name should be updated");

    // ── Step 4: Delete tag (cleanup) ──
    let output = fabio()
        .args(["admin", "delete-tag", "--tag-id", &tag_id])
        .output()
        .unwrap();

    let delete_json = assert_admin_output(&output).expect("delete-tag should succeed");
    assert_eq!(delete_json["data"]["status"], "deleted");

    // ── Step 5: Verify tag is gone ──
    let output = fabio().args(["admin", "list-tags"]).output().unwrap();
    let list_json = assert_admin_output(&output).expect("list-tags should still succeed");
    let still_present = list_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"].as_str() == Some(&tag_id));
    assert!(!still_present, "Tag {tag_id} should be gone after deletion");
}

/// Extract the tag ID from the create-tags response.
/// The response format may vary — handles both array and object responses.
fn extract_tag_id(json: &Value, name: &str) -> String {
    // Try: data is array of created tags
    if let Some(arr) = json["data"].as_array() {
        for tag in arr {
            if tag["displayName"].as_str() == Some(name)
                && let Some(id) = tag["id"].as_str()
            {
                return id.to_string();
            }
        }
    }
    // Try: data is an object with nested tags
    if let Some(tags) = json["data"]["tags"].as_array() {
        for tag in tags {
            if tag["displayName"].as_str() == Some(name)
                && let Some(id) = tag["id"].as_str()
            {
                return id.to_string();
            }
        }
    }
    // Try: data.value array
    if let Some(tags) = json["data"]["value"].as_array() {
        for tag in tags {
            if tag["displayName"].as_str() == Some(name)
                && let Some(id) = tag["id"].as_str()
            {
                return id.to_string();
            }
        }
    }
    // Fallback: data itself has an id
    if let Some(id) = json["data"]["id"].as_str() {
        return id.to_string();
    }
    String::new()
}

// ─── Tier 1: Domain Lifecycle (create → show → update → list → delete) ──────
// Domains can be cleanly created and deleted. A fresh test domain is used
// to avoid interfering with real organizational domains.

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_domain_lifecycle() {
    let domain_name = unique_name("fabio-test-domain");
    let domain_desc = "E2E test domain - safe to delete";

    // ── Step 1: Create domain ──
    let output = fabio()
        .args([
            "admin",
            "create-domain",
            "--name",
            &domain_name,
            "--description",
            domain_desc,
        ])
        .output()
        .unwrap();

    let Some(create_json) = assert_admin_output(&output) else {
        eprintln!("SKIP: no admin access for domain lifecycle test");
        return;
    };

    let domain_id = create_json["data"]["id"]
        .as_str()
        .expect("Created domain should have 'id'")
        .to_string();

    // ── Step 2: Show domain ──
    let output = fabio()
        .args(["admin", "show-domain", "--domain-id", &domain_id])
        .output()
        .unwrap();

    let show_json = assert_admin_output(&output).expect("show-domain should succeed");
    assert_eq!(show_json["data"]["id"].as_str().unwrap(), domain_id);
    assert_eq!(
        show_json["data"]["displayName"].as_str().unwrap(),
        domain_name
    );

    // ── Step 3: Update domain ──
    let new_name = format!("{domain_name}-updated");
    let output = fabio()
        .args([
            "admin",
            "update-domain",
            "--domain-id",
            &domain_id,
            "--name",
            &new_name,
        ])
        .output()
        .unwrap();

    let update_json = assert_admin_output(&output).expect("update-domain should succeed");
    assert_eq!(
        update_json["data"]["displayName"].as_str().unwrap(),
        new_name
    );

    // ── Step 4: List domains and verify ours is present ──
    let output = fabio().args(["admin", "list-domains"]).output().unwrap();
    let list_json = assert_admin_output(&output).expect("list-domains should succeed");
    let found = list_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["id"].as_str() == Some(domain_id.as_str()));
    assert!(found, "Created domain {domain_id} not found in list");

    // ── Step 5: List domain workspaces (should be empty for new domain) ──
    let output = fabio()
        .args(["admin", "list-domain-workspaces", "--domain-id", &domain_id])
        .output()
        .unwrap();
    let ws_json = assert_admin_output(&output).expect("list-domain-workspaces should succeed");
    assert_list_envelope(&ws_json);

    // ── Step 6: List domain role assignments ──
    let output = fabio()
        .args([
            "admin",
            "list-domain-role-assignments",
            "--domain-id",
            &domain_id,
        ])
        .output()
        .unwrap();
    let roles_json =
        assert_admin_output(&output).expect("list-domain-role-assignments should succeed");
    assert_list_envelope(&roles_json);

    // ── Step 7: Delete domain (cleanup) ──
    let output = fabio()
        .args(["admin", "delete-domain", "--domain-id", &domain_id])
        .output()
        .unwrap();
    let delete_json = assert_admin_output(&output).expect("delete-domain should succeed");
    assert_eq!(delete_json["data"]["status"], "deleted");
}

// ─── Tier 2: Domain Workspace Assignments (on ephemeral domain) ──────────────
// Creates a domain, assigns/unassigns workspaces, then deletes the domain.

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_domain_workspace_assignment_lifecycle() {
    let cfg = TestConfig::from_env();
    let domain_name = unique_name("fabio-test-assign");

    // ── Create domain ──
    let output = fabio()
        .args([
            "admin",
            "create-domain",
            "--name",
            &domain_name,
            "--description",
            "Workspace assignment test",
        ])
        .output()
        .unwrap();

    let Some(create_json) = assert_admin_output(&output) else {
        eprintln!("SKIP: no admin access for domain assignment test");
        return;
    };

    let domain_id = create_json["data"]["id"]
        .as_str()
        .expect("domain id")
        .to_string();

    // ── Assign workspace to domain ──
    let output = fabio()
        .args([
            "admin",
            "assign-domain-workspaces",
            "--domain-id",
            &domain_id,
            "--workspace-ids",
            &cfg.source_workspace,
        ])
        .output()
        .unwrap();
    assert_admin_output(&output).expect("assign-domain-workspaces should succeed");

    // ── Verify workspace is listed under domain ──
    let output = fabio()
        .args(["admin", "list-domain-workspaces", "--domain-id", &domain_id])
        .output()
        .unwrap();
    let list_json = assert_admin_output(&output).expect("list-domain-workspaces should succeed");
    let found = list_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"].as_str() == Some(&cfg.source_workspace));
    assert!(found, "Assigned workspace should appear in domain");

    // ── Unassign workspace from domain ──
    let output = fabio()
        .args([
            "admin",
            "unassign-domain-workspaces",
            "--domain-id",
            &domain_id,
            "--workspace-ids",
            &cfg.source_workspace,
        ])
        .output()
        .unwrap();
    assert_admin_output(&output).expect("unassign-domain-workspaces should succeed");

    // ── Verify workspace is removed ──
    let output = fabio()
        .args(["admin", "list-domain-workspaces", "--domain-id", &domain_id])
        .output()
        .unwrap();
    let list_json = assert_admin_output(&output).expect("list after unassign");
    let still_found = list_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"].as_str() == Some(&cfg.source_workspace));
    assert!(!still_found, "Workspace should be removed after unassign");

    // ── Cleanup: delete domain ──
    let output = fabio()
        .args(["admin", "delete-domain", "--domain-id", &domain_id])
        .output()
        .unwrap();
    assert_admin_output(&output).expect("delete-domain cleanup");
}

// ─── Tier 2: Workspace Admin Access Grant/Revoke ─────────────────────────────
// Grant temporary admin access, then revoke it.

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_grant_revoke_workspace_access() {
    let cfg = TestConfig::from_env();

    // ── Grant admin access ──
    let output = fabio()
        .args([
            "admin",
            "grant-admin-access",
            "--workspace",
            &cfg.source_workspace,
        ])
        .output()
        .unwrap();

    let Some(_) = assert_admin_output(&output) else {
        eprintln!("SKIP: no admin access for grant/revoke test");
        return;
    };

    // ── Remove admin access (rollback) ──
    let output = fabio()
        .args([
            "admin",
            "remove-admin-access",
            "--workspace",
            &cfg.source_workspace,
        ])
        .output()
        .unwrap();
    assert_admin_output(&output).expect("remove-admin-access should succeed");
}

// ─── Tier 3: Dry-Run Validations for Destructive Commands ───────────────────
// Verify that --dry-run works correctly (returns request body without executing).

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_update_tenant_setting_dry_run() {
    let body = serde_json::json!({
        "enabled": true,
        "enabledSecurityGroups": []
    });

    let output = fabio()
        .args([
            "admin",
            "update-tenant-setting",
            "--setting-name",
            "SomeSetting",
            "--content",
            &body.to_string(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    // Dry-run always succeeds (no API call)
    assert!(output.status.success(), "dry-run should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_create_tags_dry_run() {
    let body = serde_json::json!({
        "createTagsRequest": [{ "displayName": "dry-run-tag" }]
    });

    let output = fabio()
        .args([
            "admin",
            "create-tags",
            "--content",
            &body.to_string(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_delete_tag_dry_run() {
    let output = fabio()
        .args([
            "admin",
            "delete-tag",
            "--tag-id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_revoke_external_data_share_dry_run() {
    let output = fabio()
        .args([
            "admin",
            "revoke-external-data-share",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--item-id",
            "11111111-1111-1111-1111-111111111111",
            "--share-id",
            "22222222-2222-2222-2222-222222222222",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_bulk_set_labels_dry_run() {
    let body = serde_json::json!({
        "items": [
            { "id": "00000000-0000-0000-0000-000000000000", "labelId": "test-label" }
        ]
    });

    let output = fabio()
        .args([
            "admin",
            "bulk-set-labels",
            "--content",
            &body.to_string(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_bulk_remove_labels_dry_run() {
    let body = serde_json::json!({
        "items": [
            { "id": "00000000-0000-0000-0000-000000000000" }
        ]
    });

    let output = fabio()
        .args([
            "admin",
            "bulk-remove-labels",
            "--content",
            &body.to_string(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_remove_all_sharing_links_dry_run() {
    let body = serde_json::json!({
        "items": [
            { "workspaceId": "00000000-0000-0000-0000-000000000000", "itemId": "11111111-1111-1111-1111-111111111111" }
        ]
    });

    let output = fabio()
        .args([
            "admin",
            "remove-all-sharing-links",
            "--content",
            &body.to_string(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_bulk_remove_sharing_links_dry_run() {
    let body = serde_json::json!({
        "sharingLinks": [
            { "linkId": "00000000-0000-0000-0000-000000000000" }
        ]
    });

    let output = fabio()
        .args([
            "admin",
            "bulk-remove-sharing-links",
            "--content",
            &body.to_string(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_create_domain_dry_run() {
    let output = fabio()
        .args([
            "admin",
            "create-domain",
            "--name",
            "dry-run-domain",
            "--description",
            "Should not be created",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_delete_domain_dry_run() {
    let output = fabio()
        .args([
            "admin",
            "delete-domain",
            "--domain-id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_assign_domain_workspaces_dry_run() {
    let output = fabio()
        .args([
            "admin",
            "assign-domain-workspaces",
            "--domain-id",
            "00000000-0000-0000-0000-000000000000",
            "--workspace-ids",
            "11111111-1111-1111-1111-111111111111",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_unassign_domain_workspaces_dry_run() {
    let output = fabio()
        .args([
            "admin",
            "unassign-domain-workspaces",
            "--domain-id",
            "00000000-0000-0000-0000-000000000000",
            "--workspace-ids",
            "11111111-1111-1111-1111-111111111111",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_delete_capacity_tenant_override_dry_run() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "delete-capacity-tenant-override",
            "--capacity-id",
            &cfg.capacity_id,
            "--setting-name",
            "SomeSetting",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_update_capacity_tenant_override_dry_run() {
    let cfg = TestConfig::from_env();
    let body = serde_json::json!({ "enabled": true });

    let output = fabio()
        .args([
            "admin",
            "update-capacity-tenant-override",
            "--capacity-id",
            &cfg.capacity_id,
            "--setting-name",
            "SomeSetting",
            "--content",
            &body.to_string(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_restore_workspace_dry_run() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "restore-workspace",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "restored-ws",
            "--capacity-id",
            &cfg.capacity_id,
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

// ─── Tier 3: Update-tag validation ──────────────────────────────────────────
// Validates that update-tag requires at least one field.

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_update_tag_requires_field() {
    let output = fabio()
        .args([
            "admin",
            "update-tag",
            "--tag-id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(json["error"]["code"], "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_update_domain_requires_field() {
    let output = fabio()
        .args([
            "admin",
            "update-domain",
            "--domain-id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(json["error"]["code"], "INVALID_INPUT");
}

// ─── Tier 3: Workload Assignment Dry-Run ────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_create_workload_assignment_dry_run() {
    let body = serde_json::json!({
        "workloadId": "test-workload",
        "capacityId": "00000000-0000-0000-0000-000000000000"
    });

    let output = fabio()
        .args([
            "admin",
            "create-workload-assignment",
            "--content",
            &body.to_string(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_delete_workload_assignment_dry_run() {
    let output = fabio()
        .args([
            "admin",
            "delete-workload-assignment",
            "--assignment-id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

// ─── Tier 3: Grant/Revoke Dry-Run ───────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_grant_admin_access_dry_run() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "grant-admin-access",
            "--workspace",
            &cfg.source_workspace,
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_remove_admin_access_dry_run() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "remove-admin-access",
            "--workspace",
            &cfg.source_workspace,
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

// ─── Tier 0: list-user-access ───────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_list_user_access() {
    // Use a well-known dummy user ID — will return empty list or FORBIDDEN
    let output = fabio()
        .args([
            "admin",
            "list-user-access",
            "--user-id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .output()
        .unwrap();

    if let Some(json) = assert_admin_output(&output) {
        assert_list_envelope(&json);
    }
}

// ─── Tier 3: Domain Assign-by-Capacities/Principals Dry-Run ─────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_assign_domain_workspaces_by_capacities_dry_run() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "assign-domain-workspaces-by-capacities",
            "--domain-id",
            "00000000-0000-0000-0000-000000000000",
            "--capacity-ids",
            &cfg.capacity_id,
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_assign_domain_workspaces_by_principals_dry_run() {
    let output = fabio()
        .args([
            "admin",
            "assign-domain-workspaces-by-principals",
            "--domain-id",
            "00000000-0000-0000-0000-000000000000",
            "--principal-ids",
            "11111111-1111-1111-1111-111111111111",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

// ─── Tier 3: Domain Role Bulk Assign/Unassign Dry-Run ───────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_bulk_assign_domain_roles_dry_run() {
    let body = serde_json::json!({
        "principals": [
            { "id": "11111111-1111-1111-1111-111111111111", "type": "User", "role": "Contributors" }
        ]
    });

    let output = fabio()
        .args([
            "admin",
            "bulk-assign-domain-roles",
            "--domain-id",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            &body.to_string(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_bulk_unassign_domain_roles_dry_run() {
    let body = serde_json::json!({
        "principals": [
            { "id": "11111111-1111-1111-1111-111111111111", "type": "User" }
        ]
    });

    let output = fabio()
        .args([
            "admin",
            "bulk-unassign-domain-roles",
            "--domain-id",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            &body.to_string(),
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

// ─── Tier 3: Sync Domain Roles / Unassign-All Dry-Run ───────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_sync_domain_roles_to_subdomains_dry_run() {
    let output = fabio()
        .args([
            "admin",
            "sync-domain-roles-to-subdomains",
            "--domain-id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_unassign_all_domain_workspaces_dry_run() {
    let output = fabio()
        .args([
            "admin",
            "unassign-all-domain-workspaces",
            "--domain-id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
}

// ══════════════════════════════════════════════════════════════════════════════
// Phase B: Tenant settings + capacity overrides (snapshot-restore)
// ══════════════════════════════════════════════════════════════════════════════

/// Roundtrip: enable → verify → disable → verify for `AdminApisIncludeDetailedMetadata`
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_tenant_setting_roundtrip() {
    // Enable (it's normally disabled)
    let output = fabio()
        .args([
            "admin",
            "update-tenant-setting",
            "--setting-name",
            "AdminApisIncludeDetailedMetadata",
            "--content",
            r#"{"enabled":true}"#,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    // Response returns all settings in the same group
    let settings = json["data"]["tenantSettings"].as_array().unwrap();
    let updated = settings
        .iter()
        .find(|s| s["settingName"] == "AdminApisIncludeDetailedMetadata")
        .expect("Should find the updated setting in response");
    assert_eq!(updated["enabled"], true);

    // Restore to disabled
    let output = fabio()
        .args([
            "admin",
            "update-tenant-setting",
            "--setting-name",
            "AdminApisIncludeDetailedMetadata",
            "--content",
            r#"{"enabled":false}"#,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let settings = json["data"]["tenantSettings"].as_array().unwrap();
    let restored = settings
        .iter()
        .find(|s| s["settingName"] == "AdminApisIncludeDetailedMetadata")
        .expect("Should find the restored setting");
    assert_eq!(restored["enabled"], false);
}

/// Roundtrip: create override → list → delete → list (`PlatformMonitoringTenantSetting`)
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_capacity_override_roundtrip() {
    let cfg = TestConfig::from_env();

    // Create override
    let output = fabio()
        .args([
            "admin",
            "update-capacity-tenant-override",
            "--setting-name",
            "PlatformMonitoringTenantSetting",
            "--capacity-id",
            &cfg.capacity_id,
            "--content",
            r#"{"enabled":true}"#,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let overrides = json["data"]["overrides"].as_array().unwrap();
    assert!(
        !overrides.is_empty(),
        "Should have at least one override in response"
    );
    assert_eq!(
        overrides[0]["settingName"],
        "PlatformMonitoringTenantSetting"
    );

    // Verify via list
    let output = fabio()
        .args([
            "admin",
            "list-capacity-tenant-overrides",
            "--capacity-id",
            &cfg.capacity_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["count"].as_u64().unwrap() >= 1);

    // Delete override
    let output = fabio()
        .args([
            "admin",
            "delete-capacity-tenant-override",
            "--setting-name",
            "PlatformMonitoringTenantSetting",
            "--capacity-id",
            &cfg.capacity_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify deleted
    let output = fabio()
        .args([
            "admin",
            "list-capacity-tenant-overrides",
            "--capacity-id",
            &cfg.capacity_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["count"], 0);
}

/// Test sync-domain-roles-to-subdomains with parent+child domain
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_sync_roles_to_subdomain() {
    // Create parent domain
    let parent_name = unique_name("fabio-sync-parent");
    let output = fabio()
        .args(["admin", "create-domain", "--name", &parent_name])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let parent_id = json["data"]["id"].as_str().unwrap().to_string();

    // Create child subdomain
    let child_name = unique_name("fabio-sync-child");
    let output = fabio()
        .args([
            "admin",
            "create-domain",
            "--name",
            &child_name,
            "--parent-id",
            &parent_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let child_id = json["data"]["id"].as_str().unwrap().to_string();

    // Assign a Contributor on parent
    let output = fabio()
        .args([
            "admin",
            "bulk-assign-domain-roles",
            "--domain-id",
            &parent_id,
            "--content",
            r#"{"type":"Contributors","principals":[{"id":"0dde4270-4462-4e16-995c-a2fc674e04ef","type":"User"}]}"#,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Sync to subdomains
    let output = fabio()
        .args([
            "admin",
            "sync-domain-roles-to-subdomains",
            "--domain-id",
            &parent_id,
            "--role",
            "Contributor",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify child has the role
    let output = fabio()
        .args([
            "admin",
            "list-domain-role-assignments",
            "--domain-id",
            &child_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let roles = json["data"].as_array().unwrap();
    let has_user = roles.iter().any(|r| {
        r["principal"]["id"] == "0dde4270-4462-4e16-995c-a2fc674e04ef" && r["role"] == "Contributor"
    });
    assert!(has_user, "Child domain should have synced Contributor role");

    // Cleanup: delete child first, then parent
    let _ = fabio()
        .args(["admin", "delete-domain", "--domain-id", &child_id])
        .output();
    let _ = fabio()
        .args(["admin", "delete-domain", "--domain-id", &parent_id])
        .output();
}

// ─── Phase C1: Domain Sandbox Roundtrip ──────────────────────────────────────

/// Test full domain workspace lifecycle: assign → unassign-all → assign-by-capacities → unassign
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_domain_workspace_lifecycle() {
    let cfg = TestConfig::from_env();
    let domain_name = unique_name("fabio-c1-ws-lifecycle");

    // Create sandbox domain
    let output = fabio()
        .args(["admin", "create-domain", "--name", &domain_name])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let domain_id = json["data"]["id"].as_str().unwrap().to_string();

    // Assign workspace directly
    let output = fabio()
        .args([
            "admin",
            "assign-domain-workspaces",
            "--domain-id",
            &domain_id,
            "--workspace-ids",
            &cfg.source_workspace,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify workspace is assigned
    let output = fabio()
        .args(["admin", "list-domain-workspaces", "--domain-id", &domain_id])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["count"].as_u64().unwrap() >= 1);

    // Unassign all workspaces
    let output = fabio()
        .args([
            "admin",
            "unassign-all-domain-workspaces",
            "--domain-id",
            &domain_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify empty
    let output = fabio()
        .args(["admin", "list-domain-workspaces", "--domain-id", &domain_id])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["count"], 0);

    // Assign by capacity
    let output = fabio()
        .args([
            "admin",
            "assign-domain-workspaces-by-capacities",
            "--domain-id",
            &domain_id,
            "--capacity-ids",
            &cfg.capacity_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify capacity-scoped workspaces assigned
    let output = fabio()
        .args(["admin", "list-domain-workspaces", "--domain-id", &domain_id])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["count"].as_u64().unwrap() >= 1);

    // Unassign all again
    let output = fabio()
        .args([
            "admin",
            "unassign-all-domain-workspaces",
            "--domain-id",
            &domain_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Cleanup
    let _ = fabio()
        .args(["admin", "delete-domain", "--domain-id", &domain_id])
        .output();
}

/// Test assign-domain-workspaces-by-principals and bulk role assign/unassign
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_domain_principal_and_roles() {
    let domain_name = unique_name("fabio-c1-principal");
    let caller_id = "0dde4270-4462-4e16-995c-a2fc674e04ef";

    // Create sandbox domain
    let output = fabio()
        .args(["admin", "create-domain", "--name", &domain_name])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let domain_id = json["data"]["id"].as_str().unwrap().to_string();

    // Assign workspaces by principal
    let output = fabio()
        .args([
            "admin",
            "assign-domain-workspaces-by-principals",
            "--domain-id",
            &domain_id,
            "--principal-ids",
            caller_id,
            "--principal-type",
            "User",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify list-domain-workspaces returns valid envelope
    // (count may be 0 if user's workspaces are already assigned to other domains)
    let output = fabio()
        .args(["admin", "list-domain-workspaces", "--domain-id", &domain_id])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_list_envelope(&json);

    // Unassign all workspaces before role testing
    let output = fabio()
        .args([
            "admin",
            "unassign-all-domain-workspaces",
            "--domain-id",
            &domain_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Bulk assign contributor role
    let assign_body =
        format!(r#"{{"type":"Contributors","principals":[{{"id":"{caller_id}","type":"User"}}]}}"#);
    let output = fabio()
        .args([
            "admin",
            "bulk-assign-domain-roles",
            "--domain-id",
            &domain_id,
            "--content",
            &assign_body,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify role present
    let output = fabio()
        .args([
            "admin",
            "list-domain-role-assignments",
            "--domain-id",
            &domain_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let roles = json["data"].as_array().unwrap();
    let has_user = roles
        .iter()
        .any(|r| r["principal"]["id"] == caller_id && r["role"] == "Contributor");
    assert!(has_user, "Expected user to have Contributor role");

    // Bulk unassign role
    let unassign_body =
        format!(r#"{{"type":"Contributors","principals":[{{"id":"{caller_id}","type":"User"}}]}}"#);
    let output = fabio()
        .args([
            "admin",
            "bulk-unassign-domain-roles",
            "--domain-id",
            &domain_id,
            "--content",
            &unassign_body,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify role removed
    let output = fabio()
        .args([
            "admin",
            "list-domain-role-assignments",
            "--domain-id",
            &domain_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let roles = json["data"].as_array().unwrap();
    let has_user = roles
        .iter()
        .any(|r| r["principal"]["id"] == caller_id && r["role"] == "Contributor");
    assert!(!has_user, "Expected user role to be removed");

    // Cleanup
    let _ = fabio()
        .args(["admin", "delete-domain", "--domain-id", &domain_id])
        .output();
}

// ─── Phase C2: Workspace Restore Roundtrip ───────────────────────────────────

/// Test workspace create → delete → restore → verify → cleanup
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_workspace_restore_roundtrip() {
    let cfg = TestConfig::from_env();
    let ws_name = unique_name("fabio-c2-restore");

    // Create workspace
    let output = fabio()
        .args(["workspace", "create", "--name", &ws_name])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let ws_id = json["data"]["id"].as_str().unwrap().to_string();

    // Delete workspace
    let output = fabio()
        .args(["workspace", "delete", "--id", &ws_id])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Restore workspace
    let restored_name = format!("{ws_name}-restored");
    let output = fabio()
        .args([
            "admin",
            "restore-workspace",
            "--workspace",
            &ws_id,
            "--name",
            &restored_name,
            "--capacity-id",
            &cfg.capacity_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify workspace is back (admin show)
    let output = fabio()
        .args(["admin", "show-workspace", "--workspace", &ws_id])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["state"], "Active");

    // Final cleanup: delete again
    let _ = fabio()
        .args(["workspace", "delete", "--id", &ws_id])
        .output();
}

// ─── Phase C3: Workload Assignment Roundtrip ─────────────────────────────────

/// Test workload assignment: create tenant assignment → verify → delete → verify
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_workload_assignment_roundtrip() {
    let workload_id = "Neo4j.GraphAnalytics";

    // Create tenant assignment
    let body = format!(r#"{{"type":"Tenant","workloadId":"{workload_id}"}}"#);
    let output = fabio()
        .args(["admin", "create-workload-assignment", "--content", &body])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let assignment_id = json["data"]["id"].as_str().unwrap().to_string();
    assert_eq!(json["data"]["type"], "Tenant");
    assert_eq!(json["data"]["workloadId"], workload_id);

    // Verify assignment appears in list
    let output = fabio()
        .args(["admin", "list-workload-assignments"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let assignments = json["data"].as_array().unwrap();
    let has_assignment = assignments
        .iter()
        .any(|a| a["id"] == assignment_id.as_str());
    assert!(has_assignment, "Expected new assignment in list");

    // Delete assignment
    let output = fabio()
        .args([
            "admin",
            "delete-workload-assignment",
            "--assignment-id",
            &assignment_id,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify removed
    let output = fabio()
        .args(["admin", "list-workload-assignments"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let assignments = json["data"].as_array().unwrap();
    let has_assignment = assignments
        .iter()
        .any(|a| a["id"] == assignment_id.as_str());
    assert!(!has_assignment, "Expected assignment to be removed");
}

// ─── Phase D: Labels, Sharing Links, External Data Shares ────────────────────

/// Test remove-all-sharing-links returns LRO Succeeded status
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_remove_all_sharing_links() {
    let output = fabio()
        .args([
            "admin",
            "remove-all-sharing-links",
            "--content",
            r#"{"sharingLinkType":"OrgLink"}"#,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    // LRO returns status=Succeeded with percentComplete=100
    assert_eq!(json["data"]["status"], "Succeeded");
    assert_eq!(json["data"]["percentComplete"], 100);
}

/// Test bulk-remove-sharing-links with Report type returns per-item status
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_bulk_remove_sharing_links_report() {
    let output = fabio()
        .args([
            "admin",
            "bulk-remove-sharing-links",
            "--content",
            r#"{"items":[{"id":"00000000-0000-0000-0000-000000000001","type":"Report"}],"sharingLinkType":"OrgLink"}"#,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    // Returns per-item status array
    let statuses = json["data"]["itemRemoveSharingLinksStatus"]
        .as_array()
        .unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0]["status"], "NotFound");
}

/// Test bulk-remove-sharing-links rejects non-Report types
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_bulk_remove_sharing_links_unsupported_type() {
    let cfg = TestConfig::from_env();
    let body = format!(
        r#"{{"items":[{{"id":"{}","type":"Lakehouse"}}],"sharingLinkType":"OrgLink"}}"#,
        cfg.source_lakehouse
    );
    let output = fabio()
        .args(["admin", "bulk-remove-sharing-links", "--content", &body])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(json["error"]["code"], "API_ERROR");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not supported")
    );
}

/// Test revoke-external-data-share returns `NOT_FOUND` for non-existent share
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_revoke_external_data_share_not_found() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "admin",
            "revoke-external-data-share",
            "--workspace",
            &cfg.source_workspace,
            "--item-id",
            &cfg.source_lakehouse,
            "--share-id",
            "00000000-0000-0000-0000-000000000001",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(json["error"]["code"], "NOT_FOUND");
}

/// Test bulk-remove-labels returns per-item status (`NotFound` when no label set)
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_bulk_remove_labels() {
    let cfg = TestConfig::from_env();
    let body = format!(
        r#"{{"items":[{{"id":"{}","type":"Lakehouse"}}]}}"#,
        cfg.source_lakehouse
    );
    let output = fabio()
        .args(["admin", "bulk-remove-labels", "--content", &body])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    // Returns per-item status — NotFound when item has no label
    let statuses = json["data"]["itemsChangeLabelStatus"].as_array().unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0]["status"], "NotFound");
}

/// Test bulk-set-labels returns error without Purview labels configured
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn admin_bulk_set_labels_no_purview() {
    let cfg = TestConfig::from_env();
    let body = format!(
        r#"{{"items":[{{"id":"{}","type":"Lakehouse"}}],"labelId":"00000000-0000-0000-0000-000000000001"}}"#,
        cfg.source_lakehouse
    );
    let output = fabio()
        .args(["admin", "bulk-set-labels", "--content", &body])
        .output()
        .unwrap();
    // May succeed (200) with error status per item, or fail at API level
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Either stdout has per-item error or stderr has API error about labels
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("Label") || combined.contains("label"),
        "Expected label-related error, got stdout={stdout} stderr={stderr}"
    );
}

// ─── Admin list-workspaces encryption params ─────────────────────────────────

#[test]
fn admin_list_workspaces_include_encryption_help() {
    fabio()
        .args(["admin", "list-workspaces", "--help"])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant with admin role"]
#[serial]
fn admin_list_workspaces_with_encryption_filter() {
    let _cfg = TestConfig::from_env();
    let assert = fabio()
        .args(["admin", "list-workspaces", "--include", "encryption"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["data"].is_array());
}
