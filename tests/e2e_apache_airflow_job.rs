use assert_cmd::Command;
use serial_test::serial;

mod common;
use common::TestConfig;

fn fabio() -> Command {
    Command::cargo_bin("fabio").unwrap()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn airflow_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "apache-airflow-job",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["data"].is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn airflow_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "apache-airflow-job",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-dag",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "apache-airflow-job create");
}

#[test]
fn airflow_update_compute_dry_run() {
    fabio()
        .args([
            "apache-airflow-job",
            "update-compute",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--pool-template-id",
            "00000000-0000-0000-0000-000000000002",
            "--dry-run",
        ])
        .assert()
        .success();
}

#[test]
fn airflow_update_compute_missing_pool_template_id_fails() {
    fabio()
        .args([
            "apache-airflow-job",
            "update-compute",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            // missing --pool-template-id
        ])
        .assert()
        .failure();
}

/// Validates the `ApacheAirflowJob` definition spec added to
/// `definition_requirements.json`: `getDefinition` returns the canonical
/// `apacheairflowjob-content.json` part, confirming `deploy_strategy=content`
/// in the item-capability matrix.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn airflow_get_definition_returns_content_part() {
    let cfg = TestConfig::from_env();
    let name = format!("aaj_def_{}", std::process::id());

    let created = fabio()
        .args([
            "apache-airflow-job",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let created_out = String::from_utf8_lossy(&created.get_output().stdout);
    let created_json: serde_json::Value = serde_json::from_str(&created_out).unwrap();
    let id = created_json["data"]["id"].as_str().unwrap().to_string();

    let def = fabio()
        .args([
            "apache-airflow-job",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let def_out = String::from_utf8_lossy(&def.get_output().stdout);
    let def_json: serde_json::Value = serde_json::from_str(&def_out).unwrap();
    let parts: Vec<&str> = def_json["data"]["definition"]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["path"].as_str())
        .collect();
    assert!(
        parts.contains(&"apacheairflowjob-content.json"),
        "getDefinition must return apacheairflowjob-content.json; got {parts:?}"
    );

    fabio()
        .args([
            "apache-airflow-job",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
}
