//! End-to-end integration tests for `fabio sql-endpoint` commands.

mod common;

use common::{TestConfig, extract_count, extract_data, fabio, parse_json};
use serial_test::serial;
use std::fs;
use tempfile::TempDir;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["sql-endpoint", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_dry_run_refresh_metadata() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "sql-endpoint",
            "refresh-metadata",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--timeout",
            r#"{"value":10,"timeUnit":"Minutes"}"#,
            "--recreate-tables",
            "--tables",
            r#"[{"schema":"sales","tableNames":["Orders","OrderDetails"]}]"#,
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "sql-endpoint refresh-metadata");
    assert_eq!(data["destructive"], true);
    assert_eq!(
        data["details"]["request"]["timeout"]["value"].as_f64(),
        Some(10.0)
    );
    assert_eq!(data["details"]["request"]["timeout"]["timeUnit"], "Minutes");
    assert_eq!(data["details"]["request"]["recreateTables"], true);
    assert_eq!(data["details"]["request"]["tables"][0]["schema"], "sales");
    assert_eq!(
        data["details"]["request"]["tables"][0]["tableNames"],
        serde_json::json!(["Orders", "OrderDetails"])
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_refresh_metadata_rejects_too_many_tables() {
    let cfg = TestConfig::from_env();
    let names = (0..26)
        .map(|index| format!(r#""Table{index}""#))
        .collect::<Vec<_>>()
        .join(",");
    let tables = format!(r#"[{{"schema":"dbo","tableNames":[{names}]}}]"#);

    let assert = fabio()
        .args([
            "sql-endpoint",
            "refresh-metadata",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--tables",
            &tables,
            "--dry-run",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "INVALID_INPUT");
}

// refresh-metadata renders the per-table sync results as a list so agents can
// filter (e.g. --query "[?status!='Success']").
#[test]
#[ignore = "requires live Fabric tenant with a lakehouse SQL endpoint"]
#[serial]
fn sql_endpoint_refresh_metadata_returns_table_list() {
    let cfg = TestConfig::from_env();
    let Ok(endpoint_id) = std::env::var("FABIO_TEST_SQL_ENDPOINT_ID") else {
        return; // skip when not configured
    };
    let assert = fabio()
        .args([
            "sql-endpoint",
            "refresh-metadata",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &endpoint_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    // Per-table results are a list; each row has tableName + status.
    let data = extract_data(&json);
    assert!(data.is_array());
    if let Some(first) = data.as_array().and_then(|a| a.first()) {
        assert!(first.get("tableName").is_some());
        assert!(first.get("status").is_some());
    }
}

// Selective refresh: only the requested table is synchronized, and the API
// returns it (schema-qualified for schema-enabled parent items).
// Requires FABIO_TEST_SQL_ENDPOINT_ID and FABIO_TEST_SQL_ENDPOINT_TABLE
// (optionally FABIO_TEST_SQL_ENDPOINT_SCHEMA, defaults to dbo).
#[test]
#[ignore = "requires live Fabric tenant with a lakehouse SQL endpoint"]
#[serial]
fn sql_endpoint_refresh_metadata_selective_returns_only_selected_table() {
    let cfg = TestConfig::from_env();
    let (Ok(endpoint_id), Ok(table)) = (
        std::env::var("FABIO_TEST_SQL_ENDPOINT_ID"),
        std::env::var("FABIO_TEST_SQL_ENDPOINT_TABLE"),
    ) else {
        return; // skip when not configured
    };
    let schema =
        std::env::var("FABIO_TEST_SQL_ENDPOINT_SCHEMA").unwrap_or_else(|_| "dbo".to_string());
    let tables = serde_json::json!([{ "schema": schema, "tableNames": [table] }]).to_string();

    let assert = fabio()
        .args([
            "sql-endpoint",
            "refresh-metadata",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &endpoint_id,
            "--tables",
            &tables,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let rows = data.as_array().expect("expected a per-table result list");
    assert_eq!(rows.len(), 1, "selective refresh must return one table");
    let table_name = rows[0]["tableName"]
        .as_str()
        .expect("result row missing 'tableName'");
    // Schema-enabled parent items return "schema.table"; others return "table".
    assert!(
        table_name == table || table_name == format!("{schema}.{table}"),
        "unexpected tableName {table_name}"
    );
    assert!(rows[0].get("status").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_update_audit_settings_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "sql-endpoint",
            "update-audit-settings",
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
fn sql_endpoint_set_audit_actions_requires_valid_endpoint() {
    let cfg = TestConfig::from_env();

    // Using a non-existent ID should return NOT_FOUND
    let assert = fabio()
        .args([
            "sql-endpoint",
            "set-audit-actions",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--actions",
            "BATCH_COMPLETED_GROUP",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "NOT_FOUND");
}

// ─── Query tests ─────────────────────────────────────────────────────────────

/// Helper: find the SQL endpoint ID by listing endpoints in the source workspace.
fn find_sql_endpoint_id(cfg: &TestConfig) -> String {
    let assert = fabio()
        .args(["sql-endpoint", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().expect("expected array of SQL endpoints");
    assert!(
        !arr.is_empty(),
        "no SQL endpoints found in source workspace"
    );
    // Return the first endpoint's ID
    arr[0]["id"]
        .as_str()
        .expect("SQL endpoint missing 'id' field")
        .to_string()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_query_nonexistent_id_fails() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "sql-endpoint",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--sql",
            "SELECT 1 AS x",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "NOT_FOUND");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_query_select() {
    let cfg = TestConfig::from_env();
    let endpoint_id = find_sql_endpoint_id(&cfg);

    let assert = fabio()
        .args([
            "sql-endpoint",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &endpoint_id,
            "--sql",
            "SELECT TOP 3 TABLE_NAME FROM INFORMATION_SCHEMA.TABLES ORDER BY TABLE_NAME",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let count = extract_count(&json);
    assert!(count > 0, "expected at least one row");
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();
    assert!(
        arr[0].get("TABLE_NAME").is_some(),
        "expected TABLE_NAME column in result"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_query_from_file() {
    let cfg = TestConfig::from_env();
    let endpoint_id = find_sql_endpoint_id(&cfg);

    let dir = TempDir::new().unwrap();
    let sql_file = dir.path().join("test.sql");
    fs::write(
        &sql_file,
        "SELECT TOP 1 TABLE_SCHEMA FROM INFORMATION_SCHEMA.TABLES",
    )
    .unwrap();

    let assert = fabio()
        .args([
            "sql-endpoint",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &endpoint_id,
            "--sql",
            &format!("@{}", sql_file.display()),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let count = extract_count(&json);
    assert_eq!(count, 1);
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();
    assert!(arr[0].get("TABLE_SCHEMA").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_query_from_stdin() {
    let cfg = TestConfig::from_env();
    let endpoint_id = find_sql_endpoint_id(&cfg);

    let assert = fabio()
        .args([
            "sql-endpoint",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &endpoint_id,
        ])
        .write_stdin("SELECT TOP 1 TABLE_TYPE FROM INFORMATION_SCHEMA.TABLES")
        .assert()
        .success();

    let json = parse_json(&assert);
    let count = extract_count(&json);
    assert_eq!(count, 1);
}

// ---------------------------------------------------------------------------
// sql-endpoint queries-running
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_queries_running() {
    let cfg = TestConfig::from_env();

    // List to get the SQL endpoint ID
    let assert = fabio()
        .args(["sql-endpoint", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if items.is_empty() {
        eprintln!("No SQL endpoints found, skipping");
        return;
    }
    let ep_id = items[0]["id"].as_str().unwrap();

    fabio()
        .args([
            "sql-endpoint",
            "queries-running",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            ep_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// sql-endpoint queries-frequent
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_queries_frequent() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["sql-endpoint", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if items.is_empty() {
        eprintln!("No SQL endpoints found, skipping");
        return;
    }
    let ep_id = items[0]["id"].as_str().unwrap();

    fabio()
        .args([
            "sql-endpoint",
            "queries-frequent",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            ep_id,
            "--top",
            "5",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// sql-endpoint queries-long-running
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_queries_long_running() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["sql-endpoint", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if items.is_empty() {
        eprintln!("No SQL endpoints found, skipping");
        return;
    }
    let ep_id = items[0]["id"].as_str().unwrap();

    fabio()
        .args([
            "sql-endpoint",
            "queries-long-running",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            ep_id,
            "--top",
            "5",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// sql-endpoint queries-history
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_queries_history() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["sql-endpoint", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if items.is_empty() {
        eprintln!("No SQL endpoints found, skipping");
        return;
    }
    let ep_id = items[0]["id"].as_str().unwrap();

    fabio()
        .args([
            "sql-endpoint",
            "queries-history",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            ep_id,
            "--top",
            "5",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// sql-endpoint pool-insights
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_pool_insights() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["sql-endpoint", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if items.is_empty() {
        eprintln!("No SQL endpoints found, skipping");
        return;
    }
    let ep_id = items[0]["id"].as_str().unwrap();

    let assert = fabio()
        .args([
            "sql-endpoint",
            "pool-insights",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            ep_id,
            "--top",
            "5",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(json.get("data").is_some());
}
