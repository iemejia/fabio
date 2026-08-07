//! E2E integration tests for the `fabio job-scheduler` command group.
//!
//! Tests job instance listing and schedule operations against live Fabric API.

mod common;

use common::{TestConfig, extract_count, extract_data, fabio, parse_json};

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_list_instances() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "job-scheduler",
            "list-instances",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    // Should return a list (possibly empty for items with no job history)
    assert!(json.get("data").is_some() || json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_list_instances_with_limit() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "job-scheduler",
            "list-instances",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--limit",
            "1",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let count = extract_count(&json);
    assert!(count <= 1);
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_run_on_demand_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "job-scheduler",
            "run-on-demand",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--job-type",
            "RunNotebook",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["jobType"], "RunNotebook");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_run_on_demand_with_wait_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "job-scheduler",
            "run-on-demand",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--job-type",
            "RunNotebook",
            "--wait",
            "--timeout",
            "120",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["wait"], true);
    assert_eq!(data["details"]["timeout"], 120);
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_run_on_demand_with_cancel_on_timeout_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "job-scheduler",
            "run-on-demand",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--job-type",
            "Pipeline",
            "--wait",
            "--cancel-on-timeout",
            "--execution-data",
            r#"{"tableName":"test"}"#,
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["cancelOnTimeout"], true);
    assert_eq!(data["details"]["jobType"], "Pipeline");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_cancel_instance_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "job-scheduler",
            "cancel-instance",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--job-instance-id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_list_schedules() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "job-scheduler",
            "list-schedules",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--job-type",
            "RunNotebook",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    // Should return a list (possibly empty)
    assert!(json.get("data").is_some() || json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_create_schedule_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "job-scheduler",
            "create-schedule",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--job-type",
            "RunNotebook",
            "--config",
            r#"{"type":"Cron","startDateTime":"2026-01-01T00:00:00","endDateTime":"2026-12-31T23:59:00","localTimeZoneId":"UTC","interval":10}"#,
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

// The schedule config must be NESTED under `configuration` — the API rejects a
// flat/top-level spread with InvalidInput. Offline dry-run regression.
#[test]
fn job_scheduler_create_schedule_nests_configuration() {
    let output = fabio()
        .args([
            "--dry-run",
            "job-scheduler",
            "create-schedule",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--job-type",
            "RunNotebook",
            "--enabled",
            "--config",
            r#"{"type":"Cron","startDateTime":"2026-01-01T00:00:00","endDateTime":"2026-12-31T23:59:00","localTimeZoneId":"UTC","interval":10}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    let details = &data["details"];
    assert_eq!(details["enabled"], true);
    assert_eq!(details["configuration"]["type"], "Cron");
    assert_eq!(details["configuration"]["interval"], 10);
    // Must NOT be spread at the top level.
    assert!(details.get("type").is_none());
    assert!(details.get("interval").is_none());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_delete_schedule_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "job-scheduler",
            "delete-schedule",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--job-type",
            "RunNotebook",
            "--schedule-id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_run_on_demand_fire_and_forget() {
    let cfg = TestConfig::from_env();

    // Run without --wait — should return immediately with "accepted" status
    let output = fabio()
        .args([
            "job-scheduler",
            "run-on-demand",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--job-type",
            "RunNotebook",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    // Should have jobId and status=accepted
    assert_eq!(data["status"], "accepted");
    assert!(data["jobId"].is_string());
    let job_id = data["jobId"].as_str().unwrap();
    assert!(!job_id.is_empty());

    // Cancel the job we just started so it doesn't consume resources
    let _ = fabio()
        .args([
            "job-scheduler",
            "cancel-instance",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--job-instance-id",
            job_id,
        ])
        .assert();
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn job_scheduler_run_on_demand_with_wait() {
    let cfg = TestConfig::from_env();

    // Run a TableMaintenance job with --wait.
    // Use 300s timeout — Spark cold start on small capacity can take 2-5 min.
    let output = fabio()
        .args([
            "job-scheduler",
            "run-on-demand",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--job-type",
            "TableMaintenance",
            "--execution-data",
            r#"{"tableName":"sales","optimizeSettings":{"vOrder":true}}"#,
            "--wait",
            "--timeout",
            "300",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "Completed");
    assert!(data["jobId"].is_string());
}
