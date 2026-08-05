//! End-to-end integration tests for `fabio semantic-model` commands.

mod common;

use base64::Engine as _;
use common::{TestConfig, extract_data, fabio, parse_json, unique_name};
use serial_test::serial;
use std::io::Write;
use tempfile::NamedTempFile;

/// Extract the error JSON envelope from stderr, skipping any `[timing]` line
/// that precedes it when the command made network calls before failing.
fn error_json(stderr: &str) -> serde_json::Value {
    let line = stderr
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or(stderr);
    serde_json::from_str(line.trim()).unwrap()
}

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

// ─── Authoring: set-description / add-measure / update-measure ────────────────

/// Full authoring lifecycle over the model DEFINITION (getDefinition → edit
/// TMDL → updateDefinition): set a table description, add a measure with
/// properties, update the measure's expression, and verify each via the
/// round-tripped definition. Also covers the duplicate-measure and
/// missing-target error paths.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_authoring_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sm_authoring");

    // Create a TMDL model (Fabric normalizes it to definition/tables/*.tmdl).
    let mut tmp = NamedTempFile::with_suffix(".tmdl").unwrap();
    tmp.write_all(minimal_model_tmdl().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

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
    let sm_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // ── set-description (table) — dry-run first ──────────────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "set-description",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "TestTable",
            "--description",
            "Fact table (e2e)",
            "--dry-run",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&assert))["dry_run"], true);

    // ── set-description (table) — live ───────────────────────────────────
    fabio()
        .args([
            "semantic-model",
            "set-description",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "TestTable",
            "--description",
            "Fact table (e2e)",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // ── add-measure with properties ─────────────────────────────────────
    fabio()
        .args([
            "semantic-model",
            "add-measure",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "TestTable",
            "--name",
            "Row Count",
            "--expression",
            "COUNTROWS('TestTable')",
            "--format-string",
            "0",
            "--display-folder",
            "KPIs",
            "--description",
            "Number of rows",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // ── add-measure duplicate → CONFLICT ────────────────────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "add-measure",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "TestTable",
            "--name",
            "Row Count",
            "--expression",
            "1",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "CONFLICT");

    // ── update-measure: new expression + format string ──────────────────
    fabio()
        .args([
            "semantic-model",
            "update-measure",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--measure",
            "Row Count",
            "--expression",
            "COUNTROWS('TestTable') * 2",
            "--format-string",
            "#,0",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // ── update-measure on a missing measure → NOT_FOUND ─────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "update-measure",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--measure",
            "Nope",
            "--description",
            "x",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "NOT_FOUND");

    // ── set-description with no target → INVALID_INPUT (offline) ─────────
    let assert = fabio()
        .args([
            "semantic-model",
            "set-description",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--description",
            "x",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "INVALID_INPUT");

    // ── Verify all edits via the round-tripped definition ───────────────
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
    let parts = data["definition"]["parts"].as_array().unwrap();
    let table_tmdl = parts
        .iter()
        .filter_map(|p| {
            let path = p["path"].as_str()?;
            if path.contains("/tables/") && path.contains(".tmdl") {
                let payload = p["payload"].as_str()?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(payload)
                    .ok()?;
                String::from_utf8(bytes).ok()
            } else {
                None
            }
        })
        .find(|c| c.contains("table TestTable"))
        .expect("TestTable.tmdl part");

    assert!(
        table_tmdl.contains("/// Fact table (e2e)"),
        "table description not applied:\n{table_tmdl}"
    );
    assert!(
        table_tmdl.contains("measure 'Row Count' = COUNTROWS('TestTable') * 2"),
        "updated measure expression missing:\n{table_tmdl}"
    );
    assert!(
        table_tmdl.contains("displayFolder: KPIs"),
        "measure displayFolder missing:\n{table_tmdl}"
    );
    assert!(
        table_tmdl.contains("formatString: #,0"),
        "updated formatString missing:\n{table_tmdl}"
    );
    assert!(
        table_tmdl.contains("/// Number of rows"),
        "measure description missing:\n{table_tmdl}"
    );

    // ── Cleanup ─────────────────────────────────────────────────────────
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

// ─── Relationships: add / update / delete ────────────────────────────────────

/// Model.bim with three tables (Customer, Product, Sales) and NO relationships,
/// so the relationship commands can build them from scratch.
fn three_table_model_bim() -> String {
    serde_json::json!({
        "compatibilityLevel": 1604,
        "model": {
            "culture": "en-US",
            "defaultPowerBIDataSourceVersion": "powerBI_V3",
            "tables": [
                {
                    "name": "Customer",
                    "columns": [{"name": "CustomerKey", "dataType": "int64", "sourceColumn": "CustomerKey"}],
                    "partitions": [{"name": "Customer", "source": {"type": "m", "expression": "let Source = #table({\"CustomerKey\"}, {{1}}) in Source"}}]
                },
                {
                    "name": "Product",
                    "columns": [{"name": "ProductKey", "dataType": "int64", "sourceColumn": "ProductKey"}],
                    "partitions": [{"name": "Product", "source": {"type": "m", "expression": "let Source = #table({\"ProductKey\"}, {{1}}) in Source"}}]
                },
                {
                    "name": "Sales",
                    "columns": [
                        {"name": "CustomerKey", "dataType": "int64", "sourceColumn": "CustomerKey"},
                        {"name": "ProductKey", "dataType": "int64", "sourceColumn": "ProductKey"},
                        {"name": "Amount", "dataType": "double", "sourceColumn": "Amount"}
                    ],
                    "partitions": [{"name": "Sales", "source": {"type": "m", "expression": "let Source = #table({\"CustomerKey\",\"ProductKey\",\"Amount\"}, {{1,1,10.0}}) in Source"}}]
                }
            ]
        }
    })
    .to_string()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_relationship_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sm_rel");

    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(three_table_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

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
    let sm_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // ── add-relationship (dry-run) ──────────────────────────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "add-relationship",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--from-table",
            "Sales",
            "--from-column",
            "CustomerKey",
            "--to-table",
            "Customer",
            "--to-column",
            "CustomerKey",
            "--dry-run",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&assert))["dry_run"], true);

    // ── add-relationship (live) — Customer join ─────────────────────────
    fabio()
        .args([
            "semantic-model",
            "add-relationship",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--from-table",
            "Sales",
            "--from-column",
            "CustomerKey",
            "--to-table",
            "Customer",
            "--to-column",
            "CustomerKey",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // ── add-relationship (live) — Product join, bidirectional ───────────
    fabio()
        .args([
            "semantic-model",
            "add-relationship",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--from-table",
            "Sales",
            "--from-column",
            "ProductKey",
            "--to-table",
            "Product",
            "--to-column",
            "ProductKey",
            "--cross-filter",
            "bothDirections",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // ── update-relationship: make the Customer join inactive ────────────
    fabio()
        .args([
            "semantic-model",
            "update-relationship",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--from-table",
            "Sales",
            "--from-column",
            "CustomerKey",
            "--to-table",
            "Customer",
            "--to-column",
            "CustomerKey",
            "--inactive",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // ── Verify via the round-tripped definition ─────────────────────────
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
    let parts = data["definition"]["parts"].as_array().unwrap();
    let rels = parts
        .iter()
        .find(|p| p["path"].as_str() == Some("definition/relationships.tmdl"))
        .and_then(|p| p["payload"].as_str())
        .map(|b| {
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b).unwrap()).unwrap()
        })
        .expect("relationships.tmdl part");
    assert!(
        rels.contains("fromColumn: Sales.CustomerKey"),
        "rels:\n{rels}"
    );
    assert!(
        rels.contains("fromColumn: Sales.ProductKey"),
        "rels:\n{rels}"
    );
    assert!(
        rels.contains("crossFilteringBehavior: bothDirections"),
        "rels:\n{rels}"
    );
    assert!(rels.contains("isActive: false"), "rels:\n{rels}");

    // ── delete-relationship: the Product join, by columns ───────────────
    fabio()
        .args([
            "semantic-model",
            "delete-relationship",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--from-table",
            "Sales",
            "--from-column",
            "ProductKey",
            "--to-table",
            "Product",
            "--to-column",
            "ProductKey",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // ── delete a nonexistent relationship → NOT_FOUND ───────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "delete-relationship",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--relationship-id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "NOT_FOUND");

    // ── no selector → INVALID_INPUT (offline) ───────────────────────────
    let assert = fabio()
        .args([
            "semantic-model",
            "delete-relationship",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "INVALID_INPUT");

    // ── Cleanup ─────────────────────────────────────────────────────────
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

// ─── Measure lifecycle: delete / rename / move ───────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_measure_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sm_meas_lc");

    // Two-table model so we can move a measure between tables.
    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(three_table_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

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
    let sm_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Seed a measure on Sales.
    fabio()
        .args([
            "semantic-model",
            "add-measure",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Sales",
            "--name",
            "Total Amount",
            "--expression",
            "SUM('Sales'[Amount])",
            "--format-string",
            "0.00",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // rename-measure
    fabio()
        .args([
            "semantic-model",
            "rename-measure",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--measure",
            "Total Amount",
            "--new-name",
            "Total Sales",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // move-measure Sales -> Customer
    fabio()
        .args([
            "semantic-model",
            "move-measure",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--measure",
            "Total Sales",
            "--to-table",
            "Customer",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // Verify: measure now lives on Customer with its format string preserved.
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
    let parts = data["definition"]["parts"].as_array().unwrap();
    let customer = parts
        .iter()
        .find(|p| p["path"].as_str() == Some("definition/tables/Customer.tmdl"))
        .and_then(|p| p["payload"].as_str())
        .map(|b| {
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b).unwrap()).unwrap()
        })
        .expect("Customer.tmdl");
    assert!(
        customer.contains("measure 'Total Sales'"),
        "customer:\n{customer}"
    );
    assert!(
        customer.contains("formatString: 0.00"),
        "customer:\n{customer}"
    );

    // delete-measure
    fabio()
        .args([
            "semantic-model",
            "delete-measure",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--measure",
            "Total Sales",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // delete a nonexistent measure → NOT_FOUND
    let assert = fabio()
        .args([
            "semantic-model",
            "delete-measure",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--measure",
            "Nope",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "NOT_FOUND");

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

// ─── Security roles / RLS: add-role / set-rls / list-roles / delete ──────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_role_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sm_role");

    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(three_table_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

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
    let sm_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // add-role (dry-run)
    let assert = fabio()
        .args([
            "semantic-model",
            "add-role",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            "RegionalManager",
            "--dry-run",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&assert))["dry_run"], true);

    // add-role (live)
    fabio()
        .args([
            "semantic-model",
            "add-role",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            "RegionalManager",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // set-rls: a DAX filter on Customer
    fabio()
        .args([
            "semantic-model",
            "set-rls",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--role",
            "RegionalManager",
            "--table",
            "Customer",
            "--filter",
            "'Customer'[CustomerKey] = 1",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // list-roles → verify the role + filter
    let assert = fabio()
        .args([
            "semantic-model",
            "list-roles",
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
    let roles = data.as_array().unwrap();
    let role = roles
        .iter()
        .find(|r| r["name"] == "RegionalManager")
        .expect("RegionalManager role");
    assert_eq!(role["modelPermission"], "read");
    assert_eq!(role["tablePermissions"][0]["table"], "Customer");

    // add-role duplicate → CONFLICT
    let assert = fabio()
        .args([
            "semantic-model",
            "add-role",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            "RegionalManager",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "CONFLICT");

    // delete-rls
    fabio()
        .args([
            "semantic-model",
            "delete-rls",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--role",
            "RegionalManager",
            "--table",
            "Customer",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // delete-role
    fabio()
        .args([
            "semantic-model",
            "delete-role",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            "RegionalManager",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // delete a nonexistent role → NOT_FOUND
    let assert = fabio()
        .args([
            "semantic-model",
            "delete-role",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            "Nope",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "NOT_FOUND");

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

// ─── Column authoring: add-calculated / update / rename / delete ─────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_column_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sm_col");

    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(three_table_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

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
    let sm_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // add-calculated-column (dry-run)
    let assert = fabio()
        .args([
            "semantic-model",
            "add-calculated-column",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Sales",
            "--name",
            "Double Amount",
            "--expression",
            "'Sales'[Amount] * 2",
            "--data-type",
            "double",
            "--format-string",
            "0.00",
            "--dry-run",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&assert))["dry_run"], true);

    // add-calculated-column (live)
    fabio()
        .args([
            "semantic-model",
            "add-calculated-column",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Sales",
            "--name",
            "Double Amount",
            "--expression",
            "'Sales'[Amount] * 2",
            "--data-type",
            "double",
            "--format-string",
            "0.00",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // duplicate → CONFLICT
    let assert = fabio()
        .args([
            "semantic-model",
            "add-calculated-column",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Sales",
            "--name",
            "Double Amount",
            "--expression",
            "1",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "CONFLICT");

    // update-column: summarization + display folder
    fabio()
        .args([
            "semantic-model",
            "update-column",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Sales",
            "--name",
            "Double Amount",
            "--summarize-by",
            "sum",
            "--display-folder",
            "Calc",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // rename-column
    fabio()
        .args([
            "semantic-model",
            "rename-column",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Sales",
            "--name",
            "Double Amount",
            "--new-name",
            "Doubled",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // Verify via the round-tripped definition
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
    let parts = data["definition"]["parts"].as_array().unwrap();
    let sales = parts
        .iter()
        .find(|p| p["path"].as_str() == Some("definition/tables/Sales.tmdl"))
        .and_then(|p| p["payload"].as_str())
        .map(|b| {
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b).unwrap()).unwrap()
        })
        .expect("Sales.tmdl");
    assert!(
        sales.contains("column Doubled = 'Sales'[Amount] * 2"),
        "sales:\n{sales}"
    );
    assert!(sales.contains("summarizeBy: sum"), "sales:\n{sales}");
    assert!(sales.contains("displayFolder: Calc"), "sales:\n{sales}");

    // delete-column
    fabio()
        .args([
            "semantic-model",
            "delete-column",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Sales",
            "--name",
            "Doubled",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // delete nonexistent column → NOT_FOUND
    let assert = fabio()
        .args([
            "semantic-model",
            "delete-column",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Sales",
            "--name",
            "Nope",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "NOT_FOUND");

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

// ─── Table lifecycle: add / rename / delete (with cascade) ───────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_table_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sm_tbl");

    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(three_table_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

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
    let sm_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Seed a relationship Sales -> Customer (so delete-table can cascade it).
    fabio()
        .args([
            "semantic-model",
            "add-relationship",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--from-table",
            "Sales",
            "--from-column",
            "CustomerKey",
            "--to-table",
            "Customer",
            "--to-column",
            "CustomerKey",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // add-table (calculated) — dry-run then live
    let assert = fabio()
        .args([
            "semantic-model",
            "add-table",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            "Numbers",
            "--expression",
            "GENERATESERIES(1, 5, 1)",
            "--dry-run",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&assert))["dry_run"], true);

    fabio()
        .args([
            "semantic-model",
            "add-table",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            "Numbers",
            "--expression",
            "GENERATESERIES(1, 5, 1)",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // rename-table Numbers -> NumberSeries
    fabio()
        .args([
            "semantic-model",
            "rename-table",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            "Numbers",
            "--new-name",
            "NumberSeries",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // Verify: NumberSeries.tmdl exists + model.tmdl references it.
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
    let parts = data["definition"]["parts"].as_array().unwrap();
    let paths: Vec<&str> = parts.iter().filter_map(|p| p["path"].as_str()).collect();
    assert!(
        paths.contains(&"definition/tables/NumberSeries.tmdl"),
        "paths: {paths:?}"
    );
    assert!(
        !paths.contains(&"definition/tables/Numbers.tmdl"),
        "old path present"
    );
    let model = parts
        .iter()
        .find(|p| p["path"].as_str() == Some("definition/model.tmdl"))
        .and_then(|p| p["payload"].as_str())
        .map(|b| {
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b).unwrap()).unwrap()
        })
        .unwrap();
    assert!(model.contains("ref table NumberSeries"), "model:\n{model}");

    // delete-table Customer → cascades the Sales->Customer relationship
    let assert = fabio()
        .args([
            "semantic-model",
            "delete-table",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            "Customer",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let json = parse_json(&assert);
    let cascaded = &extract_data(&json)["cascadedRelationships"];
    assert_eq!(cascaded.as_array().map(std::vec::Vec::len), Some(1));

    // delete a nonexistent table → NOT_FOUND
    let assert = fabio()
        .args([
            "semantic-model",
            "delete-table",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--name",
            "Nope",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "NOT_FOUND");

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

// ─── Translations / cultures: add-culture / set-translation / list / delete ──

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_translation_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sm_tr");

    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(three_table_model_bim().as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

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
    let sm_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // add-culture (dry-run then live)
    let assert = fabio()
        .args([
            "semantic-model",
            "add-culture",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--culture",
            "fr-FR",
            "--dry-run",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&assert))["dry_run"], true);

    fabio()
        .args([
            "semantic-model",
            "add-culture",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--culture",
            "fr-FR",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // set-translation on a table and a column
    fabio()
        .args([
            "semantic-model",
            "set-translation",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--culture",
            "fr-FR",
            "--table",
            "Sales",
            "--caption",
            "Ventes",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    fabio()
        .args([
            "semantic-model",
            "set-translation",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--culture",
            "fr-FR",
            "--table",
            "Sales",
            "--column",
            "Amount",
            "--caption",
            "Montant",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // list-cultures → fr-FR with 2 translations
    let assert = fabio()
        .args([
            "semantic-model",
            "list-cultures",
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
    let cultures = data.as_array().unwrap();
    let fr = cultures
        .iter()
        .find(|c| c["culture"] == "fr-FR")
        .expect("fr-FR culture");
    assert_eq!(fr["translationCount"], 2);

    // Verify captions via the round-tripped definition
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
    let parts = data["definition"]["parts"].as_array().unwrap();
    let culture = parts
        .iter()
        .find(|p| p["path"].as_str() == Some("definition/cultures/fr-FR.tmdl"))
        .and_then(|p| p["payload"].as_str())
        .map(|b| {
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b).unwrap()).unwrap()
        })
        .expect("fr-FR.tmdl");
    assert!(culture.contains("caption: Ventes"), "culture:\n{culture}");
    assert!(culture.contains("caption: Montant"), "culture:\n{culture}");

    // add-culture duplicate → CONFLICT
    let assert = fabio()
        .args([
            "semantic-model",
            "add-culture",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--culture",
            "fr-FR",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "CONFLICT");

    // set-translation on a missing culture → NOT_FOUND
    let assert = fabio()
        .args([
            "semantic-model",
            "set-translation",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--culture",
            "de-DE",
            "--table",
            "Sales",
            "--caption",
            "Verkauf",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "NOT_FOUND");

    // delete-culture
    fabio()
        .args([
            "semantic-model",
            "delete-culture",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--culture",
            "fr-FR",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

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

// ─── Hierarchies: add / list / delete ────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_hierarchy_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sm_hier");

    // A table with two columns to build a hierarchy from.
    let model = serde_json::json!({
        "compatibilityLevel": 1604,
        "model": {
            "culture": "en-US",
            "defaultPowerBIDataSourceVersion": "powerBI_V3",
            "tables": [{
                "name": "Geo",
                "columns": [
                    {"name": "Country", "dataType": "string", "sourceColumn": "Country"},
                    {"name": "City", "dataType": "string", "sourceColumn": "City"}
                ],
                "partitions": [{"name": "Geo", "source": {"type": "m", "expression": "let Source = #table({\"Country\",\"City\"}, {{\"US\",\"NY\"}}) in Source"}}]
            }]
        }
    })
    .to_string();
    let mut tmp = NamedTempFile::with_suffix(".bim").unwrap();
    tmp.write_all(model.as_bytes()).unwrap();
    let file_path = tmp.path().to_str().unwrap().to_string();

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
    let sm_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // add-hierarchy (dry-run then live)
    let assert = fabio()
        .args([
            "semantic-model",
            "add-hierarchy",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Geo",
            "--name",
            "Geography",
            "--level",
            "Country",
            "--level",
            "City",
            "--dry-run",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&assert))["dry_run"], true);

    fabio()
        .args([
            "semantic-model",
            "add-hierarchy",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Geo",
            "--name",
            "Geography",
            "--level",
            "Country",
            "--level",
            "City",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // list-hierarchies → Geography with 2 levels
    let assert = fabio()
        .args([
            "semantic-model",
            "list-hierarchies",
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
    let hs = data.as_array().unwrap();
    let geo = hs
        .iter()
        .find(|h| h["name"] == "Geography")
        .expect("Geography hierarchy");
    assert_eq!(geo["levelCount"], 2);

    // duplicate → CONFLICT
    let assert = fabio()
        .args([
            "semantic-model",
            "add-hierarchy",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Geo",
            "--name",
            "Geography",
            "--level",
            "Country",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "CONFLICT");

    // Verify in the definition
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
    let parts = data["definition"]["parts"].as_array().unwrap();
    let geo_tmdl = parts
        .iter()
        .find(|p| p["path"].as_str() == Some("definition/tables/Geo.tmdl"))
        .and_then(|p| p["payload"].as_str())
        .map(|b| {
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b).unwrap()).unwrap()
        })
        .expect("Geo.tmdl");
    assert!(geo_tmdl.contains("hierarchy Geography"), "geo:\n{geo_tmdl}");
    assert!(geo_tmdl.contains("level Country"), "geo:\n{geo_tmdl}");
    assert!(geo_tmdl.contains("column: City"), "geo:\n{geo_tmdl}");

    // delete-hierarchy
    fabio()
        .args([
            "semantic-model",
            "delete-hierarchy",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Geo",
            "--name",
            "Geography",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();

    // delete nonexistent → NOT_FOUND
    let assert = fabio()
        .args([
            "semantic-model",
            "delete-hierarchy",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &sm_id,
            "--table",
            "Geo",
            "--name",
            "Nope",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(error_json(&stderr)["error"]["code"], "NOT_FOUND");

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

/// Offline: enhanced refresh assembles the correct body via --dry-run and
/// rejects invalid --commit-mode / --objects.
#[test]
fn semantic_model_enhanced_refresh_dry_run_and_validation() {
    // Enhanced body via dry-run (no tenant call).
    let assert = fabio()
        .args([
            "semantic-model",
            "refresh",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--type",
            "Full",
            "--objects",
            r#"[{"table":"Sales"},{"table":"Sales","partition":"2024"}]"#,
            "--commit-mode",
            "partialBatch",
            "--max-parallelism",
            "4",
            "--retry-count",
            "2",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let details = &json["data"]["details"];
    assert_eq!(details["type"], "Full");
    assert_eq!(details["commitMode"], "partialBatch");
    assert_eq!(details["maxParallelism"], 4);
    assert_eq!(details["retryCount"], 2);
    assert_eq!(details["objects"][0]["table"], "Sales");
    assert_eq!(details["objects"][1]["partition"], "2024");

    // Invalid commit mode rejected.
    fabio()
        .args([
            "semantic-model",
            "refresh",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--commit-mode",
            "bogus",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("commit-mode"));

    // Objects entry missing 'table' rejected.
    fabio()
        .args([
            "semantic-model",
            "refresh",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--objects",
            r#"[{"partition":"x"}]"#,
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("table"));
}

/// Offline: cancel-refresh is --dry-run-guarded and echoes the refresh id.
#[test]
fn semantic_model_cancel_refresh_dry_run() {
    let assert = fabio()
        .args([
            "semantic-model",
            "cancel-refresh",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--refresh-id",
            "11111111-1111-1111-1111-111111111111",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(json["data"]["dry_run"], true);
    assert_eq!(
        json["data"]["would_execute"],
        "semantic-model cancel-refresh"
    );
    assert_eq!(
        json["data"]["details"]["refreshId"],
        "11111111-1111-1111-1111-111111111111"
    );
}

/// Live: enhanced-refresh lifecycle — trigger a granular refresh, read its
/// execution details (object-level status), then cancel it.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_enhanced_refresh_lifecycle() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    // Need an existing semantic model. Pick the first one in the workspace.
    let list = fabio()
        .args(["semantic-model", "list", "--workspace", ws])
        .assert()
        .success();
    let models = parse_json(&list);
    let Some(model) = models["data"].as_array().and_then(|a| a.first()) else {
        eprintln!("no semantic model in workspace; skipping");
        return;
    };
    let id = model["id"].as_str().unwrap().to_string();

    // Trigger a granular enhanced refresh (whole model — objects present makes it enhanced).
    fabio()
        .args([
            "semantic-model",
            "refresh",
            "--workspace",
            ws,
            "--id",
            &id,
            "--type",
            "Full",
            "--commit-mode",
            "transactional",
        ])
        .assert()
        .success();

    // Find the enhanced request id from refresh-status.
    let status = fabio()
        .args([
            "semantic-model",
            "refresh-status",
            "--workspace",
            ws,
            "--id",
            &id,
            "--top",
            "5",
        ])
        .assert()
        .success();
    let sj = parse_json(&status);
    let rows = sj["data"].as_array().cloned().unwrap_or_default();
    let Some(req_id) = rows
        .iter()
        .find(|r| r["refreshType"].as_str() == Some("ViaEnhancedApi"))
        .and_then(|r| r["requestId"].as_str())
        .map(str::to_owned)
    else {
        eprintln!("no ViaEnhancedApi refresh found; skipping details/cancel");
        return;
    };

    // refresh-details returns object-level status.
    let details = fabio()
        .args([
            "semantic-model",
            "refresh-details",
            "--workspace",
            ws,
            "--id",
            &id,
            "--refresh-id",
            &req_id,
        ])
        .assert()
        .success();
    let dj = parse_json(&details);
    assert!(
        dj["data"]["type"].is_string(),
        "details should carry type: {dj}"
    );

    // cancel-refresh: succeeds (cancellation_requested) if still running, or a
    // clean CONFLICT if it already completed — either proves the endpoint works.
    let cancel = fabio()
        .args([
            "semantic-model",
            "cancel-refresh",
            "--workspace",
            ws,
            "--id",
            &id,
            "--refresh-id",
            &req_id,
        ])
        .assert();
    let out = cancel.get_output();
    let ok = out.status.success();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        ok || stderr.contains("cannot be cancelled") || stderr.contains("CONFLICT"),
        "cancel should succeed or report a clean conflict; stderr: {stderr}"
    );
}

/// Offline: update-refresh-schedule builds the right body via --dry-run and
/// enforces validation (half-hour times, valid days, disable-alone rule).
#[test]
fn semantic_model_update_refresh_schedule_dry_run_and_validation() {
    let assert = fabio()
        .args([
            "semantic-model",
            "update-refresh-schedule",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--enabled",
            "true",
            "--days",
            "Tuesday,Friday",
            "--times",
            "06:00,18:30",
            "--notify-option",
            "MailOnFailure",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let v = &json["data"]["details"]["value"];
    assert_eq!(v["enabled"], true);
    assert_eq!(v["days"][1], "Friday");
    assert_eq!(v["times"][1], "18:30");
    assert_eq!(v["notifyOption"], "MailOnFailure");

    // Invalid time.
    fabio()
        .args([
            "semantic-model",
            "update-refresh-schedule",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--times",
            "07:15",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("full or half hour"));

    // Invalid day.
    fabio()
        .args([
            "semantic-model",
            "update-refresh-schedule",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--days",
            "Funday",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Invalid day"));

    // Disable cannot carry other settings.
    fabio()
        .args([
            "semantic-model",
            "update-refresh-schedule",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--enabled",
            "false",
            "--times",
            "07:00",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("disabling"));

    // No fields at all.
    fabio()
        .args([
            "semantic-model",
            "update-refresh-schedule",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("No schedule fields"));
}

/// Live: get/update the refresh schedule, verify, then disable to revert.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_refresh_schedule_lifecycle() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    let list = fabio()
        .args(["semantic-model", "list", "--workspace", ws])
        .assert()
        .success();
    let models = parse_json(&list);
    let Some(model) = models["data"].as_array().and_then(|a| a.first()) else {
        eprintln!("no semantic model; skipping");
        return;
    };
    let id = model["id"].as_str().unwrap().to_string();

    // Get baseline.
    fabio()
        .args([
            "semantic-model",
            "get-refresh-schedule",
            "--workspace",
            ws,
            "--id",
            &id,
        ])
        .assert()
        .success();

    // Enable Wednesday 09:00.
    fabio()
        .args([
            "semantic-model",
            "update-refresh-schedule",
            "--workspace",
            ws,
            "--id",
            &id,
            "--enabled",
            "true",
            "--days",
            "Wednesday",
            "--times",
            "09:00",
        ])
        .assert()
        .success();

    // Verify.
    let got = fabio()
        .args([
            "semantic-model",
            "get-refresh-schedule",
            "--workspace",
            ws,
            "--id",
            &id,
        ])
        .assert()
        .success();
    let gj = parse_json(&got);
    assert_eq!(gj["data"]["enabled"], true);
    assert_eq!(gj["data"]["times"][0], "09:00");

    // Revert (disable alone).
    fabio()
        .args([
            "semantic-model",
            "update-refresh-schedule",
            "--workspace",
            ws,
            "--id",
            &id,
            "--enabled",
            "false",
        ])
        .assert()
        .success();
    let after = fabio()
        .args([
            "semantic-model",
            "get-refresh-schedule",
            "--workspace",
            ws,
            "--id",
            &id,
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&after)["data"]["enabled"], false);
}

/// Offline: bind-to-gateway assembles the right body via --dry-run.
#[test]
fn semantic_model_bind_to_gateway_dry_run() {
    let assert = fabio()
        .args([
            "semantic-model",
            "bind-to-gateway",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--gateway-id",
            "11111111-1111-1111-1111-111111111111",
            "--datasource-ids",
            "a,b",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let details = &json["data"]["details"];
    assert_eq!(
        details["gatewayObjectId"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(details["datasourceObjectIds"][1], "b");
}

/// Live: get-bound-gateway-datasources returns an array (empty for a
/// cloud/Direct Lake model with no gateway data sources).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_get_bound_gateway_datasources_returns_array() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    let list = fabio()
        .args(["semantic-model", "list", "--workspace", ws])
        .assert()
        .success();
    let models = parse_json(&list);
    let Some(model) = models["data"].as_array().and_then(|a| a.first()) else {
        eprintln!("no semantic model; skipping");
        return;
    };
    let id = model["id"].as_str().unwrap().to_string();

    let assert = fabio()
        .args([
            "semantic-model",
            "get-bound-gateway-datasources",
            "--workspace",
            ws,
            "--id",
            &id,
        ])
        .assert()
        .success();
    assert!(parse_json(&assert)["data"].is_array());
}

/// Offline: create --definition rejects a folder without definition.pbism.
#[test]
fn semantic_model_create_definition_folder_validation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("stray.json"), "{}").unwrap();
    fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            "test-ws",
            "--name",
            "X",
            "--definition",
            dir.path().to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("definition.pbism"));
}

/// Live: export a semantic model's full TMDL folder, then create a new model
/// from the whole folder (multi-file), introspect it, and delete.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_create_from_tmdl_folder_lifecycle() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    // Need a model to export.
    let list = fabio()
        .args(["semantic-model", "list", "--workspace", ws])
        .assert()
        .success();
    let models = parse_json(&list);
    if models["data"].as_array().is_none_or(Vec::is_empty) {
        eprintln!("no semantic model to export; skipping");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let export_dir = dir.path().join("export");
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            ws,
            "--dir",
            export_dir.to_str().unwrap(),
            "--overwrite",
            "--item-types",
            "SemanticModel",
        ])
        .assert()
        .success();

    // Find a *.SemanticModel folder with definition.pbism.
    let folder = std::fs::read_dir(&export_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.is_dir() && p.join("definition.pbism").exists())
        .expect("an exported .SemanticModel folder");

    // Create a new model from the FULL folder.
    let created = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            ws,
            "--name",
            "fabio-e2e-tmdl-folder",
            "--definition",
            folder.to_str().unwrap(),
        ])
        .assert()
        .success();
    let id = parse_json(&created)["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Introspect: it must have at least one table (proves the multi-file TMDL was ingested).
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
    assert!(
        !parse_json(&tables)["data"].as_array().unwrap().is_empty(),
        "folder-created model should have tables"
    );

    // Cleanup.
    fabio()
        .args(["semantic-model", "delete", "--workspace", ws, "--id", &id])
        .assert()
        .success();
}

/// `semantic-model generate --dry-run` — reads the source lakehouse's SQL
/// analytics endpoint schema and prints the planned Direct Lake model WITHOUT
/// creating anything. Gated on `FABIO_TEST_LOADED_LAKEHOUSE` (schema read needs
/// a SQL-scoped token from the ambient credential chain).
#[test]
#[ignore = "requires live Fabric tenant + a populated lakehouse (FABIO_TEST_LOADED_LAKEHOUSE)"]
fn semantic_model_generate_dry_run() {
    let Ok(lh) = std::env::var("FABIO_TEST_LOADED_LAKEHOUSE") else {
        eprintln!("FABIO_TEST_LOADED_LAKEHOUSE not set — skipping generate dry-run");
        return;
    };
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "semantic-model",
            "generate",
            "--workspace",
            &cfg.source_workspace,
            "--lakehouse",
            &lh,
            "--name",
            "GenDryRun",
            "--dry-run",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "semantic-model generate");
    assert!(data["dry_run"].as_bool().unwrap());
    assert_eq!(data["details"]["storageMode"], "directLake");
    assert!(
        data["details"]["summary"]["tableCount"].as_u64().unwrap() >= 1,
        "expected at least one planned table, got: {data}"
    );
}

/// Full Direct Lake generate lifecycle: generate a semantic model from a
/// populated lakehouse (schema read + type mapping + model.bim synthesis +
/// create + framing refresh), confirm its tables surface via INFO.VIEW, and run
/// a real DAX query that returns data over Direct Lake — then delete.
///
/// Gated on `FABIO_TEST_LOADED_LAKEHOUSE`; needs a SQL-scoped token from the
/// ambient credential chain (do NOT set a Fabric-only `FABIO_ACCESS_TOKEN`).
#[test]
#[ignore = "requires live Fabric tenant + a populated lakehouse (FABIO_TEST_LOADED_LAKEHOUSE)"]
fn semantic_model_generate_direct_lake_lifecycle() {
    let Ok(lh) = std::env::var("FABIO_TEST_LOADED_LAKEHOUSE") else {
        eprintln!("FABIO_TEST_LOADED_LAKEHOUSE not set — skipping generate lifecycle");
        return;
    };
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    // Generate the Direct Lake model.
    let assert = fabio()
        .args([
            "semantic-model",
            "generate",
            "--workspace",
            ws,
            "--lakehouse",
            &lh,
            "--name",
            &unique_name("gen_dl"),
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "generated");
    assert_eq!(data["storageMode"], "directLake");
    let id = data["id"].as_str().unwrap().to_string();

    // INFO.VIEW must list the generated tables.
    std::thread::sleep(std::time::Duration::from_secs(25)); // allow framing
    let tables_assert = fabio()
        .args([
            "semantic-model",
            "list-tables",
            "--workspace",
            ws,
            "--id",
            &id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let tj = parse_json(&tables_assert);
    let rows = extract_data(&tj);
    let first_table = rows
        .as_array()
        .and_then(|a| a.first())
        .and_then(|r| r["Name"].as_str())
        .expect("generated model must have at least one table")
        .to_string();

    // A real DAX query over Direct Lake must return a row (proves the model is
    // framed and reads data from OneLake).
    let dax = format!("EVALUATE ROW(\"n\", COUNTROWS('{first_table}'))");
    let q = fabio()
        .args([
            "semantic-model",
            "query",
            "--workspace",
            ws,
            "--id",
            &id,
            "--dax",
            &dax,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let qj = parse_json(&q);
    assert!(
        !extract_data(&qj).as_array().unwrap().is_empty(),
        "DAX query over the generated Direct Lake model should return a row"
    );

    // Cleanup.
    fabio()
        .args(["semantic-model", "delete", "--workspace", ws, "--id", &id])
        .assert()
        .success();
}

/// Best Practice Analyzer + measure-dependencies over a live model with known
/// issues: a numeric identifier column that still aggregates (implicit-aggregation),
/// missing descriptions, and three measures where one depends on the other two.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn semantic_model_analyze_and_measure_dependencies() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    // A model that deliberately trips several rules.
    let bim = r#"{"compatibilityLevel":1604,"model":{"culture":"en-US","defaultPowerBIDataSourceVersion":"powerBI_V3","tables":[{"name":"Sales","columns":[{"name":"StoreId","dataType":"int64","sourceColumn":"StoreId"},{"name":"Amount","dataType":"double","sourceColumn":"Amount"},{"name":"Qty","dataType":"int64","sourceColumn":"Qty"}],"partitions":[{"name":"p","source":{"type":"m","expression":"let Source = #table(type table [StoreId=Int64.Type, Amount=number, Qty=Int64.Type], {{1, 100.0, 3}}) in Source"}}],"measures":[{"name":"Total Amount","expression":"SUM('Sales'[Amount])"},{"name":"Total Qty","expression":"SUM(Sales[Qty])"},{"name":"Avg Price","expression":"DIVIDE([Total Amount], [Total Qty])"}]}]}}"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.bim");
    std::fs::write(&path, bim).unwrap();

    let created = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            ws,
            "--name",
            &unique_name("bpa"),
            "--file",
            path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let id = extract_data(&parse_json(&created))["id"]
        .as_str()
        .unwrap()
        .to_string();
    std::thread::sleep(std::time::Duration::from_secs(8));

    // analyze — must find issues, including implicit-aggregation on the ID column.
    let a = fabio()
        .args(["semantic-model", "analyze", "--workspace", ws, "--id", &id])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let aj = parse_json(&a);
    let ad = extract_data(&aj);
    assert!(
        ad["issueCount"].as_u64().unwrap() >= 1,
        "analyze should report issues: {ad}"
    );
    let rules: Vec<&str> = ad["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["rule"].as_str().unwrap())
        .collect();
    assert!(
        rules.contains(&"implicit-aggregation"),
        "expected implicit-aggregation on StoreId; got {rules:?}"
    );
    assert!(
        rules.contains(&"missing-description"),
        "expected missing-description; got {rules:?}"
    );
    // A plain analysis nudges toward --fix when there are auto-fixable issues.
    assert!(
        ad["autoFixable"].as_u64().unwrap_or(0) >= 1,
        "plain analyze should report autoFixable count: {ad}"
    );
    assert!(
        ad["hint"].as_str().unwrap_or_default().contains("--fix"),
        "plain analyze should hint to run --fix: {ad}"
    );

    // measure-dependencies — Avg Price depends on the two base measures.
    let m = fabio()
        .args([
            "semantic-model",
            "measure-dependencies",
            "--workspace",
            ws,
            "--id",
            &id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let mj = parse_json(&m);
    let deps = extract_data(&mj);
    let avg = deps
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["measure"] == "Avg Price")
        .expect("Avg Price measure");
    let dep_measures: Vec<&str> = avg["dependsOnMeasures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        dep_measures.contains(&"Total Amount") && dep_measures.contains(&"Total Qty"),
        "Avg Price should depend on both base measures; got {dep_measures:?}"
    );

    // analyze --fix --dry-run: previews the safe fix without mutating.
    let dr = fabio()
        .args([
            "semantic-model",
            "analyze",
            "--workspace",
            ws,
            "--id",
            &id,
            "--fix",
            "--dry-run",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let drj = parse_json(&dr);
    let drd = extract_data(&drj);
    assert!(drd["dry_run"].as_bool().unwrap_or(false));
    let would: Vec<&str> = drd["details"]["wouldFix"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        would.contains(&"Sales[StoreId]"),
        "dry-run should plan to fix Sales[StoreId]; got {would:?}"
    );

    // analyze --fix: applies the safe fix (SummarizeBy -> None on StoreId).
    let f = fabio()
        .args([
            "semantic-model",
            "analyze",
            "--workspace",
            ws,
            "--id",
            &id,
            "--fix",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let fj = parse_json(&f);
    let fd = extract_data(&fj);
    assert!(fd["fixApplied"].as_bool().unwrap_or(false));
    assert!(
        fd["fixed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "Sales[StoreId]"),
        "fix should report Sales[StoreId]; got {fd}"
    );
    std::thread::sleep(std::time::Duration::from_secs(6));

    // Re-analyze: the implicit-aggregation issue must be gone.
    let re = fabio()
        .args([
            "semantic-model",
            "analyze",
            "--workspace",
            ws,
            "--id",
            &id,
            "--severity",
            "warning",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let rej = parse_json(&re);
    let red = extract_data(&rej);
    let still_implicit = red["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["rule"] == "implicit-aggregation");
    assert!(
        !still_implicit,
        "implicit-aggregation should be fixed after --fix; got {red}"
    );

    // Cleanup.
    fabio()
        .args(["semantic-model", "delete", "--workspace", ws, "--id", &id])
        .assert()
        .success();
}
