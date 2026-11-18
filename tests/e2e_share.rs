mod common;

use common::{extract_data, fabio, parse_json};

#[test]
fn test_share_create_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "create",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--name",
            "dry-run-share",
            "--definition-content",
            r#"{"assets":[],"recipients":[]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share create");
    assert_eq!(data["dry_run"], true);
}

#[test]
fn test_share_delete_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "delete",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share delete");
    assert_eq!(data["dry_run"], true);
}

#[test]
fn test_share_update_definition_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "update-definition",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            r#"{"assets":[],"recipients":[]}"#,
            "--update-metadata",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share update-definition");
    assert_eq!(data["details"]["updateMetadata"], true);
}

#[test]
fn test_share_initialize_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "initialize",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"assets":[{"item":{"referenceType":"ById","itemId":"00000000-0000-0000-0000-000000000011","workspaceId":"00000000-0000-0000-0000-000000000001"},"shareAs":"MirroredOneLake"}],"recipients":[{"type":"User","email":"recipient@example.com"}],"sharePolicies":[{"type":"ShareExpiration","properties":{"expirationDateTime":"2027-12-31T23:59:59Z"}}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share initialize");
    assert_eq!(
        data["details"]["request"]["assets"][0]["shareAs"],
        "MirroredOneLake"
    );
    assert_eq!(data["details"]["request"]["recipients"][0]["type"], "User");
}

#[test]
fn test_share_bulk_remove_assets_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-remove-assets",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"items":[{"referenceType":"ById","itemId":"00000000-0000-0000-0000-000000000011","workspaceId":"00000000-0000-0000-0000-000000000001"}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-remove-assets");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["destructive"], true);
}

#[test]
fn test_share_bulk_add_assets_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-add-assets",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"assets":[{"item":{"referenceType":"ById","itemId":"00000000-0000-0000-0000-000000000011","workspaceId":"00000000-0000-0000-0000-000000000001"},"shareAs":"Self"}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-add-assets");
    assert_eq!(data["details"]["request"]["assets"][0]["shareAs"], "Self");
}

#[test]
fn test_share_bulk_add_recipients_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-add-recipients",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"recipients":[{"type":"ServicePrincipal","principalId":"00000000-0000-0000-0000-000000000012","tenantId":"00000000-0000-0000-0000-000000000013"}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-add-recipients");
    assert_eq!(
        data["details"]["request"]["recipients"][0]["type"],
        "ServicePrincipal"
    );
}

#[test]
fn test_share_bulk_remove_recipients_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-remove-recipients",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"recipients":[{"type":"User","email":"recipient@example.com"}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-remove-recipients");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["destructive"], true);
}

#[test]
fn test_share_bulk_add_policies_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-add-policies",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"sharePolicies":[{"type":"ShareExpiration","properties":{"expirationDateTime":"2027-12-31T23:59:59Z"}}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-add-policies");
    assert_eq!(
        data["details"]["request"]["sharePolicies"][0]["type"],
        "ShareExpiration"
    );
}

#[test]
fn test_share_update_policy_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "update-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--policy-id",
            "00000000-0000-0000-0000-000000000014",
            "--content",
            r#"{"id":"00000000-0000-0000-0000-000000000014","type":"ShareExpiration","properties":{"expirationDateTime":"2027-12-31T23:59:59Z"}}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share update-policy");
    assert_eq!(
        data["details"]["request"]["properties"]["expirationDateTime"],
        "2027-12-31T23:59:59Z"
    );
    assert_eq!(data["destructive"], true);
}

#[test]
fn test_share_bulk_remove_policies_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-remove-policies",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"sharePolicyIds":["00000000-0000-0000-0000-000000000014"]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-remove-policies");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["destructive"], true);
}
