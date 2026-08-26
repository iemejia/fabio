use assert_cmd::Command;
use serial_test::serial;

mod common;
use common::TestConfig;

fn fabio() -> Command {
    Command::cargo_bin("fabio").unwrap()
}

// NOTE (live coverage): the typed REST surface for the Google Lakehouse runtime
// catalog is a very new preview and may return NOT_FOUND / InvalidItemType in
// tenants/regions where it has not rolled out yet. The `#[ignore]` live tests
// below therefore assert on the dry-run/validation paths and tolerate a
// NOT_FOUND from `list`/`show` (still a well-formed, correctly-wired response).
// The un-ignored tests are fully offline (dry-run + clap validation).

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn mirrored_google_lakehouse_catalog_list_returns_array_or_not_found() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "mirrored-google-lakehouse-catalog",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert();
    let out = assert.get_output();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert!(json["data"].is_array());
    } else {
        // Endpoint not yet rolled out in this region -> structured NOT_FOUND.
        assert!(
            stderr.contains("NOT_FOUND") || stderr.contains("API_ERROR"),
            "Expected array or structured NOT_FOUND, got: {stderr}"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn mirrored_google_lakehouse_catalog_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "mirrored-google-lakehouse-catalog",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "mgllc_dry_run",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["data"]["would_execute"],
        "mirrored-google-lakehouse-catalog create"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn mirrored_google_lakehouse_catalog_dry_run_delete_is_destructive() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "mirrored-google-lakehouse-catalog",
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
    assert_eq!(
        json["data"]["would_execute"],
        "mirrored-google-lakehouse-catalog delete"
    );
    // Destructive dry-run previews must carry the confirm-with-user signal.
    assert_eq!(json["data"]["destructive"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn mirrored_google_lakehouse_catalog_show_invalid_id_returns_error() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "mirrored-google-lakehouse-catalog",
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

// ─── Offline validation (run in CI without a tenant) ─────────────────────────

#[test]
fn mirrored_google_lakehouse_catalog_update_requires_a_field() {
    // update with neither --name nor --description must fail with a clear error.
    let assert = fabio()
        .args([
            "mirrored-google-lakehouse-catalog",
            "update",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("--name") || stderr.contains("--description"),
        "Expected hint about --name/--description, got: {stderr}"
    );
}

// list-scopes / list-tables require --connection-id (the catalog mirroring
// source is a REQUIRED query param). Offline clap-validation regression.
#[test]
fn mirrored_google_lakehouse_catalog_list_scopes_requires_connection_id() {
    fabio()
        .args([
            "mirrored-google-lakehouse-catalog",
            "list-scopes",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();
}

#[test]
fn mirrored_google_lakehouse_catalog_list_tables_requires_connection_id() {
    fabio()
        .args([
            "mirrored-google-lakehouse-catalog",
            "list-tables",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();
}
