//! End-to-end integration tests for `fabio monitoring` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
fn monitoring_create_dry_run_includes_all_optional_fields() {
    let assert = fabio()
        .args([
            "--dry-run",
            "monitoring",
            "create",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "Monitoring_1",
            "--description",
            "Workspace diagnostics",
            "--folder-id",
            "00000000-0000-0000-0000-000000000001",
            "--sensitivity-label",
            "00000000-0000-0000-0000-000000000002",
            "--content",
            r#"{"operationsAgent":{"enabled":false}}"#,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(json["data"]["would_execute"], "monitoring create");
    assert_eq!(
        json["data"]["details"]["definition"]["parts"][0]["path"],
        "MonitoringProperties.json"
    );
}

#[test]
fn monitoring_update_definition_dry_run_includes_metadata_flag() {
    let assert = fabio()
        .env("COPILOT_CLI", "1")
        .args([
            "--dry-run",
            "monitoring",
            "update-definition",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--content",
            r#"{"operationsAgent":{"enabled":false}}"#,
            "--update-metadata",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(
        json["data"]["would_execute"],
        "monitoring update-definition"
    );
    assert_eq!(json["data"]["details"]["updateMetadata"], true);
    assert_eq!(json["data"]["destructive"], true);
    assert!(json["data"]["agentNotice"].is_string());
}

#[test]
fn monitoring_data_collection_mutations_are_dry_run_guarded() {
    for command in ["enable-data-collection", "disable-data-collection"] {
        let assert = fabio()
            .args([
                "--dry-run",
                "monitoring",
                command,
                "--workspace",
                "00000000-0000-0000-0000-000000000000",
                "--id",
                "00000000-0000-0000-0000-000000000001",
            ])
            .assert()
            .success();
        let json = parse_json(&assert);
        assert_eq!(
            json["data"]["would_execute"],
            format!("monitoring {command}")
        );
    }
}

#[test]
fn monitoring_delete_dry_run_is_destructive() {
    let assert = fabio()
        .env("COPILOT_CLI", "1")
        .args([
            "--dry-run",
            "monitoring",
            "delete",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(json["data"]["would_execute"], "monitoring delete");
}

#[test]
fn monitoring_update_requires_a_field() {
    let assert = fabio()
        .args([
            "monitoring",
            "update",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let error: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
}

#[test]
fn monitoring_create_rejects_invalid_display_name() {
    let assert = fabio()
        .args([
            "--dry-run",
            "monitoring",
            "create",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "invalid name",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let error: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant with Monitoring preview enabled"]
#[serial]
fn monitoring_create_show_and_delete() {
    let config = TestConfig::from_env();
    let name = common::unique_name("monitoring");
    let create = fabio()
        .args([
            "monitoring",
            "create",
            "--workspace",
            &config.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let created = parse_json(&create);
    let id = extract_data(&created)["id"].as_str().unwrap().to_string();

    let show = fabio()
        .args([
            "monitoring",
            "show",
            "--workspace",
            &config.dest_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&show))["type"], "Monitoring");

    fabio()
        .args([
            "monitoring",
            "delete",
            "--workspace",
            &config.dest_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
}
