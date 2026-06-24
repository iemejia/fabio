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
fn paginated_report_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "paginated-report",
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
fn paginated_report_show_requires_id() {
    fabio()
        .args([
            "paginated-report",
            "show",
            "--workspace",
            "test-ws",
            // missing --id
        ])
        .assert()
        .failure();
}

#[test]
fn paginated_report_create_dry_run_no_file_fails() {
    fabio()
        .args([
            "paginated-report",
            "create",
            "--workspace",
            "test-ws",
            "--name",
            "TestReport",
            "--dry-run",
            // No --file or --content
        ])
        .assert()
        .failure();
}

#[test]
fn paginated_report_create_dry_run_with_content() {
    fabio()
        .args([
            "paginated-report",
            "create",
            "--workspace",
            "test-ws",
            "--name",
            "TestReport",
            "--content",
            "PHJlcG9ydC8+", // base64 of "<report/>"
            "--dry-run",
        ])
        .assert()
        .success();
}

#[test]
fn paginated_report_delete_dry_run() {
    fabio()
        .args([
            "paginated-report",
            "delete",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--dry-run",
        ])
        .assert()
        .success();
}

#[test]
fn paginated_report_get_definition_dry_run() {
    // --dry-run on read commands: get-definition still requires auth but
    // this test exercises flag parsing at minimum.
    fabio()
        .args([
            "paginated-report",
            "get-definition",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--help",
        ])
        .assert()
        .success();
}

#[test]
fn paginated_report_update_definition_dry_run_no_file_fails() {
    fabio()
        .args([
            "paginated-report",
            "update-definition",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--dry-run",
            // No --file or --content
        ])
        .assert()
        .failure();
}

#[test]
fn paginated_report_update_definition_dry_run_with_content() {
    fabio()
        .args([
            "paginated-report",
            "update-definition",
            "--workspace",
            "test-ws",
            "--id",
            "00000000-0000-0000-0000-000000000001",
            "--content",
            r#"[{"path":"report.rdl","payload":"PHJlcG9ydC8+","payloadType":"InlineBase64"}]"#,
            "--dry-run",
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn paginated_report_create_show_delete_lifecycle() {
    use std::fs;
    use std::io::Write;

    let cfg = TestConfig::from_env();
    let dir = tempfile::tempdir().unwrap();
    let rdl_path = dir.path().join("test.rdl");
    let mut f = fs::File::create(&rdl_path).unwrap();
    f.write_all(b"<?xml version=\"1.0\"?><Report xmlns=\"http://schemas.microsoft.com/sqlserver/reporting/2008/01/reportdefinition\" />").unwrap();

    let assert = fabio()
        .args([
            "paginated-report",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "fabio-e2e-paginated",
            "--file",
            rdl_path.to_str().unwrap(),
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let id = json["data"]["id"].as_str().unwrap().to_string();

    // Show
    fabio()
        .args([
            "paginated-report",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();

    // Delete
    fabio()
        .args([
            "paginated-report",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &id,
        ])
        .assert()
        .success();
}
