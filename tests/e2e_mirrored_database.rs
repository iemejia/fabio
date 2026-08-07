//! E2E integration tests for the `fabio mirrored-database` command group.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};

#[test]
#[ignore = "requires live Fabric tenant"]
fn mirrored_database_list() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "mirrored-database",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    assert!(json.get("data").is_some());
    assert!(json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn mirrored_database_create_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "mirrored-database",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-mirror-db",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn mirrored_database_update_requires_fields() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "mirrored-database",
            "update",
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
fn mirrored_database_delete_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "mirrored-database",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn mirrored_database_start_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "mirrored-database",
            "start",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn mirrored_database_stop_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "mirrored-database",
            "stop",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

/// `mirrored-database landing-zone` builds the `OneLake` landing-zone URL purely
/// from the workspace + item id — no API call, so it runs without a tenant.
#[test]
fn mirrored_database_landing_zone_url_is_constructed() {
    let ws = "aaaaaaaa-1111-2222-3333-444444444444";
    let id = "bbbbbbbb-1111-2222-3333-444444444444";
    let assert = fabio()
        .args([
            "mirrored-database",
            "landing-zone",
            "--workspace",
            ws,
            "--id",
            id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let url = data["landingZoneUrl"].as_str().unwrap();
    assert!(url.starts_with("https://onelake.dfs.fabric.microsoft.com/"));
    assert!(url.contains(ws) && url.contains(id));
    assert!(url.ends_with("/Files/LandingZone"));
}

/// Live: `create --open-mirroring` configures the push-based `GenericMirror`
/// definition (an empty create leaves the item `MirroringDefinitionMissing`), and
/// `landing-zone` returns the item's `OneLake` landing-zone URL.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial_test::serial]
fn mirrored_database_open_mirroring_lifecycle() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;
    let name = common::unique_name("openmirror");

    let assert = fabio()
        .args([
            "mirrored-database",
            "create",
            "--workspace",
            ws,
            "--name",
            &name,
            "--open-mirroring",
        ])
        .timeout(std::time::Duration::from_mins(3))
        .assert()
        .success();
    let md_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // The GenericMirror definition must be present (no MirroringDefinitionMissing).
    let assert = fabio()
        .args([
            "mirrored-database",
            "get-definition",
            "--workspace",
            ws,
            "--id",
            &md_id,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("mirroring.json"));

    // Landing zone URL targets this item's OneLake Files.
    let assert = fabio()
        .args([
            "mirrored-database",
            "landing-zone",
            "--workspace",
            ws,
            "--id",
            &md_id,
        ])
        .assert()
        .success();
    let url = extract_data(&parse_json(&assert))["landingZoneUrl"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(url.contains(&md_id) && url.ends_with("/Files/LandingZone"));

    fabio()
        .args([
            "mirrored-database",
            "delete",
            "--workspace",
            ws,
            "--id",
            &md_id,
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// status / table-status use POST (regression: they previously used GET and
// returned EntityNotFound). table-status renders a per-table list.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant with a mirrored database"]
fn mirrored_database_status_and_table_status() {
    let cfg = TestConfig::from_env();
    let Ok(md_id) = std::env::var("FABIO_TEST_MIRRORED_DATABASE_ID") else {
        return; // skip when not configured
    };

    // status returns a {status: ...} object (via POST).
    let assert = fabio()
        .args([
            "mirrored-database",
            "status",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &md_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(extract_data(&json).get("status").is_some());

    // table-status returns a per-table list (via POST).
    let assert = fabio()
        .args([
            "mirrored-database",
            "table-status",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &md_id,
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(extract_data(&json).is_array());
}
