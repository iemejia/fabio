//! End-to-end integration tests for `fabio reflex` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

/// Build a minimal simulator-based Reflex definition with an `AttributeTrigger` rule.
/// This is a self-contained definition (no external data sources) that exercises:
/// - `container-v1` (pipeline root)
/// - `simulatorSource-v1` (built-in event generator)
/// - `timeSeriesView-v1` Event (source event selection)
/// - `timeSeriesView-v1` Object (identity grouping)
/// - `timeSeriesView-v1` Attribute (identity + value extraction)
/// - `timeSeriesView-v1` Rule (`AttributeTrigger` with email action)
fn build_simulator_definition() -> String {
    serde_json::json!([
        {
            "uniqueIdentifier": "00aa00aa-bb11-cc22-dd33-44ee44ee44ee",
            "payload": { "name": "Temperature monitoring", "type": "samples" },
            "type": "container-v1"
        },
        {
            "uniqueIdentifier": "11bb11bb-cc22-dd33-ee44-55ff55ff55ff",
            "payload": {
                "name": "Package delivery",
                "runSettings": {
                    "startTime": "2026-05-25T12:00:00Z",
                    "stopTime": "2026-06-25T12:00:00Z"
                },
                "version": "V2_0",
                "type": "PackageShipment",
                "parentContainer": { "targetUniqueIdentifier": "00aa00aa-bb11-cc22-dd33-44ee44ee44ee" }
            },
            "type": "simulatorSource-v1"
        },
        {
            "uniqueIdentifier": "22cc22cc-dd33-ee44-ff55-66aa66aa66aa",
            "payload": {
                "name": "Package delivery events",
                "parentContainer": { "targetUniqueIdentifier": "00aa00aa-bb11-cc22-dd33-44ee44ee44ee" },
                "definition": {
                    "type": "Event",
                    "instance": "{\"templateId\":\"SourceEvent\",\"templateVersion\":\"1.1\",\"steps\":[{\"name\":\"SourceEventStep\",\"id\":\"aaaa0000-bb11-2222-33cc-444444dddddd\",\"rows\":[{\"name\":\"SourceSelector\",\"kind\":\"SourceReference\",\"arguments\":[{\"name\":\"entityId\",\"type\":\"string\",\"value\":\"11bb11bb-cc22-dd33-ee44-55ff55ff55ff\"}]}]}]}"
                }
            },
            "type": "timeSeriesView-v1"
        },
        {
            "uniqueIdentifier": "33dd33dd-ee44-ff55-aa66-77bb77bb77bb",
            "payload": {
                "name": "Package",
                "parentContainer": { "targetUniqueIdentifier": "00aa00aa-bb11-cc22-dd33-44ee44ee44ee" },
                "definition": { "type": "Object" }
            },
            "type": "timeSeriesView-v1"
        },
        {
            "uniqueIdentifier": "44ee44ee-ff55-aa66-bb77-88cc88cc88cc",
            "payload": {
                "name": "PackageId",
                "parentObject": { "targetUniqueIdentifier": "33dd33dd-ee44-ff55-aa66-77bb77bb77bb" },
                "parentContainer": { "targetUniqueIdentifier": "00aa00aa-bb11-cc22-dd33-44ee44ee44ee" },
                "definition": {
                    "type": "Attribute",
                    "instance": "{\"templateId\":\"IdentityPartAttribute\",\"templateVersion\":\"1.1\",\"steps\":[{\"name\":\"IdPartStep\",\"id\":\"bbbb1111-cc22-3333-44dd-555555eeeeee\",\"rows\":[{\"name\":\"TypeAssertion\",\"kind\":\"TypeAssertion\",\"arguments\":[{\"name\":\"op\",\"type\":\"string\",\"value\":\"Text\"},{\"name\":\"format\",\"type\":\"string\",\"value\":\"\"}]}]}]}"
                }
            },
            "type": "timeSeriesView-v1"
        },
        {
            "uniqueIdentifier": "55ff55ff-aa66-bb77-cc88-99dd99dd99dd",
            "payload": {
                "name": "Temperature",
                "parentObject": { "targetUniqueIdentifier": "33dd33dd-ee44-ff55-aa66-77bb77bb77bb" },
                "parentContainer": { "targetUniqueIdentifier": "00aa00aa-bb11-cc22-dd33-44ee44ee44ee" },
                "definition": {
                    "type": "Attribute",
                    "instance": "{\"templateId\":\"BasicEventAttribute\",\"templateVersion\":\"1.1\",\"steps\":[{\"name\":\"EventSelectStep\",\"id\":\"cccc2222-dd33-4444-55ee-666666ffffff\",\"rows\":[{\"name\":\"EventSelector\",\"kind\":\"Event\",\"arguments\":[{\"kind\":\"EventReference\",\"type\":\"complex\",\"arguments\":[{\"name\":\"entityId\",\"type\":\"string\",\"value\":\"22cc22cc-dd33-ee44-ff55-66aa66aa66aa\"}],\"name\":\"event\"}]},{\"name\":\"EventFieldSelector\",\"kind\":\"EventField\",\"arguments\":[{\"name\":\"fieldName\",\"type\":\"string\",\"value\":\"Temperature\"}]}]},{\"name\":\"EventComputeStep\",\"id\":\"dddd3333-ee44-5555-66ff-777777aaaaaa\",\"rows\":[{\"name\":\"TypeAssertion\",\"kind\":\"TypeAssertion\",\"arguments\":[{\"name\":\"op\",\"type\":\"string\",\"value\":\"Number\"},{\"name\":\"format\",\"type\":\"string\",\"value\":\"\"}]}]}]}"
                }
            },
            "type": "timeSeriesView-v1"
        },
        {
            "uniqueIdentifier": "66aa66aa-bb77-cc88-dd99-00ee00ee00ee",
            "payload": {
                "name": "Temperature too high",
                "parentObject": { "targetUniqueIdentifier": "33dd33dd-ee44-ff55-aa66-77bb77bb77bb" },
                "parentContainer": { "targetUniqueIdentifier": "00aa00aa-bb11-cc22-dd33-44ee44ee44ee" },
                "definition": {
                    "type": "Rule",
                    "instance": "{\"templateId\":\"AttributeTrigger\",\"templateVersion\":\"1.1\",\"steps\":[{\"name\":\"ScalarSelectStep\",\"id\":\"eeee4444-ff55-6666-77aa-888888bbbbbb\",\"rows\":[{\"name\":\"AttributeSelector\",\"kind\":\"Attribute\",\"arguments\":[{\"kind\":\"AttributeReference\",\"type\":\"complex\",\"arguments\":[{\"name\":\"entityId\",\"type\":\"string\",\"value\":\"55ff55ff-aa66-bb77-cc88-99dd99dd99dd\"}],\"name\":\"attribute\"}]},{\"name\":\"NumberSummary\",\"kind\":\"NumberSummary\",\"arguments\":[{\"name\":\"op\",\"type\":\"string\",\"value\":\"Average\"},{\"kind\":\"TimeDrivenWindowSpec\",\"type\":\"complex\",\"arguments\":[{\"name\":\"width\",\"type\":\"timeSpan\",\"value\":600000.0},{\"name\":\"hop\",\"type\":\"timeSpan\",\"value\":600000.0}],\"name\":\"window\"}]}]},{\"name\":\"ScalarDetectStep\",\"id\":\"ffff5555-aa66-7777-88bb-999999cccccc\",\"rows\":[{\"name\":\"NumberBecomes\",\"kind\":\"NumberBecomes\",\"arguments\":[{\"name\":\"op\",\"type\":\"string\",\"value\":\"BecomesGreaterThan\"},{\"name\":\"value\",\"type\":\"number\",\"value\":30.0}]},{\"name\":\"OccurrenceOption\",\"kind\":\"EachTime\",\"arguments\":[]}]},{\"name\":\"ActStep\",\"id\":\"0000aaaa-11bb-cccc-dd22-eeeeee333333\",\"rows\":[{\"name\":\"EmailBinding\",\"kind\":\"EmailMessage\",\"arguments\":[{\"name\":\"messageLocale\",\"type\":\"string\",\"value\":\"en-us\"},{\"name\":\"sentTo\",\"type\":\"array\",\"values\":[{\"type\":\"string\",\"value\":\"admin@fabio-test.onmicrosoft.com\"}]},{\"name\":\"copyTo\",\"type\":\"array\",\"values\":[]},{\"name\":\"bCCTo\",\"type\":\"array\",\"values\":[]},{\"name\":\"subject\",\"type\":\"array\",\"values\":[{\"type\":\"string\",\"value\":\"Alert: Temperature exceeded 30C\"}]},{\"name\":\"headline\",\"type\":\"array\",\"values\":[{\"type\":\"string\",\"value\":\"Temperature threshold exceeded\"}]},{\"name\":\"optionalMessage\",\"type\":\"array\",\"values\":[{\"type\":\"string\",\"value\":\"A package has exceeded the safe temperature limit.\"}]},{\"name\":\"additionalInformation\",\"type\":\"array\",\"values\":[]}]}]}]}",
                    "settings": { "shouldRun": true, "shouldApplyRuleOnUpdate": false }
                }
            },
            "type": "timeSeriesView-v1"
        }
    ]).to_string()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn reflex_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["reflex", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn reflex_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("rx_test");

    // Create
    let assert = fabio()
        .args([
            "reflex",
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
            "reflex",
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
fn reflex_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "reflex",
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
fn reflex_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "reflex",
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
fn reflex_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "reflex",
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
    assert_eq!(json["data"]["would_execute"], "reflex create");
}

// ─── Definition Tests ────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn reflex_get_definition_returns_entities() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("rx_getdef");

    // Create a fresh reflex
    let assert = fabio()
        .args([
            "reflex",
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
    let id = data["id"].as_str().unwrap().to_string();

    // Get definition — newly created reflex should have ReflexEntities.json with []
    let assert = fabio()
        .args([
            "reflex",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();

    // Should contain ReflexEntities.json
    let paths: Vec<&str> = parts.iter().map(|p| p["path"].as_str().unwrap()).collect();
    assert!(
        paths.contains(&"ReflexEntities.json"),
        "Expected ReflexEntities.json part, got: {paths:?}"
    );

    // Decode and verify it's an empty array
    let entities_part = parts
        .iter()
        .find(|p| p["path"] == "ReflexEntities.json")
        .unwrap();
    let payload = entities_part["payload"].as_str().unwrap();
    let decoded =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, payload).unwrap();
    let entities: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert!(
        entities.as_array().unwrap().is_empty(),
        "New reflex should have empty entities array"
    );

    // Cleanup
    fabio()
        .args([
            "reflex",
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
fn reflex_update_definition_simulator_pipeline() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("rx_upddef");

    // Create a fresh reflex
    let assert = fabio()
        .args([
            "reflex",
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
    let id = data["id"].as_str().unwrap().to_string();

    // Write the simulator definition to a temp file
    let dir = tempfile::tempdir().unwrap();
    let def_path = dir.path().join("entities.json");
    std::fs::write(&def_path, build_simulator_definition()).unwrap();

    // Update definition
    let assert = fabio()
        .args([
            "reflex",
            "update-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
            "--file",
            def_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "definition_updated");

    // Verify definition was persisted with get-definition
    let assert = fabio()
        .args([
            "reflex",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();
    let entities_part = parts
        .iter()
        .find(|p| p["path"] == "ReflexEntities.json")
        .unwrap();
    let payload = entities_part["payload"].as_str().unwrap();
    let decoded =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, payload).unwrap();
    let entities: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    let arr = entities.as_array().unwrap();

    // Should have 7 entities (container + source + event + object + 2 attributes + rule)
    assert_eq!(arr.len(), 7, "Expected 7 entities, got {}", arr.len());

    // Verify entity types
    let types: Vec<&str> = arr.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(types.iter().filter(|t| **t == "container-v1").count(), 1);
    assert_eq!(
        types.iter().filter(|t| **t == "simulatorSource-v1").count(),
        1
    );
    assert_eq!(
        types.iter().filter(|t| **t == "timeSeriesView-v1").count(),
        5
    );

    // Verify rule entity has AttributeTrigger template
    let rule = arr
        .iter()
        .find(|e| {
            e["payload"]["definition"]["type"]
                .as_str()
                .is_some_and(|t| t == "Rule")
        })
        .expect("No Rule entity found");
    let instance_str = rule["payload"]["definition"]["instance"].as_str().unwrap();
    let instance: serde_json::Value = serde_json::from_str(instance_str).unwrap();
    assert_eq!(instance["templateId"], "AttributeTrigger");
    assert_eq!(instance["templateVersion"], "1.1");

    // Verify rule has 3 steps: ScalarSelectStep, ScalarDetectStep, ActStep
    let steps = instance["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0]["name"], "ScalarSelectStep");
    assert_eq!(steps[1]["name"], "ScalarDetectStep");
    assert_eq!(steps[2]["name"], "ActStep");

    // Verify settings
    let settings = &rule["payload"]["definition"]["settings"];
    assert_eq!(settings["shouldRun"], true);
    assert_eq!(settings["shouldApplyRuleOnUpdate"], false);

    // Cleanup
    fabio()
        .args([
            "reflex",
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
fn reflex_update_definition_replaces_entities() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("rx_replace");

    // Create
    let assert = fabio()
        .args([
            "reflex",
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
    let id = data["id"].as_str().unwrap().to_string();

    // First: push full simulator definition
    let dir = tempfile::tempdir().unwrap();
    let def_path = dir.path().join("entities.json");
    std::fs::write(&def_path, build_simulator_definition()).unwrap();

    fabio()
        .args([
            "reflex",
            "update-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
            "--file",
            def_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    // Second: replace with a minimal definition (just container + simulator)
    let minimal = serde_json::json!([
        {
            "uniqueIdentifier": "aa000001-0001-0001-0001-000000000001",
            "payload": { "name": "Minimal container", "type": "samples" },
            "type": "container-v1"
        },
        {
            "uniqueIdentifier": "bb000002-0002-0002-0002-000000000002",
            "payload": {
                "name": "Minimal source",
                "runSettings": {
                    "startTime": "2026-06-01T00:00:00Z",
                    "stopTime": "2026-07-01T00:00:00Z"
                },
                "version": "V2_0",
                "type": "PackageShipment",
                "parentContainer": { "targetUniqueIdentifier": "aa000001-0001-0001-0001-000000000001" }
            },
            "type": "simulatorSource-v1"
        }
    ])
    .to_string();

    let def2_path = dir.path().join("minimal.json");
    std::fs::write(&def2_path, &minimal).unwrap();

    fabio()
        .args([
            "reflex",
            "update-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
            "--file",
            def2_path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    // Verify: should now have only 2 entities
    let assert = fabio()
        .args([
            "reflex",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let parts = data["definition"]["parts"].as_array().unwrap();
    let entities_part = parts
        .iter()
        .find(|p| p["path"] == "ReflexEntities.json")
        .unwrap();
    let payload = entities_part["payload"].as_str().unwrap();
    let decoded =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, payload).unwrap();
    let entities: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(
        entities.as_array().unwrap().len(),
        2,
        "Expected 2 entities after replacement"
    );

    // Cleanup
    fabio()
        .args([
            "reflex",
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
fn reflex_update_definition_inline_content() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("rx_inline");

    // Create
    let assert = fabio()
        .args([
            "reflex",
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
    let id = data["id"].as_str().unwrap().to_string();

    // Use --content flag with minimal definition
    let content = serde_json::json!([
        {
            "uniqueIdentifier": "cc000001-0001-0001-0001-000000000001",
            "payload": { "name": "Inline test", "type": "samples" },
            "type": "container-v1"
        }
    ])
    .to_string();

    let assert = fabio()
        .args([
            "reflex",
            "update-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &id,
            "--content",
            &content,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "definition_updated");

    // Cleanup
    fabio()
        .args([
            "reflex",
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
fn reflex_update_definition_requires_input() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "reflex",
            "update-definition",
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
fn reflex_dry_run_update_definition() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "reflex",
            "update-definition",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            "[]",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "reflex update-definition");
    assert!(data["dry_run"].as_bool().unwrap());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn reflex_dry_run_delete() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "reflex",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "reflex delete");
    assert!(data["dry_run"].as_bool().unwrap());
}

// ─── Create-Trigger Tests ───────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn reflex_create_trigger_dry_run_email() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "reflex",
            "create-trigger",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "Test Email Alert",
            "--eventhouse-id",
            "00000000-0000-0000-0000-000000000001",
            "--database",
            "TestDB",
            "--table",
            "Events",
            "--condition",
            "severity > 3",
            "--action",
            "email",
            "--recipients",
            "admin@example.com,ops@example.com",
            "--interval",
            "120",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "reflex create-trigger");
    assert_eq!(data["details"]["action"], "email");
    assert_eq!(data["details"]["table"], "Events");
    assert_eq!(data["details"]["condition"], "severity > 3");
    assert_eq!(data["details"]["interval_seconds"], 120);
    assert_eq!(data["details"]["entity_count"], 5);
    let recipients = data["details"]["recipients"].as_array().unwrap();
    assert_eq!(recipients.len(), 2);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn reflex_create_trigger_dry_run_teams() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "reflex",
            "create-trigger",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "Test Teams Alert",
            "--eventhouse-id",
            "00000000-0000-0000-0000-000000000001",
            "--database",
            "TestDB",
            "--table",
            "StormEvents",
            "--condition",
            "State == 'ILLINOIS'",
            "--action",
            "teams",
            "--recipients",
            "team@example.com",
            "--message",
            "Storm detected!",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["action"], "teams");
    assert_eq!(data["details"]["entity_count"], 5);
}

#[test]
#[ignore = "requires live Fabric tenant with Eventhouse"]
#[serial]
fn reflex_create_trigger_live_email() {
    let cfg = TestConfig::from_env();

    // Create the trigger live
    let output = fabio()
        .args([
            "reflex",
            "create-trigger",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "fabio-e2e-trigger-test",
            "--eventhouse-id",
            "9b3b6a66-7c0c-4fd6-8465-6d63e42df579",
            "--database",
            "AlertDB",
            "--table",
            "TestEvents",
            "--condition",
            "level > 5",
            "--action",
            "email",
            "--recipients",
            "test@example.com",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);

    // Should have created the reflex (id present) regardless of definition update success
    assert!(data.get("id").is_some(), "Should return reflex ID: {data}");
    let reflex_id = data["id"].as_str().unwrap();

    // Clean up: delete the created reflex
    fabio()
        .args([
            "reflex",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            reflex_id,
        ])
        .assert()
        .success();
}

// ─── Activator MCP server: rule management ──────────────────────────────────

/// `--readonly` must block a mutating MCP tool BEFORE any auth/network call.
/// Fully offline (the readonly guard fires before the MCP client connects).
#[test]
fn reflex_delete_rule_readonly_blocked() {
    let assert = fabio()
        .args([
            "--readonly",
            "reflex",
            "delete-rule",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--rule-id",
            "00000000-0000-0000-0000-000000000002",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let line = stderr
        .lines()
        .find(|l| l.starts_with('{'))
        .expect("json error on stderr");
    let err: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(err["error"]["code"], "READONLY_MODE");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn reflex_dry_run_start_rule() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "reflex",
            "start-rule",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--rule-id",
            "00000000-0000-0000-0000-000000000001",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "reflex start-rule");
    assert!(data["dry_run"].as_bool().unwrap());
    assert_eq!(data["details"]["tool"], "start_rule");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn reflex_dry_run_delete_rule() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "reflex",
            "delete-rule",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--rule-id",
            "00000000-0000-0000-0000-000000000001",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "reflex delete-rule");
    assert!(data["dry_run"].as_bool().unwrap());
}

/// Live: create a reflex, resolve its Activator MCP URL, and list its rules
/// (empty for a fresh reflex) through the MCP client, then clean up.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn reflex_mcp_url_and_list_rules_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("rx_mcp");

    let assert = fabio()
        .args([
            "reflex",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let created = parse_json(&assert);
    let id = extract_data(&created)["id"].as_str().unwrap().to_string();

    // mcp-url: deterministic URL shape + existence check.
    let assert = fabio()
        .args([
            "reflex",
            "mcp-url",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
    let url_json = parse_json(&assert);
    let data = extract_data(&url_json);
    let expected = format!("/mcp/workspaces/{}/reflexes/{id}", cfg.source_workspace);
    assert!(
        data["mcpUrl"].as_str().unwrap().ends_with(&expected),
        "unexpected mcpUrl: {data}"
    );
    assert_eq!(data["exists"], true);

    // list-rules: a fresh reflex has no rules (empty array via the MCP client).
    let assert = fabio()
        .args([
            "reflex",
            "list-rules",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let rules_json = parse_json(&assert);
    let data = extract_data(&rules_json);
    assert!(data["rules"].is_array(), "expected a rules array: {data}");

    // Cleanup.
    fabio()
        .args([
            "reflex",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
}
