//! `OneLake` discovery must never follow returned links or use the Fabric audience.
mod common;

use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "00000000-0000-0000-0000-000000000001";
const ITEM: &str = "00000000-0000-0000-0000-000000000002";

#[test]
fn endpoints_discovery_metadata_names_storage_scope_and_read_only_contract() {
    let result = common::fabio()
        .args(["context", "describe", "item", "get-endpoints"])
        .assert()
        .success();
    let value = common::parse_json(&result);
    assert_eq!(value["data"]["auth_scope"], "storage");
    assert_eq!(value["data"]["mutates"], false);
    assert!(value["data"]["output_example"].is_object());
}

fn command(server: &MockServer) -> assert_cmd::Command {
    let mut command = common::mock_fabio(server);
    command
        .env("FABIO_NO_VERSION_CHECK", "1")
        .env("FABIO_STORAGE_ACCESS_TOKEN", "storage-test-token")
        .env("FABIO_ONELAKE_TABLE_ENDPOINT", server.uri())
        .args([
            "--readonly",
            "item",
            "get-endpoints",
            "--workspace",
            WS,
            "--id",
            ITEM,
        ]);
    command
}

#[test]
fn endpoints_uses_one_storage_scoped_get_and_never_follows_connections() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server =
        runtime.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
            .and(path(format!("/api/v2/{WS}/{ITEM}/endpoints")))
            .and(header("authorization", "Bearer storage-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "workspace":{"id":WS}, "items":[{"id":ITEM,"platformType":"Lakehouse","engines":[{
                    "name":"OneLake", "recommended":true,
                    "mcpServers":[{"url":format!("{}/must-not-connect",server.uri()),
                        "auth":{"audience":"https://storage.azure.com/"},"tags":["read","write"]}],
                    "skills":{"repository":format!("{}/must-not-fetch",server.uri())}
                }]}]
            })))
            .expect(1).mount(&server).await;
            server
        });
    let result = command(&server).assert().success();
    let json = common::parse_json(&result);
    assert_eq!(json["data"]["items"][0]["id"], ITEM);
    assert_eq!(json["data"]["connectionsVerified"], false);
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        1
    );
}

#[test]
fn endpoints_rejects_invalid_input_without_network() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(MockServer::start());
    let result = common::mock_fabio(&server)
        .env("FABIO_ONELAKE_TABLE_ENDPOINT", server.uri())
        .args([
            "item",
            "get-endpoints",
            "--workspace",
            WS,
            "--id",
            "../other",
        ])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        0
    );
}

#[test]
fn endpoints_not_found_teaches_preview_limit_without_retrying_another_surface() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404).set_body_json(
                json!({"error":{"code":"NotFound","message":"Discovery unavailable"}}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        server
    });
    let result = command(&server).arg("--quiet").assert().failure();
    let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "NOT_FOUND");
    assert!(
        error["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("OneLake integration")
    );
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        1
    );
}

#[test]
fn endpoints_rejects_cross_item_success_response() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"workspace":{"id":WS},"items":[{"id":WS,"engines":[]}]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        server
    });
    let result = command(&server).arg("--quiet").assert().failure();
    let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "API_ERROR");
    assert_eq!(result.get_output().stdout.len(), 0);
}

#[test]
#[ignore = "requires FABIO_TEST_ENDPOINT_WORKSPACE/FABIO_TEST_ENDPOINT_ITEM and live storage credentials; read-only"]
fn endpoints_live_discovery() {
    let workspace =
        std::env::var("FABIO_TEST_ENDPOINT_WORKSPACE").expect("endpoint workspace fixture");
    let item = std::env::var("FABIO_TEST_ENDPOINT_ITEM").expect("endpoint item fixture");
    let output = common::fabio()
        .args([
            "--readonly",
            "--wrap-untrusted",
            "item",
            "get-endpoints",
            "--workspace",
            &workspace,
            "--id",
            &item,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .output()
        .expect("run fabio");
    assert!(
        output.status.success(),
        "live discovery failed; inspect private diagnostics"
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("JSON response");
    assert_eq!(result["data"]["connectionsVerified"], false);
    assert!(
        result["data"]["items"][0]["engines"].is_array(),
        "engines must be an array"
    );
}
