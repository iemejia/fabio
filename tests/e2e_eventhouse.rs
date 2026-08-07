//! End-to-end integration tests for `fabio eventhouse` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventhouse_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["eventhouse", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventhouse_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("eh_test");

    // Create
    let assert = fabio()
        .args([
            "eventhouse",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let eh_id = data["id"].as_str().unwrap().to_string();

    // Delete
    let assert = fabio()
        .args([
            "eventhouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventhouse_update_name() {
    let cfg = TestConfig::from_env();
    let original = common::unique_name("eh_upd_o");
    let updated = common::unique_name("eh_upd_n");

    // Create
    let assert = fabio()
        .args([
            "eventhouse",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &original,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let eh_id = data["id"].as_str().unwrap().to_string();

    // Update
    let assert = fabio()
        .args([
            "eventhouse",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
            "--name",
            &updated,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], updated);

    // Cleanup
    fabio()
        .args([
            "eventhouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventhouse_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "eventhouse",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventhouse_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "eventhouse",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-dry-run",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "eventhouse create");
}

// eventhouse create --min-consumption-units sets
// creationPayload.minimumConsumptionUnits. Offline dry-run regression.
#[test]
fn eventhouse_create_min_consumption_units_in_body() {
    let assert = fabio()
        .args([
            "--dry-run",
            "eventhouse",
            "create",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--name",
            "eh_min",
            "--min-consumption-units",
            "2.25",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(extract_data(&json)["details"]["minConsumptionUnits"], 2.25);
}
