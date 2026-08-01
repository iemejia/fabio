use assert_cmd::Command;
use predicates::prelude::*;
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
fn paginated_report_get_definition_help() {
    // Exercises flag/help parsing for get-definition.
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
    // A minimal but VALID RDL (2016 schema) with a single textbox and no data
    // source — renders without a dataset. The file is deliberately named
    // differently from the report display name to prove the fix synthesizes the
    // definition part path as `<displayName>.rdl` (not the file basename).
    // Regression: the create body must NOT include a `format` field — the Fabric
    // API rejects `format: "PaginatedReportDefinition"` with `InvalidDefinitionFormat`.
    let rdl_path = dir.path().join("some-other-filename.rdl");
    let mut f = fs::File::create(&rdl_path).unwrap();
    f.write_all(
        br#"<?xml version="1.0" encoding="utf-8"?>
<Report xmlns="http://schemas.microsoft.com/sqlserver/reporting/2016/01/reportdefinition" xmlns:rd="http://schemas.microsoft.com/SQLServer/reporting/reportdesigner">
  <ReportSections>
    <ReportSection>
      <Body>
        <ReportItems>
          <Textbox Name="TextBox1">
            <Paragraphs><Paragraph><TextRuns><TextRun><Value>Hello from fabio</Value></TextRun></TextRuns></Paragraph></Paragraphs>
            <Top>0.1in</Top><Left>0.1in</Left><Height>0.3in</Height><Width>4in</Width>
          </Textbox>
        </ReportItems>
        <Height>1in</Height>
      </Body>
      <Width>6.5in</Width>
      <Page><PageHeight>11in</PageHeight><PageWidth>8.5in</PageWidth></Page>
    </ReportSection>
  </ReportSections>
</Report>"#,
    )
    .unwrap();

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
    assert_eq!(json["data"]["type"], "PaginatedReport");

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

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn paginated_report_export_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "paginated-report",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--format",
            "PDF",
            "--out",
            "/tmp/fabio_pr_export.pdf",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "paginated-report export");
    assert_eq!(json["data"]["details"]["format"], "PDF");
}

// A Power-BI-only format (PNG) must be rejected for a paginated report... actually
// PNG-as-IMAGE differs; here we assert a clearly-invalid format is rejected with an
// enumerated hint (offline validation, no tenant call needed beyond arg parsing).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn paginated_report_export_rejects_unknown_format() {
    let cfg = TestConfig::from_env();
    fabio()
        .args([
            "paginated-report",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--format",
            "TXT",
            "--out",
            "/tmp/fabio_pr_export.txt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unsupported export format"));
}

// Live plumbing: exercising ExportTo against a non-existent report must reach the
// Power BI API and return a clean not-found error (validates auth, URL, error path).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn paginated_report_export_plumbing_not_found() {
    let cfg = TestConfig::from_env();
    fabio()
        .args([
            "paginated-report",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--format",
            "PDF",
            "--out",
            "/tmp/fabio_pr_export.pdf",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("PowerBIEntityNotFound"));
}
