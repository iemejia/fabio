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

/// A `WarehouseSnapshot` exposes its own read-only `properties.connectionString`,
/// so `warehouse query --id <snapshot>` must resolve it as a SQL-queryable item
/// (the resolver previously only recognized Warehouse and Lakehouse types).
/// Gated on a snapshot id so it runs only when a snapshot fixture is provided.
#[test]
#[ignore = "requires live Fabric tenant with a warehouse snapshot"]
#[serial]
fn warehouse_snapshot_is_tsql_queryable() {
    let cfg = TestConfig::from_env();
    let Ok(snapshot_id) = std::env::var("FABIO_TEST_WAREHOUSE_SNAPSHOT_ID") else {
        return; // skip when not configured
    };

    let assert = fabio()
        .args([
            "warehouse",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &snapshot_id,
            "--sql",
            "SELECT 1 AS test",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // A result set (list envelope), NOT the "Could not determine SQL connection" error.
    assert!(json.get("data").is_some());
    assert_eq!(json["data"][0]["test"], 1);
}

/// Self-contained: create a snapshot of the loaded warehouse fixture, exercise the
/// warehouse-snapshot data-plane commands (connection-string, list-tables, query),
/// then delete it. Gated on `FABIO_TEST_LOADED_WAREHOUSE` (a warehouse with tables).
#[test]
#[ignore = "requires live Fabric tenant with a loaded warehouse"]
#[serial]
fn warehouse_snapshot_data_plane_lifecycle() {
    let cfg = TestConfig::from_env();
    let Ok(warehouse_id) = std::env::var("FABIO_TEST_LOADED_WAREHOUSE") else {
        return; // skip when not configured
    };
    let name = format!(
        "fabio_snap_e2e_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );

    // Create the snapshot.
    let assert = fabio()
        .args([
            "warehouse-snapshot",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--warehouse-id",
            &warehouse_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let snap_id = json["data"]["id"].as_str().unwrap().to_string();

    // connection-string resolves the snapshot's read-only SQL endpoint.
    let assert = fabio()
        .args([
            "warehouse-snapshot",
            "connection-string",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &snap_id,
        ])
        .assert()
        .success();
    let json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&assert.get_output().stdout)).unwrap();
    assert!(
        json["data"]["connectionString"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );

    // list-tables returns a result set.
    fabio()
        .args([
            "warehouse-snapshot",
            "list-tables",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &snap_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    // query executes read-only T-SQL against the snapshot.
    let assert = fabio()
        .args([
            "warehouse-snapshot",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &snap_id,
            "--sql",
            "SELECT COUNT(*) AS n FROM INFORMATION_SCHEMA.TABLES",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&assert.get_output().stdout)).unwrap();
    assert!(json["data"][0]["n"].as_i64().unwrap() >= 0);

    // Cleanup.
    fabio()
        .args([
            "warehouse-snapshot",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &snap_id,
        ])
        .assert()
        .success();
}
