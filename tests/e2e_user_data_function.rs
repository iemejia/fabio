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
fn user_data_function_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "user-data-function",
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
fn user_data_function_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "user-data-function",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-udf",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "user-data-function create");
}

// ─── invoke ──────────────────────────────────────────────────────────────────
// These first three run offline (validation/dry-run happen before auth).

#[test]
fn invoke_dry_run_builds_body() {
    let assert = fabio()
        .args([
            "user-data-function",
            "invoke",
            "--url",
            "https://x.fabric.microsoft.com/functions/hello",
            "--parameter",
            "name=John",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "user-data-function invoke");
    assert_eq!(json["data"]["details"]["body"]["name"], "John");
}

#[test]
fn invoke_rejects_untrusted_url() {
    fabio()
        .args([
            "user-data-function",
            "invoke",
            "--url",
            "https://evil.example.com/f",
            "--parameter",
            "name=John",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("untrusted domain"));
}

#[test]
fn invoke_rejects_bad_parameter() {
    fabio()
        .args([
            "user-data-function",
            "invoke",
            "--url",
            "https://x.fabric.microsoft.com/f",
            "--parameter",
            "noequals",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Invalid parameter"));
}

// Live plumbing: a POST to a trusted-but-nonexistent Fabric URL must attach auth,
// reach Fabric, and surface a clean not-found error (validates auth + error path).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn invoke_plumbing_not_found() {
    let _cfg = TestConfig::from_env();
    fabio()
        .args([
            "user-data-function",
            "invoke",
            "--url",
            "https://api.fabric.microsoft.com/nonexistent-udf-endpoint/invoke",
            "--parameter",
            "name=John",
            "--timeout",
            "30",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Function invocation failed"));
}
