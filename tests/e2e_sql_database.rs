//! End-to-end integration tests for `fabio sql-database` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use predicates::prelude::*;
use serial_test::serial;
use std::io::Write;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_database_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["sql-database", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_database_create_show_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("sqldb_test");

    // Create
    let assert = fabio()
        .args([
            "sql-database",
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
    let db_id = data["id"].as_str().unwrap().to_string();

    // Show
    let assert = fabio()
        .args([
            "sql-database",
            "show",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"], db_id);
    assert_eq!(data["displayName"], name);
    // Verify properties are returned
    assert!(data.get("properties").is_some());

    // Delete
    let assert = fabio()
        .args([
            "sql-database",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
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
fn sql_database_update_description() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("sqldb_upd");

    // Create
    let assert = fabio()
        .args([
            "sql-database",
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
    let db_id = data["id"].as_str().unwrap().to_string();

    // Update description
    let assert = fabio()
        .args([
            "sql-database",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--description",
            "Updated via E2E test",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["description"], "Updated via E2E test");

    // Cleanup
    fabio()
        .args([
            "sql-database",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_database_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "sql-database",
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
fn sql_database_list_deleted_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "sql-database",
            "list-deleted",
            "--workspace",
            &cfg.source_workspace,
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
fn sql_database_dry_run_create() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "sql-database",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            "dry_run_test",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "sql-database create");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_database_get_audit_settings() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("sqldb_aud");

    // Create
    let assert = fabio()
        .args([
            "sql-database",
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
    let db_id = data["id"].as_str().unwrap().to_string();

    // Get audit settings
    let assert = fabio()
        .args([
            "sql-database",
            "get-audit-settings",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should have state field
    assert!(data.get("state").is_some());

    // Cleanup
    fabio()
        .args([
            "sql-database",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
        ])
        .assert()
        .success();
}

// ===========================================================================
// sql-database query (TDS)
// ===========================================================================

/// Helper: find or create a SQL database in the dest workspace for query tests.
/// Returns the database ID.
fn ensure_sql_database(cfg: &TestConfig) -> String {
    let assert = fabio()
        .args(["sql-database", "list", "--workspace", &cfg.dest_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();

    // Use existing database if available
    if let Some(db) = items.first() {
        return db["id"].as_str().unwrap().to_string();
    }

    // Create one for tests
    let name = common::unique_name("sqldb_qry");
    let assert = fabio()
        .args([
            "sql-database",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    data["id"].as_str().unwrap().to_string()
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_query_select_one() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    // Run a simple SELECT 1
    let assert = fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--sql",
            "SELECT 1 AS test_col",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should be an array with one row
    let rows = data.as_array().expect("expected array of rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["test_col"], 1);
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_query_multiple_rows() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    let assert = fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--sql",
            "SELECT val FROM (VALUES (1),(2),(3)) AS t(val)",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let rows = data.as_array().expect("expected array of rows");
    assert_eq!(rows.len(), 3);
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_query_from_stdin() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    let assert = fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
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
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_query_from_file() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    let tmp_dir = tempfile::TempDir::new().unwrap();
    let sql_file = tmp_dir.path().join("test.sql");
    std::fs::write(&sql_file, "SELECT 99 AS from_file").unwrap();

    let sql_arg = format!("@{}", sql_file.to_str().unwrap());
    let assert = fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
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
    assert_eq!(rows[0]["from_file"], 99);
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_query_table_output() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    // Table output should render without crashing
    fabio()
        .args([
            "--output",
            "table",
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--sql",
            "SELECT 1 AS col1, 'hello' AS col2",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success()
        .stdout(predicate::str::contains("col1"))
        .stdout(predicate::str::contains("col2"));
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_query_csv_output() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    let assert = fabio()
        .args([
            "--output",
            "csv",
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--sql",
            "SELECT 1 AS num, 'hello' AS txt, CAST(NULL AS VARCHAR) AS empty",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "CSV should have header + data row, got: {stdout}"
    );
    assert_eq!(lines[0], "num,txt,empty");
    assert_eq!(lines[1], "1,hello,");
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_query_tsv_output() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    let assert = fabio()
        .args([
            "--output",
            "tsv",
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--sql",
            "SELECT 99 AS val, 'world' AS msg",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "TSV should have header + data row, got: {stdout}"
    );
    assert_eq!(lines[0], "val\tmsg");
    assert_eq!(lines[1], "99\tworld");
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_query_ddl_no_resultset() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    // DDL that doesn't return a result set
    let assert = fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--sql",
            "DECLARE @x INT = 1",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should report success with no rows
    assert!(data.get("message").is_some() || data.is_array());
}

// ===========================================================================
// sql-database connection-string
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_connection_string_returns_info() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    let assert = fabio()
        .args([
            "sql-database",
            "connection-string",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should return server, database, port, and connectionString
    assert!(data.get("server").is_some(), "missing 'server' field");
    assert!(data.get("database").is_some(), "missing 'database' field");
    assert!(data.get("port").is_some(), "missing 'port' field");
    let conn_str = data["connectionString"].as_str().unwrap();
    assert!(
        conn_str.contains("Server=tcp:"),
        "connection string should have Server=tcp:"
    );
    assert!(
        conn_str.contains("ActiveDirectoryDefault"),
        "connection string should specify AAD auth"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_database_query_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--sql",
            "SELECT 1",
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

// ─── Import tests ────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_import_csv() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    // Create a temp CSV file
    let mut tmpfile = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(tmpfile, "id,name,age,active").unwrap();
    writeln!(tmpfile, "1,Alice,30,true").unwrap();
    writeln!(tmpfile, "2,Bob,25,false").unwrap();
    writeln!(tmpfile, "3,Charlie,35,true").unwrap();
    tmpfile.flush().unwrap();

    let file_path = tmpfile.path().to_str().unwrap().to_string();

    // Import CSV
    let assert = fabio()
        .args([
            "sql-database",
            "import",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--file",
            &file_path,
            "--table",
            "test_csv_import",
            "--drop-if-exists",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["rows_inserted"], 3);
    assert_eq!(data["table"], "test_csv_import");

    // Verify data via query
    let assert = fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--sql",
            "SELECT COUNT(*) AS cnt FROM [test_csv_import]",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    let json = parse_json(&assert);
    let rows = extract_data(&json).as_array().unwrap().clone();
    assert_eq!(rows[0]["cnt"], 3);

    // Cleanup
    let _ = fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--sql",
            "DROP TABLE IF EXISTS [test_csv_import]",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_import_json() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    // Create a temp JSON file
    let mut tmpfile = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    write!(
        tmpfile,
        r#"[
        {{"order_id": 1, "product": "Widget", "price": 9.99, "qty": 2}},
        {{"order_id": 2, "product": "Gadget", "price": 19.99, "qty": 1}},
        {{"order_id": 3, "product": "Doohickey", "price": 4.50, "qty": 5}}
    ]"#
    )
    .unwrap();
    tmpfile.flush().unwrap();

    let file_path = tmpfile.path().to_str().unwrap().to_string();

    // Import JSON
    let assert = fabio()
        .args([
            "sql-database",
            "import",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--file",
            &file_path,
            "--table",
            "test_json_import",
            "--drop-if-exists",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["rows_inserted"], 3);

    // Verify data
    let assert = fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--sql",
            "SELECT product, price FROM [test_json_import] WHERE order_id = 2",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    let json = parse_json(&assert);
    let rows = extract_data(&json).as_array().unwrap().clone();
    assert_eq!(rows[0]["product"], "Gadget");

    // Cleanup
    let _ = fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--sql",
            "DROP TABLE IF EXISTS [test_json_import]",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_import_dry_run() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    // Create temp CSV
    let mut tmpfile = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(tmpfile, "x,y").unwrap();
    writeln!(tmpfile, "1,hello").unwrap();
    tmpfile.flush().unwrap();
    let file_path = tmpfile.path().to_str().unwrap().to_string();

    // Dry-run should NOT create the table
    let assert = fabio()
        .args([
            "sql-database",
            "import",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--file",
            &file_path,
            "--table",
            "test_dryrun_import",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // dry-run returns the plan
    assert_eq!(data["total_rows"], 1);
    assert!(
        data["create_table_ddl"]
            .as_str()
            .unwrap()
            .contains("CREATE TABLE")
    );
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_import_unsupported_format() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    let mut tmpfile = tempfile::Builder::new().suffix(".xml").tempfile().unwrap();
    writeln!(tmpfile, "<data/>").unwrap();
    tmpfile.flush().unwrap();
    let file_path = tmpfile.path().to_str().unwrap().to_string();

    fabio()
        .args([
            "sql-database",
            "import",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--file",
            &file_path,
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Unsupported file format"));
}

#[test]
#[ignore = "requires live Fabric tenant with SQL database"]
#[serial]
fn sql_database_import_file_not_found() {
    let cfg = TestConfig::from_env();
    let db_id = ensure_sql_database(&cfg);

    fabio()
        .args([
            "sql-database",
            "import",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &db_id,
            "--file",
            "/tmp/nonexistent_file_xyz.csv",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Cannot read CSV file"));
}

// ─── CMK Encryption ──────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_database_revalidate_cmk_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "sql-database",
            "revalidate-cmk",
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
    assert_eq!(data["would_execute"], "sql-database revalidate-cmk");
}

// ---------------------------------------------------------------------------
// sql-database queries-kill dry-run
// ---------------------------------------------------------------------------

#[test]
fn sql_database_queries_kill_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "sql-database",
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
// sql-database statistics-create dry-run
// ---------------------------------------------------------------------------

#[test]
fn sql_database_statistics_create_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "sql-database",
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
// sql-database statistics-update dry-run
// ---------------------------------------------------------------------------

#[test]
fn sql_database_statistics_update_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "sql-database",
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
// sql-database statistics-delete dry-run
// ---------------------------------------------------------------------------

#[test]
fn sql_database_statistics_delete_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "sql-database",
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

// sql-database update-audit-settings --predicate-expression (SQL Audit predicate)
//
// Hermetic test: `--predicate-expression` is serialized as top-level
// `predicateExpression` in the audit-settings body (matching sql-endpoint /
// warehouse), and the shared WHERE/length validation is applied. Covers the
// SQL-Database-specific serialization path independently of the shared validator.
#[test]
fn sql_database_update_audit_predicate_dry_run() {
    let assert = fabio()
        .args([
            "sql-database",
            "update-audit-settings",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000002",
            "--predicate-expression",
            "NOT statement LIKE 'SELECT %'",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(
        extract_data(&json)["details"]["predicateExpression"],
        "NOT statement LIKE 'SELECT %'"
    );
}

#[test]
fn sql_database_update_audit_rejects_leading_where() {
    let assert = fabio()
        .args([
            "sql-database",
            "update-audit-settings",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000002",
            "--predicate-expression",
            "WHERE statement LIKE 'SELECT %'",
            "--dry-run",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("WHERE keyword"), "stderr: {stderr}");
}
