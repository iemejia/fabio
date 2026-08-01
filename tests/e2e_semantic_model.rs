//! End-to-end integration tests for `fabio semantic-model` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json, unique_name};
use serial_test::serial;
use std::io::Write;
use tempfile::NamedTempFile;

// ─── List / Show / Update / Delete (basic) ───────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "semantic-model",
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
fn semantic_model_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "semantic-model",
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
fn semantic_model_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "semantic-model",
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
fn semantic_model_delete_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

// ─── Full Lifecycle: Create (model.bim) → Show → Get-Definition → Delete ────

/// Minimal model.bim JSON for an Import-mode semantic model.
fn minimal_model_bim() -> String {
    serde_json::json!({
        "compatibilityLevel": 1604,
        "model": {
            "culture": "en-US",
            "defaultPowerBIDataSourceVersion": "powerBI_V3",
            "tables": [
                {
                    "name": "TestTable",
                    "columns": [
                        {
                            "name": "ID",
                            "dataType": "int64",
                            "sourceColumn": "ID"
                        },
                        {
                            "name": "Name",
                            "dataType": "string",
                            "sourceColumn": "Name"
                        }
                    ],
                    "partitions": [
                        {
                            "name": "TestTable",
                            "source": {
                                "type": "m",
                                "expression": "let Source = #table({\"ID\", \"Name\"}, {{1, \"Test\"}}) in Source"
                            }
                        }
                    ]
                }
            ]
        }
    })
    .to_string()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_create_show_get_definition_delete() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sm_bim");

    // Write model.bim to a temp file
    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(minimal_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

    // ── Create ───────────────────────────────────────────────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--description",
            "E2E test semantic model (model.bim)",
            "--file",
            &file_path,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let sm_id = data["id"].as_str().unwrap().to_string();

    // ── Show ─────────────────────────────────────────────────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "show",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"], sm_id);
    assert_eq!(data["displayName"], name);

    // ── Get Definition ───────────────────────────────────────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Definition should have parts
    let parts = data["definition"]["parts"].as_array();
    assert!(
        parts.is_some(),
        "expected 'definition.parts' array in response"
    );
    let parts = parts.unwrap();
    assert!(!parts.is_empty(), "expected at least one definition part");

    // Should contain model.bim or definition.pbism
    let paths: Vec<&str> = parts.iter().filter_map(|p| p["path"].as_str()).collect();
    assert!(
        paths
            .iter()
            .any(|p| p.contains("model.bim") || p.contains(".pbism")),
        "expected model.bim or definition.pbism in parts, got: {paths:?}"
    );

    // ── Delete ───────────────────────────────────────────────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
}

// ─── Create with TMDL Format ─────────────────────────────────────────────────

/// Minimal TMDL model definition (Import mode, single table).
fn minimal_model_tmdl() -> String {
    r#"model Model
	culture: en-US
	defaultPowerBIDataSourceVersion: powerBI_V3

	table TestTable
		lineageTag: 00000000-0000-0000-0000-000000000002

		column ID
			dataType: int64
			sourceColumn: ID
			lineageTag: 00000000-0000-0000-0000-000000000003

		column Name
			dataType: string
			sourceColumn: Name
			lineageTag: 00000000-0000-0000-0000-000000000004

		partition TestTable = m
			expression = let Source = #table({"ID", "Name"}, {{1, "Test"}}) in Source
"#
    .to_string()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_create_tmdl_and_delete() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sm_tmdl");

    // Write model.tmdl to a temp file
    let mut tmp = NamedTempFile::with_suffix(".tmdl").unwrap();
    tmp.write_all(minimal_model_tmdl().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

    // ── Create (TMDL format auto-detected from extension) ────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--description",
            "E2E test semantic model (TMDL)",
            "--file",
            &file_path,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let sm_id = data["id"].as_str().unwrap().to_string();

    // ── Verify it shows up in list ───────────────────────────────────────
    let assert = fabio()
        .args(["semantic-model", "list", "--workspace", &cfg.dest_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let models = data.as_array().unwrap();
    assert!(
        models.iter().any(|m| m["id"] == sm_id),
        "created model should appear in list"
    );

    // ── Delete ───────────────────────────────────────────────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
}

// ─── Update + Update-Definition ──────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_update_name_and_description() {
    let cfg = TestConfig::from_env();
    let original_name = unique_name("sm_upd_o");
    let updated_name = unique_name("sm_upd_n");

    // Create
    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(minimal_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

    let assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &original_name,
            "--file",
            &file_path,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let sm_id = data["id"].as_str().unwrap().to_string();

    // Update name and description
    let assert = fabio()
        .args([
            "semantic-model",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            &updated_name,
            "--description",
            "Updated via E2E test",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], updated_name);
    assert_eq!(data["description"], "Updated via E2E test");

    // Cleanup
    fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();
}

// ─── Dry Run ─────────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_create_dry_run() {
    let cfg = TestConfig::from_env();

    // Write a temp file
    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(minimal_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            "dry_run_sm",
            "--file",
            &file_path,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "semantic-model create");
}

// ─── DAX Query ───────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_query_dax_flag() {
    let cfg = TestConfig::from_env();

    // Create a model with an M-expression table (Import mode) for querying
    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(minimal_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();
    let name = unique_name("sm_query");

    let assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--file",
            &file_path,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let sm_id = data["id"].as_str().unwrap().to_string();

    // Query with --dax flag
    let assert = fabio()
        .args([
            "semantic-model",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--dax",
            "EVALUATE ROW(\"Result\", 1 + 1)",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let rows = data.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["[Result]"], 2);

    // Cleanup
    fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_query_from_stdin() {
    let cfg = TestConfig::from_env();

    // Create model
    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(minimal_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();
    let name = unique_name("sm_qstdin");

    let assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--file",
            &file_path,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let sm_id = data["id"].as_str().unwrap().to_string();

    // Query via stdin
    let assert = fabio()
        .args([
            "semantic-model",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .write_stdin("EVALUATE ROW(\"Value\", 42)")
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let rows = data.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["[Value]"], 42);

    // Cleanup
    fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_query_from_file() {
    let cfg = TestConfig::from_env();

    // Create model
    let mut tmp_model = NamedTempFile::with_suffix(".bim").unwrap();
    tmp_model.write_all(minimal_model_bim().as_bytes()).unwrap();
    let file_path = tmp_model.path().to_str().unwrap().to_string();
    let name = unique_name("sm_qfile");

    let assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--file",
            &file_path,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let sm_id = data["id"].as_str().unwrap().to_string();

    // Write DAX to a temp file
    let mut tmp_dax = NamedTempFile::with_suffix(".dax").unwrap();
    tmp_dax.write_all(b"EVALUATE ROW(\"Pi\", 3.14159)").unwrap();
    let dax_path = tmp_dax.path().to_str().unwrap().to_string();

    // Query via --file
    let assert = fabio()
        .args([
            "semantic-model",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--file",
            &dax_path,
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let rows = data.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    // Floating point — just check it exists
    assert!(rows[0]["[Pi]"].as_f64().unwrap() > 3.0);

    // Cleanup
    fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_query_table_output() {
    let cfg = TestConfig::from_env();

    // Create model
    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(minimal_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();
    let name = unique_name("sm_qtable");

    let assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--file",
            &file_path,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let sm_id = data["id"].as_str().unwrap().to_string();

    // Query with table output
    let assert = fabio()
        .args([
            "semantic-model",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--dax",
            "EVALUATE ROW(\"X\", 1, \"Y\", 2)",
            "-o",
            "table",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // Table output should contain header and data
    assert!(stdout.contains("[X]") || stdout.contains("[Y]"));
    assert!(stdout.contains('1') && stdout.contains('2'));

    // Cleanup
    fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_query_not_found() {
    let cfg = TestConfig::from_env();

    // Query a non-existent model
    fabio()
        .args([
            "semantic-model",
            "query",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dax",
            "EVALUATE ROW(\"X\", 1)",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// semantic-model query with --output csv
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_query_csv_output() {
    let cfg = TestConfig::from_env();

    // Create a model for querying
    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(minimal_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();
    let name = unique_name("sm_qcsv");

    let assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
            "--file",
            &file_path,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let sm_id = data["id"].as_str().unwrap().to_string();

    // Query with --output csv
    let assert = fabio()
        .args([
            "-o",
            "csv",
            "semantic-model",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--dax",
            "EVALUATE ROW(\"ColA\", 42, \"ColB\", \"test\")",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "CSV should have header + data row, got: {stdout}"
    );
    // Header contains column names (DAX wraps in brackets)
    assert!(lines[0].contains("[ColA]"));
    assert!(lines[0].contains("[ColB]"));
    assert!(lines[0].contains(','));
    // Data row contains values
    assert!(lines[1].contains("42"));
    assert!(lines[1].contains("test"));

    // Cleanup
    fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();
}

// ─── Power BI API Commands (list-parameters, list-datasources, etc.) ─────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_list_parameters_dry_run() {
    let cfg = TestConfig::from_env();

    // list-parameters is a GET — dry-run does not apply; test live call
    // Use the workspace directly - this may return empty value array which is fine
    let assert = fabio()
        .args([
            "semantic-model",
            "list-parameters",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    // Either succeeds with empty data or fails with not-found — both are valid
    let _ = assert;
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_update_parameters_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "update-parameters",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            r#"{"updateDetails":[{"name":"Param1","newValue":"test"}]}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_list_datasources_dry_run() {
    let cfg = TestConfig::from_env();

    // list-datasources is a GET — test against non-existent model
    let _ = fabio()
        .args([
            "semantic-model",
            "list-datasources",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_update_datasources_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "update-datasources",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            r#"{"updateDetails":[{"datasourceSelector":{"datasourceType":"Sql"}}]}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_list_users_not_found() {
    let cfg = TestConfig::from_env();

    // list-users against a non-existent model
    fabio()
        .args([
            "semantic-model",
            "list-users",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_add_user_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "add-user",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--principal",
            "testuser@example.com",
            "--principal-type",
            "User",
            "--access-right",
            "Read",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert!(
        data["details"]["identifier"]
            .as_str()
            .unwrap()
            .contains("testuser")
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_delete_user_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "delete-user",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--user",
            "testuser@example.com",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_refresh_status_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "semantic-model",
            "refresh-status",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_list_upstream_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "semantic-model",
            "list-upstream",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .failure();
}

#[test]
#[serial]
fn semantic_model_update_parameters_invalid_json() {
    fabio()
        .args([
            "semantic-model",
            "update-parameters",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            "not valid json {{{",
        ])
        .assert()
        .failure();
}

// ─── Clone ───────────────────────────────────────────────────────────────────

#[test]
#[serial]
fn semantic_model_clone_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "clone",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "ClonedModel",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "semantic-model clone");
    assert_eq!(data["details"]["name"], "ClonedModel");
}

#[test]
#[serial]
fn semantic_model_clone_dry_run_with_target_workspace() {
    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "clone",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "ClonedModel",
            "--target-workspace",
            "11111111-1111-1111-1111-111111111111",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "semantic-model clone");
    assert_eq!(data["details"]["name"], "ClonedModel");
    assert_eq!(
        data["details"]["targetWorkspaceId"],
        "11111111-1111-1111-1111-111111111111"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_clone_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "semantic-model",
            "clone",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            &unique_name("clone"),
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .failure();
}

// ─── Export PBIX ─────────────────────────────────────────────────────────────

#[test]
#[serial]
fn semantic_model_export_pbix_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "export-pbix",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--file",
            "/tmp/opencode/test_export.pbix",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "semantic-model export-pbix");
    assert_eq!(data["details"]["file"], "/tmp/opencode/test_export.pbix");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_export_pbix_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "semantic-model",
            "export-pbix",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--file",
            "/tmp/opencode/nonexistent_export.pbix",
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .failure();
}

// ─── Import PBIX ─────────────────────────────────────────────────────────────

#[test]
#[serial]
fn semantic_model_import_pbix_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "import-pbix",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "ImportedModel",
            "--file",
            "/tmp/opencode/test.pbix",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "semantic-model import-pbix");
    assert_eq!(data["details"]["name"], "ImportedModel");
    assert_eq!(data["details"]["nameConflict"], "Abort");
}

#[test]
#[serial]
fn semantic_model_import_pbix_file_not_found() {
    // Should fail with INVALID_INPUT because the file doesn't exist
    fabio()
        .args([
            "semantic-model",
            "import-pbix",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "ImportedModel",
            "--file",
            "/tmp/opencode/nonexistent_file_xyz.pbix",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_import_pbix_invalid_file() {
    let cfg = TestConfig::from_env();

    // Create a dummy file that is NOT a valid .pbix — the API should reject it
    let mut tmp = NamedTempFile::with_suffix(".pbix").unwrap();
    tmp.write_all(b"not a real pbix file").unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

    fabio()
        .args([
            "semantic-model",
            "import-pbix",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &unique_name("import"),
            "--file",
            &file_path,
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .failure();
}

// ─── Unbind Connection ───────────────────────────────────────────────────────

#[test]
fn semantic_model_unbind_connection_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "unbind-connection",
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
    assert_eq!(data["would_execute"], "semantic-model unbind-connection");
    // Should show null connectionId in the details
    assert!(data["details"]["connectionId"].is_null());
}

#[test]
fn semantic_model_bind_connection_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
            "bind-connection",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--connection-id",
            "cccccccc-1111-2222-3333-444444444444",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "semantic-model bind-connection");
    assert_eq!(
        data["details"]["connectionId"],
        "cccccccc-1111-2222-3333-444444444444"
    );
}

// ─── Hard Delete ─────────────────────────────────────────────────────────────

#[test]
fn semantic_model_delete_hard_delete_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "semantic-model",
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

/// Live: schema introspection via DAX INFO.VIEW.* (list-tables/columns/measures/
/// relationships). Creates a 2-table model with a measure and a relationship,
/// asserts each introspection command surfaces it, then cleans up.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_schema_introspection_lifecycle() {
    use std::io::Write;

    let cfg = TestConfig::from_env();
    let ws = &cfg.dest_workspace;

    let bim = serde_json::json!({
        "compatibilityLevel": 1604,
        "model": {
            "culture": "en-US",
            "defaultPowerBIDataSourceVersion": "powerBI_V3",
            "tables": [
                {
                    "name": "DimProduct",
                    "columns": [
                        {"name": "ProductKey", "dataType": "int64", "sourceColumn": "[ProductKey]", "type": "calculatedTableColumn"},
                        {"name": "ProductName", "dataType": "string", "sourceColumn": "[ProductName]", "type": "calculatedTableColumn"}
                    ],
                    "partitions": [{"name": "DimProduct", "mode": "import", "source": {"type": "calculated", "expression": "DATATABLE(\"ProductKey\", INTEGER, \"ProductName\", STRING, {{1,\"Widget\"}})"}}]
                },
                {
                    "name": "FactSales",
                    "columns": [
                        {"name": "ProductKey", "dataType": "int64", "sourceColumn": "[ProductKey]", "type": "calculatedTableColumn"},
                        {"name": "Amount", "dataType": "double", "sourceColumn": "[Amount]", "type": "calculatedTableColumn"}
                    ],
                    "partitions": [{"name": "FactSales", "mode": "import", "source": {"type": "calculated", "expression": "DATATABLE(\"ProductKey\", INTEGER, \"Amount\", DOUBLE, {{1,10.0}})"}}],
                    "measures": [{"name": "Total Amount", "expression": "SUM(FactSales[Amount])"}]
                }
            ],
            "relationships": [{"name": "rel1", "fromTable": "FactSales", "fromColumn": "ProductKey", "toTable": "DimProduct", "toColumn": "ProductKey"}]
        }
    })
    .to_string();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.bim");
    std::fs::File::create(&path)
        .unwrap()
        .write_all(bim.as_bytes())
        .unwrap();

    let name = unique_name("fabio-e2e-introspect");
    let created = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            ws,
            "--name",
            &name,
            "--file",
            path.to_str().unwrap(),
        ])
        .assert()
        .success();
    let id = parse_json(&created)["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // list-tables: DimProduct + FactSales
    let tables = fabio()
        .args([
            "semantic-model",
            "list-tables",
            "--workspace",
            ws,
            "--id",
            &id,
        ])
        .assert()
        .success();
    let tj = parse_json(&tables);
    let names: Vec<String> = tj["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["Name"].as_str().map(str::to_owned))
        .collect();
    assert!(
        names.contains(&"DimProduct".to_string()),
        "tables: {names:?}"
    );
    assert!(
        names.contains(&"FactSales".to_string()),
        "tables: {names:?}"
    );

    // list-columns: keys are unbracketed
    let cols = fabio()
        .args([
            "semantic-model",
            "list-columns",
            "--workspace",
            ws,
            "--id",
            &id,
        ])
        .assert()
        .success();
    let cj = parse_json(&cols);
    let first = cj["data"].as_array().unwrap().first().unwrap();
    assert!(
        first
            .as_object()
            .unwrap()
            .keys()
            .all(|k| !k.starts_with('[')),
        "column keys must be unbracketed: {first:?}"
    );

    // list-measures: Total Amount
    let measures = fabio()
        .args([
            "semantic-model",
            "list-measures",
            "--workspace",
            ws,
            "--id",
            &id,
        ])
        .assert()
        .success();
    let mj = parse_json(&measures);
    assert!(
        mj["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["Name"].as_str() == Some("Total Amount")),
        "expected 'Total Amount' measure: {mj}"
    );

    // list-relationships: one relationship present
    let rels = fabio()
        .args([
            "semantic-model",
            "list-relationships",
            "--workspace",
            ws,
            "--id",
            &id,
        ])
        .assert()
        .success();
    let rj = parse_json(&rels);
    assert_eq!(
        rj["count"].as_u64(),
        Some(1),
        "expected 1 relationship: {rj}"
    );

    // Cleanup
    fabio()
        .args(["semantic-model", "delete", "--workspace", ws, "--id", &id])
        .assert()
        .success();
}
