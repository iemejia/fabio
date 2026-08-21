//! End-to-end integration tests for `fabio sql-endpoint` commands.

mod common;

use common::{TestConfig, extract_count, extract_data, fabio, parse_json};
use serial_test::serial;
use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

// ─── Offline validation guards (no live tenant required) ─────────────────────
//
// The selective refresh-metadata request is validated client-side BEFORE any
// network call, so these guards fire hermetically with dummy IDs and run in CI.

/// Parse the first JSON error object from stderr.
fn refresh_metadata_error(args: &[&str]) -> serde_json::Value {
    let mut full = vec![
        "sql-endpoint",
        "refresh-metadata",
        "--workspace",
        "00000000-0000-0000-0000-000000000000",
        "--id",
        "00000000-0000-0000-0000-000000000000",
    ];
    full.extend_from_slice(args);

    let assert = fabio().args(&full).assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    stderr
        .lines()
        .find(|line| line.starts_with('{'))
        .map_or_else(
            || panic!("No JSON error in stderr: {stderr}"),
            |line| serde_json::from_str(line).expect("parse stderr JSON"),
        )
}

#[test]
fn refresh_metadata_rejects_reserved_schema() {
    let err =
        refresh_metadata_error(&["--tables", r#"[{"schema":"sys","tableNames":["objects"]}]"#]);
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(
        err["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("reserved system schema")),
        "unexpected message: {err}"
    );
}

#[test]
fn refresh_metadata_rejects_reserved_information_schema() {
    let err = refresh_metadata_error(&[
        "--tables",
        r#"[{"schema":"INFORMATION_SCHEMA","tableNames":["TABLES"]}]"#,
    ]);
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(
        err["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("reserved system schema")),
        "unexpected message: {err}"
    );
}

#[test]
fn refresh_metadata_rejects_empty_table_names() {
    let err = refresh_metadata_error(&["--tables", r#"[{"schema":"dbo","tableNames":[]}]"#]);
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(
        err["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("at least one table")),
        "unexpected message: {err}"
    );
}

#[test]
fn refresh_metadata_rejects_invalid_timeout_time_unit() {
    let err = refresh_metadata_error(&["--timeout", r#"{"value":5,"timeUnit":"Weeks"}"#]);
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(
        err["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("Invalid timeout timeUnit")),
        "unexpected message: {err}"
    );
}

#[test]
fn refresh_metadata_rejects_non_positive_timeout() {
    let err = refresh_metadata_error(&["--timeout", r#"{"value":0,"timeUnit":"Minutes"}"#]);
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(
        err["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("positive finite")),
        "unexpected message: {err}"
    );
}

#[test]
fn refresh_metadata_rejects_malformed_tables_json() {
    let err = refresh_metadata_error(&["--tables", "not-json"]);
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
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

// Selective refresh of a non-existent table: the API returns a per-table row
// with status "Failure" and an `error` object carrying `errorCode`
// "DeltaTableNotFound". Agents rely on this shape to filter failures
// (e.g. --query "[?status!='Success']"). The command still exits 0 because the
// LRO itself succeeded; failures are per-row.
#[test]
#[ignore = "requires live Fabric tenant with a lakehouse SQL endpoint"]
#[serial]
fn sql_endpoint_refresh_metadata_selective_missing_table_reports_failure_row() {
    let cfg = TestConfig::from_env();
    let Ok(endpoint_id) = std::env::var("FABIO_TEST_SQL_ENDPOINT_ID") else {
        return; // skip when not configured
    };
    let schema =
        std::env::var("FABIO_TEST_SQL_ENDPOINT_SCHEMA").unwrap_or_else(|_| "dbo".to_string());
    // A table name that is extremely unlikely to exist in the endpoint.
    let tables =
        serde_json::json!([{ "schema": schema, "tableNames": ["fabio_nonexistent_table_zzz"] }])
            .to_string();

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
    assert_eq!(
        rows[0]["status"], "Failure",
        "missing table must report a Failure row: {}",
        rows[0]
    );
    assert_eq!(
        rows[0]["error"]["errorCode"], "DeltaTableNotFound",
        "missing table must carry a DeltaTableNotFound error: {}",
        rows[0]
    );
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

/// Hermetic (mock-server) test for `sql-endpoint mcp-url`: an existing SQL
/// analytics endpoint yields the item-scoped + global remote MCP server URLs
/// (`.../items/{id}/sqlEndpoint`) with `exists=true`; a missing one still emits
/// the deterministic URLs with `exists=false` + hint. No live tenant required.
#[test]
fn sql_endpoint_mcp_url_mocked_exists_and_missing() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let workspace = "00000000-0000-0000-0000-000000000001";
    let present = "00000000-0000-0000-0000-0000000000cc";
    let missing = "00000000-0000-0000-0000-0000000000dd";

    let (server_uri, _server) = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/workspaces/{workspace}/sqlEndpoints/{present}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": present,
                "displayName": "MockSqlEp"
            })))
            .mount(&server)
            .await;
        (server.uri(), server)
    });

    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args([
            "sql-endpoint",
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
    assert_eq!(data["exists"], true);
    assert!(data["hint"].is_null());

    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args([
            "sql-endpoint",
            "mcp-url",
            "--workspace",
            workspace,
            "--id",
            missing,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    assert_eq!(data["exists"], false);
    assert!(!data["hint"].as_str().unwrap().is_empty());
}

/// Live test: schema discovery over a SQL analytics endpoint. Picks the first
/// SQL endpoint in the source workspace, asserts `list-tables` returns a list
/// (system views are always present) and `describe-table` on a queryinsights
/// view returns its columns.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_schema_discovery_lifecycle() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["sql-endpoint", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let items = extract_data(&parse_json(&assert))
        .as_array()
        .unwrap()
        .clone();
    let Some(ep_id) = items.first().and_then(|e| e["id"].as_str()) else {
        eprintln!("No SQL endpoint in source workspace; skipping schema-discovery test");
        return;
    };

    // list-tables returns a list envelope (queryinsights/sys views always exist).
    let assert = fabio()
        .args([
            "sql-endpoint",
            "list-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            ep_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(extract_data(&json).is_array());
    assert!(json["count"].as_u64().unwrap() >= 1);

    // describe-table on a known system view returns ordered columns.
    let assert = fabio()
        .args([
            "sql-endpoint",
            "describe-table",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            ep_id,
            "--table",
            "queryinsights.long_running_queries",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let cols = extract_data(&parse_json(&assert))
        .as_array()
        .unwrap()
        .clone();
    assert!(!cols.is_empty());
    assert_eq!(cols[0]["ORDINAL_POSITION"], 1);
    assert!(cols.iter().all(|c| c["COLUMN_NAME"].is_string()));
}

/// Live test: `sql-endpoint query --via-mcp` runs over the remote Fabric DW MCP
/// server and returns the list envelope (values as strings — CSV is untyped).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn sql_endpoint_query_via_mcp() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["sql-endpoint", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();
    let items = extract_data(&parse_json(&assert))
        .as_array()
        .unwrap()
        .clone();
    let Some(ep_id) = items.first().and_then(|e| e["id"].as_str()) else {
        eprintln!("No SQL endpoint in source workspace; skipping --via-mcp test");
        return;
    };

    let assert = fabio()
        .args([
            "sql-endpoint",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            ep_id,
            "--via-mcp",
            "--sql",
            "SELECT TOP 2 TABLE_SCHEMA, TABLE_NAME FROM INFORMATION_SCHEMA.TABLES ORDER BY TABLE_NAME",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(extract_data(&json).is_array());
    // Result columns are surfaced as object keys, matching the native-TDS shape.
    if let Some(first) = extract_data(&json).as_array().unwrap().first() {
        assert!(first["TABLE_NAME"].is_string());
    }
}
