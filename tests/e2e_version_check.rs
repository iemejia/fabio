//! End-to-end tests for the passive "newer fabio available" version notice.
//!
//! These are hermetic (no network): each test points fabio at a throwaway HOME
//! seeded with a *fresh* `version-check.json`, so the cache is never considered
//! stale and no background refresh is spawned. We use `profile list` as a
//! representative offline command that emits a JSON envelope.

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;

/// Environment variables that make `agent::detect_agent()` return `Some`.
/// Mirrors `AGENT_ENV_VARS` in `src/agent.rs`; used to force a clean
/// "no agent" state for the negative test regardless of the parent environment
/// (which may itself be an agent shell).
const AGENT_ENV_VARS: &[&str] = &[
    "CLAUDE_CODE",
    "CLAUDECODE",
    "CURSOR_AGENT",
    "CURSOR_TRACE_ID",
    "COPILOT_CLI",
    "GITHUB_COPILOT",
    "VSCODE_AGENT",
    "OPENCODE_AGENT",
    "OPENCODE",
    "CODEX",
    "CODEX_CLI_AGENT",
    "WINDSURF_AGENT",
    "CLINE_AGENT",
    "CLINE_ACTIVE",
    "DEVIN_AGENT",
    "AIDER_AGENT",
    "CONTINUE_AGENT",
    "OPENCLAW_SHELL",
    "GEMINI_CLI",
    "GOOSE_TERMINAL",
    "KIRO",
    "AUGMENT_AGENT",
    "ANTIGRAVITY_AGENT",
    "AMP_CURRENT_THREAD_ID",
];

/// Create a throwaway HOME seeded with a fresh cache advertising `latest`.
fn seeded_home(tag: &str, latest: &str) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("fabio-vc-{tag}-{ts}"));
    let fabio_dir = home.join(".fabio");
    fs::create_dir_all(&fabio_dir).unwrap();
    // Fresh timestamp (now) => never stale => no background refresh spawned.
    let now = chrono::Utc::now().to_rfc3339();
    let cache = format!(r#"{{"last_checked":"{now}","latest_version":"{latest}"}}"#);
    fs::write(fabio_dir.join("version-check.json"), cache).unwrap();
    home
}

/// `fabio profile list` with HOME/USERPROFILE pointed at `home`.
fn fabio_with_home(home: &PathBuf) -> Command {
    let mut cmd = Command::cargo_bin("fabio").unwrap();
    cmd.args(["profile", "list", "--output", "json"]);
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home); // Windows home resolution
    cmd
}

fn stdout_json(assert: &assert_cmd::assert::Assert) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    serde_json::from_str(&stdout).expect("stdout is JSON")
}

#[test]
fn notice_present_for_agent_when_update_available() {
    let home = seeded_home("agent", "99.0.0");
    let assert = fabio_with_home(&home)
        .env("CLAUDECODE", "1") // force agent detection
        .assert()
        .success();
    let json = stdout_json(&assert);

    let notice = &json["updateAvailable"];
    assert!(
        notice.is_object(),
        "expected updateAvailable object, got: {json}"
    );
    assert_eq!(notice["latest"], "99.0.0");
    assert!(notice["current"].is_string());
    // installMethod/upgradeCommand are environment-dependent (docker in CI,
    // standalone locally) — assert presence, not an exact value.
    assert!(notice["installMethod"].is_string());
    assert!(
        !notice["upgradeCommand"].as_str().unwrap().is_empty(),
        "upgradeCommand should be non-empty"
    );
    // Agent notice must steer the agent to refresh its cached schema.
    let agent_notice = notice["agentNotice"].as_str().unwrap_or_default();
    assert!(
        agent_notice.contains("context agent"),
        "agentNotice should mention `fabio context agent`: {agent_notice}"
    );

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn no_notice_without_agent() {
    let home = seeded_home("noagent", "99.0.0");
    let mut cmd = fabio_with_home(&home);
    // Ensure no agent is detected even if the parent shell is an agent.
    for var in AGENT_ENV_VARS {
        cmd.env_remove(var);
    }
    let assert = cmd.assert().success();
    let json = stdout_json(&assert);
    assert!(
        json.get("updateAvailable").is_none(),
        "no notice expected without an agent, got: {json}"
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn no_notice_when_opted_out() {
    let home = seeded_home("optout", "99.0.0");
    let assert = fabio_with_home(&home)
        .env("CLAUDECODE", "1")
        .env("FABIO_NO_VERSION_CHECK", "1")
        .assert()
        .success();
    let json = stdout_json(&assert);
    assert!(
        json.get("updateAvailable").is_none(),
        "no notice expected when opted out, got: {json}"
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn no_notice_when_already_current() {
    // Seed a cache whose latest is older than any real build => not newer.
    let home = seeded_home("current", "0.0.1");
    let assert = fabio_with_home(&home)
        .env("CLAUDECODE", "1")
        .assert()
        .success();
    let json = stdout_json(&assert);
    assert!(
        json.get("updateAvailable").is_none(),
        "no notice expected when up to date, got: {json}"
    );
    let _ = fs::remove_dir_all(&home);
}
