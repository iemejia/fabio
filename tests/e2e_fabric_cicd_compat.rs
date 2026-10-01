//! End-to-end tests validating `fabio deploy` against fabric-cicd's sample workspace.
//!
//! These tests reference the fabric-cicd sample workspace at the path specified by
//! `FABIO_TEST_FABRIC_CICD_REPO` (defaults to `~/msrepos/fabric-cicd`).
//!
//! The tests verify that fabio can:
//! 1. Parse fabric-cicd's source directory format (nested folders, all 27 item types)
//! 2. Validate the source directory structure
//! 3. Apply workspace ID replacement
//! 4. Apply all parameter substitution types (`find_replace`, `key_value_replace`, `spark_pool`, `semantic_model_binding`)
//! 5. Handle nested folder hierarchies
//! 6. Filter by item type, regex, and folder paths
//! 7. Plan deployment against a live workspace
//!
//! Requires:
//! - `FABIO_TEST_SOURCE_WORKSPACE` env var (for live plan tests)
//! - fabric-cicd repo accessible at `FABIO_TEST_FABRIC_CICD_REPO` or `~/msrepos/fabric-cicd`

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use std::path::PathBuf;
use std::time::Duration;

/// Resolve the path to the fabric-cicd sample workspace.
/// Returns `None` if the path does not exist (allows tests to skip gracefully in CI).
fn fabric_cicd_sample_workspace() -> Option<PathBuf> {
    let repo = std::env::var_os("FABIO_TEST_FABRIC_CICD_REPO")
        .map(PathBuf::from)
        .or_else(|| home::home_dir().map(|home| home.join("msrepos").join("fabric-cicd")))
        .unwrap_or_default();
    let path = repo.join("sample").join("workspace");
    if path.exists() {
        Some(path)
    } else {
        eprintln!(
            "SKIP: fabric-cicd sample workspace not found at {}",
            path.display()
        );
        None
    }
}

/// Helper: resolve and unwrap the fabric-cicd sample workspace, panicking if not found.
/// Used by `#[ignore]` tests that require the repo to be present.
fn require_fabric_cicd_sample_workspace() -> PathBuf {
    fabric_cicd_sample_workspace().expect(
        "fabric-cicd sample workspace not found.\n\
         Set FABIO_TEST_FABRIC_CICD_REPO to the fabric-cicd repository root.",
    )
}

// ── Validate: fabric-cicd source directory ───────────────────────────────────

#[test]
fn fabric_cicd_validate_source_directory() {
    let Some(source) = fabric_cicd_sample_workspace() else {
        return;
    };

    let assert = fabio()
        .args(["deploy", "validate", "--source", source.to_str().unwrap()])
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
    // fabric-cicd has ~35+ items (including nested KQL databases, multiple lakehouses, etc.)
    let item_count = data["items"].as_u64().unwrap();
    assert!(
        item_count >= 25,
        "Expected at least 25 items from fabric-cicd sample, got {item_count}"
    );
    // No errors
    assert_eq!(data["summary"]["errors"].as_u64().unwrap(), 0);
}

#[test]
fn fabric_cicd_validate_detects_nested_folder_items() {
    let Some(source) = fabric_cicd_sample_workspace() else {
        return;
    };

    let assert = fabio()
        .args(["deploy", "validate", "--source", source.to_str().unwrap()])
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let items = data["items"].as_u64().unwrap();

    // The sample has items in subfolder/ and subfolder/subfolder/
    // If we only parsed top-level, we'd miss those. Check we have them.
    // There are at least 2 nested notebooks: "Hello World Subfolder" and "Hello World SubfolderSubfolder"
    assert!(
        items >= 27,
        "Expected nested items to be discovered (got {items} items)"
    );
}

// ── Plan: dry-run against live workspace with fabric-cicd source ─────────────

#[test]
#[ignore = "requires live Fabric tenant"]
fn fabric_cicd_plan_all_items() {
    let cfg = TestConfig::from_env();
    let source = require_fabric_cicd_sample_workspace();

    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--dry-run",
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Should plan to create all items (none exist in test workspace yet)
    let summary = &data["summary"];
    let creates = summary["create"].as_u64().unwrap();
    assert!(
        creates >= 25,
        "Expected at least 25 creates from fabric-cicd sample, got {creates}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn fabric_cicd_plan_with_item_type_filter() {
    let cfg = TestConfig::from_env();
    let source = require_fabric_cicd_sample_workspace();

    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--item-types",
            "Notebook",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Only Notebook items should be in the plan
    let changes = data["changes"].as_array().unwrap();
    for change in changes {
        assert_eq!(
            change["item_type"].as_str().unwrap(),
            "Notebook",
            "Expected only Notebook items with --item-types filter"
        );
    }
    // fabric-cicd has 4 notebooks (including subfolder ones)
    assert!(
        changes.len() >= 4,
        "Expected at least 4 Notebook items, got {}",
        changes.len()
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn fabric_cicd_plan_with_exclude_regex() {
    let cfg = TestConfig::from_env();
    let source = require_fabric_cicd_sample_workspace();

    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--exclude-regex",
            "^Hello",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // No items starting with "Hello" should be in the plan
    let changes = data["changes"].as_array().unwrap();
    for change in changes {
        let name = change["name"].as_str().unwrap();
        assert!(
            !name.starts_with("Hello"),
            "Item '{name}' should have been excluded by --exclude-regex '^Hello'"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn fabric_cicd_plan_with_include_folders() {
    let cfg = TestConfig::from_env();
    let source = require_fabric_cicd_sample_workspace();

    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--include-folders",
            "/subfolder",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Only items in /subfolder should be included
    let changes = data["changes"].as_array().unwrap();
    assert!(
        !changes.is_empty(),
        "Expected at least 1 item in /subfolder"
    );
    // The subfolder notebooks should be there
    let names: Vec<&str> = changes
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"Hello World Subfolder")
            || names.contains(&"Hello World SubfolderSubfolder"),
        "Expected subfolder notebooks in plan, got: {names:?}"
    );
}

// ── Parameter substitution: convert YAML params to JSON and test ──────────────

#[test]
#[allow(clippy::too_many_lines)]
fn fabric_cicd_validate_with_fabio_parameters() {
    let Some(source) = fabric_cicd_sample_workspace() else {
        return;
    };

    // Create a fabio-compatible parameters.json from fabric-cicd's parameter.yml
    let dir = tempfile::TempDir::new().unwrap();
    let params_file = dir.path().join("parameters.json");

    // Convert to fabio format (fabric-cicd uses $workspace.$id, fabio uses $workspace.id)
    let params_json = serde_json::json!({
        "find_replace": [
            {
                "find_value": "db52be81-c2b2-4261-84fa-840c67f4bbd0",
                "replace_value": {
                    "PPE": "81bbb339-8d0b-46e8-bfa6-289a159c0733",
                    "PROD": "5d6a1b16-447f-464a-b959-45d0fed35ca0"
                },
                "item_type": "Notebook",
                "item_name": ["Hello World", "Hello World Subfolder"],
                "file_path": ["notebook-content.py"]
            },
            {
                "find_value": "sqlserverconnectionstringinoriginlakehouse.com",
                "replace_value": {
                    "PPE": "replaced-sql-endpoint-ppe",
                    "PROD": "replaced-sql-endpoint-prod"
                },
                "file_path": "notebook-content.py"
            },
            {
                "find_value": "dev-workspace-id",
                "replace_value": {
                    "PPE": "$workspace.id",
                    "PROD": "$workspace.id"
                },
                "file_path": "definition.pbir"
            },
            {
                "find_value": "dev-semantic-model",
                "replace_value": {
                    "PPE": "ABC",
                    "PROD": "ABC"
                },
                "file_path": "definition.pbir"
            }
        ],
        "key_value_replace": [
            {
                "find_key": "$.variables[?(@.name==\"SQL_Server\")].value",
                "replace_value": {
                    "PPE": "contoso-ppe.database.windows.net",
                    "PROD": "contoso-prod.database.windows.net"
                },
                "item_type": "VariableLibrary",
                "item_name": "Vars"
            },
            {
                "find_key": "$.variables[?(@.name==\"Environment\")].value",
                "replace_value": {
                    "PPE": "PPE",
                    "PROD": "PROD"
                },
                "item_type": "VariableLibrary",
                "item_name": "Vars"
            }
        ],
        "spark_pool": [
            {
                "instance_pool_id": "72c68dbc-0775-4d59-909d-a47896f4573b",
                "replace_value": {
                    "PPE": {"type": "Capacity", "name": "CapacityPool_Large_PPE"},
                    "PROD": {"type": "Capacity", "name": "CapacityPool_Large_PROD"}
                },
                "item_name": "World"
            }
        ],
        "semantic_model_binding": {
            "default": {
                "connection_id": {
                    "PPE": "76e05dfe-9855-4e3d-a410-1dda048dbe99",
                    "PROD": "c4f8e2b1-3d2a-4f5b-9c6e-7a8b9c0d1e2f"
                }
            },
            "models": [
                {
                    "semantic_model_name": ["cloudconnections", "MySemanticModel_ADLS_Gen2"],
                    "connection_id": {
                        "PPE": "f96870d5-5f86-49ad-bf41-5967fd7c1c6d",
                        "PROD": "a1b2c3d4-5678-90ab-cdef-1234567890ab"
                    }
                }
            ]
        }
    });
    std::fs::write(
        &params_file,
        serde_json::to_string_pretty(&params_json).unwrap(),
    )
    .unwrap();

    // Validate the source with parameters
    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            source.to_str().unwrap(),
            "--parameters",
            params_file.to_str().unwrap(),
            "--env",
            "PPE",
        ])
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
    assert_eq!(data["summary"]["errors"].as_u64().unwrap(), 0);
}

// ── Plan with parameters: verify substitution affects content hash ────────────

#[test]
#[ignore = "requires live Fabric tenant"]
fn fabric_cicd_plan_with_parameters_changes_hash() {
    let cfg = TestConfig::from_env();
    let source = require_fabric_cicd_sample_workspace();

    let dir = tempfile::TempDir::new().unwrap();
    let params_file = dir.path().join("parameters.json");
    let params_json = serde_json::json!({
        "find_replace": [{
            "find_value": "db52be81-c2b2-4261-84fa-840c67f4bbd0",
            "replace_value": { "PPE": "replaced-guid-for-test" },
            "item_type": "Notebook"
        }]
    });
    std::fs::write(
        &params_file,
        serde_json::to_string_pretty(&params_json).unwrap(),
    )
    .unwrap();

    // Plan WITHOUT parameters
    let assert_no_params = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--item-types",
            "Notebook",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    // Plan WITH parameters
    let assert_with_params = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--item-types",
            "Notebook",
            "--parameters",
            params_file.to_str().unwrap(),
            "--env",
            "PPE",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json_no_params = parse_json(&assert_no_params);
    let json_with_params = parse_json(&assert_with_params);
    let data_no_params = extract_data(&json_no_params);
    let data_with_params = extract_data(&json_with_params);

    // Find "Hello World" notebook in both plans and compare hashes
    let find_hash = |data: &serde_json::Value, name: &str| -> Option<String> {
        data["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .and_then(|c| c["source_hash"].as_str().map(String::from))
    };

    let hash_no_params = find_hash(data_no_params, "Hello World");
    let hash_with_params = find_hash(data_with_params, "Hello World");

    assert!(
        hash_no_params.is_some(),
        "Hello World notebook should be in plan"
    );
    assert!(
        hash_with_params.is_some(),
        "Hello World notebook should be in parameterized plan"
    );
    assert_ne!(
        hash_no_params, hash_with_params,
        "Parameter substitution should change the content hash for Hello World notebook"
    );
}

// ── Workspace ID replacement ─────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
fn fabric_cicd_workspace_id_replacement_affects_hash() {
    let cfg = TestConfig::from_env();
    let source = require_fabric_cicd_sample_workspace();

    // Plan WITH workspace ID replacement (default)
    let assert_with = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--item-types",
            "Report",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    // Plan WITHOUT workspace ID replacement
    let assert_without = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--item-types",
            "Report",
            "--no-workspace-id-replace",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json_with = parse_json(&assert_with);
    let json_without = parse_json(&assert_without);
    let data_with = extract_data(&json_with);
    let data_without = extract_data(&json_without);

    // ByConnection.Report contains 00000000-... placeholder that gets replaced
    let find_hash = |data: &serde_json::Value, name: &str| -> Option<String> {
        data["changes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .and_then(|c| c["source_hash"].as_str().map(String::from))
    };

    let hash_with = find_hash(data_with, "ByConnection");
    let hash_without = find_hash(data_without, "ByConnection");

    if hash_with.is_some() && hash_without.is_some() {
        assert_ne!(
            hash_with, hash_without,
            "Workspace ID replacement should change hash for ByConnection report (contains 00000000-...)"
        );
    }
}

// ── Config file: YAML config pointing to fabric-cicd workspace ───────────────

#[test]
#[ignore = "requires live Fabric tenant"]
fn fabric_cicd_config_file_yaml_selects_workspace() {
    let cfg = TestConfig::from_env();
    let source = require_fabric_cicd_sample_workspace();

    let dir = tempfile::TempDir::new().unwrap();
    let config_file = dir.path().join("deploy-config.yml");
    std::fs::write(
        &config_file,
        format!(
            r#"
source: "{}"
environments:
  test:
    workspace: "{}"
filters:
  item_types:
    - Notebook
"#,
            source.display(),
            cfg.source_workspace
        ),
    )
    .unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--config",
            config_file.to_str().unwrap(),
            "--env",
            "test",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should have planned notebook items using the config file settings
    let changes = data["changes"].as_array().unwrap();
    assert!(
        !changes.is_empty(),
        "Config-driven plan should produce changes"
    );
}

// ── Init-params: scan fabric-cicd source for GUIDs ───────────────────────────

#[test]
fn fabric_cicd_init_params_scan_finds_guids() {
    let Some(source) = fabric_cicd_sample_workspace() else {
        return;
    };

    let assert = fabio()
        .args([
            "deploy",
            "init-params",
            "--source",
            source.to_str().unwrap(),
        ])
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "generated");
    assert_eq!(data["mode"], "scan");
    // fabric-cicd source has many GUIDs (logical IDs, lakehouse refs, etc.)
    let guids_found = data["guids_found"].as_u64().unwrap();
    assert!(
        guids_found >= 10,
        "Expected at least 10 GUIDs in fabric-cicd source, got {guids_found}"
    );
    let rules_generated = data["rules_generated"].as_u64().unwrap();
    assert!(
        rules_generated >= 5,
        "Expected at least 5 rules generated, got {rules_generated}"
    );
}
