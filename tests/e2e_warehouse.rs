//! End-to-end integration tests for `fabio warehouse` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use predicates::prelude::*;
use serial_test::serial;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

/// Regression for 952ed8a: temporal TDS columns must render as readable ISO-8601
/// (DATE → `2026-01-15`, DATETIME2 → `2026-01-15T14:30:45.123…`), NOT the raw
/// internal representation (`"739630 days since 0001-01-01"`) or the old wrong
/// 1900-epoch approximation. Self-contained (CAST literals, no table needed).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_query_renders_datetime_as_iso8601() {
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
            "SELECT CAST('2026-01-15' AS DATE) AS d, \
             CAST('2026-01-15T14:30:45.123' AS DATETIME2) AS dt",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let rows = extract_data(&json);
    let row = &rows.as_array().expect("rows")[0];
    assert_eq!(row["d"], "2026-01-15", "DATE must render as ISO-8601");
    let dt = row["dt"].as_str().expect("dt is a string");
    assert!(
        dt.starts_with("2026-01-15T14:30:45.123"),
        "DATETIME2 must render as ISO-8601, got: {dt}"
    );
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

// ---------------------------------------------------------------------------
// warehouse mcp-url: emit the remote Fabric Data Warehouse MCP server URLs
// ---------------------------------------------------------------------------

/// Hermetic (mock-server) test: an existing warehouse yields the item-scoped +
/// global remote MCP server URLs with `exists=true` and a consumption note; a
/// missing warehouse still yields the deterministic URLs with `exists=false` +
/// hint. Uses a loopback mock endpoint so no live tenant is required.
#[test]
fn warehouse_mcp_url_mocked_exists_and_missing() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let workspace = "00000000-0000-0000-0000-000000000001";
    let present = "00000000-0000-0000-0000-0000000000aa";
    let missing = "00000000-0000-0000-0000-0000000000bb";

    let (server_uri, _server) = rt.block_on(async {
        let server = MockServer::start().await;
        // Only the "present" warehouse GET succeeds; the missing one 404s (default).
        Mock::given(method("GET"))
            .and(path(format!(
                "/workspaces/{workspace}/warehouses/{present}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": present,
                "displayName": "MockWH",
                "type": "Warehouse"
            })))
            .mount(&server)
            .await;
        (server.uri(), server)
    });

    // Existing warehouse: exists=true, note present, no hint.
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args([
            "warehouse",
            "mcp-url",
            "--workspace",
            workspace,
            "--id",
            present,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    assert_eq!(
        data["mcpUrl"].as_str().unwrap(),
        format!("{server_uri}/mcp/dataPlane/workspaces/{workspace}/items/{present}/sqlEndpoint")
    );
    assert_eq!(
        data["globalMcpUrl"].as_str().unwrap(),
        format!("{server_uri}/mcp/dataPlane/sqlEndpoint")
    );
    assert_eq!(data["transport"], "http");
    assert_eq!(data["exists"], true);
    assert!(data["note"].as_str().unwrap().contains("execute_query"));
    assert!(data["hint"].is_null());

    // Missing warehouse: deterministic URL still emitted, exists=false + hint.
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args([
            "warehouse",
            "mcp-url",
            "--workspace",
            workspace,
            "--id",
            missing,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    assert!(
        data["mcpUrl"]
            .as_str()
            .unwrap()
            .ends_with(&format!("/items/{missing}/sqlEndpoint"))
    );
    assert_eq!(data["exists"], false);
    assert!(data["note"].is_null());
    assert!(!data["hint"].as_str().unwrap().is_empty());
}

/// Live test: emit the remote MCP server URL for a real warehouse and assert the
/// canonical `api.fabric.microsoft.com` shape with `exists=true`.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_mcp_url_lifecycle() {
    let cfg = TestConfig::from_env();

    // Find (or skip if none) a warehouse in the source workspace.
    let assert = fabio()
        .args(["warehouse", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let Some(wh_id) = items.first().and_then(|w| w["id"].as_str()) else {
        eprintln!("No warehouse in source workspace; skipping mcp-url lifecycle test");
        return;
    };

    let expected = format!(
        "https://api.fabric.microsoft.com/v1/mcp/dataPlane/workspaces/{}/items/{}/sqlEndpoint",
        cfg.source_workspace, wh_id
    );
    let assert = fabio()
        .args([
            "warehouse",
            "mcp-url",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            wh_id,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    assert_eq!(data["mcpUrl"], expected);
    assert_eq!(
        data["globalMcpUrl"],
        "https://api.fabric.microsoft.com/v1/mcp/dataPlane/sqlEndpoint"
    );
    assert_eq!(data["exists"], true);
    assert!(data["note"].as_str().unwrap().contains("MCP server"));
}

// ---------------------------------------------------------------------------
// warehouse list-tables / describe-table: INFORMATION_SCHEMA schema discovery
// ---------------------------------------------------------------------------

/// Live test: discover tables and columns of a warehouse over `INFORMATION_SCHEMA`.
/// Creates a warehouse, creates a table via `query`, then asserts `list-tables`
/// surfaces it and `describe-table` returns its columns.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_schema_discovery_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("wh_schema");

    // Create a warehouse.
    let assert = fabio()
        .args([
            "warehouse",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let wh_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create a table via a DDL query.
    fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--sql",
            "CREATE TABLE dbo.fabio_schema_probe (id INT NOT NULL, label VARCHAR(50) NULL)",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // list-tables (scoped to dbo) must surface the new table.
    let assert = fabio()
        .args([
            "warehouse",
            "list-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--schema",
            "dbo",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let rows = extract_data(&parse_json(&assert))
        .as_array()
        .unwrap()
        .clone();
    assert!(
        rows.iter()
            .any(|r| r["TABLE_NAME"].as_str() == Some("fabio_schema_probe")),
        "list-tables should include the created table: {rows:?}"
    );

    // describe-table returns the two columns in order.
    let assert = fabio()
        .args([
            "warehouse",
            "describe-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--table",
            "dbo.fabio_schema_probe",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let cols = extract_data(&parse_json(&assert))
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(cols.len(), 2, "expected 2 columns, got {cols:?}");
    assert_eq!(cols[0]["COLUMN_NAME"], "id");
    assert_eq!(cols[0]["DATA_TYPE"], "int");
    assert_eq!(cols[0]["IS_NULLABLE"], "NO");
    assert_eq!(cols[1]["COLUMN_NAME"], "label");
    assert_eq!(cols[1]["DATA_TYPE"], "varchar");
    assert_eq!(cols[1]["IS_NULLABLE"], "YES");

    // A non-existent table yields an empty list (count 0), not an error.
    let assert = fabio()
        .args([
            "warehouse",
            "describe-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--table",
            "dbo.this_table_does_not_exist",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(json["count"], 0);

    // Clean up.
    fabio()
        .args([
            "warehouse",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--hard-delete",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// warehouse copy-into: COPY INTO bulk ingestion (authoring)
// ---------------------------------------------------------------------------

/// Hermetic test: `--dry-run` previews the generated COPY INTO SQL (with the SAS
/// secret redacted) and never touches the network; input validation rejects a
/// non-HTTPS/non-storage source and an unknown file type. No live tenant needed.
#[test]
fn warehouse_copy_into_dry_run_and_validation() {
    let ws = "00000000-0000-0000-0000-000000000001";
    let wh = "00000000-0000-0000-0000-000000000002";

    // Dry-run preview with a SAS token: SQL is shown, secret is redacted.
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_SQL_ACCESS_TOKEN", "fake-test-token")
        .args([
            "warehouse",
            "copy-into",
            "--workspace",
            ws,
            "--id",
            wh,
            "--table",
            "dbo.Orders",
            "--source",
            "https://acct.blob.core.windows.net/c/data.csv",
            "--file-type",
            "csv",
            "--first-row",
            "2",
            "--sas-token",
            "sv=2022&sig=TOPSECRETSIG",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "warehouse copy-into");
    let sql = data["details"]["sql"].as_str().unwrap();
    assert!(sql.contains("COPY INTO [dbo].[Orders]"));
    assert!(sql.contains("FILE_TYPE = 'CSV'"));
    assert!(sql.contains("FIRSTROW = 2"));
    assert!(sql.contains("SECRET = '***REDACTED***'"));
    // The secret must not appear anywhere in the output.
    let raw = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        !raw.contains("TOPSECRETSIG"),
        "SAS secret leaked into output"
    );

    // Unknown file type is rejected (enumerating error), before any network call.
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_SQL_ACCESS_TOKEN", "fake-test-token")
        .args([
            "warehouse",
            "copy-into",
            "--workspace",
            ws,
            "--id",
            wh,
            "--table",
            "dbo.Orders",
            "--source",
            "https://acct.dfs.core.windows.net/c/data.parquet",
            "--file-type",
            "json",
            "--dry-run",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(
        err["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("CSV, PARQUET")
    );

    // A non-storage / non-HTTPS source is rejected.
    for bad in [
        "http://acct.dfs.core.windows.net/c/data.csv",
        "https://evil.example.com/data.csv",
    ] {
        fabio()
            .env("FABIO_ACCESS_TOKEN", "fake-test-token")
            .env("FABIO_SQL_ACCESS_TOKEN", "fake-test-token")
            .args([
                "warehouse",
                "copy-into",
                "--workspace",
                ws,
                "--id",
                wh,
                "--table",
                "dbo.Orders",
                "--source",
                bad,
                "--file-type",
                "csv",
                "--dry-run",
            ])
            .assert()
            .failure();
    }
}

/// Hermetic test: `--auth-mode workspace-identity` emits a managed-identity
/// CREDENTIAL (no secret) in the dry-run SQL, and the mode's flag validation
/// rejects a conflicting `--sas-token` and an inconsistent `--auth-mode sas`
/// without a token. No live tenant needed.
#[test]
fn warehouse_copy_into_workspace_identity_auth() {
    let ws = "00000000-0000-0000-0000-000000000001";
    let wh = "00000000-0000-0000-0000-000000000002";

    // Workspace-identity auth: managed-identity credential, no secret.
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_SQL_ACCESS_TOKEN", "fake-test-token")
        .args([
            "warehouse",
            "copy-into",
            "--workspace",
            ws,
            "--id",
            wh,
            "--table",
            "dbo.Orders",
            "--source",
            "https://acct.dfs.core.windows.net/c/data.parquet",
            "--file-type",
            "parquet",
            "--auth-mode",
            "workspace-identity",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    let sql = data["details"]["sql"].as_str().unwrap();
    assert!(sql.contains("CREDENTIAL = (IDENTITY = 'Managed Identity')"));
    assert!(!sql.contains("SECRET ="));

    // workspace-identity + --sas-token is a conflict (fails before any network call).
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_SQL_ACCESS_TOKEN", "fake-test-token")
        .args([
            "warehouse",
            "copy-into",
            "--workspace",
            ws,
            "--id",
            wh,
            "--table",
            "dbo.Orders",
            "--source",
            "https://acct.dfs.core.windows.net/c/data.parquet",
            "--file-type",
            "parquet",
            "--auth-mode",
            "workspace-identity",
            "--sas-token",
            "sv=2022&sig=SIG",
            "--dry-run",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cannot be combined with --sas-token")
    );

    // --auth-mode sas without a token is rejected.
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_SQL_ACCESS_TOKEN", "fake-test-token")
        .args([
            "warehouse",
            "copy-into",
            "--workspace",
            ws,
            "--id",
            wh,
            "--table",
            "dbo.Orders",
            "--source",
            "https://acct.dfs.core.windows.net/c/data.parquet",
            "--file-type",
            "parquet",
            "--auth-mode",
            "sas",
            "--dry-run",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("requires --sas-token")
    );
}
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_copy_into_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("wh_copyinto");

    // Upload a small CSV into the source lakehouse's Files area.
    let csv = std::env::temp_dir().join(format!("{name}.csv"));
    std::fs::write(&csv, "Region,Amount\nWest,100\nEast,250\nWest,75\n").unwrap();
    let dest = format!("Files/{name}/data.csv");
    fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            csv.to_str().unwrap(),
            "--dest-path",
            &dest,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Create a warehouse + target table.
    let assert = fabio()
        .args([
            "warehouse",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let wh_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();
    fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--sql",
            "CREATE TABLE dbo.copy_probe (Region VARCHAR(50), Amount INT)",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // COPY INTO from OneLake via Entra passthrough (no SAS), skipping the header.
    let source = format!(
        "https://onelake.dfs.fabric.microsoft.com/{}/{}/{}",
        cfg.source_workspace, cfg.source_lakehouse, dest
    );
    let assert = fabio()
        .args([
            "warehouse",
            "copy-into",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--table",
            "dbo.copy_probe",
            "--source",
            &source,
            "--file-type",
            "CSV",
            "--first-row",
            "2",
            "--field-terminator",
            ",",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&assert))["status"], "loaded");

    // Verify the rows landed (3 rows: West 100+75, East 250).
    let assert = fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--sql",
            "SELECT COUNT(*) AS n, SUM(Amount) AS total FROM dbo.copy_probe",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let rows = extract_data(&parse_json(&assert))
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        rows[0]["n"]
            .as_i64()
            .or_else(|| rows[0]["n"].as_str().and_then(|s| s.parse().ok())),
        Some(3)
    );

    // Clean up: warehouse + uploaded file.
    fabio()
        .args([
            "warehouse",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--hard-delete",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            &dest,
        ])
        .assert()
        .success();
    let _ = std::fs::remove_file(&csv);
}

// ---------------------------------------------------------------------------
// warehouse query --via-mcp: execute over the remote Fabric DW MCP server
// ---------------------------------------------------------------------------

/// Live test: run a SELECT via the remote MCP server (`--via-mcp`) and assert it
/// returns the same list envelope as the native-TDS path. Values come back as
/// strings (CSV is untyped) — the one documented difference — so compare as text.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn warehouse_query_via_mcp_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("wh_viamcp");

    // Create a warehouse + a table with one known row.
    let assert = fabio()
        .args([
            "warehouse",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let wh_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();
    fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--sql",
            "CREATE TABLE dbo.viamcp (Region VARCHAR(50), Amount INT); \
             INSERT INTO dbo.viamcp VALUES ('West', 100)",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Query via the remote MCP server — no FABIO_SQL_ACCESS_TOKEN needed.
    let assert = fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--via-mcp",
            "--sql",
            "SELECT Region, Amount FROM dbo.viamcp",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let rows = extract_data(&json).as_array().unwrap().clone();
    assert_eq!(json["count"], 1);
    assert_eq!(rows[0]["Region"], "West");
    // CSV is untyped, so Amount comes back as the string "100".
    assert_eq!(rows[0]["Amount"], "100");

    // A bad query surfaces the server error (not a panic).
    let assert = fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--via-mcp",
            "--sql",
            "SELECT * FROM dbo.no_such_table_here",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "API_ERROR");

    // Clean up.
    fabio()
        .args([
            "warehouse",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &wh_id,
            "--hard-delete",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// warehouse update-audit-settings --predicate-expression (SQL Audit predicate)
// ---------------------------------------------------------------------------

/// Hermetic test: `--predicate-expression` is placed as top-level
/// `predicateExpression` in the audit-settings body (matching sql-endpoint /
/// sql-database). Verified via `--dry-run` — no live tenant required.
#[test]
fn warehouse_update_audit_settings_predicate_dry_run() {
    let ws = "00000000-0000-0000-0000-000000000001";
    let wh = "00000000-0000-0000-0000-000000000002";
    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .args([
            "warehouse",
            "update-audit-settings",
            "--workspace",
            ws,
            "--id",
            wh,
            "--state",
            "Enabled",
            "--predicate-expression",
            "database_principal_name <> 'dbo'",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let details = &extract_data(&json)["details"];
    assert_eq!(details["state"], "Enabled");
    assert_eq!(
        details["predicateExpression"],
        "database_principal_name <> 'dbo'"
    );
}

#[test]
fn warehouse_queries_running_follow_flags_require_follow() {
    // Offline: the --follow-only flags are rejected before any network call.
    let assert = fabio()
        .args([
            "warehouse",
            "queries-running",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--interval",
            "3",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let json: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(json["error"]["code"], "INVALID_INPUT");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--follow")
    );
}
