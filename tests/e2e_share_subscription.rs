mod common;

use common::{extract_data, fabio, parse_json};

#[test]
fn test_share_subscription_create_dry_run() {
    let assert = fabio()
        .args([
            "share-subscription",
            "create",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--name",
            "dry-run-subscription",
            "--invitation-url",
            "https://app.fabric.microsoft.com/share/invite/example",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share-subscription create");
    assert_eq!(
        data["details"]["request"]["creationPayload"]["invitationUrl"],
        "https://app.fabric.microsoft.com/share/invite/example"
    );
}

#[test]
fn test_share_subscription_delete_dry_run() {
    let assert = fabio()
        .args([
            "share-subscription",
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
    assert_eq!(data["would_execute"], "share-subscription delete");
    assert_eq!(data["dry_run"], true);
}
