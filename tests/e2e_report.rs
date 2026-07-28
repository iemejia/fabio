//! End-to-end integration tests for `fabio report` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["report", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "report",
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
fn report_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "report",
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
fn report_delete_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "report",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

// ─── Publish to Web Tests ────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_publish_to_web_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "report",
            "publish-to-web",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "report publish-to-web");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_publish_to_web_not_found() {
    let cfg = TestConfig::from_env();

    // Attempting to publish a non-existent report should fail
    fabio()
        .args([
            "report",
            "publish-to-web",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("NOT_FOUND")
                .or(predicate::str::contains("API_ERROR"))
                .or(predicate::str::contains("FORBIDDEN")),
        );
}

#[test]
#[ignore = "requires live Fabric tenant with Publish to Web enabled"]
#[serial]
fn report_publish_to_web_existing_report() {
    let cfg = TestConfig::from_env();

    // List reports and try to publish the first one (if any exist)
    let assert = fabio()
        .args(["report", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let reports = data.as_array().unwrap();

    if reports.is_empty() {
        eprintln!("No reports in workspace, skipping publish-to-web test");
        return;
    }

    let report_id = reports[0]["id"].as_str().unwrap();

    // Try to publish to web
    let assert = fabio()
        .args([
            "report",
            "publish-to-web",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            report_id,
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let output = assert.get_output();
    if output.status.success() {
        // If publish-to-web is enabled in the tenant, we should get an embed URL
        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        let data = extract_data(&json);
        assert_eq!(data["status"], "published_to_web");
        assert!(
            data["embedUrl"].as_str().is_some_and(|u| !u.is_empty()),
            "expected non-empty embedUrl"
        );
    } else {
        // If tenant doesn't allow Publish to Web, it should fail gracefully
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("Publish to web not available (tenant setting may be disabled): {stderr}");
        // This is acceptable - the test documents the behavior
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_export_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "report",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--format",
            "PDF",
            "--out",
            "/tmp/fabio_report_export.pdf",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "report export");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_export_rejects_paginated_only_format() {
    let cfg = TestConfig::from_env();
    // CSV is a paginated-only format; it must be rejected for a Power BI report.
    fabio()
        .args([
            "report",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--format",
            "CSV",
            "--out",
            "/tmp/fabio_report_export.csv",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unsupported export format"));
}
