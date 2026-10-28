mod common;

use common::{TestConfig, extract_data, fabio, parse_json};

const WORKSPACE: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const PLAN: &str = "bbbbbbbb-1111-2222-3333-444444444444";

#[test]
#[ignore = "requires live Fabric tenant"]
fn test_list_live() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "deployment-plan",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    assert!(json["data"].is_array());
    assert!(json["count"].is_number());
}

#[test]
fn test_create_dry_run_serializes_definition() {
    let output = fabio()
        .args([
            "--dry-run",
            "deployment-plan",
            "create",
            "--workspace",
            WORKSPACE,
            "--name",
            "Ordered deployment",
            "--content",
            "executableFormat: DeploymentPlanV1",
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "deployment-plan create");
    assert_eq!(
        data["details"]["request"]["definition"]["parts"][0]["path"],
        "plan.yml"
    );
}

#[test]
fn test_update_definition_dry_run_includes_metadata_flag() {
    let output = fabio()
        .args([
            "--dry-run",
            "deployment-plan",
            "update-definition",
            "--workspace",
            WORKSPACE,
            "--id",
            PLAN,
            "--content",
            "executableFormat: DeploymentPlanV1",
            "--update-metadata",
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "deployment-plan update-definition");
    assert_eq!(data["details"]["updateMetadata"], true);
}

#[test]
fn test_delete_dry_run_is_destructive() {
    let output = fabio()
        .args([
            "--dry-run",
            "deployment-plan",
            "delete",
            "--workspace",
            WORKSPACE,
            "--id",
            PLAN,
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "deployment-plan delete");
    assert_eq!(data["destructive"], true);
}
