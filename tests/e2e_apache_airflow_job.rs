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
