//! End-to-end integration tests for `fabio connection` commands.

mod common;

use common::{extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_list_returns_json_array() {
    let assert = fabio().args(["connection", "list"]).assert().success();

    let json = parse_json(&assert);
    let data = json
        .get("data")
        .and_then(|d| d.as_array())
        .expect("data should be an array");
    assert!(
        !data.is_empty(),
        "expected at least one connection in tenant"
    );

    let first = &data[0];
    assert!(
        first.get("id").is_some(),
        "each connection should have an id"
    );
    assert!(
        first.get("displayName").is_some(),
        "each connection should have a displayName"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_show_existing() {
    // First get an existing connection ID from list
    let list_assert = fabio().args(["connection", "list"]).assert().success();
    let list_json = parse_json(&list_assert);
    let connections = list_json["data"].as_array().expect("data should be array");
    let first_id = connections[0]["id"].as_str().expect("id should be string");

    // Show that connection
    let assert = fabio()
        .args(["connection", "show", "--id", first_id])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"].as_str().unwrap(), first_id);
    assert!(data.get("displayName").is_some());
    assert!(data.get("connectivityType").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_show_nonexistent_returns_error() {
    let assert = fabio()
        .args([
            "connection",
            "show",
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let json: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be JSON error envelope");
    assert!(
        json.get("error").is_some(),
        "expected error envelope for nonexistent connection"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_create_delete_lifecycle() {
    let name = common::unique_name("conn_test");

    // Create a Web connection with Anonymous auth (skip test to avoid connectivity issues)
    let assert = fabio()
        .args([
            "connection",
            "create",
            "--name",
            &name,
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "Web",
            "--parameters",
            r#"{"url":"https://github.com/iemejia/fabio-test-connection"}"#,
            "--credential-type",
            "Anonymous",
            "--privacy-level",
            "Organizational",
            "--skip-test-connection",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"].as_str().unwrap(), name);
    assert_eq!(data["connectivityType"].as_str().unwrap(), "ShareableCloud");
    assert_eq!(data["connectionDetails"]["type"].as_str().unwrap(), "Web");
    assert_eq!(
        data["credentialDetails"]["credentialType"]
            .as_str()
            .unwrap(),
        "Anonymous"
    );
    let id = data["id"]
        .as_str()
        .expect("created connection should have id");

    // Show the created connection
    let assert = fabio()
        .args(["connection", "show", "--id", id])
        .assert()
        .success();
    let show_json = parse_json(&assert);
    let show_data = extract_data(&show_json);
    assert_eq!(show_data["displayName"].as_str().unwrap(), name);

    // Delete the connection
    let assert = fabio()
        .args(["connection", "delete", "--id", id])
        .assert()
        .success();
    let del_json = parse_json(&assert);
    let del_data = extract_data(&del_json);
    assert_eq!(del_data["status"].as_str().unwrap(), "deleted");

    // Verify it's gone
    fabio()
        .args(["connection", "show", "--id", id])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_create_dry_run() {
    let name = common::unique_name("conn_dry");

    let assert = fabio()
        .args([
            "connection",
            "create",
            "--name",
            &name,
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "Web",
            "--parameters",
            r#"{"url":"https://example.com"}"#,
            "--credential-type",
            "Anonymous",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"].as_str().unwrap(), "dry_run");
    assert!(
        data["message"].as_str().unwrap().contains(&name),
        "dry run message should mention the connection name"
    );

    // Verify nothing was actually created (list should not contain our name)
    let list_assert = fabio().args(["connection", "list"]).assert().success();
    let list_json = parse_json(&list_assert);
    let connections = list_json["data"].as_array().unwrap();
    let found = connections
        .iter()
        .any(|c| c["displayName"].as_str() == Some(&name));
    assert!(!found, "dry run should not actually create the connection");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_update_requires_at_least_one_field() {
    // Should fail when no --name, --privacy-level, or --credential-type provided
    fabio()
        .args([
            "connection",
            "update",
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_list_supported_types() {
    let assert = fabio()
        .args(["connection", "list-supported-types"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = json
        .get("data")
        .and_then(|d| d.as_array())
        .expect("data should be an array");
    assert!(
        !data.is_empty(),
        "expected at least one supported connection type"
    );
}

// ─── Credential Type Validation ─────────────────────────────────────────────

#[test]
fn connection_create_workspace_identity_credential_type_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "connection",
            "create",
            "--name",
            "test-conn",
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "Web",
            "--parameters",
            r#"{"url": "https://example.com"}"#,
            "--credential-type",
            "WorkspaceIdentity",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

#[test]
fn connection_create_keypair_credential_type_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "connection",
            "create",
            "--name",
            "test-conn",
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "Snowflake",
            "--parameters",
            r#"{"server": "acct.snowflakecomputing.com"}"#,
            "--credential-type",
            "KeyPair",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

#[test]
fn connection_create_invalid_credential_type_rejected() {
    fabio()
        .args([
            "connection",
            "create",
            "--name",
            "test-conn",
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "Web",
            "--parameters",
            r#"{"url": "https://example.com"}"#,
            "--credential-type",
            "InvalidType",
        ])
        .assert()
        .failure();
}

#[test]
fn connection_create_streaming_virtual_network_gateway_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "connection",
            "create",
            "--name",
            "test-streaming-vng-conn",
            "--connectivity-type",
            "StreamingVirtualNetworkGateway",
            "--connection-type",
            "SQL",
            "--parameters",
            r#"{"server": "contoso.database.windows.net", "database": "sales"}"#,
            "--gateway-id",
            "93491300-cfbd-402f-bf17-9ace59a92354",
            "--credential-type",
            "Basic",
            "--credentials",
            r#"{"username": "admin", "password": "secret"}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

/// Live test that verifies the `GATEWAY ID` column is shown in table output
/// when at least one connection in the tenant has a non-null `gatewayId`, and
/// omitted otherwise. Covers both branches of the conditional column logic.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_list_table_output_gateway_id_column() {
    // First, get the JSON list to know whether any connection has a gatewayId.
    let json_assert = fabio().args(["connection", "list"]).assert().success();
    let json = parse_json(&json_assert);
    let data = json
        .get("data")
        .and_then(|d| d.as_array())
        .expect("data should be an array");

    let any_gateway_id = data.iter().any(|item| {
        item.get("gatewayId")
            .is_some_and(|v| !v.is_null() && v.as_str().is_some_and(|s| !s.is_empty()))
    });

    // Now get the table output.
    let table_assert = fabio()
        .args(["connection", "list", "--output", "table"])
        .assert()
        .success();
    let table_stdout = String::from_utf8_lossy(&table_assert.get_output().stdout);

    if any_gateway_id {
        assert!(
            table_stdout.contains("GATEWAY ID"),
            "expected 'GATEWAY ID' column in table output when at least one connection has gatewayId, got:\n{table_stdout}"
        );
    } else {
        assert!(
            !table_stdout.contains("GATEWAY ID"),
            "unexpected 'GATEWAY ID' column in table output when no connection has gatewayId, got:\n{table_stdout}"
        );
    }
}

#[test]
fn connection_create_virtual_network_gateway_requires_gateway_id() {
    fabio()
        .args([
            "--dry-run",
            "connection",
            "create",
            "--name",
            "test-vng-conn",
            "--connectivity-type",
            "VirtualNetworkGateway",
            "--connection-type",
            "SQL",
            "--parameters",
            r#"{"server": "contoso.database.windows.net"}"#,
            "--credential-type",
            "Basic",
        ])
        .assert()
        .failure();
}

#[test]
fn connection_create_streaming_virtual_network_gateway_requires_gateway_id() {
    fabio()
        .args([
            "--dry-run",
            "connection",
            "create",
            "--name",
            "test-streaming-vng-conn",
            "--connectivity-type",
            "StreamingVirtualNetworkGateway",
            "--connection-type",
            "SQL",
            "--parameters",
            r#"{"server": "contoso.database.windows.net"}"#,
            "--credential-type",
            "Basic",
        ])
        .assert()
        .failure();
}
