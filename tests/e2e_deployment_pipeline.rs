//! E2E integration tests for the `fabio deployment-pipeline` command group.
//!
//! Tests deployment pipeline CRUD and stage operations.

mod common;

use common::{extract_count, extract_data, fabio, parse_json, unique_name};

#[test]
#[ignore = "requires live Fabric tenant"]
fn deployment_pipeline_list() {
    let output = fabio()
        .args(["deployment-pipeline", "list"])
        .assert()
        .success();

    let json = parse_json(&output);
    // Should return a list envelope
    assert!(json.get("data").is_some());
    assert!(json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn deployment_pipeline_list_with_limit() {
    let output = fabio()
        .args(["deployment-pipeline", "list", "--limit", "2"])
        .assert()
        .success();

    let json = parse_json(&output);
    let count = extract_count(&json);
    assert!(count <= 2);
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn deployment_pipeline_create_dry_run() {
    let name = unique_name("test-dp");

    let output = fabio()
        .args([
            "deployment-pipeline",
            "create",
            "--name",
            &name,
            "--description",
            "E2E test pipeline",
            "--stage",
            "Development",
            "--stage",
            "Production:public",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

// The create API requires a `stages` array; these run offline (guard/validation
// happen before any network call).
#[test]
fn deployment_pipeline_create_includes_stages_in_body() {
    let output = fabio()
        .args([
            "--dry-run",
            "deployment-pipeline",
            "create",
            "--name",
            "offline-dp",
            "--stage",
            "Development",
            "--stage",
            "Production:public",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let stages = data["details"]["stages"]
        .as_array()
        .expect("stages array in dry-run body");
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0]["displayName"], "Development");
    assert_eq!(stages[0]["isPublic"], false);
    assert_eq!(stages[1]["displayName"], "Production");
    assert_eq!(stages[1]["isPublic"], true);
}

#[test]
fn deployment_pipeline_create_requires_stages() {
    // No --stage / --stages-json → must fail before any network call.
    fabio()
        .args(["deployment-pipeline", "create", "--name", "no-stages"])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn deployment_pipeline_update_requires_fields() {
    // Should fail with no --name or --description
    fabio()
        .args([
            "deployment-pipeline",
            "update",
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn deployment_pipeline_delete_dry_run() {
    let output = fabio()
        .args([
            "deployment-pipeline",
            "delete",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn deployment_pipeline_deploy_dry_run() {
    let output = fabio()
        .args([
            "deployment-pipeline",
            "deploy",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--source-stage-id",
            "11111111-1111-1111-1111-111111111111",
            "--note",
            "test deployment",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

#[test]
fn deployment_pipeline_deploy_item_options_dry_run() {
    let source_item = "6bfe235c-6d7b-41b7-98a6-2b8276b3e82b";
    let item_options =
        format!(r#"[{{"sourceItemId":"{source_item}","options":{{"validateOnly":true}}}}]"#);
    let output = fabio()
        .args([
            "deployment-pipeline",
            "deploy",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--source-stage-id",
            "11111111-1111-1111-1111-111111111111",
            "--allow-cross-region-deployment",
            "--item-options",
            &item_options,
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let details = &extract_data(&json)["details"];
    assert_eq!(
        details["options"]["allowCrossRegionDeployment"],
        serde_json::Value::Bool(true)
    );
    assert_eq!(
        details["options"]["itemOptionsBySourceItemId"][0]["sourceItemId"],
        source_item
    );
    assert_eq!(
        details["options"]["itemOptionsBySourceItemId"][0]["options"]["validateOnly"],
        true
    );
}

#[test]
fn deployment_pipeline_deploy_item_options_purge_is_destructive() {
    // A per-item option entry nesting allowPurgeData makes the deploy irreversible;
    // the dry-run must carry the purge warning + destructive signal.
    let source_item = "6bfe235c-6d7b-41b7-98a6-2b8276b3e82b";
    let item_options =
        format!(r#"[{{"sourceItemId":"{source_item}","options":{{"allowPurgeData":true}}}}]"#);
    let output = fabio()
        .args([
            "deployment-pipeline",
            "deploy",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--source-stage-id",
            "11111111-1111-1111-1111-111111111111",
            "--item-options",
            &item_options,
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["destructive"], true);
    assert!(
        data["details"]["warning"]
            .as_str()
            .is_some_and(|w| w.contains("allowPurgeData")),
        "nested-purge deploy preview must warn"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn deployment_pipeline_assign_workspace_dry_run() {
    let output = fabio()
        .args([
            "deployment-pipeline",
            "assign-workspace",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--stage-id",
            "11111111-1111-1111-1111-111111111111",
            "--workspace",
            "22222222-2222-2222-2222-222222222222",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}
