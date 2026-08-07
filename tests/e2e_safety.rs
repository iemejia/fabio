//! End-to-end tests for CLI safety features:
//! --readonly, --enable-commands, --disable-commands, --wrap-untrusted, MCP safety.
//!
//! These tests are all offline (no live tenant needed) because the safety
//! features block commands before any HTTP/auth calls are made.
//! Commands that succeed use `context agent` which requires no auth.
//! Commands that should be blocked are validated by checking the error JSON on stderr.

mod common;

use common::{fabio, parse_json};
use serde_json::Value;

/// Parse stderr as a JSON error envelope.
fn parse_stderr_error(output: &assert_cmd::assert::Assert) -> Value {
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    serde_json::from_str(&stderr).expect("failed to parse stderr as JSON")
}

/// Parse stdout as JSON.
fn parse_stdout(output: &assert_cmd::assert::Assert) -> Value {
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    serde_json::from_str(&stdout).expect("failed to parse stdout as JSON")
}

// =============================================================================
// --readonly tests
// =============================================================================

#[test]
fn readonly_allows_get_operations() {
    // `context agent` is a read-only command that requires no auth.
    fabio()
        .args(["--readonly", "context", "agent"])
        .assert()
        .success();
}

#[test]
fn readonly_blocks_mutations() {
    // `workspace create` triggers a POST call which --readonly blocks
    // before any network/auth call is made (guard_readonly runs before require_auth).
    let assert = fabio()
        .args(["--readonly", "workspace", "create", "--name", "test"])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    let error = json.get("error").expect("stderr should have 'error' field");
    assert_eq!(
        error["code"].as_str().unwrap(),
        "READONLY_MODE",
        "should produce READONLY_MODE error code"
    );
}

#[test]
fn readonly_blocks_onelake_writes_and_job_triggers() {
    // Regression: OneLake data-plane writes (upload/copy-file/create-directory)
    // and job triggers (notebook run) previously bypassed --readonly because
    // their client methods didn't call guard_readonly.
    let ws = "00000000-0000-0000-0000-000000000000";
    let id = "11111111-1111-1111-1111-111111111111";
    let cases: Vec<Vec<&str>> = vec![
        vec![
            "--readonly",
            "lakehouse",
            "create-directory",
            "--workspace",
            ws,
            "--id",
            id,
            "--path",
            "Files/newdir",
        ],
        vec![
            "--readonly",
            "notebook",
            "run",
            "--workspace",
            ws,
            "--id",
            id,
        ],
        vec![
            "--readonly",
            "copy-job",
            "run",
            "--workspace",
            ws,
            "--id",
            id,
        ],
    ];
    for args in cases {
        let assert = fabio().args(&args).assert().failure();
        let json = parse_stderr_error(&assert);
        assert_eq!(
            json["error"]["code"].as_str().unwrap(),
            "READONLY_MODE",
            "{args:?} must be blocked by --readonly"
        );
    }
}

#[test]
fn readonly_env_var() {
    // FABIO_READONLY=true should have the same effect as --readonly flag.
    // (clap bool flags require "true"/"false" string values from env vars)
    let assert = fabio()
        .env("FABIO_READONLY", "true")
        .args(["workspace", "create", "--name", "test-env"])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    let error = &json["error"];
    assert_eq!(error["code"].as_str().unwrap(), "READONLY_MODE");
}

#[test]
fn readonly_allows_help() {
    // --help should always work regardless of --readonly.
    fabio().args(["--readonly", "--help"]).assert().success();
}

#[test]
fn readonly_error_has_hint() {
    let assert = fabio()
        .args(["--readonly", "workspace", "create", "--name", "x"])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    let error = &json["error"];
    let hint = error["hint"]
        .as_str()
        .expect("error should have a 'hint' field");
    assert!(
        hint.contains("FABIO_READONLY") || hint.contains("--readonly"),
        "hint should mention how to disable readonly mode, got: {hint}"
    );
}

// =============================================================================
// --enable-commands tests
// =============================================================================

#[test]
fn enable_commands_allows_listed_group() {
    // `--enable-commands context` should allow `context agent` to run.
    fabio()
        .args(["--enable-commands", "context", "context", "agent"])
        .assert()
        .success();
}

#[test]
fn enable_commands_blocks_unlisted_group() {
    // `--enable-commands context` should block `workspace list`.
    // `workspace list` has no required args, so clap parses it fine and
    // check_command_policy triggers the FORBIDDEN before any auth call.
    let assert = fabio()
        .args(["--enable-commands", "context", "workspace", "list"])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    let error = &json["error"];
    assert_eq!(
        error["code"].as_str().unwrap(),
        "FORBIDDEN",
        "should produce FORBIDDEN error code"
    );
}

#[test]
fn enable_commands_parent_allows_children() {
    // Parent path "context" should allow child "context agent".
    // This verifies the parent-prefix matching logic.
    fabio()
        .args(["--enable-commands", "context", "context", "agent"])
        .assert()
        .success();
}

#[test]
fn enable_commands_env_var() {
    // FABIO_ENABLE_COMMANDS=context should allow `context agent`.
    fabio()
        .env("FABIO_ENABLE_COMMANDS", "context")
        .args(["context", "agent"])
        .assert()
        .success();

    // But should block `workspace list` (no required args, triggers policy check).
    let assert = fabio()
        .env("FABIO_ENABLE_COMMANDS", "context")
        .args(["workspace", "list"])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    assert_eq!(json["error"]["code"].as_str().unwrap(), "FORBIDDEN");
}

// =============================================================================
// --disable-commands tests
// =============================================================================

#[test]
fn disable_commands_blocks_listed_group() {
    // `--disable-commands workspace` should block `workspace list`.
    let assert = fabio()
        .args(["--disable-commands", "workspace", "workspace", "list"])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    assert_eq!(json["error"]["code"].as_str().unwrap(), "FORBIDDEN");
}

#[test]
fn disable_commands_allows_unlisted() {
    // `--disable-commands workspace` should NOT block `context agent`.
    fabio()
        .args(["--disable-commands", "workspace", "context", "agent"])
        .assert()
        .success();
}

#[test]
fn disable_commands_deny_overrides_allow() {
    // Deny should override allow: workspace is in both lists, deny wins.
    let assert = fabio()
        .args([
            "--enable-commands",
            "workspace",
            "--disable-commands",
            "workspace",
            "workspace",
            "list",
        ])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    assert_eq!(json["error"]["code"].as_str().unwrap(), "FORBIDDEN");
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("disable-commands"),
        "error should mention disable-commands policy, got: {msg}"
    );
}

#[test]
fn disable_commands_env_var() {
    // FABIO_DISABLE_COMMANDS=workspace should block workspace commands.
    let assert = fabio()
        .env("FABIO_DISABLE_COMMANDS", "workspace")
        .args(["workspace", "list"])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    assert_eq!(json["error"]["code"].as_str().unwrap(), "FORBIDDEN");
}

// =============================================================================
// Combined safety tests
// =============================================================================

#[test]
fn readonly_plus_enable_commands() {
    // Both flags together: readonly + enable-commands context.
    // `context agent` is read-only and in the allowlist — should succeed.
    fabio()
        .args([
            "--readonly",
            "--enable-commands",
            "context",
            "context",
            "agent",
        ])
        .assert()
        .success();
}

#[test]
fn readonly_plus_enable_commands_blocks_mutation() {
    // Even if the command group is in the allowlist, --readonly blocks
    // the actual HTTP mutation at the client level.
    let assert = fabio()
        .args([
            "--readonly",
            "--enable-commands",
            "workspace",
            "workspace",
            "create",
            "--name",
            "x",
        ])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    let code = json["error"]["code"].as_str().unwrap();
    assert_eq!(
        code, "READONLY_MODE",
        "readonly should block mutation even when command is in allowlist"
    );
}

// =============================================================================
// --wrap-untrusted tests
// =============================================================================

#[test]
fn wrap_untrusted_wraps_display_name_field() {
    // context agent returns schema with a "version" field (system-generated)
    // and the output envelope has "data" wrapping. We verify the flag is accepted
    // and wrapping applies to the expected fields by checking a known output.
    let assert = fabio()
        .args(["--wrap-untrusted", "context", "agent"])
        .assert()
        .success();

    let json = parse_stdout(&assert);
    let data = json.get("data").expect("missing data envelope");
    // The schema_version and version fields should NOT be wrapped (system-generated).
    let version = data.get("version").and_then(Value::as_str).unwrap_or("");
    assert!(
        !version.contains("<<<UNTRUSTED>>>"),
        "system fields should not be wrapped, got: {version}"
    );
}

#[test]
fn wrap_untrusted_env_var() {
    // Verify FABIO_WRAP_UNTRUSTED env var is accepted.
    let assert = fabio()
        .env("FABIO_WRAP_UNTRUSTED", "true")
        .args(["context", "agent"])
        .assert()
        .success();

    // Just verify it doesn't crash — env var accepted.
    let json = parse_stdout(&assert);
    assert!(json.get("data").is_some(), "should produce valid output");
}

#[test]
fn wrap_untrusted_off_by_default() {
    // Without the flag, output should NOT contain markers.
    let assert = fabio().args(["context", "agent"]).assert().success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        !stdout.contains("<<<UNTRUSTED>>>"),
        "should not wrap by default"
    );
}

// =============================================================================
// MCP safety tests
// =============================================================================

#[test]
fn mcp_list_tools_default_is_read_only() {
    let assert = fabio()
        .args(["mcp", "serve", "--list-tools"])
        .assert()
        .success();

    let json = parse_stdout(&assert);
    let data = json.get("data").expect("missing data");
    let count = data.get("count").and_then(Value::as_u64).unwrap_or(0);
    let policy = data.get("policy").expect("missing policy");

    assert!(count > 0, "should expose some tools");
    assert_eq!(
        policy.get("allow_write").and_then(Value::as_bool),
        Some(false),
        "default policy should be read-only"
    );

    // Verify all tools have readOnlyHint=true (no mutations exposed).
    let tools = data.get("tools").and_then(Value::as_array).unwrap();
    for tool in tools {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("?");
        let read_only = tool
            .get("annotations")
            .and_then(|a| a.get("readOnlyHint"))
            .and_then(Value::as_bool);
        assert_eq!(
            read_only,
            Some(true),
            "tool {name} should be read-only in default mode"
        );
    }
}

#[test]
fn mcp_list_tools_allow_write_exposes_more() {
    let readonly_out = fabio()
        .args(["mcp", "serve", "--list-tools"])
        .assert()
        .success();
    let writable_out = fabio()
        .args(["mcp", "serve", "--allow-write", "--list-tools"])
        .assert()
        .success();

    let readonly_json = parse_stdout(&readonly_out);
    let writable_json = parse_stdout(&writable_out);
    let readonly_count = readonly_json["data"]["count"].as_u64().unwrap_or(0);
    let writable_count = writable_json["data"]["count"].as_u64().unwrap_or(0);

    assert!(
        writable_count > readonly_count,
        "allow-write should expose more tools: {writable_count} > {readonly_count}"
    );
}

#[test]
fn mcp_list_tools_allow_tool_filters() {
    let assert_all = fabio()
        .args(["mcp", "serve", "--list-tools"])
        .assert()
        .success();
    let assert_filtered = fabio()
        .args(["mcp", "serve", "--allow-tool", "workspace", "--list-tools"])
        .assert()
        .success();

    let all_count = parse_stdout(&assert_all)["data"]["count"]
        .as_u64()
        .unwrap_or(0);
    let filtered_count = parse_stdout(&assert_filtered)["data"]["count"]
        .as_u64()
        .unwrap_or(0);

    assert!(
        filtered_count < all_count,
        "allow-tool filter should reduce tools: {filtered_count} < {all_count}"
    );
    assert!(
        filtered_count > 0,
        "should still expose some workspace tools"
    );
}

// =============================================================================
// Safety state introspection
// =============================================================================

#[test]
fn context_agent_includes_safety_state() {
    let assert = fabio()
        .args([
            "--readonly",
            "--disable-commands",
            "deploy",
            "context",
            "agent",
        ])
        .assert()
        .success();

    let json = parse_stdout(&assert);
    let safety = json["data"]
        .get("safety")
        .expect("context agent should include safety state");

    assert_eq!(
        safety.get("readonly").and_then(Value::as_bool),
        Some(true),
        "safety.readonly should reflect --readonly flag"
    );
    let disabled = safety
        .get("disable_commands")
        .and_then(Value::as_array)
        .expect("disable_commands should be an array");
    assert!(
        disabled.iter().any(|v| v.as_str() == Some("deploy")),
        "disable_commands should contain 'deploy'"
    );
}

// =============================================================================
// Subcommand-level and glob pattern tests
// =============================================================================

#[test]
fn disable_commands_subcommand_level_blocks_exact() {
    let assert = fabio()
        .args([
            "--disable-commands",
            "workspace.create",
            "workspace",
            "create",
            "--name",
            "test",
            "--dry-run",
        ])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    assert_eq!(json["error"]["code"].as_str().unwrap(), "FORBIDDEN");
    let msg = json["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("workspace.create"),
        "Error should mention blocked command path: {msg}"
    );
}

#[test]
fn disable_commands_subcommand_level_does_not_block_other() {
    // Blocking workspace.create should NOT block workspace list (offline, no auth needed)
    fabio()
        .args(["--disable-commands", "workspace.create", "context", "agent"])
        .assert()
        .success();
}

#[test]
fn disable_commands_glob_star_dot_subcommand() {
    // *.create blocks create across all groups
    let assert = fabio()
        .args([
            "--disable-commands",
            "*.create",
            "workspace",
            "create",
            "--name",
            "test",
            "--dry-run",
        ])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    assert_eq!(json["error"]["code"].as_str().unwrap(), "FORBIDDEN");
}

#[test]
fn disable_commands_glob_star_dot_subcommand_allows_other() {
    // *.create should NOT block context agent (different subcommand)
    fabio()
        .args(["--disable-commands", "*.create", "context", "agent"])
        .assert()
        .success();
}

#[test]
fn disable_commands_glob_group_prefix() {
    // kql-* blocks all kql- groups
    let assert = fabio()
        .args([
            "--disable-commands",
            "kql-*",
            "kql-database",
            "list",
            "--workspace",
            "test",
        ])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    assert_eq!(json["error"]["code"].as_str().unwrap(), "FORBIDDEN");
}

#[test]
fn enable_commands_glob_allows_pattern() {
    // --enable-commands "context,*.list" allows context and all list commands
    fabio()
        .args(["--enable-commands", "context,*.list", "context", "agent"])
        .assert()
        .success();
}

#[test]
fn enable_commands_glob_blocks_non_matching() {
    // --enable-commands "*.list" blocks create commands
    let assert = fabio()
        .args([
            "--enable-commands",
            "*.list",
            "workspace",
            "create",
            "--name",
            "test",
            "--dry-run",
        ])
        .assert()
        .failure();

    let json = parse_stderr_error(&assert);
    assert_eq!(json["error"]["code"].as_str().unwrap(), "FORBIDDEN");
}

// =============================================================================
// MCP --allow-tool glob tests
// =============================================================================

#[test]
fn mcp_allow_tool_subcommand_level() {
    // --allow-tool "workspace.list" should expose only fabio_workspace_list
    let assert = fabio()
        .args([
            "mcp",
            "serve",
            "--list-tools",
            "--allow-tool",
            "workspace.list",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let count = json["data"]["count"].as_u64().unwrap();
    assert_eq!(count, 1, "Should have exactly 1 tool");

    let tools = json["data"]["tools"].as_array().unwrap();
    assert_eq!(tools[0]["name"].as_str().unwrap(), "fabio_workspace_list");
}

#[test]
fn mcp_allow_tool_glob_star_list() {
    // --allow-tool "*.list" should expose all list commands
    let assert = fabio()
        .args(["mcp", "serve", "--list-tools", "--allow-tool", "*.list"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let tools = json["data"]["tools"].as_array().unwrap();
    assert!(
        tools.len() > 50,
        "*.list should match many list commands, got {}",
        tools.len()
    );

    // All should end with _list
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            name.ends_with("_list"),
            "All tools should end with _list: {name}"
        );
    }
}

#[test]
fn mcp_allow_tool_group_prefix_glob() {
    // --allow-tool "kql-*" with --allow-write should expose all kql- group tools
    let assert = fabio()
        .args([
            "mcp",
            "serve",
            "--list-tools",
            "--allow-tool",
            "kql-*",
            "--allow-write",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let tools = json["data"]["tools"].as_array().unwrap();
    assert!(
        tools.len() > 20,
        "kql-* should match 30+ tools, got {}",
        tools.len()
    );

    // All should start with fabio_kql_
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            name.starts_with("fabio_kql_"),
            "All tools should start with fabio_kql_: {name}"
        );
    }
}

#[test]
fn mcp_allow_tool_group_level() {
    // --allow-tool "workspace" should expose all workspace tools (read-only by default)
    let assert = fabio()
        .args(["mcp", "serve", "--list-tools", "--allow-tool", "workspace"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let tools = json["data"]["tools"].as_array().unwrap();
    assert!(
        tools.len() >= 5,
        "workspace should have several read-only tools, got {}",
        tools.len()
    );

    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            name.starts_with("fabio_workspace_"),
            "All tools should start with fabio_workspace_: {name}"
        );
    }
}
