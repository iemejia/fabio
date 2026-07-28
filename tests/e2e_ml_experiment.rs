//! End-to-end integration tests for `fabio ml-experiment` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ml_experiment_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "ml-experiment",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ml_experiment_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("mle_test");

    // Create
    let assert = fabio()
        .args([
            "ml-experiment",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let id = data["id"].as_str().unwrap().to_string();

    // Delete
    let assert = fabio()
        .args([
            "ml-experiment",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ml_experiment_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "ml-experiment",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ml_experiment_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "ml-experiment",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ml_experiment_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "ml-experiment",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-dry-run",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "ml-experiment create");
}

/// Full `MLflow` run lifecycle: create an experiment, log a run with two metric
/// steps via the `MLflow` REST API (using the `rest` passthrough), then exercise
/// the new list-runs / get-run / get-metric-history commands. Values are read
/// before cleanup so the experiment is always deleted.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn ml_experiment_run_tracking_lifecycle() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let cfg = TestConfig::from_env();
    let ws = cfg.source_workspace;
    let now_ms = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();

    // Create the experiment.
    let created = parse_json(
        &fabio()
            .args([
                "ml-experiment",
                "create",
                "--workspace",
                &ws,
                "--name",
                "fabio_e2e_runs",
            ])
            .assert()
            .success(),
    );
    let exp_id = extract_data(&created)["id"].as_str().unwrap().to_string();

    let mlflow = |path: &str| format!("/workspaces/{ws}/mlflow/api/2.0/mlflow/{path}");

    // Create a run via the MLflow REST API.
    let run_created = parse_json(
        &fabio()
            .args([
                "rest",
                "call",
                "--method",
                "post",
                "--path",
                &mlflow("runs/create"),
                "--body",
                &format!(
                    r#"{{"experiment_id":"{exp_id}","start_time":{now_ms},"tags":[{{"key":"mlflow.runName","value":"fabio-e2e-run"}}]}}"#
                ),
            ])
            .assert()
            .success(),
    );
    let run_id = run_created["data"]["run"]["info"]["run_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Log two steps of an "accuracy" metric, then finish the run.
    for (step, value, ts) in [(0u32, 0.80_f64, now_ms), (1, 0.93, now_ms + 1000)] {
        fabio()
            .args([
                "rest",
                "call",
                "--method",
                "post",
                "--path",
                &mlflow("runs/log-metric"),
                "--body",
                &format!(
                    r#"{{"run_id":"{run_id}","key":"accuracy","value":{value},"timestamp":{ts},"step":{step}}}"#
                ),
            ])
            .assert()
            .success();
    }
    fabio()
        .args([
            "rest",
            "call",
            "--method",
            "post",
            "--path",
            &mlflow("runs/update"),
            "--body",
            &format!(r#"{{"run_id":"{run_id}","status":"FINISHED","end_time":{now_ms}}}"#),
        ])
        .assert()
        .success();

    // list-runs: the run should appear.
    let runs = parse_json(
        &fabio()
            .args([
                "ml-experiment",
                "list-runs",
                "--workspace",
                &ws,
                "--id",
                &exp_id,
            ])
            .assert()
            .success(),
    );
    let run_present = extract_data(&runs)
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["info"]["run_id"].as_str() == Some(run_id.as_str()));

    // get-run: fetch the run details.
    let run = parse_json(
        &fabio()
            .args([
                "ml-experiment",
                "get-run",
                "--workspace",
                &ws,
                "--run-id",
                &run_id,
            ])
            .assert()
            .success(),
    );
    let run_status = extract_data(&run)["info"]["status"]
        .as_str()
        .unwrap()
        .to_string();

    // get-metric-history: two steps.
    let history = parse_json(
        &fabio()
            .args([
                "ml-experiment",
                "get-metric-history",
                "--workspace",
                &ws,
                "--run-id",
                &run_id,
                "--metric",
                "accuracy",
            ])
            .assert()
            .success(),
    );
    let history_count = extract_data(&history).as_array().unwrap().len();

    // Cleanup before assertions.
    fabio()
        .args([
            "ml-experiment",
            "delete",
            "--workspace",
            &ws,
            "--id",
            &exp_id,
        ])
        .assert()
        .success();

    assert!(run_present, "list-runs should include the created run");
    assert_eq!(run_status, "FINISHED");
    assert_eq!(
        history_count, 2,
        "get-metric-history should return two steps"
    );
}
