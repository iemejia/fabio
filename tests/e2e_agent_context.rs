use assert_cmd::Command;

fn fabio() -> Command {
    Command::cargo_bin("fabio").unwrap()
}

#[test]
fn agent_context_returns_schema() {
    let assert = fabio().args(["agent-context"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // Should have commands object
    assert!(json["data"]["commands"].is_object());
    let commands = json["data"]["commands"].as_object().unwrap();
    assert!(!commands.is_empty(), "agent-context should list commands");
}

#[test]
fn agent_context_includes_workspace_command() {
    let assert = fabio().args(["agent-context"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let commands = json["data"]["commands"].as_object().unwrap();
    assert!(
        commands.contains_key("workspace"),
        "agent-context should include 'workspace' command"
    );
}

#[test]
fn agent_context_output_table_format() {
    let assert = fabio()
        .args(["--output", "table", "agent-context"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // Table format should contain column headers or structured text
    assert!(!stdout.is_empty());
}

#[test]
fn agent_context_includes_app_backend_command_with_delete_flag() {
    let assert = fabio().args(["agent-context"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let commands = json["data"]["commands"].as_object().unwrap();

    let app_backend = commands
        .get("app-backend")
        .expect("agent-context should include 'app-backend' command");
    let hard_delete_type = &app_backend["subcommands"]["delete"]["flags"]["--hard-delete"]["type"];
    assert_eq!(hard_delete_type, "bool");
}
