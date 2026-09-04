//! End-to-end integration tests for `fabio connection` commands.

mod common;

use common::{extract_data, fabio, parse_json};
use serial_test::serial;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

/// Regression for 6158aad: `connection update` must include the required
/// `connectivityType` discriminator (resolved via a GET of the connection),
/// otherwise the API rejects the PATCH with `InvalidInput`. Gated on
/// `FABIO_TEST_CONNECTION_ID` (an existing connection to update in place).
#[test]
#[ignore = "requires live Fabric tenant + FABIO_TEST_CONNECTION_ID"]
#[serial]
fn connection_update_includes_connectivity_type() {
    let Ok(conn_id) = std::env::var("FABIO_TEST_CONNECTION_ID") else {
        return; // skip when not configured
    };
    // Setting privacy-level to None is an in-place no-op update. It succeeds ONLY
    // if fabio resolves and includes the connectivityType discriminator; the old
    // flat body was rejected with InvalidInput.
    let assert = fabio()
        .args([
            "connection",
            "update",
            "--id",
            &conn_id,
            "--privacy-level",
            "None",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(json.get("data").is_some());
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

// ── GATEWAY ID dynamic column (offline, wiremock) ────────────────────────────

/// Verifies that `connection list --output table` shows a `GATEWAY ID` column
/// when the API response contains at least one connection with a non-null,
/// non-empty `gatewayId`.
#[test]
fn connection_list_table_shows_gateway_id_column_when_connections_have_gateway_id() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    // Start mock server and register the `/connections` route.
    let (server_uri, _server) = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/connections"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    {
                        "id": "conn-1",
                        "displayName": "VNet Conn",
                        "connectivityType": "VirtualNetworkGateway",
                        "gatewayId": "gw-abc123"
                    },
                    {
                        "id": "conn-2",
                        "displayName": "ShareableCloud Conn",
                        "connectivityType": "ShareableCloud"
                    }
                ]
            })))
            .mount(&server)
            .await;
        let uri = server.uri();
        (uri, server)
    });

    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args(["connection", "list", "--output", "table"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("GATEWAY ID"),
        "expected 'GATEWAY ID' column header when at least one connection has a non-null gatewayId, got:\n{stdout}"
    );
    assert!(
        stdout.contains("gw-abc123"),
        "expected gatewayId value in table output, got:\n{stdout}"
    );
}

/// Verifies that `connection list --output table` shows the recency columns
/// (`LAST BOUND`, `LAST USED`) when the API returns the `connectionRecency`
/// object, and omits them otherwise.
#[test]
fn connection_list_table_shows_recency_columns_when_present() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let (server_uri, _server) = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/connections"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    {
                        "id": "conn-1",
                        "displayName": "Recent Conn",
                        "connectivityType": "ShareableCloud",
                        "connectionRecency": {
                            "createdDateTime": "2026-06-01T00:00:00Z",
                            "lastBoundDateTime": "2026-06-02T00:00:00Z",
                            "lastCredentialUsedDateTime": "2026-06-03T00:00:00Z"
                        }
                    }
                ]
            })))
            .mount(&server)
            .await;
        let uri = server.uri();
        (uri, server)
    });

    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args(["connection", "list", "--output", "table"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("LAST BOUND") && stdout.contains("LAST USED"),
        "expected recency columns when connectionRecency is present, got:\n{stdout}"
    );
}

/// `connection find-stale` flags never-bound connections created after the
/// cutoff, and does NOT flag active connections or pre-cutoff connections.
#[test]
fn connection_find_stale_flags_never_bound_offline() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let (server_uri, _server) = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/connections"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    {
                        "id": "stale-1",
                        "displayName": "NeverBound",
                        "connectivityType": "ShareableCloud",
                        "connectionRecency": { "createdDateTime": "2026-06-01T00:00:00Z" }
                    },
                    {
                        "id": "active-1",
                        "displayName": "Active",
                        "connectivityType": "ShareableCloud",
                        "connectionRecency": {
                            "createdDateTime": "2026-06-01T00:00:00Z",
                            "lastBoundDateTime": "2026-06-02T00:00:00Z",
                            "lastCredentialUsedDateTime": "2999-01-01T00:00:00Z"
                        }
                    },
                    {
                        "id": "old-1",
                        "displayName": "PreCutoff",
                        "connectivityType": "ShareableCloud",
                        "connectionRecency": { "createdDateTime": "2026-01-01T00:00:00Z" }
                    }
                ]
            })))
            .mount(&server)
            .await;
        let uri = server.uri();
        (uri, server)
    });

    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args(["connection", "find-stale"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = json["data"].as_array().expect("data array");
    assert_eq!(data.len(), 1, "only the never-bound connection is flagged");
    assert_eq!(data[0]["id"], "stale-1");
    assert_eq!(data[0]["reason"], "never-bound");
}

/// `connection find-duplicates` keeps the most-recently-used connection and
/// reports the others as consolidation candidates pointing at the keeper.
#[test]
fn connection_find_duplicates_offline() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let (server_uri, _server) = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/connections"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    {
                        "id": "dup-old",
                        "displayName": "Old",
                        "connectivityType": "ShareableCloud",
                        "connectionDetails": { "type": "SQL", "path": "srv;db" },
                        "connectionRecency": { "createdDateTime": "2026-06-01T00:00:00Z", "lastCredentialUsedDateTime": "2026-06-10T00:00:00Z" }
                    },
                    {
                        "id": "dup-new",
                        "displayName": "New",
                        "connectivityType": "ShareableCloud",
                        "connectionDetails": { "type": "SQL", "path": "srv;db" },
                        "connectionRecency": { "createdDateTime": "2026-06-01T00:00:00Z", "lastCredentialUsedDateTime": "2026-08-10T00:00:00Z" }
                    }
                ]
            })))
            .mount(&server)
            .await;
        let uri = server.uri();
        (uri, server)
    });

    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args(["connection", "find-duplicates"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = json["data"].as_array().expect("data array");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], "dup-old");
    assert_eq!(data[0]["keepId"], "dup-new");
}

/// `connection find-single-owner` flags a connection whose only Owner is an
/// individual user (issuing a per-connection roleAssignments read).
#[test]
fn connection_find_single_owner_offline() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let (server_uri, _server) = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/connections"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    { "id": "own-1", "displayName": "SingleOwner", "connectivityType": "ShareableCloud" }
                ]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/connections/own-1/roleAssignments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    { "id": "ra-1", "role": "Owner", "principal": { "id": "user-abc", "type": "User" } }
                ]
            })))
            .mount(&server)
            .await;
        let uri = server.uri();
        (uri, server)
    });

    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args(["connection", "find-single-owner"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = json["data"].as_array().expect("data array");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], "own-1");
    assert_eq!(data[0]["ownerPrincipalId"], "user-abc");
}

/// Verifies that `connection list --output table` omits the `GATEWAY ID` column
/// when no connection in the API response has a non-null/non-empty `gatewayId`.
#[test]
fn connection_list_table_omits_gateway_id_column_when_no_connections_have_gateway_id() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let (server_uri, _server) = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/connections"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    {
                        "id": "conn-1",
                        "displayName": "ShareableCloud Conn",
                        "connectivityType": "ShareableCloud"
                    },
                    {
                        "id": "conn-2",
                        "displayName": "Another Conn",
                        "connectivityType": "ShareableCloud",
                        "gatewayId": null
                    }
                ]
            })))
            .mount(&server)
            .await;
        let uri = server.uri();
        (uri, server)
    });

    let assert = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args(["connection", "list", "--output", "table"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        !stdout.contains("GATEWAY ID"),
        "unexpected 'GATEWAY ID' column header when no connection has a non-null gatewayId, got:\n{stdout}"
    );
}

// ─── Creation method (connectionDetails.creationMethod) ──────────────────────

#[test]
fn connection_create_dry_run_shows_creation_method_override() {
    // Many connection types have a creationMethod that differs from the type name,
    // e.g. EventHub -> EventHub.Contents. The dry-run preview must reflect the override.
    let assert = fabio()
        .args([
            "connection",
            "create",
            "--name",
            "eh-conn",
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "EventHub",
            "--creation-method",
            "EventHub.Contents",
            "--parameters",
            r#"{"endpoint":"sb://x.servicebus.windows.net","entityPath":"h"}"#,
            "--credential-type",
            "WorkspaceIdentity",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["connectionType"], "EventHub");
    assert_eq!(data["creationMethod"], "EventHub.Contents");
}

#[test]
fn connection_create_dry_run_creation_method_defaults_to_type() {
    let assert = fabio()
        .args([
            "connection",
            "create",
            "--name",
            "sql-conn",
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "SQL",
            "--parameters",
            r#"{"server":"s","database":"d"}"#,
            "--credential-type",
            "Basic",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    // When --creation-method is omitted it defaults to the connection type.
    assert_eq!(data["creationMethod"], "SQL");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_creation_method_accepted_by_api() {
    // Proves the creationMethod handling at the API-contract level without needing a
    // real Event Hub. EventHub's creation method is EventHub.Contents.
    //
    // (1) An EXPLICITLY wrong method (the type name) is rejected by the API.
    // (2) Auto-resolution (no --creation-method) picks EventHub.Contents, so the
    //     connection details are accepted and the API proceeds to test the identity.
    let bad_name = common::unique_name("eh_badcm");
    let bad = fabio()
        .args([
            "connection",
            "create",
            "--name",
            &bad_name,
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "EventHub",
            "--creation-method",
            "EventHub", // wrong on purpose (real method is EventHub.Contents)
            "--parameters",
            r#"{"endpoint":"sb://example.servicebus.windows.net","entityPath":"h"}"#,
            "--credential-type",
            "WorkspaceIdentity",
        ])
        .assert()
        .failure();
    let bad_err = String::from_utf8_lossy(&bad.get_output().stdout)
        + String::from_utf8_lossy(&bad.get_output().stderr);
    assert!(
        bad_err.contains("InvalidConnectionDetails"),
        "an explicitly wrong creation method should be rejected, got: {bad_err}"
    );

    // Auto-resolution: omit --creation-method; fabio resolves EventHub -> EventHub.Contents.
    let auto_name = common::unique_name("eh_autocm");
    let good = fabio()
        .args([
            "connection",
            "create",
            "--name",
            &auto_name,
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "EventHub",
            "--parameters",
            r#"{"endpoint":"sb://example.servicebus.windows.net","entityPath":"h"}"#,
            "--credential-type",
            "WorkspaceIdentity",
        ])
        .assert()
        .failure();
    let good_err = String::from_utf8_lossy(&good.get_output().stdout)
        + String::from_utf8_lossy(&good.get_output().stderr);
    // Auto-resolved method makes the connection details valid; the failure is now the
    // unreachable fake endpoint, NOT invalid connection details.
    assert!(
        !good_err.contains("InvalidConnectionDetails"),
        "auto-resolution should pick EventHub.Contents so the details are accepted, got: {good_err}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_creation_method_ambiguous_type_teaches() {
    // AzureDataExplorer has multiple creation methods; without --creation-method fabio
    // must fail fast and enumerate the valid values instead of guessing.
    let name = common::unique_name("adx_ambig");
    let assert = fabio()
        .args([
            "connection",
            "create",
            "--name",
            &name,
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "AzureDataExplorer",
            "--parameters",
            r#"{"cluster":"https://example.kusto.windows.net"}"#,
            "--credential-type",
            "OAuth2",
        ])
        .assert()
        .failure();
    let err = String::from_utf8_lossy(&assert.get_output().stdout)
        + String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        err.contains("multiple creation methods")
            && err.contains("AzureDataExplorer.Contents")
            && err.contains("AzureDataExplorer.KqlDatabase"),
        "ambiguous type should enumerate valid creation methods, got: {err}"
    );
}

// ─── Code-first (allowUsageInUserControlledCode) toggle ──────────────────────

#[test]
fn connection_create_dry_run_shows_code_first_flags() {
    let assert = fabio()
        .args([
            "connection",
            "create",
            "--name",
            "cf-conn",
            "--connectivity-type",
            "ShareableCloud",
            "--connection-type",
            "Web",
            "--parameters",
            r#"{"url":"https://example.com"}"#,
            "--credential-type",
            "Anonymous",
            "--allow-code-first-artifacts",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["allowUsageInUserControlledCode"], true);
    // Not requested -> false.
    assert_eq!(data["allowConnectionUsageInGateway"], false);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn connection_allow_code_first_artifacts_lifecycle() {
    let name = common::unique_name("conn_codefirst");

    // Create a Web connection with the code-first toggle enabled.
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
            "--skip-test-connection",
            "--allow-code-first-artifacts",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let id = data["id"]
        .as_str()
        .expect("created connection id")
        .to_string();

    // The connection must report allowUsageInUserControlledCode = true.
    let assert = fabio()
        .args(["connection", "show", "--id", &id])
        .assert()
        .success();
    let show = parse_json(&assert);
    let show_data = extract_data(&show);
    assert_eq!(
        show_data["allowUsageInUserControlledCode"], true,
        "connection created with --allow-code-first-artifacts should report the flag, got: {show_data}"
    );

    // Clean up.
    fabio()
        .args(["connection", "delete", "--id", &id])
        .assert()
        .success();
}
