//! End-to-end tests for the interactive Livy API (`fabio spark run` / session mgmt).
//!
//! Offline tests exercise the dry-run boundary and input validation (they fire
//! before any network call). The live test drives a real Spark session on a
//! lakehouse and is `#[ignore]`d like the other live suites (a cold session can
//! take ~2 minutes to start).

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

// ─── Offline: dry-run + validation ───────────────────────────────────────────

#[test]
fn spark_run_dry_run_previews_without_executing() {
    let assert = fabio()
        .args([
            "--dry-run",
            "spark",
            "run",
            "--workspace",
            "ws",
            "--lakehouse",
            "lh",
            "--code",
            "print(1)",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "spark run");
    assert_eq!(data["details"]["language"], "pyspark");
}

#[test]
fn spark_create_livy_session_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "spark",
            "create-livy-session",
            "--workspace",
            "ws",
            "--lakehouse",
            "lh",
            "--wait",
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "spark create-livy-session");
}

#[test]
fn spark_delete_livy_session_dry_run_is_destructive() {
    let assert = fabio()
        .args([
            "--dry-run",
            "spark",
            "delete-livy-session",
            "--workspace",
            "ws",
            "--lakehouse",
            "lh",
            "--session-id",
            "sid",
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&assert)).clone();
    assert_eq!(data["dry_run"], true);
    // delete-livy-session is annotated destructive in commands.json, so the guard
    // adds the destructive marker.
    assert_eq!(data["destructive"], true);
}

#[test]
fn spark_run_rejects_invalid_conf_json() {
    let assert = fabio()
        .args([
            "spark",
            "run",
            "--workspace",
            "ws",
            "--lakehouse",
            "lh",
            "--code",
            "print(1)",
            "--conf",
            "not-json",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
}

// ─── Live: run Spark code end to end ─────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn spark_run_one_shot_executes_pyspark() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "spark",
            "run",
            "--workspace",
            &cfg.source_workspace,
            "--lakehouse",
            &cfg.source_lakehouse,
            "--code",
            "print(sum(range(10)))",
            "--language",
            "pyspark",
            "--timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["text"], "45");
    // The one-shot always cleans up its ephemeral session.
    assert_eq!(data["sessionDeleted"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn spark_session_lifecycle_create_run_delete() {
    let cfg = TestConfig::from_env();

    // Create + wait for idle.
    let assert = fabio()
        .args([
            "spark",
            "create-livy-session",
            "--workspace",
            &cfg.source_workspace,
            "--lakehouse",
            &cfg.source_lakehouse,
            "--wait",
            "--timeout",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(6))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["state"], "idle");
    let sid = data["sessionId"].as_str().unwrap().to_string();

    // Run a Spark SQL statement (results arrive under data["application/json"]).
    let assert = fabio()
        .args([
            "spark",
            "run-statement",
            "--workspace",
            &cfg.source_workspace,
            "--lakehouse",
            &cfg.source_lakehouse,
            "--session-id",
            &sid,
            "--code",
            "SELECT 1 AS a",
            "--language",
            "sql",
            "--timeout",
            "120",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "ok");
    assert_eq!(data["data"]["application/json"]["data"][0][0], 1);

    // Delete the session.
    let assert = fabio()
        .args([
            "spark",
            "delete-livy-session",
            "--workspace",
            &cfg.source_workspace,
            "--lakehouse",
            &cfg.source_lakehouse,
            "--session-id",
            &sid,
        ])
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&assert))["status"], "deleted");
}
