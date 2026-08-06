//! End-to-end integration tests for `fabio notebook` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_create_get_definition_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("nb_test");

    // Create notebook
    let assert = fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--content",
            "print('integration test')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "Succeeded");

    // Find the notebook ID
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let nb = items
        .iter()
        .find(|i| i["displayName"] == name)
        .expect("created notebook not found in item list");
    let nb_id = nb["id"].as_str().unwrap().to_string();

    // Get definition
    let assert = fabio()
        .args([
            "notebook",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should have definition.parts
    let parts = data["definition"]["parts"].as_array().unwrap();
    assert!(!parts.is_empty());
    assert!(parts.iter().any(|p| p["path"] == "notebook-content.py"));

    // Delete
    fabio()
        .args([
            "notebook",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("deleted"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_run_status_stop() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("nb_run");

    // Create a simple notebook
    fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--content",
            "import time; time.sleep(30); print('done')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Find notebook ID
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let nb = items
        .iter()
        .find(|i| i["displayName"] == name)
        .expect("notebook not found");
    let nb_id = nb["id"].as_str().unwrap().to_string();

    // Run
    let assert = fabio()
        .args([
            "notebook",
            "run",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "started");
    let job_id = data["jobId"].as_str().unwrap().to_string();
    assert!(!job_id.is_empty());

    // Status
    let assert = fabio()
        .args([
            "notebook",
            "status",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
            "--job-id",
            &job_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Status should be NotStarted or InProgress (Spark cold start)
    let status = data["status"].as_str().unwrap();
    assert!(
        ["NotStarted", "InProgress", "Completed"].contains(&status),
        "unexpected status: {status}"
    );

    // Stop (cancel)
    let assert = fabio()
        .args([
            "notebook",
            "stop",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
            "--job-id",
            &job_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "cancelled");

    // Delete notebook
    fabio()
        .args([
            "notebook",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_run_with_wait() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("nb_wait");

    // Create a quick notebook
    fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--content",
            "print('quick run')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Find notebook ID
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let nb = items
        .iter()
        .find(|i| i["displayName"] == name)
        .expect("notebook not found");
    let nb_id = nb["id"].as_str().unwrap().to_string();

    // Run with --wait (use a generous timeout for Spark cold start)
    let assert = fabio()
        .args([
            "notebook",
            "run",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
            "--wait",
            "--timeout",
            "600",
        ])
        .timeout(std::time::Duration::from_mins(11))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "Completed");
    assert!(data.get("jobId").is_some());

    // Delete notebook
    fabio()
        .args([
            "notebook",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// notebook run --wait with a failing notebook (exception → Failed status)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_run_with_wait_fails() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("nb_fail");

    // Create a notebook that raises an exception
    fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--content",
            "raise Exception('intentional failure for test')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Find notebook ID
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let nb = items
        .iter()
        .find(|i| i["displayName"] == name)
        .expect("notebook not found");
    let nb_id = nb["id"].as_str().unwrap().to_string();

    // Run with --wait — should complete but with Failed status
    let assert = fabio()
        .args([
            "notebook",
            "run",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
            "--wait",
            "--timeout",
            "600",
        ])
        .timeout(std::time::Duration::from_mins(11))
        .assert();

    // The command may succeed (reporting status=Failed) or fail outright
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        let data = &json["data"];
        assert_eq!(
            data["status"], "Failed",
            "Expected Failed status for error notebook: {data}"
        );
    } else {
        // If command failed, error should mention failure
        assert!(
            stderr.contains("Failed") || stderr.contains("error"),
            "Expected failure indication in stderr: {stderr}"
        );
    }

    // Delete notebook
    fabio()
        .args([
            "notebook",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// notebook delete non-existent returns error
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_delete_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "notebook",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR",
        "Expected NOT_FOUND or API_ERROR, got: {code}"
    );
}

// ---------------------------------------------------------------------------
// notebook get-definition for non-existent notebook returns error
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_get_definition_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "notebook",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR",
        "Expected error for non-existent notebook, got: {code}"
    );
}

// ===========================================================================
// notebook list / show / update / update-definition
// ===========================================================================

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_list_returns_notebooks() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["notebook", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let arr = data.as_array().unwrap();
    assert!(!arr.is_empty(), "expected at least one notebook");

    let first = &arr[0];
    assert!(first.get("id").is_some());
    assert!(first.get("displayName").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_show_returns_details() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "notebook",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"], cfg.notebook_id);
    assert!(data.get("displayName").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_update_name() {
    let cfg = TestConfig::from_env();
    let original = common::unique_name("nb_upd_o");
    let updated = common::unique_name("nb_upd_n");

    // Create
    fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &original,
            "--content",
            "print('update test')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Find ID
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let nb = items.iter().find(|i| i["displayName"] == original).unwrap();
    let nb_id = nb["id"].as_str().unwrap().to_string();

    // Update
    let assert = fabio()
        .args([
            "notebook",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
            "--name",
            &updated,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], updated);

    // Cleanup
    fabio()
        .args([
            "notebook",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_update_definition_with_content() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("nb_upddef");

    // Create
    fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--content",
            "print('original')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // Find ID
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let nb = items.iter().find(|i| i["displayName"] == name).unwrap();
    let nb_id = nb["id"].as_str().unwrap().to_string();

    // Update definition
    let assert = fabio()
        .args([
            "notebook",
            "update-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
            "--content",
            "print('updated content')",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "definition_updated");

    // Cleanup
    fabio()
        .args([
            "notebook",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_update_definition_requires_input() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "notebook",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
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
fn notebook_get_definition_strip_output() {
    let cfg = TestConfig::from_env();

    // --strip-output should succeed and return a valid definition
    let assert = fabio()
        .args([
            "notebook",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.notebook_id,
            "--strip-output",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should still have definition.parts
    let parts = data["definition"]["parts"].as_array().unwrap();
    assert!(!parts.is_empty());
    // Should have notebook-content part
    let has_content_part = parts.iter().any(|p| {
        p["path"]
            .as_str()
            .unwrap_or("")
            .contains("notebook-content")
    });
    assert!(has_content_part);
}

// ─── Hard Delete ─────────────────────────────────────────────────────────────

#[test]
fn notebook_delete_hard_delete_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "notebook",
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

// ─── Run with parameters ─────────────────────────────────────────────────────

#[test]
fn notebook_run_dry_run_no_params() {
    let assert = fabio()
        .args([
            "--dry-run",
            "notebook",
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
    // body should be null when no parameters
    assert!(data["details"]["body"].is_null());
}

#[test]
fn notebook_run_dry_run_with_parameters() {
    let assert = fabio()
        .args([
            "--dry-run",
            "notebook",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--parameters",
            r#"[{"name":"p1","value":"hello","type":"Text"}]"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    let body = &data["details"]["body"];
    assert_eq!(body["parameters"][0]["name"], "p1");
}

#[test]
fn notebook_run_dry_run_with_compute_type() {
    let assert = fabio()
        .args([
            "--dry-run",
            "notebook",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--compute-type",
            "Jupyter",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    let body = &data["details"]["body"];
    assert_eq!(body["executionData"]["compute"], "Jupyter");
}

#[test]
fn notebook_run_invalid_compute_type() {
    let assert = fabio()
        .args([
            "notebook",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--compute-type",
            "Invalid",
        ])
        .assert()
        .failure();

    let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stderr).unwrap();
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Invalid --compute-type")
    );
}

#[test]
fn notebook_run_dry_run_with_execution_data() {
    let assert = fabio()
        .args([
            "--dry-run",
            "notebook",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--execution-data",
            r#"{"compute":"Spark","computeConfiguration":{"name":"mySession"}}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    let body = &data["details"]["body"];
    assert_eq!(body["executionData"]["compute"], "Spark");
    assert_eq!(
        body["executionData"]["computeConfiguration"]["name"],
        "mySession"
    );
}

#[test]
fn notebook_update_definition_ipynb_envelope_preserves_format() {
    // Regression: build_update_definition_body must NOT drop definition.format.
    // An ipynb envelope must round-trip `format:"ipynb"` to updateDefinition,
    // else the server misreads the payload as raw .py (PyToIPynbFailure).
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("nb-envelope.json");
    std::fs::write(
        &file_path,
        r#"{"definition":{"format":"ipynb","parts":[{"path":"notebook-content.py","payload":"e30=","payloadType":"InlineBase64"}]}}"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "--dry-run",
            "notebook",
            "update-definition",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--file",
            &file_path.display().to_string(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["details"]["definition"]["format"], "ipynb");
}

#[test]
fn notebook_run_dry_run_execution_data_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("exec_data.json");
    std::fs::write(
        &file_path,
        r#"{"compute":"Jupyter","computeConfiguration":{"timeout":300}}"#,
    )
    .unwrap();

    let at_file = format!("@{}", file_path.display());
    let assert = fabio()
        .args([
            "--dry-run",
            "notebook",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--execution-data",
            &at_file,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    let body = &data["details"]["body"];
    assert_eq!(body["executionData"]["compute"], "Jupyter");
    assert_eq!(
        body["executionData"]["computeConfiguration"]["timeout"],
        300
    );
}

#[test]
fn notebook_run_dry_run_parameters_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("params.json");
    std::fs::write(
        &file_path,
        r#"[{"name":"env","value":"production","type":"Text"}]"#,
    )
    .unwrap();

    let at_file = format!("@{}", file_path.display());
    let assert = fabio()
        .args([
            "--dry-run",
            "notebook",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--parameters",
            &at_file,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    let body = &data["details"]["body"];
    assert_eq!(body["parameters"][0]["name"], "env");
    assert_eq!(body["parameters"][0]["value"], "production");
}

#[test]
fn notebook_run_execution_data_file_not_found() {
    let assert = fabio()
        .args([
            "notebook",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--execution-data",
            "@/nonexistent/path/exec.json",
        ])
        .assert()
        .failure();

    let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stderr).unwrap();
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(msg.contains("Failed to read file"));
}

#[test]
fn notebook_run_execution_data_file_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("bad.json");
    std::fs::write(&file_path, "this is not json").unwrap();

    let at_file = format!("@{}", file_path.display());
    let assert = fabio()
        .args([
            "notebook",
            "run",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--execution-data",
            &at_file,
        ])
        .assert()
        .failure();

    let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stderr).unwrap();
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(msg.contains("Invalid --execution-data JSON"));
}

// ─── notebook create --file tests ────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_create_from_py_file() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("nb_file_py");

    // Write a .py file
    let tmp_dir = std::env::temp_dir().join("fabio_e2e_notebook");
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let py_path = tmp_dir.join("test_script.py");
    std::fs::write(&py_path, "# E2E test\nprint('from .py file')\nx = 42").unwrap();

    // Create notebook from .py file
    let assert = fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--file",
            py_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let nb_id = data["id"].as_str().unwrap();
    assert!(!nb_id.is_empty());

    // Get definition and verify it's a valid notebook
    let assert = fabio()
        .args([
            "notebook",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            nb_id,
            "--decode",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let def_json = parse_json(&assert);
    let def_data = extract_data(&def_json);
    let parts = def_data["definition"]["parts"].as_array().unwrap();
    assert!(!parts.is_empty());
    // The decoded payload should contain our code
    let decoded = parts[0]["decodedPayload"].as_str().unwrap_or("");
    assert!(decoded.contains("from .py file"));

    // Cleanup
    fabio()
        .args([
            "notebook",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            nb_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    std::fs::remove_file(&py_path).ok();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn notebook_create_from_ipynb_file() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("nb_file_ipynb");

    // Write a .ipynb file
    let tmp_dir = std::env::temp_dir().join("fabio_e2e_notebook");
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let ipynb_path = tmp_dir.join("test_notebook.ipynb");
    let ipynb_content = serde_json::json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": {"language_info": {"name": "python"}},
        "cells": [{
            "cell_type": "code",
            "metadata": {},
            "source": ["# ipynb E2E test\n", "result = 'from .ipynb'\n"],
            "outputs": []
        }]
    });
    std::fs::write(
        &ipynb_path,
        serde_json::to_string_pretty(&ipynb_content).unwrap(),
    )
    .unwrap();

    // Create notebook from .ipynb file
    let assert = fabio()
        .args([
            "notebook",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--file",
            ipynb_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let nb_id = data["id"].as_str().unwrap();
    assert!(!nb_id.is_empty());

    // Cleanup
    fabio()
        .args([
            "notebook",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            nb_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    std::fs::remove_file(&ipynb_path).ok();
}
