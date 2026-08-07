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
fn mirrored_catalog_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "mirrored-catalog",
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
fn mirrored_catalog_create_requires_definition() {
    let cfg = TestConfig::from_env();
    // Mirrored catalog create without definition should fail with a clear error
    let assert = fabio()
        .args([
            "mirrored-catalog",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "mc_test_no_def",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("MissingDefinition")
            || stderr.contains("requires a definition")
            || stderr.contains("API_ERROR"),
        "Expected error about missing definition, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn mirrored_catalog_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "mirrored-catalog",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "mc_dry_run",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "mirrored-catalog create");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn mirrored_catalog_dry_run_delete() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "mirrored-catalog",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "mirrored-catalog delete");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn mirrored_catalog_show_invalid_id_returns_error() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "mirrored-catalog",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-ffffffffffff",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("NOT_FOUND") || stderr.contains("API_ERROR") || stderr.contains("error"),
        "Expected error for invalid ID, got: {stderr}"
    );
}

// list-scopes / list-tables require --connection-id (the catalog mirroring
// source is a REQUIRED query param). Offline clap-validation regression.
#[test]
fn mirrored_catalog_list_scopes_requires_connection_id() {
    fabio()
        .args([
            "mirrored-catalog",
            "list-scopes",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();
}

#[test]
fn mirrored_catalog_list_tables_requires_connection_id() {
    fabio()
        .args([
            "mirrored-catalog",
            "list-tables",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();
}
