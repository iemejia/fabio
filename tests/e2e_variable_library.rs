use assert_cmd::Command;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serial_test::serial;

mod common;
use common::{TestConfig, extract_data, parse_json, unique_name};

fn fabio() -> Command {
    Command::cargo_bin("fabio").unwrap()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn variable_library_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "variable-library",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(json["data"].is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn variable_library_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "variable-library",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-varlib",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(json["data"]["would_execute"], "variable-library create");
}

/// Full lifecycle test: create a variable library, update its definition
/// with value sets, list value sets, activate one, verify, then delete.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn variable_library_lifecycle_with_value_sets() {
    let cfg = TestConfig::from_env();
    let vl_name = unique_name("fabio-test-vl");

    // --- 1. Create variable library ---
    let assert = fabio()
        .args([
            "variable-library",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &vl_name,
            "--description",
            "CI/CD test variable library",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let vl_id = data["id"].as_str().expect("missing id in create response");
    assert_eq!(data["displayName"], vl_name);

    // --- 2. Update definition with variables and a "prod" value set ---
    // The API requires separate parts: variables.json, settings.json, valueSets/<name>.json
    // Value sets use "variableOverrides" (not "values") per the official schema.
    let variables_json = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/variableLibrary/definition/variables/1.0.0/schema.json",
        "variables": [
            {
                "name": "database_server",
                "type": "String",
                "value": "dev-server.database.windows.net",
                "note": "SQL server hostname"
            },
            {
                "name": "database_name",
                "type": "String",
                "value": "SalesDev",
                "note": "Database name"
            }
        ]
    });

    let settings_json = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/variableLibrary/definition/settings/1.0.0/schema.json",
        "valueSetsOrder": ["prod"]
    });

    let prod_value_set_json = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/variableLibrary/definition/valueSet/1.0.0/schema.json",
        "name": "prod",
        "variableOverrides": [
            { "name": "database_server", "value": "prod-server.database.windows.net" },
            { "name": "database_name", "value": "SalesProd" }
        ]
    });

    let vars_b64 = BASE64.encode(serde_json::to_string(&variables_json).unwrap().as_bytes());
    let settings_b64 = BASE64.encode(serde_json::to_string(&settings_json).unwrap().as_bytes());
    let prod_vs_b64 = BASE64.encode(
        serde_json::to_string(&prod_value_set_json)
            .unwrap()
            .as_bytes(),
    );

    let definition_body = serde_json::json!({
        "definition": {
            "parts": [
                { "path": "variables.json", "payload": vars_b64, "payloadType": "InlineBase64" },
                { "path": "settings.json", "payload": settings_b64, "payloadType": "InlineBase64" },
                { "path": "valueSets/prod.json", "payload": prod_vs_b64, "payloadType": "InlineBase64" }
            ]
        }
    });
    let def_content = serde_json::to_string(&definition_body).unwrap();

    // Use the REST passthrough to call updateDefinition with the full envelope
    // (rest call with --poll handles LRO polling)
    fabio()
        .args([
            "rest",
            "call",
            "--method",
            "post",
            "--path",
            &format!(
                "/workspaces/{}/variableLibraries/{}/updateDefinition",
                cfg.source_workspace, vl_id
            ),
            "--body",
            &def_content,
            "--poll",
        ])
        .assert()
        .success();

    // --- 3. Show the variable library (check properties) ---
    let assert = fabio()
        .args([
            "variable-library",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], vl_name);
    // Active value set should be "Default value set" initially (Fabric default name)
    let active = data
        .pointer("/properties/activeValueSetName")
        .and_then(|v| v.as_str())
        .unwrap_or("Default");
    assert!(
        active.contains("Default") || active.is_empty(),
        "Expected active value set containing 'Default' or empty, got: {active}"
    );

    // --- 4. List value sets ---
    let assert = fabio()
        .args([
            "variable-library",
            "list-value-sets",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let value_sets = json["data"].as_array().expect("data should be array");
    // Should have at least Default + prod
    assert!(
        value_sets.len() >= 2,
        "Expected at least 2 value sets (Default + prod), got {}",
        value_sets.len()
    );
    // Check Default exists
    assert!(
        value_sets.iter().any(|vs| vs["name"] == "Default"),
        "Default value set not found"
    );
    // Check prod exists
    assert!(
        value_sets.iter().any(|vs| vs["name"] == "prod"),
        "prod value set not found"
    );

    // --- 5. Activate value set "prod" ---
    let assert = fabio()
        .args([
            "variable-library",
            "activate-value-set",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
            "--value-set",
            "prod",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    // The PATCH response should include the updated properties
    let active_after = data
        .pointer("/properties/activeValueSetName")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(active_after, "prod", "Value set should now be 'prod'");

    // --- 6. Verify active via show ---
    let assert = fabio()
        .args([
            "variable-library",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let active_confirmed = data
        .pointer("/properties/activeValueSetName")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        active_confirmed, "prod",
        "Show should confirm 'prod' is active"
    );

    // --- 7. List value sets again — prod should be marked active ---
    let assert = fabio()
        .args([
            "variable-library",
            "list-value-sets",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let value_sets = json["data"].as_array().expect("data should be array");
    let prod_set = value_sets
        .iter()
        .find(|vs| vs["name"] == "prod")
        .expect("prod value set not found");
    assert_eq!(prod_set["active"], true, "prod should be marked active");
    let default_set = value_sets
        .iter()
        .find(|vs| vs["name"] == "Default")
        .expect("Default value set not found");
    assert_eq!(default_set["active"], false, "Default should NOT be active");

    // --- 8. Get definition (with decode) to verify content ---
    let assert = fabio()
        .args([
            "variable-library",
            "get-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
            "--decode",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should have definition.parts
    assert!(
        data.pointer("/definition/parts").is_some(),
        "Should have definition.parts"
    );

    // --- 9. Activate-value-set dry-run ---
    let assert = fabio()
        .args([
            "variable-library",
            "activate-value-set",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
            "--value-set",
            "Default",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "variable-library activate-value-set");

    // --- 10. Clean up: delete the variable library ---
    fabio()
        .args([
            "variable-library",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
        ])
        .assert()
        .success();
}

/// Test that activate-value-set with a non-existent value set returns an error.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn variable_library_activate_nonexistent_value_set_fails() {
    let cfg = TestConfig::from_env();
    let vl_name = unique_name("fabio-test-vl-err");

    // Create a minimal variable library (no value sets)
    let assert = fabio()
        .args([
            "variable-library",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &vl_name,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let vl_id = data["id"].as_str().expect("missing id");

    // Try to activate a non-existent value set
    fabio()
        .args([
            "variable-library",
            "activate-value-set",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
            "--value-set",
            "nonexistent_set_xyz",
        ])
        .assert()
        .failure();

    // Clean up
    fabio()
        .args([
            "variable-library",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
        ])
        .assert()
        .success();
}

/// Test list-value-sets on a variable library with no alternate sets
/// (should still return at least the Default set).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn variable_library_list_value_sets_default_only() {
    let cfg = TestConfig::from_env();
    let vl_name = unique_name("fabio-test-vl-def");

    // Create a minimal variable library
    let assert = fabio()
        .args([
            "variable-library",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &vl_name,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let vl_id = data["id"].as_str().expect("missing id");

    // List value sets — should have Default only
    let assert = fabio()
        .args([
            "variable-library",
            "list-value-sets",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let value_sets = json["data"].as_array().expect("data should be array");
    assert!(
        !value_sets.is_empty(),
        "Should have at least the Default value set"
    );
    assert!(
        value_sets.iter().any(|vs| vs["name"] == "Default"),
        "Default value set should always exist"
    );

    // Clean up
    fabio()
        .args([
            "variable-library",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            vl_id,
        ])
        .assert()
        .success();
}

/// Live: author a multi-part variable-library definition (variables.json +
/// settings.json + an alternate valueSets/*.json) through
/// `variable-library update-definition --file` — the envelope-passthrough path.
/// Previously that command wrapped the file as a single variables.json, so
/// alternate value sets could not be authored via the CLI. Reproduces the
/// Variable Library tutorial's value-set creation.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial_test::serial]
fn variable_library_update_definition_multipart_via_cli() {
    let cfg = TestConfig::from_env();
    let vl_name = unique_name("fabio-test-vl-mp");

    let assert = fabio()
        .args([
            "variable-library",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &vl_name,
        ])
        .assert()
        .success();
    let vl_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    let sb = "https://developer.microsoft.com/json-schemas/fabric/item/variableLibrary/definition";
    let variables = serde_json::json!({
        "$schema": format!("{sb}/variables/1.0.0/schema.json"),
        "variables": [{"name": "Env", "type": "String", "value": "dev"}]
    });
    let settings = serde_json::json!({
        "$schema": format!("{sb}/settings/1.0.0/schema.json"),
        "valueSetsOrder": ["Prod"]
    });
    let prod = serde_json::json!({
        "$schema": format!("{sb}/valueSet/1.0.0/schema.json"),
        "name": "Prod",
        "variableOverrides": [{"name": "Env", "value": "prod"}]
    });
    let enc = |v: &serde_json::Value| BASE64.encode(serde_json::to_vec(v).unwrap());
    let envelope = serde_json::json!({
        "definition": {"parts": [
            {"path": "variables.json", "payload": enc(&variables), "payloadType": "InlineBase64"},
            {"path": "settings.json", "payload": enc(&settings), "payloadType": "InlineBase64"},
            {"path": "valueSets/Prod.json", "payload": enc(&prod), "payloadType": "InlineBase64"},
        ]}
    });
    let dir = tempfile::TempDir::new().unwrap();
    let env_path = dir.path().join("vlenv.json");
    std::fs::write(&env_path, serde_json::to_vec(&envelope).unwrap()).unwrap();

    fabio()
        .args([
            "variable-library",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &vl_id,
            "--file",
            env_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // The alternate value set must now exist (proving the multi-part envelope
    // reached the server, not a single wrapped variables.json).
    let assert = fabio()
        .args([
            "variable-library",
            "list-value-sets",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &vl_id,
        ])
        .assert()
        .success();
    let names: Vec<String> = extract_data(&parse_json(&assert))
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        names.iter().any(|n| n == "Prod"),
        "expected a 'Prod' value set, got {names:?}"
    );

    // Activate it.
    fabio()
        .args([
            "variable-library",
            "activate-value-set",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &vl_id,
            "--value-set",
            "Prod",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    fabio()
        .args([
            "variable-library",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &vl_id,
        ])
        .assert()
        .success();
}
