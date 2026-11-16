//! Blank dashboard authoring is offline; publication remains an explicit separate mutation.
mod common;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use wiremock::MockServer;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scaffold_is_offline_under_readonly_and_roundtrips_into_explicit_preview() {
    let server = MockServer::start().await;
    let result = common::mock_fabio(&server)
        .args([
            "--readonly",
            "kql-dashboard",
            "scaffold",
            "--title",
            "Synthetic overview",
        ])
        .assert()
        .success();
    let value = common::parse_json(&result);
    assert_eq!(value["data"]["scaffoldOnly"], true);
    assert_eq!(value["data"]["renderingVerified"], false);
    let part = &value["data"]["definition"]["parts"][0];
    let dashboard: Value =
        serde_json::from_slice(&BASE64.decode(part["payload"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(dashboard["title"], "Synthetic overview");
    assert_eq!(dashboard["tiles"], json!([]));
    let output = common::mock_fabio(&server)
        .args([
            "--readonly",
            "--dry-run",
            "kql-dashboard",
            "update-definition",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000002",
            "--content",
            &value["data"].to_string(),
        ])
        .assert()
        .success();
    let preview = common::parse_json(&output);
    assert_eq!(preview["data"]["dry_run"], true);
    assert_eq!(preview["data"]["destructive"], true);
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[test]
fn empty_title_fails_without_emitting_a_definition() {
    let output = common::fabio()
        .args(["--quiet", "kql-dashboard", "scaffold", "--title", " "])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
    assert_eq!(output.stdout.len(), 0);
}

#[test]
fn scaffold_introspection_is_explicitly_non_mutating() {
    let result = common::fabio()
        .args(["context", "describe", "kql-dashboard", "scaffold"])
        .assert()
        .success();
    let value = common::parse_json(&result);
    assert_eq!(value["data"]["mutates"], false);
    assert!(value["data"]["output_example"].is_object());
}

#[test]
#[ignore = "requires FABIO_TEST_DASHBOARD_ID in owned disposable workspace; read-only persisted scaffold observation"]
fn live_dashboard_definition_contains_the_scaffolded_page_and_no_tiles() {
    let ws = std::env::var("FABIO_BLOG_OWNED_WORKSPACE").expect("owned workspace");
    let id = std::env::var("FABIO_TEST_DASHBOARD_ID").expect("disposable dashboard");
    let output = common::fabio()
        .args([
            "--readonly",
            "kql-dashboard",
            "get-definition",
            "--workspace",
            &ws,
            "--id",
            &id,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "live dashboard read failed; inspect privately"
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON response");
    let part = value["data"]["definition"]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["path"] == "RealTimeDashboard.json")
        .expect("canonical dashboard part");
    let dashboard: Value =
        serde_json::from_slice(&BASE64.decode(part["payload"].as_str().unwrap()).unwrap()).unwrap();
    assert!(
        dashboard["pages"]
            .as_array()
            .is_some_and(|pages| pages.len() == 1),
        "expected scaffold page"
    );
    assert!(
        dashboard["tiles"].as_array().is_some_and(Vec::is_empty),
        "unexpected tiles in fixture"
    );
    assert_eq!(dashboard["autoRefresh"]["enabled"], false);
}
