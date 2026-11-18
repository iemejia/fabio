mod common;

use common::{extract_data, fabio, parse_json};

const WORKSPACE: &str = "00000000-0000-0000-0000-000000000001";
const ITEM: &str = "00000000-0000-0000-0000-000000000002";

#[test]
fn test_create_dry_run() {
    let assert = fabio()
        .args([
            "postgresql-database",
            "create",
            "--workspace",
            WORKSPACE,
            "--name",
            "dry-run-postgres",
            "--originating-product",
            "MigrationTool",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "postgresql-database create");
    assert_eq!(
        data["details"]["request"]["creationPayload"]["originatingProduct"],
        "MigrationTool"
    );
}

#[test]
fn test_update_definition_dry_run() {
    let assert = fabio()
        .args([
            "postgresql-database",
            "update-definition",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--content",
            "{}",
            "--allow-table-rebuild",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(
        data["would_execute"],
        "postgresql-database update-definition"
    );
    assert_eq!(
        data["details"]["request"]["options"]["allowTableRebuild"],
        true
    );
}
