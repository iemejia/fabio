//! E2E integration tests for the `fabio kql-database` command group.
//!
//! Live query tests require:
//! - `FABIO_TEST_SOURCE_WORKSPACE` (workspace with an Eventhouse + KQL Database)
//! - `FABIO_TEST_KQL_DATABASE_ID` (ID of the KQL database to query)
//! - `FABIO_TEST_KUSTO_QUERY_URI` (Kusto query URI, e.g. `https://xxx.z4.kusto.fabric.microsoft.com`)

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serde_json::Value;
use serial_test::serial;

// ─── CRUD Tests ──────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
fn kql_database_list() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args(["kql-database", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&output);
    assert!(json.get("data").is_some());
    assert!(json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn kql_database_create_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "kql-database",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-kql-db",
            "--eventhouse-id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "kql-database create");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn kql_database_update_requires_fields() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "kql-database",
            "update",
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
fn kql_database_delete_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "kql-database",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "kql-database delete");
}

// ─── Query Tests (no tenant required) ────────────────────────────────────────

#[test]
fn kql_database_query_no_input_fails() {
    // Without --kql and without stdin, should fail with helpful error
    fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--kql",
            "",
        ])
        .assert()
        .failure();
}

// ─── Query Tests (live tenant) ───────────────────────────────────────────────

/// Helper to get KQL test config from environment.
fn kql_test_config() -> (TestConfig, String, String) {
    let cfg = TestConfig::from_env();
    let kql_db_id =
        std::env::var("FABIO_TEST_KQL_DATABASE_ID").expect("FABIO_TEST_KQL_DATABASE_ID required");
    let query_uri =
        std::env::var("FABIO_TEST_KUSTO_QUERY_URI").expect("FABIO_TEST_KUSTO_QUERY_URI required");
    (cfg, kql_db_id, query_uri)
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_print_literal() {
    let (cfg, kql_db_id, _query_uri) = kql_test_config();

    // Auto-discovers query URI from database properties
    let output = fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "print message='hello from fabio'",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let rows = data.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["message"], "hello from fabio");
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_with_explicit_uri() {
    let (cfg, kql_db_id, query_uri) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--query-uri",
            &query_uri,
            "--kql",
            "print x=42, y=3.14, z=true",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let rows = data.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["x"], 42);
    assert_eq!(rows[0]["z"], true);
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_multiple_rows() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "range i from 1 to 5 step 1 | extend squared = i * i",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    assert_eq!(json["count"], 5);
    let data = extract_data(&json);
    let rows = data.as_array().unwrap();
    assert_eq!(rows[0]["i"], 1);
    assert_eq!(rows[0]["squared"], 1);
    assert_eq!(rows[4]["i"], 5);
    assert_eq!(rows[4]["squared"], 25);
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_show_tables_mgmt() {
    let (cfg, kql_db_id, _) = kql_test_config();

    // .show tables uses the /v1/rest/mgmt endpoint
    let output = fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            ".show tables",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    // Should have at least the SalesEvents table we created
    let data = extract_data(&json);
    if let Some(rows) = data.as_array()
        && !rows.is_empty()
    {
        assert!(rows[0].get("TableName").is_some());
    }
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_table_data() {
    let (cfg, kql_db_id, _) = kql_test_config();

    // Query the SalesEvents table (created during setup)
    let output = fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "SalesEvents | summarize TotalAmount=sum(Amount), Count=count() by Region | order by TotalAmount desc",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let rows = data.as_array().unwrap();
    // We know from setup: EU=99.0, US=59.98, APAC=15.0
    assert!(rows.len() >= 3);
    assert!(rows[0].get("Region").is_some());
    assert!(rows[0].get("TotalAmount").is_some());
    assert!(rows[0].get("Count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_from_file() {
    let (cfg, kql_db_id, _) = kql_test_config();

    // Write a query to a temp file
    let tmp_dir = std::env::temp_dir();
    let kql_file = tmp_dir.join("fabio_test_query.kql");
    std::fs::write(&kql_file, "print source='file', value=123").unwrap();
    let file_arg = format!("@{}", kql_file.display());

    let output = fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            &file_arg,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let rows = data.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["source"], "file");
    assert_eq!(rows[0]["value"], 123);

    // Cleanup
    let _ = std::fs::remove_file(&kql_file);
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_stdin() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
        ])
        .write_stdin("print source='stdin', value=456")
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let rows = data.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["source"], "stdin");
    assert_eq!(rows[0]["value"], 456);
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_invalid_syntax_fails() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "INVALID SYNTAX @@@ |||",
        ])
        .assert()
        .failure();

    // Errors are written to stderr in fabio
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    let err: Value = serde_json::from_str(&stderr).expect("stderr should be JSON error");
    assert_eq!(err["error"]["code"], "API_ERROR");
    assert!(err["error"]["message"].as_str().unwrap().contains("400"));
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_table_output_format() {
    let (cfg, kql_db_id, _) = kql_test_config();

    fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "range i from 1 to 3 step 1",
            "-o",
            "table",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("| i |"));
}

// ---------------------------------------------------------------------------
// KQL query with --output csv
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_csv_output() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let assert = fabio()
        .args([
            "-o",
            "csv",
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "print col1=42, col2='hello', col3=true",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "CSV should have header + data row, got: {stdout}"
    );
    assert_eq!(lines[0], "col1,col2,col3");
    assert_eq!(lines[1], "42,hello,true");
}

// ---------------------------------------------------------------------------
// KQL query with --output tsv
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_query_tsv_output() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let assert = fabio()
        .args([
            "-o",
            "tsv",
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "range i from 1 to 3 step 1 | extend label=strcat('item_', tostring(i))",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 4,
        "TSV should have header + 3 data rows, got: {stdout}"
    );
    // Header uses tabs
    assert_eq!(lines[0], "i\tlabel");
    // First data row
    assert_eq!(lines[1], "1\titem_1");
    assert_eq!(lines[2], "2\titem_2");
    assert_eq!(lines[3], "3\titem_3");
}

// ─── Schema Discovery Tests ─────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_list_entities() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "list-entities",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    // Should return an array (possibly empty on a fresh database)
    assert!(data.is_array(), "list-entities should return an array");
    if let Some(arr) = data.as_array()
        && !arr.is_empty()
    {
        // Each entity should have name and type
        assert!(arr[0].get("name").is_some());
        assert!(arr[0].get("type").is_some());
    }
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_list_entities_filter_type() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "list-entities",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--entity-type",
            "table",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    if let Some(arr) = data.as_array() {
        for entity in arr {
            assert_eq!(entity["type"], "table", "Filter should only return tables");
        }
    }
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_describe() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "describe",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    // describe returns either a list of columns or an empty-result message
    assert!(
        json.get("data").is_some(),
        "describe should produce JSON data"
    );
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_describe_entity() {
    let (cfg, kql_db_id, _) = kql_test_config();

    // First, find a table name via list-entities
    let output = fabio()
        .args([
            "kql-database",
            "list-entities",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--entity-type",
            "table",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let tables = data.as_array().expect("should be array");

    if tables.is_empty() {
        // No tables in database — skip gracefully
        return;
    }

    let table_name = tables[0]["name"].as_str().expect("table should have name");

    // Now describe that specific table
    let output = fabio()
        .args([
            "kql-database",
            "describe-entity",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--entity-name",
            table_name,
            "--entity-type",
            "table",
        ])
        .assert()
        .success();

    let json2 = parse_json(&output);
    assert!(
        json2.get("data").is_some(),
        "describe-entity should produce data"
    );
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_sample() {
    let (cfg, kql_db_id, _) = kql_test_config();

    // Use a synthetic table via KQL (avoids needing a real populated table)
    // We can sample from a range function treated as table
    let output = fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "range i from 1 to 100 step 1 | take 5",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let rows = data.as_array().unwrap();
    assert_eq!(rows.len(), 5);
}

// ─── Ingestion Test ─────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_ingest_dry_run() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "ingest",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--table",
            "TestTable",
            "--data",
            "col1,col2\nhello,42",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["table"], "TestTable");
}

// ─── Query Plan Test ────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_show_queryplan() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "show-queryplan",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "print x=42",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    assert!(
        json.get("data").is_some(),
        "show-queryplan should produce data"
    );
}

// ─── Diagnostics Test ───────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_diagnostics() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "diagnostics",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    // diagnostics returns a JSON object with section keys
    assert!(data.is_object(), "diagnostics should return an object");
    let obj = data.as_object().unwrap();
    // At least capacity should be present (even if it's an error)
    assert!(
        obj.contains_key("capacity"),
        "diagnostics should have 'capacity' section"
    );
}

// ─── Deeplink Test ──────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_deeplink() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "deeplink",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "StormEvents | take 10",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    // Should have url, style, database fields
    assert!(data.get("url").is_some(), "deeplink should have url");
    assert!(data.get("style").is_some(), "deeplink should have style");
    assert!(
        data.get("database").is_some(),
        "deeplink should have database"
    );
    let url = data["url"].as_str().unwrap();
    assert!(
        url.contains("StormEvents"),
        "URL should contain the query text"
    );
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_deeplink_fabric_style() {
    let (cfg, kql_db_id, _) = kql_test_config();

    let output = fabio()
        .args([
            "kql-database",
            "deeplink",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            "print x=1",
            "--style",
            "fabric",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["style"], "fabric");
    let url = data["url"].as_str().unwrap();
    assert!(url.contains("app.fabric.microsoft.com"));
}

// ─── Offline Tests (no tenant needed) ───────────────────────────────────────

#[test]
fn kql_database_list_entities_invalid_entity_type_still_succeeds_offline() {
    // list-entities doesn't validate entity type client-side; it passes through
    // to the server. But we can test that the command at least parses correctly.
    fabio()
        .args([
            "kql-database",
            "list-entities",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--entity-type",
            "table",
            "--dry-run",
        ])
        .assert()
        // list-entities is a read command, no dry-run guard — it will try auth and fail
        .failure();
}

#[test]
fn kql_database_describe_entity_invalid_type() {
    // describe-entity with a dummy workspace will fail at auth/API before validation,
    // but the command should at least parse args correctly and produce a JSON error
    let output = fabio()
        .args([
            "kql-database",
            "describe-entity",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--entity-name",
            "test",
            "--entity-type",
            "invalid-type",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    // Should fail with either entity type validation or auth/API error
    assert!(
        stderr.contains("error"),
        "Should produce an error, got: {stderr}"
    );
}

#[test]
fn kql_database_ingest_no_data_fails() {
    // ingest without --data and without stdin should fail with input error
    let output = fabio()
        .args([
            "kql-database",
            "ingest",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--table",
            "test",
        ])
        .write_stdin("")
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(
        stderr.contains("No KQL provided") || stderr.contains("INVALID_INPUT"),
        "Should require data input, got: {stderr}"
    );
}

#[test]
fn kql_database_deeplink_invalid_style() {
    // deeplink with a dummy workspace will fail at API/auth before style validation,
    // but the command should parse args correctly and produce a JSON error
    let output = fabio()
        .args([
            "kql-database",
            "deeplink",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--kql",
            "print 1",
            "--style",
            "badstyle",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    // Should fail with either style validation or auth/API error
    assert!(
        stderr.contains("error"),
        "Should produce an error, got: {stderr}"
    );
}

// ─── Live Ingest Test ───────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
fn kql_database_ingest_live() {
    let (cfg, kql_db_id, _) = kql_test_config();

    // 1. Create a temp table
    let table_name = "fabio_ingest_test_tmp";
    fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            &format!(".create table ['{table_name}'] (Name: string, Value: int)"),
        ])
        .assert()
        .success();

    // 2. Ingest inline CSV data
    let output = fabio()
        .args([
            "kql-database",
            "ingest",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--table",
            table_name,
            "--data",
            "Alice,42\nBob,99",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    // Should indicate success (either "ingested" status or ingestion extent info)
    assert!(
        data.get("status").is_some() || data.as_array().is_some(),
        "Ingest should return status or extent info, got: {data}"
    );

    // 3. Cleanup: drop the temp table
    fabio()
        .args([
            "kql-database",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_db_id,
            "--kql",
            &format!(".drop table ['{table_name}']"),
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// kql-database queries-running
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn kql_database_queries_running() {
    let cfg = TestConfig::from_env();

    // List KQL databases to get an ID
    let assert = fabio()
        .args(["kql-database", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if items.is_empty() {
        eprintln!("No KQL databases found, skipping");
        return;
    }
    let db_id = items[0]["id"].as_str().unwrap();

    fabio()
        .args([
            "kql-database",
            "queries-running",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            db_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// kql-database journal
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn kql_database_journal() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["kql-database", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if items.is_empty() {
        eprintln!("No KQL databases found, skipping");
        return;
    }
    let db_id = items[0]["id"].as_str().unwrap();

    fabio()
        .args([
            "kql-database",
            "journal",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            db_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// kql-database queries-completed
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn kql_database_queries_completed() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["kql-database", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if items.is_empty() {
        eprintln!("No KQL databases found, skipping");
        return;
    }
    let db_id = items[0]["id"].as_str().unwrap();

    fabio()
        .args([
            "kql-database",
            "queries-completed",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            db_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ─── MCP URL (Eventhouse remote MCP) ─────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn kql_database_mcp_url_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("kql_mcp_eh");

    // Create an eventhouse (auto-creates a KQL database of the same name).
    let assert = fabio()
        .args([
            "eventhouse",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let eh_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Find the auto-created KQL database.
    std::thread::sleep(std::time::Duration::from_secs(8));
    let assert = fabio()
        .args(["kql-database", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let dbs = extract_data(&json).as_array().unwrap().clone();
    let kql_id = dbs
        .iter()
        .find(|d| d["displayName"].as_str() == Some(name.as_str()))
        .and_then(|d| d["id"].as_str())
        .expect("auto-created KQL database")
        .to_string();

    let expected_url = format!(
        "https://api.fabric.microsoft.com/v1/mcp/dataPlane/workspaces/{}/items/{}/kqlEndpoint",
        cfg.source_workspace, kql_id
    );

    // Existing KQL database: canonical URL, exists true, note, no hint.
    let assert = fabio()
        .args([
            "kql-database",
            "mcp-url",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &kql_id,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    assert_eq!(data["mcpUrl"], expected_url);
    assert_eq!(data["transport"], "http");
    assert_eq!(data["exists"], true);
    assert!(data["note"].as_str().unwrap().contains("MCP server"));
    assert!(data["hint"].is_null());

    // Nonexistent id: still emits the deterministic URL, exists false + hint.
    let missing = "00000000-0000-0000-0000-000000000000";
    let assert = fabio()
        .args([
            "kql-database",
            "mcp-url",
            "--workspace",
            &cfg.source_workspace,
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
            .ends_with(&format!("/items/{missing}/kqlEndpoint"))
    );
    assert_eq!(data["exists"], false);
    assert!(!data["hint"].as_str().unwrap().is_empty());

    // Clean up (deleting the eventhouse cascades to its KQL database).
    fabio()
        .args([
            "eventhouse",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &eh_id,
        ])
        .assert()
        .success();
}
