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
fn snowflake_database_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "snowflake-database",
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

/// The live API rejects a shell create; a Snowflake source connection is
/// required via `creationPayload.connectionId`. Offline: `--dry-run` shows the
/// connection id in the plan, and `--snowflake-database-name` requires
/// `--connection-id` (clap).
#[test]
fn snowflake_database_create_dry_run_includes_connection() {
    let assert = fabio()
        .args([
            "snowflake-database",
            "create",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "Snow1",
            "--connection-id",
            "11111111-1111-1111-1111-111111111111",
            "--snowflake-database-name",
            "MYDB",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "snowflake-database create");
    assert_eq!(
        json["data"]["details"]["connectionId"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(json["data"]["details"]["snowflakeDatabaseName"], "MYDB");
}

#[test]
fn snowflake_database_name_requires_connection_id() {
    // --snowflake-database-name without --connection-id is a clap error.
    fabio()
        .args([
            "snowflake-database",
            "create",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "Snow1",
            "--snowflake-database-name",
            "MYDB",
        ])
        .assert()
        .failure();
}
