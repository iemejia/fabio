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
fn digital_twin_builder_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "digital-twin-builder",
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
fn digital_twin_builder_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "digital-twin-builder",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-dtb",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "digital-twin-builder create");
}

/// Offline: dry-run delete with the cascade flag surfaces `deleteLakehouse`.
#[test]
fn digital_twin_builder_dry_run_delete_cascade() {
    let assert = fabio()
        .args([
            "digital-twin-builder",
            "delete",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--delete-lakehouse",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "digital-twin-builder delete");
    assert_eq!(json["data"]["details"]["deleteLakehouse"], true);
}

/// Live: create a DTB, resolve its `dtdm` data lakehouse, run a SQL query against
/// its SQL endpoint, then delete with the lakehouse cascade (no orphan left).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn digital_twin_builder_show_lakehouse_and_query_lifecycle() {
    let cfg = TestConfig::from_env();
    // DTB names allow only letters/numbers/underscores (no hyphens).
    let name = format!(
        "fabio_dtb_e2e_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );

    let assert = fabio()
        .args([
            "digital-twin-builder",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let created: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let id = created["data"]["id"].as_str().unwrap().to_string();

    // show-lakehouse resolves the auto-provisioned <name>dtdm lakehouse + SQL endpoint.
    let assert = fabio()
        .args([
            "digital-twin-builder",
            "show-lakehouse",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let sl: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    let lh_name = sl["data"]["lakehouseName"].as_str().unwrap();
    assert!(
        lh_name.ends_with("dtdm"),
        "unexpected lakehouse name: {lh_name}"
    );
    assert!(
        !sl["data"]["sqlEndpoint"]["connectionString"]
            .as_str()
            .unwrap_or("")
            .is_empty(),
        "expected a SQL endpoint connection string: {sl}"
    );

    // query the twin's data lakehouse SQL endpoint (base-layer schemas always exist).
    let assert = fabio()
        .args([
            "digital-twin-builder",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
            "--sql",
            "SELECT name FROM sys.schemas WHERE name = 'dbo'",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let q: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert!(q["data"].is_array(), "expected a result set: {q}");

    // Cleanup with cascade — the dtdm lakehouse must be removed too.
    let assert = fabio()
        .args([
            "digital-twin-builder",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
            "--delete-lakehouse",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let del: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(
        del["data"]["dataLakehouseDeleted"], true,
        "cascade failed: {del}"
    );
}
