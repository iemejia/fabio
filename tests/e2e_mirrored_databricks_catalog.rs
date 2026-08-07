use assert_cmd::Command;
use serial_test::serial;

mod common;
use common::TestConfig;

fn fabio() -> Command {
    Command::cargo_bin("fabio").unwrap()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn mirrored_databricks_catalog_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "mirrored-databricks-catalog",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["data"].is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn mirrored_databricks_catalog_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "mirrored-databricks-catalog",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test_mdc_e2e",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["data"]["would_execute"],
        "mirrored-databricks-catalog create"
    );
}

// discover-catalogs/schemas/tables require --connection-id
// (databricksWorkspaceConnectionId is a REQUIRED query param). Offline
// clap-validation regression.
#[test]
fn discover_catalogs_requires_connection_id() {
    fabio()
        .args([
            "mirrored-databricks-catalog",
            "discover-catalogs",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();
}

#[test]
fn discover_tables_requires_connection_id() {
    fabio()
        .args([
            "mirrored-databricks-catalog",
            "discover-tables",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--catalog-name",
            "cat",
            "--schema-name",
            "sch",
        ])
        .assert()
        .failure();
}
