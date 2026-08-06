//! End-to-end integration tests for `fabio kql-queryset` commands.
//!
//! Run tests require:
//! - `FABIO_TEST_SOURCE_WORKSPACE` (workspace with queryset)
//! - `FABIO_TEST_KQL_QUERYSET_ID` (ID of a queryset with saved tabs pointing to a KQL database)

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn kql_queryset_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["kql-queryset", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn kql_queryset_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("kqlqs_test");

    // Create
    let assert = fabio()
        .args([
            "kql-queryset",
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
    let id = data["id"].as_str().unwrap().to_string();

    // Delete
    let assert = fabio()
        .args([
            "kql-queryset",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
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
fn kql_queryset_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "kql-queryset",
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
fn kql_queryset_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "kql-queryset",
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
fn kql_queryset_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "kql-queryset",
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
    assert_eq!(json["data"]["would_execute"], "kql-queryset create");
}

// ─── Run Tests ───────────────────────────────────────────────────────────────

fn queryset_test_config() -> (TestConfig, String) {
    let cfg = TestConfig::from_env();
    let queryset_id = std::env::var("FABIO_TEST_KQL_QUERYSET_ID")
        .expect("FABIO_TEST_KQL_QUERYSET_ID required for run tests");
    (cfg, queryset_id)
}

#[test]
#[ignore = "requires live Fabric tenant with KQL queryset"]
#[serial]
fn kql_queryset_run_default_tab() {
    let (cfg, queryset_id) = queryset_test_config();

    let output = fabio()
        .args([
            "kql-queryset",
            "run",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &queryset_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&output);
    // Should return data (either rows or a no-results message)
    assert!(json.get("data").is_some() || json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant with KQL queryset"]
#[serial]
fn kql_queryset_run_by_tab_name() {
    let (cfg, queryset_id) = queryset_test_config();

    let output = fabio()
        .args([
            "kql-queryset",
            "run",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &queryset_id,
            "--tab",
            "EventCount",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    // EventCount should return a count result
    if let Some(rows) = data.as_array() {
        assert!(!rows.is_empty());
        assert!(rows[0].get("Count").is_some());
    }
}

#[test]
#[ignore = "requires live Fabric tenant with KQL queryset"]
#[serial]
fn kql_queryset_run_by_tab_index() {
    let (cfg, queryset_id) = queryset_test_config();

    let output = fabio()
        .args([
            "kql-queryset",
            "run",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &queryset_id,
            "--tab",
            "0",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&output);
    assert!(json.get("data").is_some() || json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant with KQL queryset"]
#[serial]
fn kql_queryset_run_tab_not_found() {
    let (cfg, queryset_id) = queryset_test_config();

    let assert = fabio()
        .args([
            "kql-queryset",
            "run",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &queryset_id,
            "--tab",
            "NonExistentTab",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "NOT_FOUND");
    assert!(
        err_json["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("Available tabs")
    );
}

#[test]
#[ignore = "requires live Fabric tenant with KQL queryset"]
#[serial]
fn kql_queryset_run_tab_index_out_of_range() {
    let (cfg, queryset_id) = queryset_test_config();

    let assert = fabio()
        .args([
            "kql-queryset",
            "run",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &queryset_id,
            "--tab",
            "99",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "NOT_FOUND");
    assert!(
        err_json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("out of range")
    );
}

#[test]
#[ignore = "requires live Fabric tenant with KQL queryset"]
#[serial]
fn kql_queryset_run_case_insensitive_tab() {
    let (cfg, queryset_id) = queryset_test_config();

    // Test case-insensitive tab lookup
    let output = fabio()
        .args([
            "kql-queryset",
            "run",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &queryset_id,
            "--tab",
            "eventcount", // lowercase version of "EventCount"
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&output);
    assert!(json.get("data").is_some() || json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant with KQL queryset"]
#[serial]
fn kql_queryset_run_not_found_queryset() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "kql-queryset",
            "run",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "NOT_FOUND");
}

// ─── Add-Tab Tests ────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn kql_queryset_add_tab_dry_run() {
    let cfg = TestConfig::from_env();
    // Dry-run fires before any network call, so it needs no real ids.
    let assert = fabio()
        .args([
            "kql-queryset",
            "add-tab",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--kql-database",
            "00000000-0000-0000-0000-000000000001",
            "--title",
            "DryRunTab",
            "--kql",
            "Sales | count",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "kql-queryset add-tab");
}

#[test]
#[ignore = "requires live Fabric tenant with a KQL queryset + KQL database"]
#[serial]
fn kql_queryset_add_tab_and_run() {
    let cfg = TestConfig::from_env();
    let queryset_id =
        std::env::var("FABIO_TEST_KQL_QUERYSET_ID").expect("FABIO_TEST_KQL_QUERYSET_ID required");
    let kql_db_id =
        std::env::var("FABIO_TEST_KQL_DATABASE_ID").expect("FABIO_TEST_KQL_DATABASE_ID required");
    let title = "e2e-add-tab";

    // Author a tab bound to the KQL database.
    let add = fabio()
        .args([
            "kql-queryset",
            "add-tab",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &queryset_id,
            "--kql-database",
            &kql_db_id,
            "--title",
            title,
            "--kql",
            "Sales | summarize Total=sum(Amount) by Region",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let add_json = parse_json(&add);
    assert_eq!(extract_data(&add_json)["status"], "tab_added");

    // Run the newly authored tab.
    let run = fabio()
        .args([
            "kql-queryset",
            "run",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &queryset_id,
            "--tab",
            title,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let run_json = parse_json(&run);
    assert!(run_json.get("data").is_some() || run_json.get("count").is_some());
}
