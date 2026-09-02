//! End-to-end integration tests for `fabio eventhouse` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventhouse_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["eventhouse", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventhouse_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("eh_test");

    // Create
    let assert = fabio()
        .args([
            "eventhouse",
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
    let eh_id = data["id"].as_str().unwrap().to_string();

    // Delete
    let assert = fabio()
        .args([
            "eventhouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
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
fn eventhouse_update_name() {
    let cfg = TestConfig::from_env();
    let original = common::unique_name("eh_upd_o");
    let updated = common::unique_name("eh_upd_n");

    // Create
    let assert = fabio()
        .args([
            "eventhouse",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &original,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let eh_id = data["id"].as_str().unwrap().to_string();

    // Update
    let assert = fabio()
        .args([
            "eventhouse",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
            "--name",
            &updated,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], updated);

    // Cleanup
    fabio()
        .args([
            "eventhouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventhouse_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "eventhouse",
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
fn eventhouse_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "eventhouse",
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
    assert_eq!(json["data"]["would_execute"], "eventhouse create");
}

// eventhouse create --min-consumption-units sets
// creationPayload.minimumConsumptionUnits. Offline dry-run regression.
#[test]
fn eventhouse_create_min_consumption_units_in_body() {
    let assert = fabio()
        .args([
            "--dry-run",
            "eventhouse",
            "create",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--name",
            "eh_min",
            "--min-consumption-units",
            "2.25",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(extract_data(&json)["details"]["minConsumptionUnits"], 2.25);
}

/// Validates that the `Eventhouse` definition spec added to
/// `definition_requirements.json` matches reality: `getDefinition` returns the
/// canonical `EventhouseProperties.json` part, and the definition round-trips
/// through `updateDefinition` (proving `deploy_strategy=content` /
/// `deployable_from_definition=true` in the item-capability matrix).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventhouse_definition_roundtrips() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("eh_def");

    // Create.
    let created = fabio()
        .args([
            "eventhouse",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let eh_id = extract_data(&parse_json(&created))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // getDefinition returns the canonical required part.
    let def_assert = fabio()
        .args([
            "eventhouse",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let def_json = parse_json(&def_assert);
    let definition = &extract_data(&def_json)["definition"];
    let parts: Vec<&str> = definition["parts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["path"].as_str())
        .collect();
    assert!(
        parts.contains(&"EventhouseProperties.json"),
        "getDefinition must return the canonical EventhouseProperties.json part; got {parts:?}"
    );

    // Round-trip: re-apply the exact definition via updateDefinition.
    let envelope = serde_json::json!({ "definition": definition });
    let tmp = std::env::temp_dir().join(format!("{name}-def.json"));
    std::fs::write(&tmp, serde_json::to_vec(&envelope).unwrap()).unwrap();
    fabio()
        .args([
            "eventhouse",
            "update-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
            "--file",
            tmp.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let _ = std::fs::remove_file(&tmp);

    // Clean up.
    fabio()
        .args([
            "eventhouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
        ])
        .assert()
        .success();
}

#[test]
fn eventhouse_query_follow_flags_require_follow() {
    // Offline: the --follow-only flags are rejected before any network call, so
    // this runs without a tenant.
    let assert = fabio()
        .args([
            "eventhouse",
            "query",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--kql",
            "Probe | count",
            "--interval",
            "2",
        ])
        .assert()
        .failure();
    // The error envelope is written to stderr.
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let json: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(json["error"]["code"], "INVALID_INPUT");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--follow")
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn eventhouse_query_oneshot_and_follow_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("eh_q");

    // Create eventhouse (auto-provisions a same-named KQL database).
    let assert = fabio()
        .args([
            "eventhouse",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let eh_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // query-uri returns the cluster URI.
    let assert = fabio()
        .args([
            "eventhouse",
            "query-uri",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
        ])
        .assert()
        .success();
    assert!(
        extract_data(&parse_json(&assert))["queryUri"]
            .as_str()
            .unwrap()
            .starts_with("https://")
    );

    // list-databases includes the auto-created database.
    let assert = fabio()
        .args([
            "eventhouse",
            "list-databases",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
        ])
        .assert()
        .success();
    assert!(extract_data(&parse_json(&assert)).is_array());

    // Create a table + ingest 3 rows (management commands via the query endpoint).
    fabio()
        .args([
            "eventhouse",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
            "--kql",
            ".create table Probe (ts:datetime, seq:long, msg:string)",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    fabio()
        .args([
            "eventhouse", "query", "--workspace", &cfg.dest_workspace, "--id", &eh_id,
            "--kql", ".ingest inline into table Probe <| 2026-01-01T00:00:01Z,1,a\n2026-01-01T00:00:02Z,2,b\n2026-01-01T00:00:03Z,3,c",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    std::thread::sleep(std::time::Duration::from_secs(5));

    // One-shot query with --timeout.
    let assert = fabio()
        .args([
            "eventhouse",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
            "--kql",
            "Probe | count",
            "--timeout",
            "30",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data[0]["Count"], 3);

    // Follow: NDJSON stream, bounded by --max-duration; final line is follow_complete.
    let assert = fabio()
        .args([
            "eventhouse",
            "query",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
            "--kql",
            "Probe | count",
            "--follow",
            "--interval",
            "2",
            "--max-duration",
            "5",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let last = stdout.lines().last().unwrap();
    let summary: serde_json::Value = serde_json::from_str(last).unwrap();
    assert_eq!(summary["status"], "follow_complete");
    assert!(summary["cycles"].as_u64().unwrap() >= 1);

    // Cleanup.
    fabio()
        .args([
            "eventhouse",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &eh_id,
        ])
        .assert()
        .success();
}
