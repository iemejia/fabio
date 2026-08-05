//! End-to-end integration tests for `fabio lakehouse` shortcut commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_shortcut_create_get_delete() {
    let cfg = TestConfig::from_env();
    let shortcut_name = common::unique_name("sc_test");

    let target_json = format!(
        r#"{{"workspaceId":"{}","itemId":"{}","path":"Files"}}"#,
        cfg.source_workspace, cfg.source_lakehouse
    );

    // Create shortcut
    let assert = fabio()
        .args([
            "lakehouse",
            "create-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &shortcut_name,
            "--path",
            "Files",
            "--target-type",
            "oneLake",
            "--target",
            &target_json,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["name"], shortcut_name);
    assert_eq!(data["path"], "Files");
    assert!(data.get("target").is_some());
    assert_eq!(data["target"]["type"], "OneLake");

    // Get shortcut
    let assert = fabio()
        .args([
            "lakehouse",
            "get-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &shortcut_name,
            "--path",
            "Files",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["name"], shortcut_name);
    assert_eq!(
        data["target"]["oneLake"]["workspaceId"],
        cfg.source_workspace
    );

    // Delete shortcut
    let assert = fabio()
        .args([
            "lakehouse",
            "delete-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &shortcut_name,
            "--path",
            "Files",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
    assert_eq!(data["name"], shortcut_name);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_get_shortcut_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
            "get-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            "nonexistent_shortcut_xyz",
            "--path",
            "Files",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR",
        "Expected NOT_FOUND or API_ERROR, got: {code}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_delete_shortcut_not_found() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "lakehouse",
            "delete-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            "nonexistent_shortcut_abc",
            "--path",
            "Files",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    let code = err_json["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "NOT_FOUND" || code == "API_ERROR",
        "Expected NOT_FOUND or API_ERROR, got: {code}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_shortcut_create_in_tables_path() {
    let cfg = TestConfig::from_env();
    let shortcut_name = common::unique_name("sc_tbl");

    let target_json = format!(
        r#"{{"workspaceId":"{}","itemId":"{}","path":"Tables"}}"#,
        cfg.source_workspace, cfg.source_lakehouse
    );

    // Create shortcut in Tables path
    let assert = fabio()
        .args([
            "lakehouse",
            "create-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &shortcut_name,
            "--path",
            "Tables",
            "--target-type",
            "oneLake",
            "--target",
            &target_json,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["name"], shortcut_name);
    assert_eq!(data["path"], "Tables");

    // Delete shortcut
    fabio()
        .args([
            "lakehouse",
            "delete-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &shortcut_name,
            "--path",
            "Tables",
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_bulk_create_shortcuts_dry_run() {
    let cfg = TestConfig::from_env();

    let content = r#"[{"name":"sc1","path":"Files","target":{"oneLake":{"workspaceId":"00000000-0000-0000-0000-000000000000","itemId":"00000000-0000-0000-0000-000000000001","path":"Files"}}}]"#;

    let assert = fabio()
        .args([
            "lakehouse",
            "bulk-create-shortcuts",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--content",
            content,
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "lakehouse bulk-create-shortcuts");
    assert!(data["details"]["createShortcutRequests"].is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_bulk_create_shortcuts_with_conflict_policy_dry_run() {
    let cfg = TestConfig::from_env();

    let content = r#"{"createShortcutRequests":[{"name":"sc1","path":"Files","target":{"oneLake":{"workspaceId":"00000000-0000-0000-0000-000000000000","itemId":"00000000-0000-0000-0000-000000000001","path":"Files"}}}]}"#;

    let assert = fabio()
        .args([
            "lakehouse",
            "bulk-create-shortcuts",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--content",
            content,
            "--conflict-policy",
            "GenerateUniqueName",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "lakehouse bulk-create-shortcuts");
}

// ─── Create Shortcut with --conflict-policy ─────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_create_shortcut_with_conflict_policy() {
    let cfg = TestConfig::from_env();

    // First create a shortcut
    let shortcut_name = common::unique_name("sc_policy");
    let target = serde_json::json!({
        "oneLake": {
            "workspaceId": cfg.source_workspace,
            "itemId": cfg.source_lakehouse,
            "path": "Files"
        }
    });

    let assert = fabio()
        .args([
            "lakehouse",
            "create-shortcut",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--name",
            &shortcut_name,
            "--path",
            "Files",
            "--target-type",
            "oneLake",
            "--target",
            &target.to_string(),
            "--conflict-policy",
            "GenerateUniqueName",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data["name"].as_str().is_some());

    // Clean up
    let created_name = data["name"].as_str().unwrap();
    fabio()
        .args([
            "lakehouse",
            "delete-shortcut",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--name",
            created_name,
            "--path",
            "Files",
        ])
        .assert()
        .success();
}

#[test]
fn lakehouse_create_shortcut_unknown_target_type_errors() {
    // Fails before any network call: the target type is validated first.
    let assert = fabio()
        .args([
            "lakehouse",
            "create-shortcut",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "x",
            "--path",
            "Files",
            "--target-type",
            "dropbox",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("INVALID_INPUT") && stderr.contains("Unknown shortcut target type"),
        "unknown target type should be rejected with an enum hint: {stderr}"
    );
    assert!(
        stderr.contains("OneDriveSharePoint"),
        "hint should enumerate valid target types: {stderr}"
    );
}

#[test]
fn lakehouse_create_shortcut_typed_missing_required_flag_errors() {
    // adlsGen2 needs --location and --connection-id; omitting --connection-id
    // must fail fast with a targeted hint (before any network call).
    let assert = fabio()
        .args([
            "lakehouse",
            "create-shortcut",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--name",
            "x",
            "--path",
            "Files",
            "--target-type",
            "AdlsGen2",
            "--location",
            "https://acct.dfs.core.windows.net/container",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("--connection-id") && stderr.contains("adlsGen2"),
        "missing --connection-id should be reported for adlsGen2: {stderr}"
    );
}

/// Live lifecycle for the typed `OneLake` target + list-shortcuts. Creates a
/// shortcut using the typed flags (not raw JSON), verifies it appears in
/// list-shortcuts, then deletes it.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_typed_onelake_shortcut_and_list() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("sc_typed");

    // Create via typed OneLake flags.
    fabio()
        .args([
            "lakehouse",
            "create-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &name,
            "--path",
            "Files",
            "--target-type",
            "OneLake",
            "--target-workspace",
            &cfg.source_workspace,
            "--target-item",
            &cfg.source_lakehouse,
            "--target-path",
            "Files",
        ])
        .assert()
        .success();

    // list-shortcuts must include it.
    let assert = fabio()
        .args([
            "lakehouse",
            "list-shortcuts",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let found = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == name);
    assert!(
        found,
        "created shortcut '{name}' must appear in list-shortcuts: {json}"
    );

    // Cleanup.
    fabio()
        .args([
            "lakehouse",
            "delete-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &name,
            "--path",
            "Files",
        ])
        .assert()
        .success();
}

/// `create-shortcut --transform csvToDelta` — the Fabric REST API accepts a
/// `transform` object and echoes it in the create response. Asserts the exact
/// shape fabio sends (type + properties). (Actual Delta materialization is an
/// async Fabric Spark job — not asserted here.)
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_create_shortcut_with_csv_transform() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("sc_xform");

    let assert = fabio()
        .args([
            "lakehouse",
            "create-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &name,
            "--path",
            "Tables",
            "--target-type",
            "OneLake",
            "--target-workspace",
            &cfg.source_workspace,
            "--target-item",
            &cfg.source_lakehouse,
            "--target-path",
            "Files",
            "--transform",
            "csvToDelta",
            "--csv-delimiter",
            ",",
            "--transform-include-subfolders",
            "--conflict-policy",
            "CreateOrOverwrite",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["transform"]["type"], "csvToDelta");
    assert_eq!(data["transform"]["includeSubfolders"], true);
    assert_eq!(data["transform"]["properties"]["delimiter"], ",");
    assert_eq!(data["transform"]["properties"]["useFirstRowAsHeader"], true);

    // Cleanup.
    fabio()
        .args([
            "lakehouse",
            "delete-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &name,
            "--path",
            "Tables",
        ])
        .assert()
        .success();
}

/// `--transform parquet` (and other portal-only transforms) is rejected offline
/// with a clear "not available via the REST API" hint — no network call.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_create_shortcut_transform_parquet_rejected() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "lakehouse",
            "create-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            "x",
            "--path",
            "Tables",
            "--target-type",
            "OneLake",
            "--target-workspace",
            &cfg.source_workspace,
            "--target-item",
            &cfg.source_lakehouse,
            "--target-path",
            "Files",
            "--transform",
            "parquet",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["error"]["code"], "INVALID_INPUT");
    assert!(
        err["error"]["hint"]
            .as_str()
            .unwrap_or_default()
            .contains("portal-only")
    );
}
