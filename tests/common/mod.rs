#![allow(dead_code)]
//! Shared test harness for fabio end-to-end integration tests.
//!
//! These tests exercise the compiled binary against a live Microsoft Fabric tenant.
//! They require valid Azure credentials (e.g., `az login`) and the following
//! environment variables to be set:
//!
//! - `FABIO_TEST_SOURCE_WORKSPACE` - Source workspace ID
//! - `FABIO_TEST_SOURCE_LAKEHOUSE` - Source lakehouse ID
//! - `FABIO_TEST_DEST_WORKSPACE` - Destination workspace ID
//! - `FABIO_TEST_DEST_LAKEHOUSE` - Destination lakehouse ID
//! - `FABIO_TEST_NOTEBOOK_ID` - Existing notebook ID in source workspace
//! - `FABIO_TEST_CAPACITY_ID` - Active capacity ID
//!
//! All integration tests are marked `#[ignore]` so they don't run during normal
//! `cargo test`. Run them explicitly with:
//!
//! ```sh
//! cargo test --test '*' -- --ignored
//! ```
//!
//! Or run a specific test group:
//!
//! ```sh
//! cargo test --test e2e_workspace -- --ignored
//! ```

use assert_cmd::Command;
use serde_json::Value;
use std::env;

/// Test configuration loaded from environment variables.
pub struct TestConfig {
    pub source_workspace: String,
    pub source_lakehouse: String,
    pub dest_workspace: String,
    pub dest_lakehouse: String,
    pub notebook_id: String,
    pub capacity_id: String,
}

impl TestConfig {
    /// Load configuration from environment variables.
    ///
    /// # Panics
    ///
    /// Panics if required environment variables are not set.
    pub fn from_env() -> Self {
        Self {
            source_workspace: required_env("FABIO_TEST_SOURCE_WORKSPACE"),
            source_lakehouse: required_env("FABIO_TEST_SOURCE_LAKEHOUSE"),
            dest_workspace: required_env("FABIO_TEST_DEST_WORKSPACE"),
            dest_lakehouse: required_env("FABIO_TEST_DEST_LAKEHOUSE"),
            notebook_id: required_env("FABIO_TEST_NOTEBOOK_ID"),
            capacity_id: required_env("FABIO_TEST_CAPACITY_ID"),
        }
    }
}

fn required_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} must be set for live E2E tests"))
}

/// Build a `fabio` command ready to execute.
pub fn fabio() -> Command {
    Command::cargo_bin("fabio").expect("failed to find fabio binary")
}

/// Build a command using a fake token and the local mock Fabric endpoint.
pub fn mock_fabio(server: &wiremock::MockServer) -> Command {
    let mut command = fabio();
    command
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server.uri());
    command
}

/// Parse the stdout of a successful fabio command as JSON.
pub fn parse_json(output: &assert_cmd::assert::Assert) -> Value {
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    serde_json::from_str(&stdout).expect("failed to parse stdout as JSON")
}

/// Parse the structured error while allowing diagnostic lines on stderr.
pub fn parse_error(output: &assert_cmd::assert::Assert) -> Value {
    String::from_utf8_lossy(&output.get_output().stderr)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value.get("error").is_some())
        .expect("missing structured error on stderr")
}

/// Extract the `data` field from a fabio JSON envelope.
pub fn extract_data(json: &Value) -> &Value {
    json.get("data").expect("missing 'data' field in response")
}

/// Extract the `count` field from a fabio JSON list envelope.
pub fn extract_count(json: &Value) -> u64 {
    json.get("count")
        .and_then(Value::as_u64)
        .expect("missing 'count' field in response")
}

/// Generate a unique name for test artifacts to avoid collisions.
pub fn unique_name(prefix: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{prefix}_{ts}")
}

/// A live-test fixture owned by one test. Never points at a pre-existing item.
///
/// Needs only `FABIO_TEST_SOURCE_WORKSPACE`, unlike the broad legacy config.
/// Explicit cleanup is asserted; Drop is a best-effort fallback after a panic.
pub struct ScopedTestItem {
    pub workspace: String,
    pub id: String,
    pub name: String,
    group: String,
    cleanup_attempted: bool,
}

impl ScopedTestItem {
    pub fn create(group: &str, prefix: &str) -> Self {
        let workspace = required_env("FABIO_TEST_SOURCE_WORKSPACE");
        let name = unique_name(prefix);
        let output = fabio()
            .args([
                "--wrap-untrusted",
                group,
                "create",
                "--workspace",
                &workspace,
                "--name",
                &name,
            ])
            .timeout(std::time::Duration::from_mins(4))
            .assert()
            .success();
        let id = extract_data(&parse_json(&output))["id"]
            .as_str()
            .expect("created fixture must return an ID")
            .to_string();
        Self {
            workspace,
            id,
            name,
            group: group.to_string(),
            cleanup_attempted: false,
        }
    }

    pub fn command(&self, subcommand: &str) -> Command {
        let mut command = fabio();
        command.args([
            "--wrap-untrusted",
            &self.group,
            subcommand,
            "--workspace",
            &self.workspace,
            "--id",
            &self.id,
        ]);
        command.timeout(std::time::Duration::from_mins(4));
        command
    }

    pub fn cleanup(mut self) {
        // Never replay a failed/uncertain explicit deletion from Drop.
        self.cleanup_attempted = true;
        self.command("delete").assert().success();
    }
}

impl Drop for ScopedTestItem {
    fn drop(&mut self) {
        if !self.cleanup_attempted {
            match self.command("delete").output() {
                Ok(output) if output.status.success() => {}
                _ => eprintln!(
                    "Warning: failed to delete a test-owned {} fixture",
                    self.group
                ),
            }
        }
    }
}
