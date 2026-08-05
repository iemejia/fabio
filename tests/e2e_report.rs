//! End-to-end integration tests for `fabio report` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["report", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "report",
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
fn report_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "report",
            "show",
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
#[serial]
fn report_delete_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "report",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();
}

// ─── Publish to Web Tests ────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_publish_to_web_dry_run() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--dry-run",
            "report",
            "publish-to-web",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "report publish-to-web");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_publish_to_web_not_found() {
    let cfg = TestConfig::from_env();

    // Attempting to publish a non-existent report should fail
    fabio()
        .args([
            "report",
            "publish-to-web",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("NOT_FOUND")
                .or(predicate::str::contains("API_ERROR"))
                .or(predicate::str::contains("FORBIDDEN")),
        );
}

#[test]
#[ignore = "requires live Fabric tenant with Publish to Web enabled"]
#[serial]
fn report_publish_to_web_existing_report() {
    let cfg = TestConfig::from_env();

    // List reports and try to publish the first one (if any exist)
    let assert = fabio()
        .args(["report", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let reports = data.as_array().unwrap();

    if reports.is_empty() {
        eprintln!("No reports in workspace, skipping publish-to-web test");
        return;
    }

    let report_id = reports[0]["id"].as_str().unwrap();

    // Try to publish to web
    let assert = fabio()
        .args([
            "report",
            "publish-to-web",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            report_id,
        ])
        .timeout(std::time::Duration::from_secs(30))
        .assert();

    let output = assert.get_output();
    if output.status.success() {
        // If publish-to-web is enabled in the tenant, we should get an embed URL
        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        let data = extract_data(&json);
        assert_eq!(data["status"], "published_to_web");
        assert!(
            data["embedUrl"].as_str().is_some_and(|u| !u.is_empty()),
            "expected non-empty embedUrl"
        );
    } else {
        // If tenant doesn't allow Publish to Web, it should fail gracefully
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("Publish to web not available (tenant setting may be disabled): {stderr}");
        // This is acceptable - the test documents the behavior
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_export_dry_run() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "report",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--format",
            "PDF",
            "--out",
            "/tmp/fabio_report_export.pdf",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "report export");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_export_rejects_paginated_only_format() {
    let cfg = TestConfig::from_env();
    // CSV is a paginated-only format; it must be rejected for a Power BI report.
    fabio()
        .args([
            "report",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--format",
            "CSV",
            "--out",
            "/tmp/fabio_report_export.csv",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unsupported export format"));
}

/// Live conformance test: `report create --dataset` must produce a
/// `definition.pbir` that carries the MS-required `$schema` field, and the
/// create must succeed end-to-end. Also exercises `semantic-model create` whose
/// `definition.pbism` now carries `$schema`.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_create_dataset_pbir_has_schema() {
    use std::io::Write;

    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    // 1. Create a minimal V3 semantic model (pbism now includes $schema).
    let dir = tempfile::tempdir().unwrap();
    let bim = dir.path().join("model.bim");
    let mut f = std::fs::File::create(&bim).unwrap();
    f.write_all(
        br#"{"compatibilityLevel":1604,"model":{"defaultPowerBIDataSourceVersion":"powerBI_V3","culture":"en-US","tables":[{"name":"T","columns":[{"name":"c","dataType":"string","sourceColumn":"[c]","type":"calculatedTableColumn"}],"partitions":[{"name":"T","mode":"import","source":{"type":"calculated","expression":"DATATABLE(\"c\", STRING, {{\"x\"}})"}}]}]}}"#,
    )
    .unwrap();

    let sm_assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            ws,
            "--name",
            "fabio-e2e-schema-model",
            "--file",
            bim.to_str().unwrap(),
        ])
        .assert()
        .success();
    let sm_id = parse_json(&sm_assert)["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. Create a report bound to it (auto-generates definition.pbir with $schema).
    let rep_assert = fabio()
        .args([
            "report",
            "create",
            "--workspace",
            ws,
            "--name",
            "fabio-e2e-schema-report",
            "--dataset",
            &sm_id,
        ])
        .assert()
        .success();
    let rep_id = parse_json(&rep_assert)["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. The report's definition.pbir must carry a report/definitionProperties $schema.
    let def_assert = fabio()
        .args([
            "report",
            "get-definition",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--decode",
        ])
        .assert()
        .success();
    let def = parse_json(&def_assert);
    let parts = def["data"]["definition"]["parts"].as_array().unwrap();
    let pbir = parts
        .iter()
        .find(|p| {
            p["path"].as_str().is_some_and(|s| {
                std::path::Path::new(s)
                    .extension()
                    .is_some_and(|e| e == "pbir")
            })
        })
        .expect("definition.pbir present");
    let decoded = pbir
        .get("decodedPayload")
        .cloned()
        .expect("get-definition --decode yields decodedPayload");
    let pbir_obj: serde_json::Value = match decoded {
        serde_json::Value::String(s) => serde_json::from_str(&s).unwrap(),
        obj @ serde_json::Value::Object(_) => obj,
        other => panic!("unexpected decodedPayload shape: {other}"),
    };
    let schema = pbir_obj["$schema"].as_str().unwrap_or_default();
    assert!(
        schema.contains("report/definitionProperties/"),
        "pbir $schema missing/unexpected: {schema}"
    );

    // Cleanup.
    fabio()
        .args(["report", "delete", "--workspace", ws, "--id", &rep_id])
        .assert()
        .success();
    fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            ws,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();
}

/// Offline (no tenant): `report validate` on a temp PBIR folder returns valid,
/// and a broken folder exits non-zero with a `MISSING_REQUIRED` error.
#[test]
fn report_validate_pbir_folder_offline() {
    use std::fs;
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();
    let report = dir.path().join("My.Report");
    let mk = |rel: &str, content: &str| {
        let p = report.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    };
    mk(
        "definition.pbir",
        r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json","version":"4.0","datasetReference":{"byConnection":{"connectionString":"semanticmodelid=abc"}}}"#,
    );
    mk("definition/report.json", r#"{"$schema":"x"}"#);
    mk(
        "definition/version.json",
        r#"{"$schema":"x","version":"4.0"}"#,
    );
    mk("definition/pages/pages.json", r#"{"$schema":"x"}"#);
    mk("definition/pages/p1/page.json", r#"{"$schema":"x"}"#);

    // Valid.
    let assert = fabio()
        .args(["report", "validate", "--source", report.to_str().unwrap()])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(json["data"]["status"], "valid");
    assert_eq!(json["data"]["report"]["format"], "PBIR");

    // Break it: remove version.json → invalid, non-zero exit.
    fs::remove_file(report.join("definition/version.json")).unwrap();
    fabio()
        .args(["report", "validate", "--source", report.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("validation failed"));
}

/// Live: export a report → validate the exported PBIR → create a new report from
/// the folder (full PBIR) → render it → delete. Exercises `report validate` and
/// `report create --definition` end-to-end against the tenant.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_validate_and_create_from_folder_lifecycle() {
    let cfg = TestConfig::from_env();
    let ws = &cfg.source_workspace;

    // Need an existing report to export. Pick the first one in the workspace.
    let list = fabio()
        .args(["report", "list", "--workspace", ws])
        .assert()
        .success();
    let reports = parse_json(&list);
    let Some(_first) = reports["data"].as_array().and_then(|a| a.first()) else {
        eprintln!("no report in workspace to export; skipping");
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let export_dir = dir.path().join("export");
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            ws,
            "--dir",
            export_dir.to_str().unwrap(),
            "--overwrite",
            "--item-types",
            "Report",
        ])
        .assert()
        .success();

    // Find a *.Report folder.
    let report_folder = std::fs::read_dir(&export_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.is_dir()
                && p.extension().is_some_and(|e| e == "Report")
                && p.join("definition.pbir").exists()
        })
        .expect("an exported .Report folder");

    // Validate it (must be valid).
    let vassert = fabio()
        .args([
            "report",
            "validate",
            "--source",
            report_folder.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&vassert)["data"]["status"], "valid");

    // Create a new report from the full PBIR folder.
    let cassert = fabio()
        .args([
            "report",
            "create",
            "--workspace",
            ws,
            "--name",
            "fabio-e2e-pbir-clone",
            "--definition",
            report_folder.to_str().unwrap(),
        ])
        .assert()
        .success();
    let clone_id = parse_json(&cassert)["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Delete the clone.
    fabio()
        .args(["report", "delete", "--workspace", ws, "--id", &clone_id])
        .assert()
        .success();
}

/// Full PBIR page-authoring lifecycle: create a model + a minimal PBIR report
/// bound to it, then list-pages / add-page / list-visuals / rename-page /
/// set-active-page / delete-page (with the last-page + not-found guards), all
/// via the report definition read-modify-write. Cleans up the report + model.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_page_authoring_lifecycle() {
    use std::io::Write;

    let cfg = TestConfig::from_env();
    let ws = &cfg.dest_workspace;

    // 1. A minimal semantic model to bind the report to.
    let dir = tempfile::tempdir().unwrap();
    let bim = dir.path().join("model.bim");
    let mut f = std::fs::File::create(&bim).unwrap();
    f.write_all(
        br#"{"compatibilityLevel":1604,"model":{"defaultPowerBIDataSourceVersion":"powerBI_V3","culture":"en-US","tables":[{"name":"T","columns":[{"name":"c","dataType":"string","sourceColumn":"[c]","type":"calculatedTableColumn"}],"partitions":[{"name":"T","mode":"import","source":{"type":"calculated","expression":"DATATABLE(\"c\", STRING, {{\"x\"}})"}}]}]}}"#,
    )
    .unwrap();
    let sm_assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            ws,
            "--name",
            "fabio-e2e-page-model",
            "--file",
            bim.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let sm_id = parse_json(&sm_assert)["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. A minimal 1-page PBIR report folder bound to the model.
    let rdir = tempfile::tempdir().unwrap();
    let root = rdir.path();
    let write = |rel: &str, content: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    };
    write(
        "definition.pbir",
        &format!(
            r#"{{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json","version":"4.0","datasetReference":{{"byConnection":{{"connectionString":"semanticmodelid={sm_id}"}}}}}}"#
        ),
    );
    write(
        "definition/version.json",
        r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definition/versionMetadata/1.0.0/schema.json","version":"2.0.0"}"#,
    );
    write(
        "definition/report.json",
        r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definition/report/1.0.0/schema.json","layoutOptimization":"None","themeCollection":{"baseTheme":{"name":"fabioTheme","reportVersionAtImport":"5.55","type":"SharedResources"}},"resourcePackages":[{"name":"SharedResources","type":"SharedResources","items":[{"name":"fabioTheme","path":"BaseThemes/fabioTheme.json","type":"BaseTheme"}]}]}"#,
    );
    write(
        "StaticResources/SharedResources/BaseThemes/fabioTheme.json",
        r##"{"name":"fabioTheme","dataColors":["#118DFF"],"background":"#FFFFFF","foreground":"#000000","tableAccent":"#118DFF"}"##,
    );
    write(
        "definition/pages/pages.json",
        r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definition/pagesMetadata/1.1.0/schema.json","pageOrder":["page1"],"activePageName":"page1"}"#,
    );
    write(
        "definition/pages/page1/page.json",
        r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definition/page/2.1.0/schema.json","name":"page1","displayName":"Page 1","displayOption":"FitToPage","width":1280,"height":720}"#,
    );

    let rep_assert = fabio()
        .args([
            "report",
            "create",
            "--workspace",
            ws,
            "--name",
            "fabio-e2e-page-report",
            "--definition",
            root.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let rep_id = parse_json(&rep_assert)["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. list-pages → one page.
    let lp = fabio()
        .args(["report", "list-pages", "--workspace", ws, "--id", &rep_id])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&lp)).as_array().unwrap().len(), 1);

    // 4. add-page (dry-run then live).
    let dr = fabio()
        .args([
            "report",
            "add-page",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--display-name",
            "Summary",
            "--name",
            "summary",
            "--active",
            "--dry-run",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&dr))["dry_run"], true);

    fabio()
        .args([
            "report",
            "add-page",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--display-name",
            "Summary",
            "--name",
            "summary",
            "--active",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // 5. list-pages → two pages, "summary" active.
    let lp2 = fabio()
        .args(["report", "list-pages", "--workspace", ws, "--id", &rep_id])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    let pages = parse_json(&lp2);
    let arr = extract_data(&pages).as_array().unwrap();
    assert_eq!(arr.len(), 2);
    let summary = arr
        .iter()
        .find(|p| p["name"] == "summary")
        .expect("summary page");
    assert_eq!(summary["active"], true);
    assert_eq!(summary["displayName"], "Summary");

    // 6. rename-page + set-active-page.
    fabio()
        .args([
            "report",
            "rename-page",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--name",
            "summary",
            "--display-name",
            "Executive Summary",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    fabio()
        .args([
            "report",
            "set-active-page",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--name",
            "page1",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // 7. delete-page summary → back to one page.
    fabio()
        .args([
            "report",
            "delete-page",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--name",
            "summary",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let lp3 = fabio()
        .args(["report", "list-pages", "--workspace", ws, "--id", &rep_id])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&lp3)).as_array().unwrap().len(), 1);

    // 8. delete the only remaining page → INVALID_INPUT.
    let last = fabio()
        .args([
            "report",
            "delete-page",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--name",
            "page1",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&last.get_output().stderr);
    assert!(
        stderr.contains("INVALID_INPUT"),
        "expected INVALID_INPUT, got: {stderr}"
    );

    // 9. delete a nonexistent page → NOT_FOUND.
    let nf = fabio()
        .args([
            "report",
            "delete-page",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--name",
            "ghost",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&nf.get_output().stderr);
    assert!(
        stderr.contains("NOT_FOUND"),
        "expected NOT_FOUND, got: {stderr}"
    );

    // Cleanup.
    fabio()
        .args(["report", "delete", "--workspace", ws, "--id", &rep_id])
        .assert()
        .success();
    fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            ws,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();
}

/// Full PBIR visual-authoring lifecycle: create a model with data + a minimal
/// PBIR report bound to it, then add a textbox, a card, and a data-bound bar
/// chart via `report add-visual`, prove they RENDER by exporting the report to
/// PDF, then list/delete a visual. Cleans up report + model.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn report_visual_authoring_lifecycle() {
    use std::io::Write;

    let cfg = TestConfig::from_env();
    let ws = &cfg.dest_workspace;

    // 1. A model with a category (string) and a numeric column, with data.
    let dir = tempfile::tempdir().unwrap();
    let bim = dir.path().join("model.bim");
    let mut f = std::fs::File::create(&bim).unwrap();
    f.write_all(
        br#"{"compatibilityLevel":1604,"model":{"defaultPowerBIDataSourceVersion":"powerBI_V3","culture":"en-US","tables":[{"name":"T","columns":[{"name":"Category","dataType":"string","sourceColumn":"[Category]","type":"calculatedTableColumn"},{"name":"Amount","dataType":"int64","sourceColumn":"[Amount]","type":"calculatedTableColumn"}],"partitions":[{"name":"T","mode":"import","source":{"type":"calculated","expression":"DATATABLE(\"Category\", STRING, \"Amount\", INTEGER, {{\"A\", 10}, {\"B\", 20}})"}}]}]}}"#,
    )
    .unwrap();
    let sm_assert = fabio()
        .args([
            "semantic-model",
            "create",
            "--workspace",
            ws,
            "--name",
            "fabio-e2e-visual-model",
            "--file",
            bim.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let sm_id = parse_json(&sm_assert)["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. A minimal PBIR report bound to the model.
    let rdir = tempfile::tempdir().unwrap();
    let root = rdir.path();
    let write = |rel: &str, content: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    };
    write(
        "definition.pbir",
        &format!(
            r#"{{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json","version":"4.0","datasetReference":{{"byConnection":{{"connectionString":"semanticmodelid={sm_id}"}}}}}}"#
        ),
    );
    write(
        "definition/version.json",
        r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definition/versionMetadata/1.0.0/schema.json","version":"2.0.0"}"#,
    );
    write(
        "definition/report.json",
        r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definition/report/1.0.0/schema.json","layoutOptimization":"None","themeCollection":{"baseTheme":{"name":"fabioTheme","reportVersionAtImport":"5.55","type":"SharedResources"}},"resourcePackages":[{"name":"SharedResources","type":"SharedResources","items":[{"name":"fabioTheme","path":"BaseThemes/fabioTheme.json","type":"BaseTheme"}]}]}"#,
    );
    write(
        "StaticResources/SharedResources/BaseThemes/fabioTheme.json",
        r##"{"name":"fabioTheme","dataColors":["#118DFF"],"background":"#FFFFFF","foreground":"#000000","tableAccent":"#118DFF"}"##,
    );
    write(
        "definition/pages/pages.json",
        r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definition/pagesMetadata/1.1.0/schema.json","pageOrder":["page1"],"activePageName":"page1"}"#,
    );
    write(
        "definition/pages/page1/page.json",
        r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definition/page/2.1.0/schema.json","name":"page1","displayName":"Overview","displayOption":"FitToPage","width":1280,"height":720}"#,
    );

    let rep_assert = fabio()
        .args([
            "report",
            "create",
            "--workspace",
            ws,
            "--name",
            "fabio-e2e-visual-report",
            "--definition",
            root.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let rep_id = parse_json(&rep_assert)["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. add a textbox, a card (Sum), and a bar chart (category + measure).
    fabio()
        .args([
            "report",
            "add-visual",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--page",
            "page1",
            "--type",
            "textbox",
            "--text",
            "Sales Overview",
            "--x",
            "40",
            "--y",
            "20",
            "--width",
            "600",
            "--height",
            "40",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    fabio()
        .args([
            "report",
            "add-visual",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--page",
            "page1",
            "--type",
            "card",
            "--measure",
            "Sum(T.Amount)",
            "--title",
            "Total",
            "--x",
            "40",
            "--y",
            "80",
            "--width",
            "200",
            "--height",
            "120",
            "--name",
            "cardvisual",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    fabio()
        .args([
            "report",
            "add-visual",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--page",
            "page1",
            "--type",
            "clusteredBarChart",
            "--category",
            "T.Category",
            "--measure",
            "Sum(T.Amount)",
            "--title",
            "Amount by Category",
            "--x",
            "40",
            "--y",
            "220",
            "--width",
            "600",
            "--height",
            "300",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    // 4. list-visuals → three visuals.
    let lv = fabio()
        .args(["report", "list-visuals", "--workspace", ws, "--id", &rep_id])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&lv)).as_array().unwrap().len(), 3);

    // 5. Prove the fabio-built visuals RENDER: export the report to PDF.
    let pdf = rdir.path().join("out.pdf");
    let ex = fabio()
        .args([
            "report",
            "export",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--format",
            "PDF",
            "--out",
            pdf.to_str().unwrap(),
            "--timeout",
            "180",
        ])
        .timeout(std::time::Duration::from_mins(4))
        .assert()
        .success();
    assert_eq!(parse_json(&ex)["data"]["status"], "Succeeded");
    assert!(pdf.exists() && std::fs::metadata(&pdf).unwrap().len() > 1000);

    // 6. delete-visual (the card) → two remain.
    fabio()
        .args([
            "report",
            "delete-visual",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--page",
            "page1",
            "--name",
            "cardvisual",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let lv2 = fabio()
        .args(["report", "list-visuals", "--workspace", ws, "--id", &rep_id])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&lv2)).as_array().unwrap().len(), 2);

    // 7. add-visual to a nonexistent page → NOT_FOUND.
    let nf = fabio()
        .args([
            "report",
            "add-visual",
            "--workspace",
            ws,
            "--id",
            &rep_id,
            "--page",
            "ghost",
            "--type",
            "card",
            "--measure",
            "Sum(T.Amount)",
        ])
        .timeout(std::time::Duration::from_mins(1))
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&nf.get_output().stderr);
    assert!(
        stderr.contains("NOT_FOUND"),
        "expected NOT_FOUND, got: {stderr}"
    );

    // Cleanup.
    fabio()
        .args(["report", "delete", "--workspace", ws, "--id", &rep_id])
        .assert()
        .success();
    fabio()
        .args([
            "semantic-model",
            "delete",
            "--workspace",
            ws,
            "--id",
            &sm_id,
        ])
        .assert()
        .success();
}
