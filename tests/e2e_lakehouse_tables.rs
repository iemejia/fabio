//! End-to-end integration tests for `fabio lakehouse` table operations
//! (load-table, copy-table, delete-table).

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;
use std::fs;
use tempfile::TempDir;

/// Helper: upload a CSV and load it as a Delta table.
/// Returns the table name.
fn create_test_table(cfg: &TestConfig, table_name: &str) -> String {
    let tmp_dir = TempDir::new().unwrap();
    let csv_content = "id,name,value\n1,alpha,100\n2,beta,200\n3,gamma,300\n";
    let local_path = tmp_dir.path().join("data.csv");
    fs::write(&local_path, csv_content).unwrap();

    let remote_csv = format!("Files/{table_name}.csv");

    // Upload CSV
    fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            local_path.to_str().unwrap(),
            "--dest-path",
            &remote_csv,
        ])
        .assert()
        .success();

    // Load into table
    fabio()
        .args([
            "lakehouse",
            "load-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            &remote_csv,
            "--table",
            table_name,
            "--mode",
            "Overwrite",
            "--format",
            "Csv",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // Cleanup the CSV
    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            &remote_csv,
        ])
        .assert()
        .success();

    table_name.to_string()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_load_table_succeeds() {
    let cfg = TestConfig::from_env();
    let table_name = common::unique_name("load_test");

    let tmp_dir = TempDir::new().unwrap();
    let csv_content = "x,y\n1,hello\n2,world\n";
    let local_path = tmp_dir.path().join("data.csv");
    fs::write(&local_path, csv_content).unwrap();

    let remote_csv = format!("Files/{table_name}.csv");

    // Upload
    fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            local_path.to_str().unwrap(),
            "--dest-path",
            &remote_csv,
        ])
        .assert()
        .success();

    // Load table
    let assert = fabio()
        .args([
            "lakehouse",
            "load-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            &remote_csv,
            "--table",
            &table_name,
            "--mode",
            "Overwrite",
            "--format",
            "Csv",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "Succeeded");

    // Verify table appears in listing
    let assert = fabio()
        .args([
            "lakehouse",
            "list-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let found = data
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"].as_str() == Some(table_name.as_str()));
    assert!(found, "table not found in listing");

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            &remote_csv,
        ])
        .assert()
        .success();

    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_copy_table_and_delete() {
    let cfg = TestConfig::from_env();
    let src_table = common::unique_name("copy_src");
    let dst_table = common::unique_name("copy_dst");

    // Create source table
    create_test_table(&cfg, &src_table);

    // Copy table to dest lakehouse
    let assert = fabio()
        .args([
            "lakehouse",
            "copy-table",
            "--source-workspace",
            &cfg.source_workspace,
            "--source-id",
            &cfg.source_lakehouse,
            "--source-table",
            &src_table,
            "--dest-workspace",
            &cfg.dest_workspace,
            "--dest-id",
            &cfg.dest_lakehouse,
            "--dest-table",
            &dst_table,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "copied");
    assert!(data["filesCopied"].as_u64().unwrap() > 0);

    // Delete destination table
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--table",
            &dst_table,
        ])
        .assert()
        .success();

    // Delete source table
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &src_table,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_delete_table_succeeds() {
    let cfg = TestConfig::from_env();
    let table_name = common::unique_name("del_test");

    // Create a table
    create_test_table(&cfg, &table_name);

    // Delete it
    let assert = fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
    assert_eq!(data["table"], table_name);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_move_table_across_workspaces() {
    let cfg = TestConfig::from_env();
    let src_table = common::unique_name("mv_src");
    let dst_table = common::unique_name("mv_dst");

    // Create source table
    create_test_table(&cfg, &src_table);

    // Move table from source to dest lakehouse with a new name
    let assert = fabio()
        .args([
            "lakehouse",
            "move-table",
            "--source-workspace",
            &cfg.source_workspace,
            "--source-id",
            &cfg.source_lakehouse,
            "--source-table",
            &src_table,
            "--dest-workspace",
            &cfg.dest_workspace,
            "--dest-id",
            &cfg.dest_lakehouse,
            "--dest-table",
            &dst_table,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "moved");
    assert_eq!(data["sourceTable"], src_table);
    assert_eq!(data["destTable"], dst_table);

    // Verify source table is gone (delete should fail)
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &src_table,
        ])
        .assert()
        .failure();

    // Cleanup: delete destination table
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--table",
            &dst_table,
        ])
        .assert()
        .success();
}

// ─── Load Table with --schema flag ──────────────────────────────────────────

#[test]
fn load_table_schema_flag_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "lakehouse",
            "load-table",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--source-path",
            "Files/data.csv",
            "--table",
            "my_table",
            "--schema",
            "dbo",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "lakehouse load-table");
}

#[test]
fn load_table_without_schema_flag_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "lakehouse",
            "load-table",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--source-path",
            "Files/data.parquet",
            "--table",
            "my_table",
            "--format",
            "Parquet",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

// ─── table-schema tests ──────────────────────────────────────────────────────

#[test]
fn lakehouse_table_schema_missing_table_flag() {
    // Missing --table should fail with clap argument error
    fabio()
        .args([
            "lakehouse",
            "table-schema",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000002",
        ])
        .assert()
        .failure();
}

/// Live E2E: read schema of a table that exists (requires tenant)
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_table_schema_succeeds() {
    let cfg = TestConfig::from_env();
    let table_name = create_test_table(&cfg, "e2e_schema_test");

    let assert = fabio()
        .args([
            "lakehouse",
            "table-schema",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["table"], table_name);
    assert_eq!(data["schema_type"], "struct");
    let fields = data["fields"]
        .as_array()
        .expect("fields should be an array");
    assert!(
        !fields.is_empty(),
        "should have at least one field in schema"
    );
    // Verify field structure
    let first = &fields[0];
    assert!(first.get("name").is_some(), "field should have name");
    assert!(first.get("type").is_some(), "field should have type");
    assert!(
        first.get("nullable").is_some(),
        "field should have nullable"
    );

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();
}

/// Live E2E: table-schema on non-existent table returns error
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_table_schema_nonexistent_table() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
            "table-schema",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            "nonexistent_table_xyz_999",
        ])
        .assert()
        .failure();

    let output = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        output.contains("NOT_FOUND") || output.contains("Delta log"),
        "stderr: {output}"
    );
}

// ─── optimize-table tests ────────────────────────────────────────────────────

#[test]
fn lakehouse_optimize_table_dry_run() {
    // Dry-run does NOT require a live tenant — validates JSON payload construction
    let assert = fabio()
        .args([
            "lakehouse",
            "optimize-table",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000002",
            "--table",
            "sales_2024",
            "--vorder",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "lakehouse optimize-table");
    // Verify the details payload structure
    let details = &data["details"];
    let exec_data = &details["executionData"];
    assert_eq!(exec_data["tableName"], "sales_2024");
    assert_eq!(exec_data["optimizeSettings"]["vOrder"], true);
}

#[test]
fn lakehouse_optimize_table_dry_run_with_zorder() {
    let assert = fabio()
        .args([
            "lakehouse",
            "optimize-table",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000002",
            "--table",
            "events",
            "--zorder",
            "region,timestamp",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    // Verify zOrderBy appears in the request body
    let exec_data = &data["details"]["executionData"];
    let z_order = exec_data["optimizeSettings"]["zOrderBy"]
        .as_array()
        .expect("zOrderBy should be an array");
    assert_eq!(z_order.len(), 2);
    assert_eq!(z_order[0], "region");
    assert_eq!(z_order[1], "timestamp");
}

// ─── vacuum-table tests ──────────────────────────────────────────────────────

#[test]
fn lakehouse_vacuum_table_dry_run() {
    let assert = fabio()
        .args([
            "lakehouse",
            "vacuum-table",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000002",
            "--table",
            "logs_archive",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "lakehouse vacuum-table");
    // Default retention: 168 hours = 7 days → "7:00:00:00"
    let exec_data = &data["details"]["executionData"];
    let retention = exec_data["vacuumSettings"]["retentionPeriod"]
        .as_str()
        .unwrap();
    assert_eq!(retention, "7:00:00:00");
}

#[test]
fn lakehouse_vacuum_table_dry_run_custom_retention() {
    let assert = fabio()
        .args([
            "lakehouse",
            "vacuum-table",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000002",
            "--table",
            "events",
            "--retain-hours",
            "48",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // 48 hours = 2 days 0 hours → "2:00:00:00"
    let exec_data = &data["details"]["executionData"];
    let retention = exec_data["vacuumSettings"]["retentionPeriod"]
        .as_str()
        .unwrap();
    assert_eq!(retention, "2:00:00:00");
}

#[test]
fn lakehouse_vacuum_table_dry_run_partial_day_retention() {
    let assert = fabio()
        .args([
            "lakehouse",
            "vacuum-table",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000002",
            "--table",
            "metrics",
            "--retain-hours",
            "30",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // 30 hours = 1 day 6 hours → "1:06:00:00"
    let exec_data = &data["details"]["executionData"];
    let retention = exec_data["vacuumSettings"]["retentionPeriod"]
        .as_str()
        .unwrap();
    assert_eq!(retention, "1:06:00:00");
}

// ─── Live optimize/vacuum tests ──────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_optimize_table_succeeds() {
    let cfg = TestConfig::from_env();
    let table_name = common::unique_name("opt_test");

    // Create a table to optimize
    create_test_table(&cfg, &table_name);

    // Run optimize
    let assert = fabio()
        .args([
            "lakehouse",
            "optimize-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
            "--vorder",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let status = data.get("status").and_then(|s| s.as_str()).unwrap_or("");
    assert!(
        status == "optimize_triggered" || data.get("id").is_some(),
        "unexpected response: {data}"
    );

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_optimize_table_with_zorder_succeeds() {
    let cfg = TestConfig::from_env();
    let table_name = common::unique_name("optz_test");

    // Create a table with columns suitable for z-ordering
    create_test_table(&cfg, &table_name);

    // Run optimize with z-order on the "name" column
    let assert = fabio()
        .args([
            "lakehouse",
            "optimize-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
            "--vorder",
            "--zorder",
            "name",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let status = data.get("status").and_then(|s| s.as_str()).unwrap_or("");
    assert!(
        status == "optimize_triggered" || data.get("id").is_some(),
        "unexpected response: {data}"
    );

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_vacuum_table_succeeds() {
    let cfg = TestConfig::from_env();
    let table_name = common::unique_name("vac_test");

    // Create a table to vacuum
    create_test_table(&cfg, &table_name);

    // Run vacuum with default retention (7 days)
    let assert = fabio()
        .args([
            "lakehouse",
            "vacuum-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let status = data.get("status").and_then(|s| s.as_str()).unwrap_or("");
    assert!(
        status == "vacuum_triggered" || data.get("id").is_some(),
        "unexpected response: {data}"
    );

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_vacuum_table_custom_retention_succeeds() {
    let cfg = TestConfig::from_env();
    let table_name = common::unique_name("vacr_test");

    // Create a table to vacuum
    create_test_table(&cfg, &table_name);

    // Run vacuum with 2-day retention
    let assert = fabio()
        .args([
            "lakehouse",
            "vacuum-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
            "--retain-hours",
            "48",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let status = data.get("status").and_then(|s| s.as_str()).unwrap_or("");
    assert!(
        status == "vacuum_triggered" || data.get("id").is_some(),
        "unexpected response: {data}"
    );

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();
}

// ─── upload-table (single-step upload + load) ────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_upload_table_csv_auto_format() {
    let cfg = TestConfig::from_env();
    let table_name = common::unique_name("upload_tbl");

    let tmp_dir = TempDir::new().unwrap();
    let csv_content = "id,name,score\n1,alice,95\n2,bob,87\n3,carol,91\n";
    let local_path = tmp_dir.path().join("scores.csv");
    fs::write(&local_path, csv_content).unwrap();

    // upload-table: single-step (auto-detects Csv from extension)
    let assert = fabio()
        .args([
            "lakehouse",
            "upload-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            local_path.to_str().unwrap(),
            "--table",
            &table_name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "loaded");
    assert_eq!(data["table"], table_name);
    assert_eq!(data["format"], "Csv");

    // Verify table exists
    let assert = fabio()
        .args([
            "lakehouse",
            "list-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let found = data
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"].as_str() == Some(table_name.as_str()));
    assert!(found, "table not found in listing after upload-table");

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_upload_table_explicit_format() {
    let cfg = TestConfig::from_env();
    let table_name = common::unique_name("upload_fmt");

    let tmp_dir = TempDir::new().unwrap();
    let csv_content = "col1,col2\nfoo,10\nbar,20\n";
    let local_path = tmp_dir.path().join("data.txt"); // non-standard extension
    fs::write(&local_path, csv_content).unwrap();

    // upload-table with explicit --format Csv (since .txt can't be auto-detected)
    let assert = fabio()
        .args([
            "lakehouse",
            "upload-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            local_path.to_str().unwrap(),
            "--table",
            &table_name,
            "--format",
            "Csv",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "loaded");
    assert_eq!(data["table"], table_name);

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_upload_table_dry_run() {
    let cfg = TestConfig::from_env();

    let tmp_dir = TempDir::new().unwrap();
    let local_path = tmp_dir.path().join("dry.csv");
    fs::write(&local_path, "a,b\n1,2\n").unwrap();

    // dry-run should NOT upload or load
    let assert = fabio()
        .args([
            "lakehouse",
            "upload-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            local_path.to_str().unwrap(),
            "--table",
            "dry_run_table",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["command"], "lakehouse upload-table");
}

#[test]
fn lakehouse_upload_table_invalid_mode() {
    let assert = fabio()
        .args([
            "lakehouse",
            "upload-table",
            "--workspace",
            "fake-ws",
            "--id",
            "fake-id",
            "--source-path",
            "/dev/null",
            "--table",
            "t",
            "--mode",
            "BadMode",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("INVALID_INPUT"),
        "expected INVALID_INPUT error code"
    );
    assert!(
        stderr.contains("Overwrite"),
        "expected valid modes in error hint"
    );
}

#[test]
fn lakehouse_upload_table_unknown_extension() {
    let tmp_dir = TempDir::new().unwrap();
    let local_path = tmp_dir.path().join("data.xyz");
    fs::write(&local_path, "stuff").unwrap();

    let assert = fabio()
        .args([
            "lakehouse",
            "upload-table",
            "--workspace",
            "fake-ws",
            "--id",
            "fake-id",
            "--source-path",
            local_path.to_str().unwrap(),
            "--table",
            "t",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("INVALID_INPUT"),
        "expected INVALID_INPUT error code"
    );
    assert!(
        stderr.contains("--format"),
        "expected hint suggesting --format flag"
    );
}

// ---------------------------------------------------------------------------
// Delete non-existent table returns error
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_delete_table_nonexistent_returns_error() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            "nonexistent_table_xyz_12345",
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

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_move_table_without_dest_name() {
    let cfg = TestConfig::from_env();
    let table_name = common::unique_name("mv_same");

    // Create source table
    create_test_table(&cfg, &table_name);

    // Move without --dest-table (should keep same name)
    let assert = fabio()
        .args([
            "lakehouse",
            "move-table",
            "--source-workspace",
            &cfg.source_workspace,
            "--source-id",
            &cfg.source_lakehouse,
            "--source-table",
            &table_name,
            "--dest-workspace",
            &cfg.dest_workspace,
            "--dest-id",
            &cfg.dest_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "moved");
    assert_eq!(data["sourceTable"], table_name);
    assert_eq!(data["destTable"], table_name);

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_copy_table_without_dest_name() {
    let cfg = TestConfig::from_env();
    let src_table = common::unique_name("cp_same");

    // Create source table
    create_test_table(&cfg, &src_table);

    // Copy without --dest-table (should keep same name)
    let assert = fabio()
        .args([
            "lakehouse",
            "copy-table",
            "--source-workspace",
            &cfg.source_workspace,
            "--source-id",
            &cfg.source_lakehouse,
            "--source-table",
            &src_table,
            "--dest-workspace",
            &cfg.dest_workspace,
            "--dest-id",
            &cfg.dest_lakehouse,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "copied");
    assert_eq!(data["sourceTable"], src_table);
    assert_eq!(data["destTable"], src_table); // Same name as source

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--table",
            &src_table,
        ])
        .assert()
        .success();

    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &src_table,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_load_table_with_append_mode() {
    let cfg = TestConfig::from_env();
    let table_name = common::unique_name("append_test");

    let tmp_dir = tempfile::TempDir::new().unwrap();
    let csv1 = "x,y\n1,a\n2,b\n";
    let csv2 = "x,y\n3,c\n4,d\n";

    let path1 = tmp_dir.path().join("data1.csv");
    let path2 = tmp_dir.path().join("data2.csv");
    std::fs::write(&path1, csv1).unwrap();
    std::fs::write(&path2, csv2).unwrap();

    let remote1 = format!("Files/{table_name}_1.csv");
    let remote2 = format!("Files/{table_name}_2.csv");

    // Upload both CSVs
    fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            path1.to_str().unwrap(),
            "--dest-path",
            &remote1,
        ])
        .assert()
        .success();

    fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            path2.to_str().unwrap(),
            "--dest-path",
            &remote2,
        ])
        .assert()
        .success();

    // Load first CSV with Overwrite
    fabio()
        .args([
            "lakehouse",
            "load-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            &remote1,
            "--table",
            &table_name,
            "--mode",
            "Overwrite",
            "--format",
            "Csv",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // Load second CSV with Append
    let assert = fabio()
        .args([
            "lakehouse",
            "load-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            &remote2,
            "--table",
            &table_name,
            "--mode",
            "Append",
            "--format",
            "Csv",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should succeed with loaded or Succeeded status
    let status = data.get("status").and_then(|s| s.as_str()).unwrap_or("");
    assert!(
        status == "loaded" || status == "Succeeded",
        "unexpected status: {status}"
    );

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            &remote1,
        ])
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
            &remote2,
        ])
        .assert()
        .success();

    fabio()
        .args([
            "lakehouse",
            "delete-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--table",
            &table_name,
        ])
        .assert()
        .success();
}

// ─── Schemas-enabled lakehouse: --schema on upload-table + list-tables fallback ─

#[test]
fn upload_table_schema_flag_dry_run() {
    // The --schema flag must be accepted by upload-table (parity with load-table)
    // so schemas-enabled lakehouses can be targeted in one step.
    let assert = fabio()
        .args([
            "--dry-run",
            "lakehouse",
            "upload-table",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--source-path",
            "/tmp/does-not-matter.csv",
            "--table",
            "my_table",
            "--format",
            "Csv",
            "--schema",
            "dbo",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "lakehouse upload-table");
}

/// Live: a schemas-enabled lakehouse (the Fabric REST load/list-tables endpoints
/// reject these). fabio must (1) load via `--schema`, (2) enumerate tables via the
/// `OneLake` fallback in `list-tables`, and (3) teach `--schema` when it is omitted.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_schemas_enabled_table_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("lh_schemas");

    // Create a schemas-enabled lakehouse.
    let assert = fabio()
        .args([
            "lakehouse",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--enable-schemas",
        ])
        .assert()
        .success();
    let lh_json = parse_json(&assert);
    let lh_id = extract_data(&lh_json)["id"].as_str().unwrap().to_string();

    // Seed a small CSV.
    let dir = TempDir::new().unwrap();
    let csv = dir.path().join("dim.csv");
    fs::write(&csv, "Id,Name\n1,alpha\n2,beta\n").unwrap();

    // load via upload-table --schema dbo (the one-step path).
    fabio()
        .args([
            "lakehouse",
            "upload-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &lh_id,
            "--source-path",
            csv.to_str().unwrap(),
            "--table",
            "dim_test",
            "--format",
            "Csv",
            "--mode",
            "Overwrite",
            "--schema",
            "dbo",
        ])
        .assert()
        .success();

    // list-tables must succeed via the OneLake fallback and show the table + schema.
    let assert = fabio()
        .args([
            "lakehouse",
            "list-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &lh_id,
        ])
        .assert()
        .success();
    let list_json = parse_json(&assert);
    let data = extract_data(&list_json);
    let rows = data.as_array().unwrap();
    let row = rows
        .iter()
        .find(|r| r["name"] == "dim_test")
        .expect("dim_test should be listed");
    assert_eq!(row["schema"], "dbo");

    // upload-table WITHOUT --schema must fail with a teaching hint about --schema.
    let assert = fabio()
        .args([
            "lakehouse",
            "upload-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &lh_id,
            "--source-path",
            csv.to_str().unwrap(),
            "--table",
            "dim_noschema",
            "--format",
            "Csv",
            "--mode",
            "Overwrite",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("--schema"),
        "expected teaching hint about --schema, got stderr: {stderr}"
    );

    // Cleanup.
    fabio()
        .args([
            "lakehouse",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &lh_id,
        ])
        .assert()
        .success();
}
