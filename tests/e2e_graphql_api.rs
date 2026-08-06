//! End-to-end integration tests for `fabio graphql-api` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["graphql-api", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("gql_test");

    // Create
    let assert = fabio()
        .args([
            "graphql-api",
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
            "graphql-api",
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
fn graphql_api_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "graphql-api",
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
fn graphql_api_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "graphql-api",
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
fn graphql_api_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "graphql-api",
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
    assert_eq!(json["data"]["would_execute"], "graphql-api create");
}

// ─── Query tests ─────────────────────────────────────────────────────────────

/// Query the `SalesGraphQL` API (requires it to exist with data source configured)
/// Uses `FABIO_TEST_GRAPHQL_API_ID` and `FABIO_TEST_SOURCE_WORKSPACE`.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_customers() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            "{ customers { items { customer_id email city } } }",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let items = data["customers"]["items"].as_array().unwrap();
    assert!(!items.is_empty());
    // Check first item has expected fields
    assert!(items[0].get("customer_id").is_some());
    assert!(items[0].get("email").is_some());
    assert!(items[0].get("city").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_with_filter() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            r#"{ products(filter: {category: {eq: "Electronics"}}) { items { product_id category price } } }"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let items = data["products"]["items"].as_array().unwrap();
    assert!(!items.is_empty());
    // All returned items should be Electronics
    for item in items {
        assert_eq!(item["category"], "Electronics");
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_from_file() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    // Write query to temp file
    let tmp_file = std::env::temp_dir().join("fabio_test_query.graphql");
    std::fs::write(&tmp_file, "{ products { items { product_id price } } }").unwrap();
    let file_arg = format!("@{}", tmp_file.display());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            &file_arg,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let items = data["products"]["items"].as_array().unwrap();
    assert!(!items.is_empty());

    // Cleanup
    let _ = std::fs::remove_file(&tmp_file);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_from_stdin() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
        ])
        .write_stdin("{ products { items { product_id } } }")
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data["products"]["items"].is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_invalid_field_returns_error() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            "{ nonexistent_type { items { id } } }",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "API_ERROR");
    assert!(
        err_json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not exist")
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--gql",
            "{ __typename }",
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
fn graphql_api_query_products_all_fields() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            "{ products { items { product_id category price } } }",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let items = data["products"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 5);
    // Verify types
    assert!(items[0]["product_id"].is_number());
    assert!(items[0]["category"].is_string());
    assert!(items[0]["price"].is_number());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_filter_by_city() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            r#"{ customers(filter: {city: {eq: "Seattle"}}) { items { customer_id email city } } }"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let items = data["customers"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["city"], "Seattle");
    assert_eq!(items[0]["customer_id"], 1);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_filter_returns_empty() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            r#"{ customers(filter: {city: {eq: "Nonexistent City"}}) { items { customer_id } } }"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let items = data["customers"]["items"].as_array().unwrap();
    assert!(items.is_empty());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_multiple_root_fields() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    // Query both customers and products in one request
    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            "{ customers { items { customer_id } } products { items { product_id } } }",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Both root fields present
    assert!(data["customers"]["items"].is_array());
    assert!(data["products"]["items"].is_array());
    assert!(!data["customers"]["items"].as_array().unwrap().is_empty());
    assert!(!data["products"]["items"].as_array().unwrap().is_empty());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_with_field_projection() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    // Use fabio's --query/-q global option for field projection
    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            "{ products { items { product_id price } } }",
            "-q",
            "products.items",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // Field projection extracts the nested path
    let items = json["data"].as_array().unwrap();
    assert!(!items.is_empty());
    assert!(items[0].get("product_id").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_table_output() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            "{ products { items { product_id category price } } }",
            "-o",
            "table",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // Table output should contain key-value pairs
    assert!(stdout.contains("products"));
    assert!(stdout.contains("Electronics"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_introspection_blocked() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    // Fabric blocks introspection by default
    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            "{ __schema { queryType { name } } }",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "API_ERROR");
    assert!(
        err_json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Introspection")
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_multiline_from_stdin() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let multiline_query = r"{
        customers(filter: {customer_id: {eq: 1}}) {
            items {
                customer_id
                email
                city
            }
        }
    }";

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
        ])
        .write_stdin(multiline_query)
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let items = data["customers"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["customer_id"], 1);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_price_filter_gte() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            r"{ products(filter: {price: {gte: 40}}) { items { product_id price } } }",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let items = data["products"]["items"].as_array().unwrap();
    // products with price >= 40: product_id 2 (49.99), 5 (89.99)
    assert!(!items.is_empty());
    for item in items {
        let price = item["price"].as_f64().unwrap();
        assert!(price >= 40.0, "Expected price >= 40, got {price}");
    }
}

/// Test that --gql flag does not conflict with --quiet
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn graphql_api_query_quiet_mode() {
    let cfg = TestConfig::from_env();
    let graphql_id = std::env::var("FABIO_TEST_GRAPHQL_API_ID")
        .unwrap_or_else(|_| "12310041-f5d0-4578-bf40-7aa461c79868".to_string());

    let assert = fabio()
        .args([
            "graphql-api",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &graphql_id,
            "--gql",
            "{ products { items { product_id } } }",
            "--quiet",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // --quiet suppresses all stdout
    assert!(stdout.is_empty());
}

/// Live: wire a GraphQL API to a Fabric SQL database table and query it.
///
/// This exercises `graphql-api update-definition` with a FULL definition
/// envelope (a `graphql-definition.json` datasource binding) — the path that
/// connects the API to a SQL source. The server auto-generates `schema.graphql`
/// from the bound object, and a GraphQL query then resolves against the
/// mirrored data. Requires an ambient SQL token (Azure CLI) for the TDS steps.
#[test]
#[ignore = "requires live Fabric tenant + SQL token"]
#[serial]
fn graphql_api_wire_to_sql_database_and_query() {
    use std::time::Duration;
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;
    let db_name = common::unique_name("gqlsqldb");

    // 1. Create a SQL database.
    let assert = fabio()
        .args([
            "sql-database",
            "create",
            "--workspace",
            ws,
            "--name",
            &db_name,
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();
    let db_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. Create a keyed table + rows via TDS (GraphQL needs a primary key).
    fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            ws,
            "--id",
            &db_id,
            "--sql",
            "CREATE TABLE dbo.Widget (WidgetID INT PRIMARY KEY, Name NVARCHAR(50), Qty INT);",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();
    fabio()
        .args([
            "sql-database",
            "query",
            "--workspace",
            ws,
            "--id",
            &db_id,
            "--sql",
            "INSERT INTO dbo.Widget VALUES (1,'a',10),(2,'b',20),(3,'c',30);",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    // 3. Find the auto-created SQL analytics endpoint for the SQL database
    //    (it is provisioned asynchronously, so poll for it).
    let mut endpoint_id = String::new();
    for _ in 0..12 {
        let assert = fabio()
            .args(["item", "list", "--workspace", ws, "--type", "SQLEndpoint"])
            .assert()
            .success();
        if let Some(eid) = extract_data(&parse_json(&assert))
            .as_array()
            .and_then(|arr| arr.iter().find(|i| i["displayName"] == db_name.as_str()))
            .and_then(|i| i["id"].as_str())
        {
            endpoint_id = eid.to_string();
            break;
        }
        std::thread::sleep(Duration::from_secs(15));
    }
    assert!(
        !endpoint_id.is_empty(),
        "SQL analytics endpoint for the SQL database was not provisioned in time"
    );

    // 4. Wait for the table schema to mirror to the analytics endpoint.
    let mut synced = false;
    for _ in 0..12 {
        let out = fabio()
            .args([
                "sql-endpoint",
                "query",
                "--workspace",
                ws,
                "--id",
                &endpoint_id,
                "--sql",
                "SELECT COUNT(*) n FROM sys.tables WHERE name='Widget'",
            ])
            .timeout(Duration::from_mins(2))
            .assert();
        let s = String::from_utf8_lossy(&out.get_output().stdout);
        if s.contains("\"n\":1") {
            synced = true;
            break;
        }
        std::thread::sleep(Duration::from_secs(15));
    }
    assert!(
        synced,
        "Widget table did not mirror to the SQL analytics endpoint in time"
    );

    // 5. Create a GraphQL API and wire it to the endpoint's Widget table.
    let name = common::unique_name("gqlwire");
    let assert = fabio()
        .args(["graphql-api", "create", "--workspace", ws, "--name", &name])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();
    let gql_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let dir = tempfile::TempDir::new().unwrap();
    let env_path = dir.path().join("gqlenv.json");
    let envelope = serde_json::json!({
        "definition": { "parts": [{
            "path": "graphql-definition.json",
            "payload": base64_encode(&serde_json::to_vec(&serde_json::json!({
                "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/graphqlApi/definition/1.0.0/schema.json",
                "datasources": [{
                    "sourceItemId": endpoint_id,
                    "sourceWorkspaceId": ws,
                    "sourceType": "SqlAnalyticsEndpoint",
                    "objects": [{
                        "graphqlType": "Widget",
                        "sourceObject": "dbo.Widget",
                        "sourceObjectType": "Table"
                    }]
                }]
            })).unwrap()),
            "payloadType": "InlineBase64"
        }]}
    });
    std::fs::write(&env_path, serde_json::to_vec(&envelope).unwrap()).unwrap();

    fabio()
        .args([
            "graphql-api",
            "update-definition",
            "--workspace",
            ws,
            "--id",
            &gql_id,
            "--file",
            env_path.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    // The definition now carries the datasource binding (proves the envelope
    // passthrough, not a schema.graphql wrap).
    let assert = fabio()
        .args([
            "graphql-api",
            "get-definition",
            "--workspace",
            ws,
            "--id",
            &gql_id,
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();
    let parts = extract_data(&parse_json(&assert))["definition"]["parts"]
        .as_array()
        .unwrap()
        .clone();
    assert!(parts.iter().any(|p| p["path"] == "graphql-definition.json"));

    // 6. Query the GraphQL API (retry until the mirrored data resolves).
    let mut got_items = false;
    for _ in 0..8 {
        let out = fabio()
            .args([
                "graphql-api",
                "query",
                "--workspace",
                ws,
                "--id",
                &gql_id,
                "--gql",
                "query { widgets { items { WidgetID Name Qty } } }",
            ])
            .timeout(Duration::from_mins(2))
            .assert();
        let s = String::from_utf8_lossy(&out.get_output().stdout);
        if s.contains("\"WidgetID\"") {
            got_items = true;
            break;
        }
        std::thread::sleep(Duration::from_secs(15));
    }
    assert!(
        got_items,
        "GraphQL query did not return the mirrored Widget rows"
    );

    // Cleanup.
    fabio()
        .args(["graphql-api", "delete", "--workspace", ws, "--id", &gql_id])
        .assert()
        .success();
    fabio()
        .args(["sql-database", "delete", "--workspace", ws, "--id", &db_id])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
