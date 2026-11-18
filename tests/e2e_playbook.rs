mod common;

use common::{extract_data, fabio, parse_json};

const WORKSPACE: &str = "00000000-0000-0000-0000-000000000001";
const ITEM: &str = "00000000-0000-0000-0000-000000000002";

#[test]
fn test_create_dry_run() {
    let assert = fabio()
        .args([
            "playbook",
            "create",
            "--workspace",
            WORKSPACE,
            "--name",
            "dry-run-playbook",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "playbook create");
    assert_eq!(data["dry_run"], true);
}

#[test]
fn test_delete_section_rejects_root_path() {
    fabio()
        .args([
            "playbook",
            "delete-section",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--path",
            "/",
            "--recursive",
            "--dry-run",
        ])
        .assert()
        .failure();
}

#[test]
fn test_put_context_dry_run() {
    let assert = fabio()
        .args([
            "playbook",
            "put-context",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--path",
            "Sales/_context/overview",
            "--summary",
            "Sales context",
            "--content-type",
            "text/markdown",
            "--data",
            "IyBTYWxlcw==",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "playbook put-context");
    assert_eq!(
        data["details"]["request"]["content"]["type"],
        "text/markdown"
    );
}
