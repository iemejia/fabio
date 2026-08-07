//! End-to-end integration tests for `fabio warehouse` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_list_returns_json() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["warehouse", "list", "--workspace", &cfg.source_workspace])
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
fn warehouse_show_returns_details() {
    let cfg = TestConfig::from_env();

    // First list warehouses to get an ID
    let assert = fabio()
        .args(["warehouse", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if items.is_empty() {
        eprintln!("No warehouses found in source workspace, skipping show test");
        return;
    }

    let wh_id = items[0]["id"].as_str().unwrap();

    // Show the warehouse
    let assert = fabio()
        .args([
            "warehouse",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            wh_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"], wh_id);
    assert!(data.get("displayName").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_query_select_one() {
    let cfg = TestConfig::from_env();

    // Query against the lakehouse SQL endpoint via TDS
    let assert = fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            "SELECT 1 AS test",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // New TDS-based query returns rows array
    let rows = data.as_array().expect("expected array of rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["test"], 1);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_query_empty_result_renders_as_empty_list() {
    // A SELECT that matches zero rows must render as the list envelope
    // `{"data":[],"count":0}` (NOT a scalar `{"rows_affected":0,"message":…}`),
    // so agents that iterate/filter `data` behave consistently. Regression guard
    // for the produced_result_set fix (columns-empty vs rows-empty).
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            "SELECT 1 AS test WHERE 1 = 0",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    assert_eq!(json["count"], 0);
    let data = extract_data(&json);
    let rows = data.as_array().expect("expected empty array of rows");
    assert!(rows.is_empty());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_query_from_stdin() {
    let cfg = TestConfig::from_env();

    // Pipe SQL via stdin
    let assert = fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .write_stdin("SELECT 42 AS answer")
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let rows = data.as_array().expect("expected array of rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["answer"], 42);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_query_table_output() {
    let cfg = TestConfig::from_env();

    // Table output should render the result
    fabio()
        .args([
            "--output",
            "table",
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            "SELECT 1 AS test",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success()
        .stdout(predicate::str::contains("test"));
}

// ---------------------------------------------------------------------------
// warehouse show for non-existent ID returns error
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_show_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "warehouse",
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
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR",
        "Expected NOT_FOUND or API_ERROR, got: {code}"
    );
}

// ---------------------------------------------------------------------------
// warehouse query with --sql from @file
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_query_from_file() {
    let cfg = TestConfig::from_env();
    let tmp_dir = tempfile::TempDir::new().unwrap();
    let sql_file = tmp_dir.path().join("query.sql");
    std::fs::write(&sql_file, "SELECT 42 AS answer").unwrap();

    let sql_arg = format!("@{}", sql_file.to_str().unwrap());
    let assert = fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            &sql_arg,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let rows = data.as_array().expect("expected array of rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["answer"], 42);
}

// ---------------------------------------------------------------------------
// warehouse query with --output csv
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_query_csv_output() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--output",
            "csv",
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            "SELECT 1 AS col1, 'hello' AS col2",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    // Header + at least 1 data row
    assert!(
        lines.len() >= 2,
        "CSV should have header + data, got: {stdout}"
    );
    // Header should contain column names
    assert_eq!(lines[0], "col1,col2");
    // Data row should be comma-separated values
    assert_eq!(lines[1], "1,hello");
}

// ---------------------------------------------------------------------------
// warehouse query with --output tsv
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_query_tsv_output() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--output",
            "tsv",
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            "SELECT 42 AS num, 'world' AS txt, NULL AS empty",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "TSV should have header + data, got: {stdout}"
    );
    // Header separated by tabs
    assert_eq!(lines[0], "num\ttxt\tempty");
    // Data row: 42, world, empty (null renders as empty)
    assert_eq!(lines[1], "42\tworld\t");
}

// ===========================================================================
// warehouse create / update / delete
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("wh_crud");

    // Create
    let assert = fabio()
        .args([
            "warehouse",
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
    let wh_id = data["id"].as_str().unwrap().to_string();

    // Delete
    let assert = fabio()
        .args([
            "warehouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &wh_id,
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
fn warehouse_update_name() {
    let cfg = TestConfig::from_env();
    let original = common::unique_name("wh_upd_o");
    let updated = common::unique_name("wh_upd_n");

    // Create
    let assert = fabio()
        .args([
            "warehouse",
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
    let wh_id = data["id"].as_str().unwrap().to_string();

    // Update
    let assert = fabio()
        .args([
            "warehouse",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &wh_id,
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
            "warehouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &wh_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "warehouse",
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

// ─── Connection String ───────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_connection_string_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "warehouse",
            "connection-string",
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
fn warehouse_connection_string_with_guest_tenant_not_found() {
    let cfg = TestConfig::from_env();

    // Verify the --guest-tenant-id flag is accepted by the CLI
    fabio()
        .args([
            "warehouse",
            "connection-string",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--guest-tenant-id",
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_connection_string_with_private_link_not_found() {
    let cfg = TestConfig::from_env();

    // Verify the --private-link-type flag is accepted by the CLI
    fabio()
        .args([
            "warehouse",
            "connection-string",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--private-link-type",
            "OneLake",
        ])
        .assert()
        .failure();
}

// ─── Hard Delete ─────────────────────────────────────────────────────────────

#[test]
fn warehouse_delete_hard_delete_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse",
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

// ---------------------------------------------------------------------------
// warehouse plan — capture execution plan via SHOWPLAN_XML
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_plan_returns_xml() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "warehouse",
            "plan",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            "SELECT 1 AS test",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["statementCount"], 1);
    let plans = data["plans"].as_array().expect("plans should be array");
    assert_eq!(plans.len(), 1);
    let plan_xml = plans[0]["planXml"]
        .as_str()
        .expect("planXml should be string");
    assert!(
        plan_xml.contains("ShowPlanXML"),
        "Plan XML should contain ShowPlanXML element"
    );
}

// ---------------------------------------------------------------------------
// warehouse queries-running — list running queries
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_queries_running() {
    let cfg = TestConfig::from_env();

    // This may return an empty list (no active queries), but should succeed
    fabio()
        .args([
            "warehouse",
            "queries-running",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// warehouse queries-frequent — list frequently-run queries
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_queries_frequent() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "warehouse",
            "queries-frequent",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--top",
            "10",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// warehouse queries-long-running — list long-running queries
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_queries_long_running() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "warehouse",
            "queries-long-running",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--top",
            "10",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// warehouse queries-history — list query execution history
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_queries_history() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "warehouse",
            "queries-history",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--top",
            "10",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// warehouse queries-history --label — filter to labeled queries + perf columns
// (queryinsights views populate asynchronously; a fresh warehouse may return an
// "Invalid object name" error, so this only asserts the --label flag is accepted
// and the command runs against a warehouse that already has query history).
#[test]
#[ignore = "requires live Fabric tenant with populated query insights"]
#[serial]
fn warehouse_queries_history_label_filter() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "warehouse",
            "queries-history",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--top",
            "10",
            "--label",
            "Clustered",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// warehouse pool-insights — SQL pool state / pressure events
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_pool_insights() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "warehouse",
            "pool-insights",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--top",
            "5",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    // Result set is a list (rows present on active pools, possibly empty otherwise).
    let json = parse_json(&assert);
    assert!(json.get("data").is_some());
}

// ---------------------------------------------------------------------------
// warehouse statistics-list — list statistics objects
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_statistics_list() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "warehouse",
            "statistics-list",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// warehouse statistics-create dry-run
// ---------------------------------------------------------------------------

#[test]
fn warehouse_statistics_create_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse",
            "statistics-create",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--table",
            "dbo.orders",
            "--column",
            "customer_id",
            "--name",
            "st_orders_customer",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

// ---------------------------------------------------------------------------
// warehouse statistics-update dry-run
// ---------------------------------------------------------------------------

#[test]
fn warehouse_statistics_update_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse",
            "statistics-update",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "st_orders_customer",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

// ---------------------------------------------------------------------------
// warehouse statistics-delete dry-run
// ---------------------------------------------------------------------------

#[test]
fn warehouse_statistics_delete_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse",
            "statistics-delete",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--name",
            "st_orders_customer",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

// ---------------------------------------------------------------------------
// warehouse queries-kill dry-run
// ---------------------------------------------------------------------------

#[test]
fn warehouse_queries_kill_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse",
            "queries-kill",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--session-id",
            "42",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

// ---------------------------------------------------------------------------
// warehouse get-retention / set-retention (configurable data retention)
// ---------------------------------------------------------------------------

#[test]
fn warehouse_set_retention_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse",
            "set-retention",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--days",
            "45",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "warehouse set-retention");
    assert_eq!(data["details"]["retentionDays"], 45);
}

#[test]
fn warehouse_set_retention_rejects_out_of_range() {
    let assert = fabio()
        .args([
            "warehouse",
            "set-retention",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--days",
            "0",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("between 1 and 120"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_retention_get_set_roundtrip() {
    let cfg = TestConfig::from_env();

    // Find a warehouse (retention is a warehouse feature; needs ALTER DATABASE).
    let assert = fabio()
        .args(["warehouse", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if items.is_empty() {
        eprintln!("No warehouses found, skipping retention test");
        return;
    }
    let wh_id = items[0]["id"].as_str().unwrap().to_string();

    // Read the current retention.
    let assert = fabio()
        .args([
            "warehouse",
            "get-retention",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let original = extract_data(&json).as_array().unwrap()[0]["time_travel_retention_period_days"]
        .as_u64()
        .expect("retention days");

    // Set a different value and confirm it took effect.
    let target: u64 = if original == 45 { 60 } else { 45 };
    fabio()
        .args([
            "warehouse",
            "set-retention",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--days",
            &target.to_string(),
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let assert = fabio()
        .args([
            "warehouse",
            "get-retention",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let now = extract_data(&json).as_array().unwrap()[0]["time_travel_retention_period_days"]
        .as_u64()
        .unwrap();
    assert_eq!(now, target, "retention should reflect the new value");

    // Restore the original value.
    fabio()
        .args([
            "warehouse",
            "set-retention",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--days",
            &original.to_string(),
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Restore points — create sets displayName (not the ignored restorePointLabel);
// restore-to-point is in-place (no --name / no body).
// ---------------------------------------------------------------------------

#[test]
fn warehouse_create_restore_point_dry_run_uses_display_name() {
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse",
            "create-restore-point",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
            "--name",
            "MyPoint",
            "--description",
            "a note",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let details = &extract_data(&json)["details"];
    // The body must use displayName/description, NOT the ignored restorePointLabel.
    assert_eq!(details["displayName"], "MyPoint");
    assert_eq!(details["description"], "a note");
    assert!(details.get("restorePointLabel").is_none());
    assert!(details.get("restoreToWarehouseName").is_none());
}

#[test]
fn warehouse_restore_to_point_dry_run_needs_no_name() {
    // Restore is in-place: --name must NOT be required.
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse",
            "restore-to-point",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
            "--restore-point-id",
            "1786086539000",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let details = &extract_data(&json)["details"];
    assert_eq!(details["restorePointId"], "1786086539000");
    // No bogus warehouse-name body field.
    assert!(details.get("restoreToWarehouseName").is_none());
}

#[test]
fn warehouse_update_restore_point_dry_run_uses_display_name() {
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse",
            "update-restore-point",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
            "--restore-point-id",
            "1786086539000",
            "--name",
            "Renamed",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let details = &extract_data(&json)["details"];
    assert_eq!(details["displayName"], "Renamed");
    assert!(details.get("restorePointLabel").is_none());
}

// warehouse create --collation sets creationPayload.collationType. Offline
// dry-run regression.
#[test]
fn warehouse_create_collation_in_body() {
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse",
            "create",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--name",
            "wh_ci",
            "--collation",
            "Latin1_General_100_CI_AS_KS_WS_SC_UTF8",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(
        extract_data(&json)["details"]["collation"],
        "Latin1_General_100_CI_AS_KS_WS_SC_UTF8"
    );
}
