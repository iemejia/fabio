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
fn warehouse_snapshot_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "warehouse-snapshot",
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
fn warehouse_snapshot_dry_run_create() {
    // Offline dry-run: regression guard that the creationPayload uses
    // parentWarehouseId (NOT warehouseId, which the API rejects with
    // NotGenericWarehouseArtifact) and supports snapshotDateTime.
    let assert = fabio()
        .args([
            "--dry-run",
            "warehouse-snapshot",
            "create",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "test-snapshot",
            "--warehouse-id",
            "11111111-1111-1111-1111-111111111111",
            "--snapshot-datetime",
            "2026-01-01T00:00:00Z",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "warehouse-snapshot create");
    let details = &json["data"]["details"];
    assert_eq!(
        details["parentWarehouseId"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(details["snapshotDateTime"], "2026-01-01T00:00:00Z");
}
