//! E2E integration tests for the `fabio capacity` command group.

mod common;

use common::{extract_count, fabio, parse_json};

#[test]
#[ignore = "requires live Fabric tenant"]
fn capacity_list() {
    let output = fabio().args(["capacity", "list"]).assert().success();

    let json = parse_json(&output);
    let data = json
        .get("data")
        .and_then(|d| d.as_array())
        .expect("data should be array");
    assert!(!data.is_empty(), "expected at least one capacity");

    let first = &data[0];
    assert!(first.get("id").is_some());
    assert!(first.get("displayName").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn capacity_list_with_limit() {
    let output = fabio()
        .args(["capacity", "list", "--limit", "1"])
        .assert()
        .success();

    let json = parse_json(&output);
    let count = extract_count(&json);
    assert!(count <= 1);
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn capacity_show() {
    // First list to get a capacity ID
    let list_output = fabio().args(["capacity", "list"]).assert().success();
    let list_json = parse_json(&list_output);
    let capacities = list_json["data"].as_array().expect("data should be array");
    let first_id = capacities[0]["id"].as_str().expect("id should be string");

    let output = fabio()
        .args(["capacity", "show", "--id", first_id])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["id"].as_str().unwrap(), first_id);
}

#[test]
fn capacity_update_surge_protection_dry_run() {
    let output = fabio()
        .args([
            "--dry-run",
            "capacity",
            "update-surge-protection",
            "--id",
            "96f3f0ff-4fe2-4712-b61b-05a456ba9357",
            "--state",
            "Enabled",
            "--rejection-threshold",
            "70",
            "--recovery-threshold",
            "50",
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    assert_eq!(
        json["data"]["would_execute"],
        "capacity update-surge-protection"
    );
    assert_eq!(json["data"]["details"]["state"], "Enabled");
    assert_eq!(json["data"]["details"]["rejectionThreshold"], 70);
    assert_eq!(json["data"]["details"]["recoveryThreshold"], 50);
}

#[test]
fn capacity_update_surge_protection_rejects_threshold_order() {
    fabio()
        .args([
            "--dry-run",
            "capacity",
            "update-surge-protection",
            "--id",
            "96f3f0ff-4fe2-4712-b61b-05a456ba9357",
            "--rejection-threshold",
            "50",
            "--recovery-threshold",
            "60",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant and capacity admin"]
fn capacity_show_surge_protection() {
    let capacity =
        std::env::var("FABIO_TEST_CAPACITY_ID").expect("FABIO_TEST_CAPACITY_ID required");
    let output = fabio()
        .args(["capacity", "show-surge-protection", "--id", &capacity])
        .assert()
        .success();
    let json = parse_json(&output);
    assert!(matches!(
        json["data"]["state"].as_str(),
        Some("Enabled" | "Disabled")
    ));
}

// ── ARM API tests ─────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
fn capacity_suspend_dry_run() {
    let output = fabio()
        .args([
            "capacity",
            "suspend",
            "--subscription",
            "00000000-0000-0000-0000-000000000000",
            "--resource-group",
            "test-rg",
            "--name",
            "testcapacity",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["dry_run"], true);
    assert!(data["would_execute"].as_str().unwrap().contains("suspend"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn capacity_resume_dry_run() {
    let output = fabio()
        .args([
            "capacity",
            "resume",
            "--subscription",
            "00000000-0000-0000-0000-000000000000",
            "--resource-group",
            "test-rg",
            "--name",
            "testcapacity",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["dry_run"], true);
    assert!(data["would_execute"].as_str().unwrap().contains("resume"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn capacity_create_dry_run() {
    let output = fabio()
        .args([
            "capacity",
            "create",
            "--subscription",
            "00000000-0000-0000-0000-000000000000",
            "--resource-group",
            "test-rg",
            "--name",
            "testcapacity",
            "--location",
            "eastus",
            "--sku",
            "F2",
            "--admin",
            "admin@contoso.com",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["dry_run"], true);
    assert!(data["would_execute"].as_str().unwrap().contains("create"));

    // Verify the details contain the expected body structure
    let details = &data["details"];
    assert_eq!(details["location"], "eastus");
    assert_eq!(details["sku"]["name"], "F2");
    assert_eq!(
        details["properties"]["administration"]["members"][0],
        "admin@contoso.com"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn capacity_update_dry_run() {
    let output = fabio()
        .args([
            "capacity",
            "update",
            "--subscription",
            "00000000-0000-0000-0000-000000000000",
            "--resource-group",
            "test-rg",
            "--name",
            "testcapacity",
            "--sku",
            "F4",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["dry_run"], true);
    assert!(data["would_execute"].as_str().unwrap().contains("update"));
    assert_eq!(data["details"]["sku"]["name"], "F4");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn capacity_update_requires_at_least_one_field() {
    fabio()
        .args([
            "capacity",
            "update",
            "--subscription",
            "00000000-0000-0000-0000-000000000000",
            "--resource-group",
            "test-rg",
            "--name",
            "testcapacity",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn capacity_delete_dry_run() {
    let output = fabio()
        .args([
            "capacity",
            "delete",
            "--subscription",
            "00000000-0000-0000-0000-000000000000",
            "--resource-group",
            "test-rg",
            "--name",
            "testcapacity",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = json.get("data").expect("should have data");
    assert_eq!(data["dry_run"], true);
    assert!(data["would_execute"].as_str().unwrap().contains("delete"));
}

#[test]
#[ignore = "requires live Azure subscription"]
fn capacity_list_skus() {
    let subscription =
        std::env::var("FABIO_TEST_SUBSCRIPTION_ID").expect("FABIO_TEST_SUBSCRIPTION_ID required");
    let output = fabio()
        .args(["capacity", "list-skus", "--subscription", &subscription])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = json
        .get("data")
        .and_then(|d| d.as_array())
        .expect("data should be array");
    assert!(!data.is_empty(), "expected at least one SKU");

    let first = &data[0];
    assert!(first.get("name").is_some());
}

#[test]
#[ignore = "requires live Azure subscription"]
fn capacity_check_name_available() {
    let subscription =
        std::env::var("FABIO_TEST_SUBSCRIPTION_ID").expect("FABIO_TEST_SUBSCRIPTION_ID required");
    let output = fabio()
        .args([
            "capacity",
            "check-name",
            "--subscription",
            &subscription,
            "--name",
            "zzztestfabiocapacityxyz123",
            "--location",
            "eastus",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = json.get("data").expect("should have data");
    // Name should be available since it's a random non-existent name
    assert!(data.get("nameAvailable").is_some());
}
