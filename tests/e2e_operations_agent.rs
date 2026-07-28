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
fn operations_agent_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "operations-agent",
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
fn operations_agent_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "operations-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-agent",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "operations-agent create");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn operations_agent_dry_run_start() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "operations-agent",
            "start",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "operations-agent start");
    assert_eq!(json["data"]["shouldRun"], true);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn operations_agent_dry_run_stop() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "operations-agent",
            "stop",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "operations-agent stop");
    assert_eq!(json["data"]["shouldRun"], false);
}

/// Full lifecycle: create an operations agent, verify it starts stopped,
/// start it, confirm the status flips to running, stop it, confirm it flips
/// back to stopped, then delete it.
///
/// NOTE: This exercises the *unconfigured* path on purpose. A fully configured
/// agent (bound data source + generated playbook) cannot be produced through the
/// public REST/definition API — Fabric zeroes the data-source `id` on definition
/// writes and the playbook is generated only in the portal/Copilot — so there is
/// no "fully start on create" E2E test. See `.agents/API-BEHAVIORS-DISCOVERED.md`
/// ("Operations Agent API Behaviors Discovered") for the live-verified details.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn operations_agent_start_stop_status_lifecycle() {
    let cfg = TestConfig::from_env();

    // Create a fresh agent to exercise start/stop without touching real ones.
    let create = fabio()
        .args([
            "operations-agent",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "fabio-e2e-opsagent-lifecycle",
        ])
        .assert()
        .success();
    let created: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&create.get_output().stdout)).unwrap();
    let id = created["data"]["id"]
        .as_str()
        .expect("created agent id")
        .to_string();

    // A freshly created agent is stopped by default.
    let status = fabio()
        .args([
            "operations-agent",
            "status",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
    let status_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&status.get_output().stdout)).unwrap();
    assert_eq!(status_json["data"]["shouldRun"], false);
    assert_eq!(status_json["data"]["state"], "stopped");
    // A stopped agent hints at the start command.
    assert!(
        status_json["data"]["hint"]
            .as_str()
            .unwrap()
            .contains("operations-agent start")
    );

    // Start it. This agent is unconfigured (no data source / playbook), so
    // Fabric coerces shouldRun back to false — the start command re-reads and
    // reports the actual persisted state plus an explanatory note.
    let start = fabio()
        .args([
            "operations-agent",
            "start",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
    let start_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&start.get_output().stdout)).unwrap();
    assert_eq!(start_json["data"]["requestedShouldRun"], true);
    // Unconfigured agents don't actually activate.
    assert_eq!(start_json["data"]["shouldRun"], false);
    assert_eq!(start_json["data"]["status"], "stopped");
    assert!(start_json["data"]["note"].is_string());
    // The refused-activation output hints at configuring then starting.
    assert!(
        start_json["data"]["hint"]
            .as_str()
            .unwrap()
            .contains("operations-agent update-definition")
    );

    // Status still reports stopped because the agent has no data source.
    let status2 = fabio()
        .args([
            "operations-agent",
            "status",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
    let status2_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&status2.get_output().stdout)).unwrap();
    assert_eq!(status2_json["data"]["shouldRun"], false);
    assert_eq!(status2_json["data"]["state"], "stopped");

    // Stop it (always persists false).
    let stop = fabio()
        .args([
            "operations-agent",
            "stop",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
    let stop_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&stop.get_output().stdout)).unwrap();
    assert_eq!(stop_json["data"]["status"], "stopped");
    assert_eq!(stop_json["data"]["shouldRun"], false);
    assert_eq!(stop_json["data"]["requestedShouldRun"], false);

    // Clean up.
    fabio()
        .args([
            "operations-agent",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
}

/// Configure path: bind a real `KustoDatabase` data source to an operations agent
/// and assert exactly what the REST/definition API persists.
///
/// This documents the live-verified behavior that fabric goes as far as accepting
/// the data-source alias/type/workspace, but ZEROES the data-source `id` and
/// coerces `shouldRun` back to false — full activation requires the portal/Copilot
/// (data-source binding + Generate Playbook), which have no public REST API.
///
/// Values are extracted from the read-back BEFORE cleanup so the eventhouse and
/// agent are always deleted even if an assertion fails.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn operations_agent_data_source_binding_roundtrip() {
    let cfg = TestConfig::from_env();
    let ws = cfg.source_workspace;

    // 1. Create an eventhouse; Fabric auto-provisions a KQL database inside it.
    let eh = fabio()
        .args([
            "eventhouse",
            "create",
            "--workspace",
            &ws,
            "--name",
            "fabio_e2e_opsagent_eh",
        ])
        .assert()
        .success();
    let eh_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&eh.get_output().stdout)).unwrap();
    let eh_id = eh_json["data"]["id"].as_str().unwrap().to_string();

    // 2. Find the KQL database whose parent is this eventhouse.
    let dbs = fabio()
        .args(["kql-database", "list", "--workspace", &ws])
        .assert()
        .success();
    let dbs_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&dbs.get_output().stdout)).unwrap();
    let kqldb_id = dbs_json["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["properties"]["parentEventhouseItemId"].as_str() == Some(eh_id.as_str()))
        .expect("KQL database for eventhouse")["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. Create the operations agent.
    let agent = fabio()
        .args([
            "operations-agent",
            "create",
            "--workspace",
            &ws,
            "--name",
            "fabio_e2e_opsagent_cfg",
        ])
        .assert()
        .success();
    let agent_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&agent.get_output().stdout)).unwrap();
    let agent_id = agent_json["data"]["id"].as_str().unwrap().to_string();

    // 4. Bind the data source + instructions + request activation.
    let cfg_body = format!(
        r#"{{
  "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/operationsAgents/definition/1.0.0/schema.json",
  "configuration": {{
    "instructions": "Alert me when the value exceeds 80.",
    "dataSources": {{ "eh": {{ "id": "{kqldb_id}", "type": "KustoDatabase", "workspaceId": "{ws}" }} }},
    "actions": {{}}
  }},
  "shouldRun": true
}}"#
    );
    let cfg_path = std::env::temp_dir().join("fabio_e2e_opsagent_cfg.json");
    std::fs::write(&cfg_path, cfg_body).unwrap();
    fabio()
        .args([
            "operations-agent",
            "update-definition",
            "--workspace",
            &ws,
            "--id",
            &agent_id,
            "--file",
            cfg_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    // 5. Read back the decoded definition and extract what we need BEFORE cleanup.
    let def = fabio()
        .args([
            "operations-agent",
            "get-definition",
            "--workspace",
            &ws,
            "--id",
            &agent_id,
            "--decode",
        ])
        .assert()
        .success();
    let def_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&def.get_output().stdout)).unwrap();
    let config = def_json["data"]["definition"]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["path"].as_str() == Some("Configurations.json"))
        .expect("Configurations.json part")["decodedPayload"]
        .clone();

    let ds = config["configuration"]["dataSources"]["eh"].clone();
    let instructions = config["configuration"]["instructions"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let ds_type = ds["type"].as_str().unwrap_or_default().to_string();
    let ds_workspace = ds["workspaceId"].as_str().unwrap_or_default().to_string();
    let ds_id = ds["id"].as_str().unwrap_or_default().to_string();
    let should_run = config["shouldRun"].as_bool().unwrap_or(true);

    // 6. Cleanup FIRST so leaked resources are impossible even if asserts fail.
    let _ = std::fs::remove_file(&cfg_path);
    fabio()
        .args([
            "operations-agent",
            "delete",
            "--workspace",
            &ws,
            "--id",
            &agent_id,
        ])
        .assert()
        .success();
    fabio()
        .args(["eventhouse", "delete", "--workspace", &ws, "--id", &eh_id])
        .assert()
        .success();

    // 7. Assertions on the extracted values.
    // Instructions persist verbatim.
    assert_eq!(instructions, "Alert me when the value exceeds 80.");
    // The data-source alias, type, and workspace survive the write...
    assert_eq!(ds_type, "KustoDatabase");
    assert_eq!(ds_workspace, ws);
    // ...but Fabric ZEROES the data-source id (binding is portal/Copilot-only).
    assert_eq!(ds_id, "00000000-0000-0000-0000-000000000000");
    // And shouldRun is coerced back to false: no generated playbook, so no activation.
    assert!(!should_run);
}
