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
//! - `FABIO_TEST_CAPACITY_ID` - Active capacity ID (optional)
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
            source_workspace: env::var("FABIO_TEST_SOURCE_WORKSPACE")
                .unwrap_or_else(|_| "00000000-0000-0000-0000-000000000001".to_string()),
            source_lakehouse: env::var("FABIO_TEST_SOURCE_LAKEHOUSE")
                .unwrap_or_else(|_| "00000000-0000-0000-0000-000000000002".to_string()),
            dest_workspace: env::var("FABIO_TEST_DEST_WORKSPACE")
                .unwrap_or_else(|_| "00000000-0000-0000-0000-000000000003".to_string()),
            dest_lakehouse: env::var("FABIO_TEST_DEST_LAKEHOUSE")
                .unwrap_or_else(|_| "00000000-0000-0000-0000-000000000004".to_string()),
            notebook_id: env::var("FABIO_TEST_NOTEBOOK_ID")
                .unwrap_or_else(|_| "00000000-0000-0000-0000-000000000005".to_string()),
            capacity_id: env::var("FABIO_TEST_CAPACITY_ID")
                .unwrap_or_else(|_| "00000000-0000-0000-0000-000000000006".to_string()),
        }
    }
}

/// Build a `fabio` command ready to execute.
pub fn fabio() -> Command {
    Command::cargo_bin("fabio").expect("failed to find fabio binary")
}

/// Parse the stdout of a successful fabio command as JSON.
pub fn parse_json(output: &assert_cmd::assert::Assert) -> Value {
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    serde_json::from_str(&stdout).expect("failed to parse stdout as JSON")
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
