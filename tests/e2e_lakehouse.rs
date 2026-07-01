//! End-to-end integration tests for `fabio lakehouse` basic commands
//! (tables, files, upload, download).

mod common;

use common::{TestConfig, extract_count, extract_data, fabio, parse_json};
use predicates::prelude::*;
use serial_test::serial;
use std::fs;
use tempfile::TempDir;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_tables_returns_list() {
    let cfg = TestConfig::from_env();

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
    let count = extract_count(&json);

    assert!(count > 0, "expected at least one table in source lakehouse");
    let arr = data.as_array().unwrap();
    let first = &arr[0];
    assert!(first.get("name").is_some());
    assert!(first.get("format").is_some());
    assert_eq!(first["format"], "delta");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_tables_table_format() {
    let cfg = TestConfig::from_env();

    // Table output for lakehouse tables
    fabio()
        .args([
            "--output",
            "table",
            "lakehouse",
            "list-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_files_returns_entries() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
            "list-files",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let count = extract_count(&json);

    assert!(count > 0, "expected at least one file entry");
    let arr = data.as_array().unwrap();
    let first = &arr[0];
    assert!(first.get("name").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_files_with_path_filter() {
    let cfg = TestConfig::from_env();

    // Upload a file into a subdirectory first
    let tmp_dir = TempDir::new().unwrap();
    let upload_path = tmp_dir.path().join("subdir_test.txt");
    fs::write(&upload_path, "path filter test").unwrap();

    let subdir = format!("Files/{}", common::unique_name("subdir"));
    let remote_path = format!("{subdir}/test.txt");

    fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            upload_path.to_str().unwrap(),
            "--dest-path",
            &remote_path,
        ])
        .assert()
        .success();

    // List with --path filter pointing to the subdirectory
    let assert = fabio()
        .args([
            "lakehouse",
            "list-files",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            &subdir,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();

    // Should find our file
    let found = arr.iter().any(|f| {
        f.get("name")
            .and_then(|n| n.as_str())
            .is_some_and(|n| n.contains("test.txt"))
    });
    assert!(found, "uploaded file not found with --path filter");

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
            &remote_path,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_upload_and_download_roundtrip() {
    let cfg = TestConfig::from_env();
    let tmp_dir = TempDir::new().unwrap();
    let file_name = common::unique_name("upload_test");
    let content = format!("test content from {file_name}");
    let remote_path = format!("Files/{file_name}.txt");

    // Create a local file
    let upload_path = tmp_dir.path().join("upload.txt");
    fs::write(&upload_path, &content).unwrap();

    // Upload
    let assert = fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            upload_path.to_str().unwrap(),
            "--dest-path",
            &remote_path,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "uploaded");
    assert_eq!(data["size"], content.len());

    // Download
    let download_path = tmp_dir.path().join("download.txt");
    let assert = fabio()
        .args([
            "lakehouse",
            "download",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            &remote_path,
            "--dest-path",
            download_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "downloaded");

    // Verify content matches
    let downloaded = fs::read_to_string(&download_path).unwrap();
    assert_eq!(downloaded, content);

    // Cleanup: delete remote file
    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            &remote_path,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_upload_nonexistent_source_errors() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            "/tmp/nonexistent_file_xyz_12345.txt",
            "--dest-path",
            "Files/should_not_exist.txt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("INVALID_INPUT").or(predicate::str::contains("error")));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_download_nonexistent_source_errors() {
    let cfg = TestConfig::from_env();
    let tmp_dir = TempDir::new().unwrap();
    let local_path = tmp_dir.path().join("should_not_be_created.txt");

    fabio()
        .args([
            "lakehouse",
            "download",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            "Files/absolutely_nonexistent_file_xyz.txt",
            "--dest-path",
            local_path.to_str().unwrap(),
        ])
        .assert()
        .failure();

    // File should not have been created
    assert!(!local_path.exists());
}

// ---------------------------------------------------------------------------
// lakehouse tables with --limit
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_tables_with_limit() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
            "list-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--limit",
            "1",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().expect("Expected array");
    assert_eq!(arr.len(), 1, "Expected exactly 1 table with --limit 1");
}

// ---------------------------------------------------------------------------
// lakehouse files with --limit
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_files_with_limit() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
            "list-files",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
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
// lakehouse upload overwrite (upload same path twice — should succeed)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_upload_overwrite() {
    let cfg = TestConfig::from_env();
    let tmp_dir = TempDir::new().unwrap();
    let file_name = common::unique_name("overwrite_test");
    let remote_path = format!("Files/{file_name}.txt");

    // Upload first version
    let upload_path = tmp_dir.path().join("v1.txt");
    fs::write(&upload_path, "version 1").unwrap();

    fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            upload_path.to_str().unwrap(),
            "--dest-path",
            &remote_path,
        ])
        .assert()
        .success();

    // Upload second version to same path (overwrite)
    let upload_path_v2 = tmp_dir.path().join("v2.txt");
    fs::write(&upload_path_v2, "version 2 updated content").unwrap();

    let assert = fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            upload_path_v2.to_str().unwrap(),
            "--dest-path",
            &remote_path,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "uploaded");
    assert_eq!(data["size"], "version 2 updated content".len());

    // Download and verify content is v2
    let download_path = tmp_dir.path().join("downloaded.txt");
    fabio()
        .args([
            "lakehouse",
            "download",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            &remote_path,
            "--dest-path",
            download_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&download_path).unwrap();
    assert_eq!(content, "version 2 updated content");

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
            &remote_path,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// lakehouse tables table output format has headers
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_files_table_output() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "--output",
            "table",
            "lakehouse",
            "list-files",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("NAME"));
}

// ===========================================================================
// Lakehouse CRUD (list, show, create, update, delete)
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_list_returns_lakehouses() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["lakehouse", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one lakehouse");

    let first = &arr[0];
    assert!(first.get("id").is_some());
    assert!(first.get("displayName").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_show_returns_details() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
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
    assert!(data.get("displayName").is_some());
    // Lakehouse-specific properties
    assert!(
        data.get("properties").is_some(),
        "Expected properties: {data}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("lh_crud");

    // Create
    let assert = fabio()
        .args([
            "lakehouse",
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
    let lh_id = data["id"].as_str().unwrap().to_string();

    // Delete
    let assert = fabio()
        .args([
            "lakehouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &lh_id,
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
fn lakehouse_create_with_description() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("lh_desc");

    let assert = fabio()
        .args([
            "lakehouse",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--description",
            "Test lakehouse with description",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    assert_eq!(data["description"], "Test lakehouse with description");
    let lh_id = data["id"].as_str().unwrap().to_string();

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &lh_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_update_name() {
    let cfg = TestConfig::from_env();
    let original = common::unique_name("lh_upd_orig");
    let updated = common::unique_name("lh_upd_new");

    // Create
    let assert = fabio()
        .args([
            "lakehouse",
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
    let lh_id = data["id"].as_str().unwrap().to_string();

    // Update name
    let assert = fabio()
        .args([
            "lakehouse",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &lh_id,
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
            "lakehouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &lh_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
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
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_delete_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
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

// ─── Query tests ─────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_query_select() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            "SELECT TOP 3 product_id, product_name FROM sales ORDER BY product_id",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let count = extract_count(&json);
    assert_eq!(count, 3, "expected 3 rows");
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();
    assert_eq!(arr[0]["product_id"], 1);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_query_from_file() {
    let cfg = TestConfig::from_env();
    let dir = TempDir::new().unwrap();
    let sql_file = dir.path().join("test.sql");
    fs::write(&sql_file, "SELECT COUNT(*) AS cnt FROM sales").unwrap();

    let assert = fabio()
        .args([
            "lakehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            &format!("@{}", sql_file.display()),
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();
    assert!(arr[0]["cnt"].as_i64().unwrap() > 0);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_query_from_stdin() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .write_stdin("SELECT TOP 1 category FROM sales")
        .assert()
        .success();
    let json = parse_json(&assert);
    let count = extract_count(&json);
    assert_eq!(count, 1);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_query_table_output() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "-o",
            "table",
            "lakehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            "SELECT TOP 2 product_id, product_name FROM sales ORDER BY product_id",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("product_id"));
    assert!(stdout.contains("Widget A"));
}

// ---------------------------------------------------------------------------
// lakehouse query with --output csv
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_query_csv_output() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "-o",
            "csv",
            "lakehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            "SELECT TOP 1 product_id, product_name FROM sales ORDER BY product_id",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "CSV should have header + data row, got: {stdout}"
    );
    // Header contains column names
    assert!(lines[0].contains("product_id"));
    assert!(lines[0].contains("product_name"));
    assert!(lines[0].contains(','));
    // Data row has values
    assert!(lines[1].contains("Widget A") || lines[1].contains(','));
}

// ---------------------------------------------------------------------------
// lakehouse query with --output tsv
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_query_tsv_output() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "-o",
            "tsv",
            "lakehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--sql",
            "SELECT TOP 1 product_id, product_name FROM sales ORDER BY product_id",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "TSV should have header + data row, got: {stdout}"
    );
    // Header uses tabs
    assert!(lines[0].contains('\t'));
    assert!(lines[0].contains("product_id"));
    // Data row uses tabs
    assert!(lines[1].contains('\t'));
}

// ─── Hard Delete ─────────────────────────────────────────────────────────────

#[test]
fn lakehouse_delete_hard_delete_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "lakehouse",
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

// ─── Materialized Lake View Execution Definitions ───────────────────────────

#[test]
fn lakehouse_create_execution_definition_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "lakehouse",
            "create-execution-definition",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--content",
            r#"{"displayName":"nightly-refresh","currentLakehouseExecutionContext":{"mode":"All"}}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["displayName"], "nightly-refresh");
}

#[test]
fn lakehouse_update_execution_definition_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "lakehouse",
            "update-execution-definition",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--execution-definition-id",
            "cccccccc-1111-2222-3333-444444444444",
            "--content",
            r#"{"description":"Updated"}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["description"], "Updated");
}

#[test]
fn lakehouse_delete_execution_definition_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "lakehouse",
            "delete-execution-definition",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--execution-definition-id",
            "cccccccc-1111-2222-3333-444444444444",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["details"]["executionDefinitionId"],
        "cccccccc-1111-2222-3333-444444444444"
    );
}

#[test]
fn lakehouse_create_execution_definition_requires_file_or_content() {
    let assert = fabio()
        .args([
            "lakehouse",
            "create-execution-definition",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
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
fn lakehouse_list_execution_definitions_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
            "list-execution-definitions",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array(), "Expected array, got: {data}");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_show_execution_definition_nonexistent_fails() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "lakehouse",
            "show-execution-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--execution-definition-id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}
