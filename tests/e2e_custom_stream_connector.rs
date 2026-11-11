mod common;

use common::{extract_data, fabio, parse_json};

#[test]
fn test_custom_stream_connector_create_dry_run() {
    let assert = fabio()
        .args([
            "custom-stream-connector",
            "create",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--name",
            "dry-run-connector",
            "--definition-content",
            r#"{"connectorDescription":"test","defaultVersion":"v1"}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "custom-stream-connector create");
    assert_eq!(data["dry_run"], true);
}

#[test]
fn test_custom_stream_connector_delete_dry_run() {
    let assert = fabio()
        .args([
            "custom-stream-connector",
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
    assert_eq!(data["would_execute"], "custom-stream-connector delete");
    assert_eq!(data["dry_run"], true);
}

#[test]
fn test_custom_stream_connector_update_definition_dry_run() {
    let assert = fabio()
        .args([
            "custom-stream-connector",
            "update-definition",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            r#"{"connectorDescription":"test","defaultVersion":"v1"}"#,
            "--update-metadata",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(
        data["would_execute"],
        "custom-stream-connector update-definition"
    );
    assert_eq!(data["details"]["updateMetadata"], true);
}
