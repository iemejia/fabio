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
