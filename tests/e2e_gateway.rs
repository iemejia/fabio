//! E2E integration tests for the `fabio gateway` command group.

mod common;

use common::{fabio, parse_json};

#[test]
#[ignore = "requires live Fabric tenant"]
fn gateway_list_returns_array() {
    let output = fabio().args(["gateway", "list"]).assert().success();

    let json = parse_json(&output);
    let data = json
        .get("data")
        .and_then(|d| d.as_array())
        .expect("data should be array");
    // Structure must be a valid array (may be empty if no gateways exist)
    let _ = data;
}

#[test]
fn gateway_dry_run_create() {
    let assert = fabio()
        .args([
            "--dry-run",
            "gateway",
            "create",
            "--name",
            "test-gw",
            "--capacity-id",
            "00000000-0000-0000-0000-000000000001",
            "--subscription-id",
            "00000000-0000-0000-0000-000000000099",
            "--resource-group",
            "rg",
            "--vnet-name",
            "vnet",
            "--subnet",
            "default",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "gateway create");
}

#[test]
fn gateway_dry_run_create_streaming() {
    let assert = fabio()
        .args([
            "--dry-run",
            "gateway",
            "create-streaming",
            "--name",
            "test-streaming-gw",
            "--subscription-id",
            "00000000-0000-0000-0000-000000000099",
            "--resource-group",
            "rg",
            "--vnet-name",
            "vnet",
            "--subnet",
            "default",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "gateway create-streaming");
}

#[test]
fn gateway_update_member_requires_field() {
    let assert = fabio()
        .args([
            "gateway",
            "update-member",
            "--gateway",
            "00000000-0000-0000-0000-000000000001",
            "--member-id",
            "00000000-0000-0000-0000-000000000002",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("display-name")
            || stderr.contains("enabled")
            || stderr.contains("must be provided")
    );
}

#[test]
fn gateway_add_role_assignment_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "gateway",
            "add-role-assignment",
            "--gateway",
            "00000000-0000-0000-0000-000000000001",
            "--principal-id",
            "00000000-0000-0000-0000-000000000099",
            "--principal-type",
            "User",
            "--role",
            "Admin",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "gateway add-role-assignment");
}

#[test]
fn gateway_dry_run_restart() {
    let assert = fabio()
        .args([
            "--dry-run",
            "gateway",
            "restart",
            "--gateway",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "gateway restart");
}

#[test]
fn gateway_dry_run_shutdown() {
    let assert = fabio()
        .args([
            "--dry-run",
            "gateway",
            "shutdown",
            "--gateway",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "gateway shutdown");
}
