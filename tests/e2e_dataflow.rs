//! End-to-end integration tests for `fabio dataflow` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataflow_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["dataflow", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataflow_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("df_test");

    // Create
    let assert = fabio()
        .args([
            "dataflow",
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
            "dataflow",
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
fn dataflow_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "dataflow",
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
fn dataflow_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "dataflow",
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
fn dataflow_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "dataflow",
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
    assert_eq!(json["data"]["would_execute"], "dataflow create");
}

// ─── Discover Parameters ─────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataflow_discover_parameters_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "dataflow",
            "discover-parameters",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

// ─── Hard Delete ─────────────────────────────────────────────────────────────

#[test]
fn dataflow_delete_hard_delete_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "dataflow",
            "delete",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--hard-delete",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["hardDelete"], true);
}

// ─── Run ─────────────────────────────────────────────────────────────────────

#[test]
fn dataflow_run_dry_run_execute() {
    let assert = fabio()
        .args([
            "--dry-run",
            "dataflow",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["jobType"], "execute");
}

#[test]
fn dataflow_run_dry_run_apply_changes() {
    let assert = fabio()
        .args([
            "--dry-run",
            "dataflow",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--job-type",
            "apply-changes",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["jobType"], "applyChanges");
}

#[test]
fn dataflow_run_dry_run_with_parameters() {
    let assert = fabio()
        .args([
            "--dry-run",
            "dataflow",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--execute-option",
            "ApplyChangesIfNeeded",
            "--parameters",
            r#"[{"parameterName":"X","type":"Automatic","value":25}]"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    let body = &data["details"]["body"];
    assert_eq!(
        body["executionData"]["executeOption"],
        "ApplyChangesIfNeeded"
    );
    assert!(body["executionData"]["parameters"].is_array());
}

#[test]
fn dataflow_run_invalid_job_type() {
    let assert = fabio()
        .args([
            "dataflow",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--job-type",
            "invalid",
        ])
        .assert()
        .failure();

    let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stderr).unwrap();
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Invalid --job-type")
    );
}

#[test]
fn dataflow_run_apply_changes_rejects_parameters() {
    let assert = fabio()
        .args([
            "dataflow",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--job-type",
            "apply-changes",
            "--execute-option",
            "SkipApplyChanges",
        ])
        .assert()
        .failure();

    let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stderr).unwrap();
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("only supported for execute")
    );
}

// ─── Execute Query ──────────────────────────────────────────────────────────

#[test]
fn dataflow_execute_query_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "dataflow",
            "execute-query",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--query-name",
            "MyTable",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "dataflow execute-query");
    assert_eq!(data["details"]["queryName"], "MyTable");
}

#[test]
fn dataflow_execute_query_dry_run_with_mashup() {
    let assert = fabio()
        .args([
            "--dry-run",
            "dataflow",
            "execute-query",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--query-name",
            "MyTable",
            "--mashup",
            "let Source = Sql.Database(\"server\", \"db\") in Source",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["details"]["queryName"], "MyTable");
    assert!(
        data["details"]["customMashupDocument"]
            .as_str()
            .unwrap()
            .contains("Sql.Database")
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataflow_execute_query_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "dataflow",
            "execute-query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--query-name",
            "NonExistentQuery",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataflow_execute_query_arrow_version_2_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "dataflow",
            "execute-query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--query-name",
            "TestQuery",
            "--arrow-version",
            "2",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "dataflow execute-query");
    assert_eq!(data["details"]["queryName"], "TestQuery");
}

/// Regression for d2f62b6: the Fabric *bytes* client helpers used to drop the
/// real API error body and surface a useless `HTTP 400 Bad Request`. The
/// dataflow `executeQuery` endpoint returns a TOP-LEVEL `{errorCode, message}`
/// envelope; fabio must surface the `message` ("Query name not found") — proving
/// the bytes-error path no longer collapses to a bare status.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataflow_execute_query_surfaces_real_error_message() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let cfg = TestConfig::from_env();
    let name = common::unique_name("df_err");

    // Create a dataflow with a minimal, valid definition (queryMetadata.json +
    // a mashup that has NO query named "MissingQuery").
    let assert = fabio()
        .args([
            "dataflow",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let mashup =
        "section Section1;\nshared RealQuery = let Source = #table({\"a\"},{{1}}) in Source;\n";
    let qm = r#"{"formatVersion":"202502","computeEngineSettings":{"allowFastCopy":false},"name":null,"allowNativeQueries":false}"#;
    let body = serde_json::json!({
        "definition": { "parts": [
            { "path": "queryMetadata.json", "payload": STANDARD.encode(qm), "payloadType": "InlineBase64" },
            { "path": "mashup.pq", "payload": STANDARD.encode(mashup), "payloadType": "InlineBase64" },
        ]}
    });
    let body_path = format!("/tmp/opencode/df_err_{}.json", std::process::id());
    std::fs::write(&body_path, serde_json::to_vec(&body).unwrap()).unwrap();
    fabio()
        .args([
            "dataflow",
            "update-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
            "--file",
            &body_path,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // execute-query against a non-existent query name → 400 with a top-level
    // {errorCode, message}. The message MUST be surfaced (not a bare HTTP 400).
    let assert = fabio()
        .args([
            "dataflow",
            "execute-query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
            "--query-name",
            "MissingQuery",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("Query name not found"),
        "must surface the real API message, got: {stderr}"
    );
    assert!(
        !stderr.contains("HTTP 400 Bad Request\""),
        "must NOT collapse to a bare HTTP status, got: {stderr}"
    );

    // Cleanup
    let _ = std::fs::remove_file(&body_path);
    fabio()
        .args([
            "dataflow",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn dataflow_execute_query_with_file_output_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "dataflow",
            "execute-query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--query-name",
            "TestQuery",
            "--file",
            "/tmp/opencode/output.arrow",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "dataflow execute-query");
}
