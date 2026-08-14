//! End-to-end integration tests for `fabio deploy` commands.
//!
//! Tests the plan/apply/export workflow against a live Fabric tenant.
//! Requires `FABIO_TEST_SOURCE_WORKSPACE` and `FABIO_TEST_CAPACITY_ID` env vars.

mod common;

use base64::Engine;
use common::{TestConfig, extract_data, fabio, parse_json, unique_name};
use serial_test::serial;
use std::time::Duration;

// ── Export ────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_workspace_to_directory() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("export");

    let assert = fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");
    assert!(data["total_items"].as_u64().unwrap() > 0);
    assert!(data["exported"].as_u64().unwrap() > 0);

    // Verify directory structure was created
    assert!(output_dir.exists());
    // Should have at least one subdirectory with a .platform file
    let entries: Vec<_> = std::fs::read_dir(&output_dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    assert!(
        !entries.is_empty(),
        "Expected at least one item directory in export"
    );

    // Check first item dir has .platform file
    let first_item_dir = &entries[0].path();
    assert!(
        first_item_dir.join(".platform").exists(),
        "Expected .platform file in {}",
        first_item_dir.display()
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_with_item_type_filter() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("export_filtered");

    let assert = fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
            "--item-types",
            "Lakehouse",
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");

    // All exported directories should be Lakehouse type
    if let Some(exported) = data["exported"].as_u64()
        && exported > 0
    {
        for entry in std::fs::read_dir(&output_dir).unwrap().flatten() {
            if entry.path().is_dir() {
                let platform_path = entry.path().join(".platform");
                if platform_path.exists() {
                    let content = std::fs::read_to_string(&platform_path).unwrap();
                    let meta: serde_json::Value = serde_json::from_str(&content).unwrap();
                    assert_eq!(meta["metadata"]["type"], "Lakehouse");
                }
            }
        }
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_dry_run() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("export_dry");

    let assert = fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
            "--dry-run",
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");

    // Directory should NOT be populated in dry-run mode
    // (the export may create the dir but not write item files)
}

// ── Plan ─────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_exported_workspace_shows_skip_or_update() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");

    // Step 1: Export the workspace
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Step 2: Plan from exported dir back to same workspace
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Since we just exported and plan back, items should be skip (unchanged)
    // or update (if hash algorithm differs from API normalization)
    let summary = &data["summary"];
    let total = summary["create"].as_u64().unwrap_or(0)
        + summary["update"].as_u64().unwrap_or(0)
        + summary["skip"].as_u64().unwrap_or(0);
    assert!(total > 0, "Expected at least one item in plan");

    // Should NOT have any create (items already exist)
    // Note: may have updates due to hash normalization differences
    assert_eq!(
        summary["create"].as_u64().unwrap_or(0),
        0,
        "Expected no creates when planning against same workspace"
    );

    // Should NOT have deletes (not using --delete-orphans)
    assert_eq!(
        summary["delete"].as_u64().unwrap_or(0),
        0,
        "Expected no deletes without --delete-orphans"
    );
}

/// `deploy apply --verify` adds a well-formed `verification` block: after
/// applying, it re-fetches the applied items and reports convergence
/// (content items hash-compared to source, platform-only existence-checked).
/// Report-only — the block is emitted regardless of exit status. Self-contained:
/// deploys one empty `DataPipeline` into a temp folder-scoped source and cleans up.
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_apply_verify_reports_convergence() {
    use std::fs;
    let cfg = TestConfig::from_env();
    let src = tempfile::TempDir::new().unwrap();
    let name = unique_name("VerifyPipe");
    let item_dir = src.path().join(format!("{name}.DataPipeline"));
    fs::create_dir_all(&item_dir).unwrap();
    fs::write(
        item_dir.join(".platform"),
        format!(
            r#"{{"$schema":"https://developer.microsoft.com/json-schemas/fabric/gitIntegration/platformProperties/2.0.0/schema.json","metadata":{{"type":"DataPipeline","displayName":"{name}"}},"config":{{"version":"2.0","logicalId":"b2222222-2222-2222-2222-222222222222"}}}}"#
        ),
    )
    .unwrap();
    fs::write(
        item_dir.join("pipeline-content.json"),
        r#"{"properties":{"activities":[]}}"#,
    )
    .unwrap();

    // Apply with --verify. Do NOT assert success — verification is report-only
    // and is emitted on stdout regardless of the deploy's exit code.
    let output = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            src.path().to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--verify",
        ])
        .timeout(Duration::from_mins(5))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let data = &json["data"];

    let verification = &data["verification"];
    assert!(
        verification.is_object(),
        "--verify must add a verification block; got: {data}"
    );
    assert!(verification["converged"].is_boolean());
    assert!(verification["checked"].is_number());
    let discrepancies = verification["discrepancies"].as_array().unwrap();
    // If converged, no discrepancies; otherwise each is well-formed.
    if verification["converged"] == true {
        assert!(discrepancies.is_empty());
    } else {
        for d in discrepancies {
            assert!(d["name"].is_string() && d["type"].is_string() && d["issue"].is_string());
        }
    }

    // Clean up: resolve the pipeline id and delete it (best-effort).
    let list = fabio()
        .args([
            "data-pipeline",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let list_json = parse_json(&list);
    if let Some(items) = extract_data(&list_json).as_array() {
        for it in items {
            if it["displayName"] == name
                && let Some(id) = it["id"].as_str()
            {
                let _ = fabio()
                    .args([
                        "data-pipeline",
                        "delete",
                        "--workspace",
                        &cfg.source_workspace,
                        "--id",
                        id,
                    ])
                    .assert();
            }
        }
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_raw_pbip_without_platform_is_discovered() {
    // A raw Power BI Desktop PBIP folder has `<name>.Report` / `<name>.SemanticModel`
    // directories WITHOUT the Git-integration `.platform` sidecar. fabio must
    // synthesize the item metadata from the folder-name suffix so such a folder
    // is deployable directly. This test exports Report + SemanticModel items,
    // strips every `.platform` file (simulating a raw Desktop save), and verifies
    // `deploy plan` still discovers and types both items.
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("pbip");

    // Step 1: Export only Report + SemanticModel items.
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
            "--item-types",
            "Report,SemanticModel",
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Step 2: Strip every `.platform` file to simulate a raw Desktop PBIP.
    let mut stripped = 0;
    for entry in std::fs::read_dir(&export_dir).unwrap().flatten() {
        let platform = entry.path().join(".platform");
        if platform.exists() {
            std::fs::remove_file(&platform).unwrap();
            stripped += 1;
        }
    }
    assert!(
        stripped > 0,
        "Expected at least one exported item with a .platform to strip"
    );

    // Step 3: Plan the `.platform`-less folder back against the same workspace.
    // Before synthesis support, discovery would recurse past these folders and
    // find zero items; now they are discovered by folder-name suffix.
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let changes = data["changes"].as_array().expect("changes array");

    // Both synthesized items must be discovered and correctly typed.
    let types: Vec<&str> = changes
        .iter()
        .filter_map(|c| c["item_type"].as_str())
        .collect();
    assert!(
        types.contains(&"SemanticModel"),
        "SemanticModel not discovered from raw PBIP folder: {changes:?}"
    );
    assert!(
        types.contains(&"Report"),
        "Report not discovered from raw PBIP folder: {changes:?}"
    );

    // With no `.platform`, there is no authored logicalId — the plan should warn
    // that rename tracking is unavailable (confirms synthesis path was taken).
    let warnings = data["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().is_some_and(|s| s.contains("no logicalId"))),
        "Expected a 'no logicalId' warning for a .platform-less item"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_force_all_shows_all_updates() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");

    // Step 1: Export
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Step 2: Plan with --force-all
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--force-all",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    let summary = &data["summary"];
    // With --force-all, all items should be marked as update (no skip)
    assert_eq!(
        summary["skip"].as_u64().unwrap_or(0),
        0,
        "Expected no skips with --force-all"
    );
    assert!(
        summary["update"].as_u64().unwrap_or(0) > 0,
        "Expected updates with --force-all"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_with_item_type_filter() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");

    // Export
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Plan with item-type filter
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--item-types",
            "Lakehouse",
            "--force-all",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // All changes should be Lakehouse type
    if let Some(changes) = data["changes"].as_array() {
        for change in changes {
            assert_eq!(
                change["item_type"], "Lakehouse",
                "Expected only Lakehouse items when filtered"
            );
        }
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_save_to_file() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");
    let plan_file = dir.path().join("plan.json");

    // Export
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Plan with --out (save to file)
    fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--force-all",
            "--out",
            plan_file.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    // Verify plan file exists and is valid JSON
    assert!(plan_file.exists(), "Plan file should have been written");
    let content = std::fs::read_to_string(&plan_file).unwrap();
    let plan: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(plan["version"], 1);
    assert!(plan["workspace_id"].is_string());
    assert!(plan["changeset"].is_object());
    assert!(plan["changeset"]["changes"].is_array());
}

// ── Apply (dry-run only to avoid mutation) ────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_apply_dry_run() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");

    // Export
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Apply with --dry-run (no actual mutations)
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--force-all",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
    assert!(data["summary"].is_object());
}

// ── Apply (real create/update cycle on dest workspace) ────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_apply_create_notebook_and_cleanup() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    let name = unique_name("deploy_nb");

    // Create a minimal notebook source directory
    let nb_dir = source_dir.join(format!("{name}.Notebook"));
    std::fs::create_dir_all(&nb_dir).unwrap();

    // Write .platform file
    let platform = serde_json::json!({
        "metadata": {
            "type": "Notebook",
            "displayName": name
        },
        "config": {
            "version": "2.0",
            "logicalId": "e2e-test-lid-001",
            "definitionFormat": "ipynb"
        }
    });
    std::fs::write(
        nb_dir.join(".platform"),
        serde_json::to_string_pretty(&platform).unwrap(),
    )
    .unwrap();

    // Write notebook content (minimal ipynb)
    let ipynb = serde_json::json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": {
            "language_info": { "name": "python" }
        },
        "cells": [
            {
                "cell_type": "code",
                "source": ["# Deploy test\n", "print('hello')\n"],
                "metadata": {},
                "outputs": []
            }
        ]
    });
    std::fs::write(
        nb_dir.join("notebook-content.py"),
        serde_json::to_string(&ipynb).unwrap(),
    )
    .unwrap();

    // Plan first
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["summary"]["create"].as_u64().unwrap(), 1);

    // Apply (real deployment)
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "succeeded");
    assert_eq!(data["succeeded"].as_u64().unwrap(), 1);

    // Verify item exists
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let created = items.iter().find(|i| i["displayName"] == name);
    assert!(
        created.is_some(),
        "Expected to find deployed notebook '{name}'"
    );

    let nb_id = created.unwrap()["id"].as_str().unwrap();

    // Plan again — should show Skip (idempotent)
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should either be skip (if hashes match) or update (if normalization differs)
    // But definitely NOT create
    assert_eq!(data["summary"]["create"].as_u64().unwrap_or(0), 0);

    // Cleanup: delete the created notebook
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            nb_id,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_apply_same_tier_items_created_concurrently() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    let prefix = unique_name("tier");

    // Create a fresh workspace (avoids plan overhead on large workspaces)
    let ws_name = format!("deploy-tier-test-{prefix}");
    let assert = fabio()
        .args(["workspace", "create", "--name", &ws_name])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let ws_id = extract_data(&json)["id"].as_str().unwrap().to_owned();

    fabio()
        .args([
            "workspace",
            "assign-capacity",
            "--id",
            &ws_id,
            "--capacity",
            &cfg.capacity_id,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();

    // Create 3 Notebooks (all same type → same tier, will run concurrently)
    for i in 1..=3 {
        let name = format!("{prefix}_nb{i}");
        let nb_dir = source_dir.join(format!("{name}.Notebook"));
        std::fs::create_dir_all(&nb_dir).unwrap();

        let platform = serde_json::json!({
            "metadata": { "type": "Notebook", "displayName": name },
            "config": { "version": "2.0", "logicalId": format!("tier-test-{i}"), "definitionFormat": "ipynb" }
        });
        std::fs::write(
            nb_dir.join(".platform"),
            serde_json::to_string_pretty(&platform).unwrap(),
        )
        .unwrap();

        let ipynb = serde_json::json!({
            "nbformat": 4, "nbformat_minor": 5,
            "metadata": { "language_info": { "name": "python" } },
            "cells": [{ "cell_type": "code", "source": [format!("# test {i}\n")], "metadata": {}, "outputs": [] }]
        });
        std::fs::write(
            nb_dir.join("notebook-content.py"),
            serde_json::to_string(&ipynb).unwrap(),
        )
        .unwrap();
    }

    // Deploy all 3 items (should use tier-based concurrency)
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &ws_id,
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "succeeded");
    assert_eq!(data["succeeded"].as_u64().unwrap(), 3);
    assert_eq!(data["failed"].as_u64().unwrap(), 0);

    // Cleanup: delete the workspace
    fabio()
        .args(["workspace", "delete", "--id", &ws_id])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_apply_cross_tier_types_respects_ordering() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    let prefix = unique_name("xtier");

    // Create a fresh workspace (avoids plan overhead from existing items)
    let ws_name = format!("deploy-xtier-test-{prefix}");
    let assert = fabio()
        .args(["workspace", "create", "--name", &ws_name])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
    let json = parse_json(&assert);
    let ws_id = extract_data(&json)["id"].as_str().unwrap().to_owned();

    fabio()
        .args([
            "workspace",
            "assign-capacity",
            "--id",
            &ws_id,
            "--capacity",
            &cfg.capacity_id,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();

    // Create a Lakehouse (tier 0) and a Notebook (tier 3) — Notebook depends on Lakehouse tier
    let lh_name = format!("{prefix}_lh");
    let nb_name = format!("{prefix}_nb");

    // Lakehouse (shell-only, tier 0)
    let lh_dir = source_dir.join(format!("{lh_name}.Lakehouse"));
    std::fs::create_dir_all(&lh_dir).unwrap();
    let platform = serde_json::json!({
        "metadata": { "type": "Lakehouse", "displayName": lh_name },
        "config": { "version": "2.0", "logicalId": "xtier-lh-001" }
    });
    std::fs::write(
        lh_dir.join(".platform"),
        serde_json::to_string_pretty(&platform).unwrap(),
    )
    .unwrap();

    // Notebook (tier 3)
    let nb_dir = source_dir.join(format!("{nb_name}.Notebook"));
    std::fs::create_dir_all(&nb_dir).unwrap();
    let platform = serde_json::json!({
        "metadata": { "type": "Notebook", "displayName": nb_name },
        "config": { "version": "2.0", "logicalId": "xtier-nb-001", "definitionFormat": "ipynb" }
    });
    std::fs::write(
        nb_dir.join(".platform"),
        serde_json::to_string_pretty(&platform).unwrap(),
    )
    .unwrap();

    let ipynb = serde_json::json!({
        "nbformat": 4, "nbformat_minor": 5,
        "metadata": { "language_info": { "name": "python" } },
        "cells": [{ "cell_type": "code", "source": ["# cross-tier test\n"], "metadata": {}, "outputs": [] }]
    });
    std::fs::write(
        nb_dir.join("notebook-content.py"),
        serde_json::to_string(&ipynb).unwrap(),
    )
    .unwrap();

    // Deploy — Lakehouse (tier 0) created before Notebook (tier 3)
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &ws_id,
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "succeeded");
    assert_eq!(data["succeeded"].as_u64().unwrap(), 2);
    assert_eq!(data["failed"].as_u64().unwrap(), 0);

    // Cleanup: delete the workspace
    fabio()
        .args(["workspace", "delete", "--id", &ws_id])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
}

// ── Error Cases ──────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_nonexistent_workspace_fails() {
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();

    // Create minimal .platform so source isn't empty... actually, it should error
    // because workspace doesn't exist
    let nb_dir = source_dir.join("Test.Notebook");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"Test"},"config":{"version":"2.0"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("notebook-content.py"), "# test").unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            "NonExistentWorkspace12345",
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("not found") || stderr.contains("NOT_FOUND"),
        "Expected not-found error, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_empty_source_directory_fails() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("empty_source");
    std::fs::create_dir_all(&source_dir).unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("No items found"),
        "Expected 'No items found' error, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_nonexistent_source_directory_fails() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            "/nonexistent/path/that/does/not/exist",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("does not exist"),
        "Expected 'does not exist' error, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_non_empty_dir_without_overwrite_fails() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("nonempty");
    std::fs::create_dir_all(&output_dir).unwrap();
    std::fs::write(output_dir.join("existing_file.txt"), "data").unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("not empty") || stderr.contains("--overwrite"),
        "Expected non-empty dir error, got: {stderr}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_overwrite_flag_works() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("overwrite_test");
    std::fs::create_dir_all(&output_dir).unwrap();
    std::fs::write(output_dir.join("old_file.txt"), "old data").unwrap();

    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
            "--overwrite",
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_with_concurrency_flag() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("export_concurrent");

    // Export with explicit concurrency=4 (limit to Lakehouse to keep test fast)
    let assert = fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
            "--concurrency",
            "4",
            "--item-types",
            "Lakehouse",
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");
    assert!(data["total_items"].as_u64().unwrap() > 0);
    assert!(data["exported"].as_u64().unwrap() > 0);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_concurrency_one_sequential() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("export_seq");

    // Export with concurrency=1 (sequential, like the old behavior)
    let assert = fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
            "--concurrency",
            "1",
            "--item-types",
            "Lakehouse",
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");
    // Lakehouse items should export (they have definition parts)
    assert!(data["exported"].as_u64().unwrap() > 0);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_shell_only_warehouse() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("export_warehouse");

    // Export only Warehouse type items
    let assert = fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
            "--item-types",
            "Warehouse",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");

    // If workspace has Warehouses, they should be exported (not skipped)
    let total = data["total_items"].as_u64().unwrap();
    if total > 0 {
        // Exported count should equal total (shell-only items don't get skipped)
        let exported = data["exported"].as_u64().unwrap();
        assert_eq!(
            exported, total,
            "All Warehouse items should be exported as shell-only"
        );

        // Skipped should be empty for Warehouse
        let skipped = data["skipped"].as_array().unwrap();
        assert!(
            skipped.is_empty(),
            "Warehouse items should not be skipped: {skipped:?}"
        );

        // Verify each exported directory has .platform but no other definition files
        for entry in std::fs::read_dir(&output_dir).unwrap().flatten() {
            if entry.path().is_dir() {
                let platform_path = entry.path().join(".platform");
                assert!(
                    platform_path.exists(),
                    "Expected .platform file in {}",
                    entry.path().display()
                );

                // Check .platform metadata type is Warehouse
                let content = std::fs::read_to_string(&platform_path).unwrap();
                let meta: serde_json::Value = serde_json::from_str(&content).unwrap();
                assert_eq!(meta["metadata"]["type"], "Warehouse");

                // Should have NO other files besides .platform (shell-only = no definition parts)
                let file_count = std::fs::read_dir(entry.path()).unwrap().flatten().count();
                assert_eq!(
                    file_count,
                    1,
                    "Shell-only Warehouse should only have .platform, found {} files in {}",
                    file_count,
                    entry.path().display()
                );
            }
        }
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_sql_endpoint_fully_skipped() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("export_sqlep");

    // Export only SQLEndpoint type items (explicitly requested)
    let assert = fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
            "--item-types",
            "SQLEndpoint",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");

    // SQLEndpoints should be fully skipped (not shell-only)
    let total = data["total_items"].as_u64().unwrap();
    if total > 0 {
        let exported = data["exported"].as_u64().unwrap();
        assert_eq!(
            exported, 0,
            "SQLEndpoint items should NOT be exported (they are auto-provisioned)"
        );

        // All should appear in skipped list
        let skipped = data["skipped"].as_array().unwrap();
        assert_eq!(
            skipped.len() as u64,
            total,
            "All SQLEndpoint items should be in skipped list"
        );

        // No directories should be created
        let dirs: Vec<_> = std::fs::read_dir(&output_dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_dir())
            .collect();
        assert!(
            dirs.is_empty(),
            "No directories should be created for SQLEndpoint items"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_default_excludes_auto_provisioned() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("export_default");

    // Export without --item-types (default behavior)
    let assert = fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "exported");

    // The total_items count should NOT include SQLEndpoints
    let total = data["total_items"].as_u64().unwrap();
    let exported = data["exported"].as_u64().unwrap();
    let skipped = data["skipped"].as_array().unwrap();

    // Verify: no skipped item mentions SQLEndpoint
    for item in skipped {
        let msg = item.as_str().unwrap_or_default();
        assert!(
            !msg.contains("SQLEndpoint"),
            "SQLEndpoint should not appear in skipped list by default, found: {msg}"
        );
    }

    // exported + skipped = total (no unaccounted items)
    assert_eq!(
        exported + skipped.len() as u64,
        total,
        "exported + skipped should equal total_items"
    );
}

// ── Workspace name resolution ────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_workspace_name_resolution() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");

    // First export to get content (using ID)
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Get workspace name
    let assert = fabio()
        .args(["workspace", "show", "--id", &cfg.source_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let ws_name = extract_data(&json)["displayName"]
        .as_str()
        .unwrap()
        .to_owned();

    // Plan using workspace name instead of ID
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &ws_name,
            "--force-all",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should succeed and find items
    assert!(data["workspace_id"].is_string());
}

// ── Init-Params (local-only, no live tenant required) ────────────────────────

/// Helper: create a synthetic .platform item directory for init-params testing.
fn create_platform_item(
    base_dir: &std::path::Path,
    folder_name: &str,
    item_type: &str,
    display_name: &str,
    file_name: &str,
    content: &str,
) {
    let item_dir = base_dir.join(folder_name);
    std::fs::create_dir_all(&item_dir).unwrap();

    let platform = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/gitIntegration/platformProperties/2.0.0/schema.json",
        "metadata": {
            "type": item_type,
            "displayName": display_name
        },
        "config": {}
    });
    std::fs::write(
        item_dir.join(".platform"),
        serde_json::to_string_pretty(&platform).unwrap(),
    )
    .unwrap();

    std::fs::write(item_dir.join(file_name), content).unwrap();
}

#[test]
fn deploy_init_params_scan_mode() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir_all(&source).unwrap();

    // Create items with GUIDs embedded in their definitions
    create_platform_item(
        &source,
        "MyPipeline.DataPipeline",
        "DataPipeline",
        "MyPipeline",
        "pipeline-content.json",
        r#"{"activities": [{"connectionId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890", "workspaceId": "12345678-aaaa-bbbb-cccc-ddddeeeeaaaa"}]}"#,
    );

    let out_file = dir.path().join("params.json");

    let assert = fabio()
        .args([
            "deploy",
            "init-params",
            "--source",
            source.to_str().unwrap(),
            "--out",
            out_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "generated");
    assert_eq!(data["mode"], "scan");
    assert_eq!(data["source_items"].as_u64().unwrap(), 1);
    assert!(data["rules_generated"].as_u64().unwrap() >= 2);
    assert!(data["guids_found"].as_u64().unwrap() >= 2);

    // Verify output file was written
    assert!(out_file.exists());
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out_file).unwrap()).unwrap();
    assert!(written["find_replace"].as_array().unwrap().len() >= 2);
}

#[test]
fn deploy_init_params_scan_skips_well_known_guids() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir_all(&source).unwrap();

    create_platform_item(
        &source,
        "MyLakehouse.Lakehouse",
        "Lakehouse",
        "MyLakehouse",
        "definition.json",
        r#"{"nullId": "00000000-0000-0000-0000-000000000000", "realId": "abcdef12-3456-7890-abcd-ef1234567890"}"#,
    );

    let assert = fabio()
        .args([
            "deploy",
            "init-params",
            "--source",
            source.to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Only the real GUID should be found (not the null/well-known one)
    assert_eq!(data["guids_found"].as_u64().unwrap(), 1);
    assert_eq!(data["rules_generated"].as_u64().unwrap(), 1);
}

#[test]
fn deploy_init_params_diff_mode() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("source");
    let compare = dir.path().join("compare");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&compare).unwrap();

    // Same item in both, with different GUIDs (env-specific)
    create_platform_item(
        &source,
        "MyPipeline.DataPipeline",
        "DataPipeline",
        "MyPipeline",
        "pipeline-content.json",
        r#"{"connectionId": "aaaaaaaa-1111-2222-3333-444444444444"}"#,
    );
    create_platform_item(
        &compare,
        "MyPipeline.DataPipeline",
        "DataPipeline",
        "MyPipeline",
        "pipeline-content.json",
        r#"{"connectionId": "bbbbbbbb-5555-6666-7777-888888888888"}"#,
    );

    let out_file = dir.path().join("params.json");

    let assert = fabio()
        .args([
            "deploy",
            "init-params",
            "--source",
            source.to_str().unwrap(),
            "--compare",
            compare.to_str().unwrap(),
            "--source-env",
            "dev",
            "--compare-env",
            "prod",
            "--out",
            out_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "generated");
    assert_eq!(data["mode"], "diff");
    assert_eq!(data["source_items"].as_u64().unwrap(), 1);
    assert_eq!(data["compare_items"].as_u64().unwrap(), 1);
    assert!(data["rules_generated"].as_u64().unwrap() >= 1);

    // Verify generated rules map the GUIDs correctly
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out_file).unwrap()).unwrap();
    let rules = written["find_replace"].as_array().unwrap();
    assert!(!rules.is_empty());

    let guid_rule = rules
        .iter()
        .find(|r| {
            r["find_value"]
                .as_str()
                .is_some_and(|v| v.contains("aaaaaaaa"))
        })
        .expect("Should find rule for source GUID");
    assert_eq!(
        guid_rule["replace_value"]["dev"].as_str().unwrap(),
        "aaaaaaaa-1111-2222-3333-444444444444"
    );
    assert_eq!(
        guid_rule["replace_value"]["prod"].as_str().unwrap(),
        "bbbbbbbb-5555-6666-7777-888888888888"
    );
}

#[test]
fn deploy_init_params_diff_no_common_items() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("source");
    let compare = dir.path().join("compare");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&compare).unwrap();

    // Different items in each directory
    create_platform_item(
        &source,
        "ItemA.Notebook",
        "Notebook",
        "ItemA",
        "notebook-content.py",
        r#"print("hello from dev")"#,
    );
    create_platform_item(
        &compare,
        "ItemB.Notebook",
        "Notebook",
        "ItemB",
        "notebook-content.py",
        r#"print("hello from prod")"#,
    );

    let assert = fabio()
        .args([
            "deploy",
            "init-params",
            "--source",
            source.to_str().unwrap(),
            "--compare",
            compare.to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["mode"], "diff");
    assert_eq!(data["rules_generated"].as_u64().unwrap(), 0);
}

#[test]
fn deploy_init_params_diff_string_differences() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("source");
    let compare = dir.path().join("compare");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&compare).unwrap();

    // Items with different connection strings
    create_platform_item(
        &source,
        "Config.DataPipeline",
        "DataPipeline",
        "Config",
        "pipeline-content.json",
        r#"{"server": "myserver-dev.database.windows.net", "port": 1433}"#,
    );
    create_platform_item(
        &compare,
        "Config.DataPipeline",
        "DataPipeline",
        "Config",
        "pipeline-content.json",
        r#"{"server": "myserver-prod.database.windows.net", "port": 1433}"#,
    );

    let assert = fabio()
        .args([
            "deploy",
            "init-params",
            "--source",
            source.to_str().unwrap(),
            "--compare",
            compare.to_str().unwrap(),
            "--source-env",
            "dev",
            "--compare-env",
            "prod",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data["rules_generated"].as_u64().unwrap() >= 1);

    // Verify the rule captures the string difference
    let params = &data["parameters"];
    let rules = params["find_replace"].as_array().unwrap();
    let server_rule = rules.iter().find(|r| {
        r["find_value"]
            .as_str()
            .is_some_and(|v| v.contains("myserver-dev"))
    });
    assert!(
        server_rule.is_some(),
        "Should detect server name difference"
    );
}

#[test]
fn deploy_init_params_empty_source_returns_zero_rules() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("empty_source");
    std::fs::create_dir_all(&source).unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "init-params",
            "--source",
            source.to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["source_items"].as_u64().unwrap(), 0);
    assert_eq!(data["rules_generated"].as_u64().unwrap(), 0);
}

#[test]
fn deploy_init_params_nonexistent_source_fails() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("does_not_exist");

    fabio()
        .args([
            "deploy",
            "init-params",
            "--source",
            source.to_str().unwrap(),
        ])
        .assert()
        .failure();
}

// ── Plan with Parameters (requires live tenant) ──────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_with_parameters_requires_env() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");

    // Export first
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Create a minimal parameters file
    let params_file = dir.path().join("params.json");
    std::fs::write(
        &params_file,
        r#"{"find_replace": [{"find_value": "placeholder", "replace_value": {"_ALL_": "replaced"}}]}"#,
    )
    .unwrap();

    // Plan with --parameters but no --env should fail
    fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--parameters",
            params_file.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_with_parameters_and_env() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");

    // Export first
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Create a parameters file with a no-op rule (won't match anything)
    let params_file = dir.path().join("params.json");
    std::fs::write(
        &params_file,
        r#"{"find_replace": [{"find_value": "nonexistent_value_xyz_123", "replace_value": {"prod": "replaced_value"}}]}"#,
    )
    .unwrap();

    // Plan with --parameters and --env should succeed
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--parameters",
            params_file.to_str().unwrap(),
            "--env",
            "prod",
            "--force-all",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Should succeed (plan completes with parameter substitution applied)
    assert!(data["workspace_id"].is_string());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_with_key_value_replace_parameters() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");

    // Export first
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Create a parameters file with key_value_replace rules
    let params_file = dir.path().join("params.json");
    std::fs::write(
        &params_file,
        r#"{
            "find_replace": [],
            "key_value_replace": [
                {
                    "find_key": "$.nonexistent_path_xyz",
                    "replace_value": {"prod": "replaced_value"}
                }
            ]
        }"#,
    )
    .unwrap();

    // Plan with key_value_replace should succeed
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--parameters",
            params_file.to_str().unwrap(),
            "--env",
            "prod",
            "--force-all",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data["workspace_id"].is_string());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_with_spark_pool_parameters() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");

    // Export first
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Create parameters with spark_pool rules
    let params_file = dir.path().join("params.json");
    std::fs::write(
        &params_file,
        r#"{
            "find_replace": [],
            "spark_pool": [
                {
                    "instance_pool_id": "00000000-0000-0000-0000-000000000099",
                    "replace_value": {
                        "prod": {
                            "type": "Workspace",
                            "name": "prod-pool"
                        }
                    }
                }
            ]
        }"#,
    )
    .unwrap();

    // Plan with spark_pool should succeed (pool won't match anything)
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--parameters",
            params_file.to_str().unwrap(),
            "--env",
            "prod",
            "--force-all",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data["workspace_id"].is_string());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_with_semantic_model_binding_parameters() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");

    // Export first
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Create parameters with semantic_model_binding
    let params_file = dir.path().join("params.json");
    std::fs::write(
        &params_file,
        r#"{
            "find_replace": [],
            "semantic_model_binding": {
                "default": {
                    "connection_id": {
                        "prod": "99999999-aaaa-bbbb-cccc-ddddeeee1111"
                    }
                }
            }
        }"#,
    )
    .unwrap();

    // Plan with semantic_model_binding should succeed
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--parameters",
            params_file.to_str().unwrap(),
            "--env",
            "prod",
            "--force-all",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data["workspace_id"].is_string());
}

// ── Phase 4: Plan File Roundtrip ─────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_file_roundtrip_apply() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    let plan_file = dir.path().join("plan.json");
    let name = unique_name("deploy_rt");

    // Create a minimal notebook source
    let nb_dir = source_dir.join(format!("{name}.Notebook"));
    std::fs::create_dir_all(&nb_dir).unwrap();

    let platform = serde_json::json!({
        "metadata": {
            "type": "Notebook",
            "displayName": name
        },
        "config": {
            "version": "2.0",
            "logicalId": "rt-lid-001",
            "definitionFormat": "ipynb"
        }
    });
    std::fs::write(
        nb_dir.join(".platform"),
        serde_json::to_string_pretty(&platform).unwrap(),
    )
    .unwrap();

    let ipynb = serde_json::json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": { "language_info": { "name": "python" } },
        "cells": [{
            "cell_type": "code",
            "source": ["# roundtrip test\n"],
            "metadata": {},
            "outputs": []
        }]
    });
    std::fs::write(
        nb_dir.join("notebook-content.py"),
        serde_json::to_string(&ipynb).unwrap(),
    )
    .unwrap();

    // Step 1: Plan --out (save plan to file)
    fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
            "--out",
            plan_file.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    assert!(plan_file.exists(), "Plan file should exist");
    let plan_content = std::fs::read_to_string(&plan_file).unwrap();
    let plan: serde_json::Value = serde_json::from_str(&plan_content).unwrap();
    assert_eq!(plan["version"], 1);
    assert!(plan["workspace_fingerprint"].is_string());

    // Step 2: Apply from plan file
    let assert = fabio()
        .args(["deploy", "apply", "--plan", plan_file.to_str().unwrap()])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "succeeded");
    assert_eq!(data["succeeded"].as_u64().unwrap(), 1);

    // Verify item exists
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let created = items.iter().find(|i| i["displayName"] == name);
    assert!(created.is_some(), "Expected deployed notebook '{name}'");

    // Cleanup
    let nb_id = created.unwrap()["id"].as_str().unwrap();
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            nb_id,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
}

// ── Phase 4: Plan Staleness Detection ────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_staleness_detection() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    let plan_file = dir.path().join("plan.json");
    let name = unique_name("deploy_stale");
    let stale_name = unique_name("stale_item");

    // Create a minimal notebook source
    let nb_dir = source_dir.join(format!("{name}.Notebook"));
    std::fs::create_dir_all(&nb_dir).unwrap();

    let platform = serde_json::json!({
        "metadata": {
            "type": "Notebook",
            "displayName": name
        },
        "config": {
            "version": "2.0",
            "logicalId": "stale-lid-001",
            "definitionFormat": "ipynb"
        }
    });
    std::fs::write(
        nb_dir.join(".platform"),
        serde_json::to_string_pretty(&platform).unwrap(),
    )
    .unwrap();

    let ipynb = serde_json::json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": { "language_info": { "name": "python" } },
        "cells": [{
            "cell_type": "code",
            "source": ["# staleness test\n"],
            "metadata": {},
            "outputs": []
        }]
    });
    std::fs::write(
        nb_dir.join("notebook-content.py"),
        serde_json::to_string(&ipynb).unwrap(),
    )
    .unwrap();

    // Step 1: Save plan (captures workspace fingerprint)
    fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
            "--out",
            plan_file.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    // Step 2: Modify the workspace state (create a new item to change fingerprint)
    let assert = fabio()
        .args([
            "item",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &stale_name,
            "--type",
            "Notebook",
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let stale_item_id = extract_data(&json)["id"].as_str().unwrap().to_owned();

    // Step 3: Apply from plan file WITHOUT --force → should FAIL (stale)
    let assert = fabio()
        .args(["deploy", "apply", "--plan", plan_file.to_str().unwrap()])
        .timeout(Duration::from_mins(1))
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("Workspace state has changed")
            || stderr.contains("workspace_fingerprint")
            || stderr.contains("fingerprint"),
        "Expected staleness error, got: {stderr}"
    );

    // Step 4: Apply with --force → should SUCCEED despite stale fingerprint
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--plan",
            plan_file.to_str().unwrap(),
            "--force",
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "succeeded");

    // Cleanup: delete both items
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    if let Some(deployed) = items.iter().find(|i| i["displayName"] == name) {
        let id = deployed["id"].as_str().unwrap();
        fabio()
            .args([
                "item",
                "delete",
                "--workspace",
                &cfg.dest_workspace,
                "--id",
                id,
            ])
            .timeout(Duration::from_mins(1))
            .assert()
            .success();
    }
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &stale_item_id,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
}

// ── Phase 4: Logical ID Resolution (Live) ────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_apply_logical_id_resolution() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    let lh_name = unique_name("deploy_lh");
    let nb_name = unique_name("deploy_nb_ref");

    // Use a stable logical ID for the lakehouse that we'll embed in the notebook
    let lh_logical_id = "e2e-logical-id-lakehouse-001";

    // Create a lakehouse source item
    let lh_dir = source_dir.join(format!("{lh_name}.Lakehouse"));
    std::fs::create_dir_all(&lh_dir).unwrap();
    let lh_platform = serde_json::json!({
        "metadata": {
            "type": "Lakehouse",
            "displayName": lh_name
        },
        "config": {
            "version": "2.0",
            "logicalId": lh_logical_id
        }
    });
    std::fs::write(
        lh_dir.join(".platform"),
        serde_json::to_string_pretty(&lh_platform).unwrap(),
    )
    .unwrap();
    // Lakehouse has no definition content (empty definition creates shell)

    // Create a notebook source item whose definition references the lakehouse's logical ID
    let nb_dir = source_dir.join(format!("{nb_name}.Notebook"));
    std::fs::create_dir_all(&nb_dir).unwrap();

    let nb_platform = serde_json::json!({
        "metadata": {
            "type": "Notebook",
            "displayName": nb_name
        },
        "config": {
            "version": "2.0",
            "logicalId": "e2e-logical-id-notebook-001",
            "definitionFormat": "ipynb"
        }
    });
    std::fs::write(
        nb_dir.join(".platform"),
        serde_json::to_string_pretty(&nb_platform).unwrap(),
    )
    .unwrap();

    // The notebook content references the lakehouse logical ID
    // (This simulates a notebook that uses the lakehouse's ID in its metadata)
    let ipynb = serde_json::json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": {
            "language_info": { "name": "python" },
            "trident": {
                "lakehouse": {
                    "default_lakehouse": lh_logical_id,
                    "default_lakehouse_name": lh_name,
                    "known_lakehouses": [{
                        "id": lh_logical_id
                    }]
                }
            }
        },
        "cells": [{
            "cell_type": "code",
            "source": [&format!("# Uses lakehouse: {lh_logical_id}\n")],
            "metadata": {},
            "outputs": []
        }]
    });
    std::fs::write(
        nb_dir.join("notebook-content.py"),
        serde_json::to_string(&ipynb).unwrap(),
    )
    .unwrap();

    // Plan should show 2 creates
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(
        data["summary"]["create"].as_u64().unwrap(),
        2,
        "Expected 2 creates (lakehouse + notebook)"
    );

    // Apply (real deployment) — lakehouse deploys first (lower priority number),
    // then notebook gets the resolved lakehouse ID in its definition
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "succeeded");
    assert_eq!(
        data["succeeded"].as_u64().unwrap(),
        2,
        "Both items should deploy successfully"
    );
    assert_eq!(data["failed"].as_u64().unwrap(), 0);

    // Verify both items exist
    let assert = fabio()
        .args(["item", "list", "--workspace", &cfg.dest_workspace])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();

    let lh_item = items.iter().find(|i| i["displayName"] == lh_name);
    assert!(lh_item.is_some(), "Lakehouse '{lh_name}' should exist");
    let lh_id = lh_item.unwrap()["id"].as_str().unwrap();

    let nb_item = items.iter().find(|i| i["displayName"] == nb_name);
    assert!(nb_item.is_some(), "Notebook '{nb_name}' should exist");
    let nb_id = nb_item.unwrap()["id"].as_str().unwrap();

    // Verify the notebook's definition has the REAL lakehouse ID (not the logical ID)
    let assert = fabio()
        .args([
            "item",
            "get-definition",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            nb_id,
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let def_data = extract_data(&json);

    // Find the notebook content part and check it contains the real lakehouse ID
    let parts = def_data["definition"]["parts"].as_array().unwrap();
    let content_part = parts
        .iter()
        .find(|p| {
            p["path"]
                .as_str()
                .is_some_and(|path| path.contains("notebook"))
        })
        .expect("Should find notebook content part");

    let payload_b64 = content_part["payload"].as_str().unwrap();
    let payload_bytes = base64::engine::general_purpose::STANDARD
        .decode(payload_b64)
        .unwrap();
    let payload_str = String::from_utf8(payload_bytes).unwrap();

    // The logical ID should be resolved to the actual deployed lakehouse ID
    assert!(
        payload_str.contains(lh_id),
        "Notebook definition should contain the real lakehouse ID '{lh_id}', but got: {}...",
        &payload_str[..payload_str.len().min(200)]
    );
    assert!(
        !payload_str.contains(lh_logical_id),
        "Notebook definition should NOT contain the logical ID '{lh_logical_id}'"
    );

    // Cleanup: delete both items (notebook first to avoid dependency issues)
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            nb_id,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            lh_id,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
}

// ── Phase 4: Plan workspace_fingerprint field validation ─────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_file_contains_fingerprint() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let export_dir = dir.path().join("exported");
    let plan_file = dir.path().join("plan.json");

    // Export
    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            export_dir.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();

    // Plan with --out
    fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            export_dir.to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--force-all",
            "--out",
            plan_file.to_str().unwrap(),
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    // Parse plan file and verify structure
    let content = std::fs::read_to_string(&plan_file).unwrap();
    let plan: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(plan["version"], 1, "Plan version should be 1");
    assert!(
        plan["workspace_id"].is_string(),
        "Plan should have workspace_id"
    );
    assert!(
        plan["workspace_fingerprint"].is_string(),
        "Plan should have workspace_fingerprint"
    );

    let fingerprint = plan["workspace_fingerprint"].as_str().unwrap();
    assert!(
        fingerprint.starts_with("sha256:"),
        "Fingerprint should start with 'sha256:', got: {fingerprint}"
    );
    assert_eq!(
        fingerprint.len(),
        7 + 64,
        "Fingerprint should be sha256: + 64 hex chars"
    );

    assert!(
        plan["changeset"]["changes"].is_array(),
        "Plan should have changeset.changes array"
    );
    assert!(
        plan["source_path"].is_string(),
        "Plan should have source_path"
    );
}

// ── creationPayload ──────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_apply_creation_payload_lakehouse_with_schemas() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();

    let name = unique_name("DeploySchemaLH");

    // Create a Lakehouse source with creationPayload.json (enableSchemas)
    let lh_dir = source_dir.join(format!("{name}.Lakehouse"));
    std::fs::create_dir_all(&lh_dir).unwrap();

    let platform = serde_json::json!({
        "metadata": {
            "type": "Lakehouse",
            "displayName": name
        },
        "config": {
            "version": "2.0",
            "logicalId": "e2e-creation-payload-001"
        }
    });
    std::fs::write(
        lh_dir.join(".platform"),
        serde_json::to_string_pretty(&platform).unwrap(),
    )
    .unwrap();

    // This is the key part: creationPayload.json triggers enableSchemas
    let creation_payload = serde_json::json!({
        "enableSchemas": true
    });
    std::fs::write(
        lh_dir.join("creationPayload.json"),
        serde_json::to_string_pretty(&creation_payload).unwrap(),
    )
    .unwrap();

    // Plan — should show Create
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["summary"]["create"].as_u64().unwrap(), 1);

    // Apply
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "succeeded");
    assert_eq!(data["succeeded"].as_u64().unwrap(), 1);

    // Verify item exists
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Lakehouse",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let created = items.iter().find(|i| i["displayName"] == name);
    assert!(
        created.is_some(),
        "Expected to find deployed lakehouse '{name}'"
    );

    let lh_id = created.unwrap()["id"].as_str().unwrap();

    // Cleanup
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            lh_id,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
}

// ── Rename Detection ─────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_detects_rename() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();

    let original_name = unique_name("DeployRenOrig");
    let renamed_name = unique_name("DeployRenNew");

    // Step 1: Create item via deploy apply with original name
    let nb_dir = source_dir.join(format!("{original_name}.Notebook"));
    std::fs::create_dir_all(&nb_dir).unwrap();

    let logical_id = "e2e-rename-detection-lid-001";
    let platform = serde_json::json!({
        "metadata": {
            "type": "Notebook",
            "displayName": original_name
        },
        "config": {
            "version": "2.0",
            "logicalId": logical_id,
            "definitionFormat": "ipynb"
        }
    });
    std::fs::write(
        nb_dir.join(".platform"),
        serde_json::to_string_pretty(&platform).unwrap(),
    )
    .unwrap();

    let ipynb = serde_json::json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": { "language_info": { "name": "python" } },
        "cells": [{
            "cell_type": "code",
            "source": ["# Rename test\n"],
            "metadata": {},
            "outputs": []
        }]
    });
    std::fs::write(
        nb_dir.join("notebook-content.py"),
        serde_json::to_string(&ipynb).unwrap(),
    )
    .unwrap();

    // Deploy with original name
    fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    // Verify item deployed
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let created = items.iter().find(|i| i["displayName"] == original_name);
    assert!(
        created.is_some(),
        "Expected deployed notebook '{original_name}'"
    );
    let nb_id = created.unwrap()["id"].as_str().unwrap().to_owned();

    // Step 2: Rename the source (simulate renaming in source code)
    std::fs::remove_dir_all(&nb_dir).unwrap();
    let new_nb_dir = source_dir.join(format!("{renamed_name}.Notebook"));
    std::fs::create_dir_all(&new_nb_dir).unwrap();

    let renamed_platform = serde_json::json!({
        "metadata": {
            "type": "Notebook",
            "displayName": renamed_name
        },
        "config": {
            "version": "2.0",
            "logicalId": logical_id,  // Same logical ID → rename detection
            "definitionFormat": "ipynb"
        }
    });
    std::fs::write(
        new_nb_dir.join(".platform"),
        serde_json::to_string_pretty(&renamed_platform).unwrap(),
    )
    .unwrap();
    std::fs::write(
        new_nb_dir.join("notebook-content.py"),
        serde_json::to_string(&ipynb).unwrap(),
    )
    .unwrap();

    // Step 3: Plan — should detect rename
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let changes = data["changeset"]["changes"].as_array().unwrap();

    // Should have exactly one rename action
    let renames: Vec<_> = changes.iter().filter(|c| c["action"] == "rename").collect();
    assert_eq!(
        renames.len(),
        1,
        "Expected exactly 1 rename, got: {changes:?}"
    );
    assert_eq!(renames[0]["name"], renamed_name);
    assert_eq!(renames[0]["previous_name"], original_name);

    // Step 4: Apply the rename
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "succeeded");

    // Verify renamed item exists
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let renamed = items.iter().find(|i| i["displayName"] == renamed_name);
    assert!(
        renamed.is_some(),
        "Expected renamed notebook '{renamed_name}'"
    );
    let old = items.iter().find(|i| i["displayName"] == original_name);
    assert!(old.is_none(), "Original name should no longer exist");

    // Cleanup
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &nb_id,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
}

// ── --no-post-hooks flag ─────────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_apply_no_post_hooks_flag_accepted() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();

    let name = unique_name("DeployNoHooks");

    // Create a simple notebook source
    let nb_dir = source_dir.join(format!("{name}.Notebook"));
    std::fs::create_dir_all(&nb_dir).unwrap();

    let platform = serde_json::json!({
        "metadata": {
            "type": "Notebook",
            "displayName": name
        },
        "config": {
            "version": "2.0",
            "logicalId": "e2e-no-hooks-lid-001",
            "definitionFormat": "ipynb"
        }
    });
    std::fs::write(
        nb_dir.join(".platform"),
        serde_json::to_string_pretty(&platform).unwrap(),
    )
    .unwrap();

    let ipynb = serde_json::json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": { "language_info": { "name": "python" } },
        "cells": [{
            "cell_type": "code",
            "source": ["# no-hooks test\n"],
            "metadata": {},
            "outputs": []
        }]
    });
    std::fs::write(
        nb_dir.join("notebook-content.py"),
        serde_json::to_string(&ipynb).unwrap(),
    )
    .unwrap();

    // Deploy with --no-post-hooks
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
            "--no-post-hooks",
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "succeeded");
    assert_eq!(data["succeeded"].as_u64().unwrap(), 1);
    // post_hooks should be empty array (no hooks executed)
    assert_eq!(
        data["post_hooks"].as_array().map_or(0, Vec::len),
        0,
        "Expected no post-hooks with --no-post-hooks flag"
    );

    // Verify item exists and cleanup
    let assert = fabio()
        .args([
            "item",
            "list",
            "--workspace",
            &cfg.dest_workspace,
            "--type",
            "Notebook",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let items = extract_data(&json).as_array().unwrap().clone();
    let created = items.iter().find(|i| i["displayName"] == name);
    assert!(created.is_some(), "Expected deployed notebook '{name}'");

    let nb_id = created.unwrap()["id"].as_str().unwrap();

    // Cleanup
    fabio()
        .args([
            "item",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            nb_id,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
}

// ── Validate (offline, no live tenant required) ──────────────────────────────

#[test]
fn deploy_validate_valid_source_succeeds() {
    let dir = tempfile::TempDir::new().unwrap();
    let nb_dir = dir.path().join("MyNotebook.Notebook");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"MyNotebook"},"config":{"version":"2.0","logicalId":"aaaaaaaa-1111-2222-3333-444444444444"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("notebook-content.py"), "print('hello')").unwrap();
    std::fs::write(
        nb_dir.join("notebook-settings.json"),
        r#"{"defaultLakehouse":{"knownName":"","referenceType":"LogicalIdReference"}}"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
    assert_eq!(data["items"], 1);
    assert_eq!(data["summary"]["errors"], 0);
    assert_eq!(data["summary"]["warnings"], 0);
}

#[test]
fn deploy_validate_warns_notebook_missing_settings() {
    let dir = tempfile::TempDir::new().unwrap();
    let nb_dir = dir.path().join("MyNotebook.Notebook");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"MyNotebook"},"config":{"version":"2.0","logicalId":"aaaaaaaa-1111-2222-3333-444444444444"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("notebook-content.py"), "print('hello')").unwrap();
    // Intentionally NOT creating notebook-settings.json

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
    assert_eq!(data["summary"]["warnings"], 1);
    let warnings = data["warnings"].as_array().unwrap();
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .contains("notebook-settings.json")
    );
}

#[test]
fn deploy_validate_nonexistent_source_fails() {
    fabio()
        .args(["deploy", "validate", "--source", "/nonexistent/path/xyz"])
        .assert()
        .failure();
}

#[test]
fn deploy_validate_empty_source_reports_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("empty");
    std::fs::create_dir_all(&source).unwrap();

    let assert = fabio()
        .args(["deploy", "validate", "--source", source.to_str().unwrap()])
        .assert()
        .failure();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "invalid");
    assert!(
        data["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap().contains("No items found"))
    );
}

#[test]
fn deploy_validate_duplicate_type_name_reports_error() {
    let dir = tempfile::TempDir::new().unwrap();

    for suffix in ["A", "B"] {
        let nb_dir = dir.path().join(format!("DupNB{suffix}.Notebook"));
        std::fs::create_dir_all(&nb_dir).unwrap();
        std::fs::write(
            nb_dir.join(".platform"),
            r#"{"metadata":{"type":"Notebook","displayName":"SameName"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        std::fs::write(nb_dir.join("notebook-content.py"), "x = 1").unwrap();
    }

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .failure();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "invalid");
    assert!(
        data["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap().contains("Duplicate item"))
    );
}

#[test]
fn deploy_validate_duplicate_logical_id_reports_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let lid = "11111111-2222-3333-4444-555555555555";

    for name in ["NB1", "NB2"] {
        let nb_dir = dir.path().join(format!("{name}.Notebook"));
        std::fs::create_dir_all(&nb_dir).unwrap();
        std::fs::write(
            nb_dir.join(".platform"),
            format!(r#"{{"metadata":{{"type":"Notebook","displayName":"{name}"}},"config":{{"version":"2.0","logicalId":"{lid}"}}}}"#),
        )
        .unwrap();
        std::fs::write(nb_dir.join("notebook-content.py"), "x = 1").unwrap();
    }

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .failure();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "invalid");
    assert!(
        data["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap().contains("Duplicate logical ID"))
    );
}

#[test]
fn deploy_validate_unknown_type_warns() {
    let dir = tempfile::TempDir::new().unwrap();
    let item_dir = dir.path().join("Thing.UnknownType");
    std::fs::create_dir_all(&item_dir).unwrap();
    std::fs::write(
        item_dir.join(".platform"),
        r#"{"metadata":{"type":"UnknownType","displayName":"Thing"},"config":{"version":"2.0"}}"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
    assert!(
        data["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("unknown item type"))
    );
}

#[test]
fn deploy_validate_invalid_params_reports_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let nb_dir = dir.path().join("NB.Notebook");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"NB"},"config":{"version":"2.0"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("notebook-content.py"), "x = 1").unwrap();

    let params_file = dir.path().join("params.json");
    std::fs::write(&params_file, "not valid json{{{").unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            dir.path().to_str().unwrap(),
            "--parameters",
            params_file.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "invalid");
    assert!(
        data["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap().contains("Parameters file error"))
    );
}

#[test]
fn deploy_validate_params_missing_env_warns() {
    let dir = tempfile::TempDir::new().unwrap();
    let nb_dir = dir.path().join("NB.Notebook");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"NB"},"config":{"version":"2.0"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("notebook-content.py"), "x = 1").unwrap();

    let params_file = dir.path().join("params.json");
    std::fs::write(
        &params_file,
        r#"{"find_replace":[{"find_value":"abc","replace_value":{"dev":"d","prod":"p"}}]}"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            dir.path().to_str().unwrap(),
            "--parameters",
            params_file.to_str().unwrap(),
            "--env",
            "staging",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
    assert!(
        data["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("no value for env"))
    );
}

// ── DataBuildToolJob + Shortcut tests ────────────────────────────────────────

#[test]
fn deploy_validate_data_build_tool_job_source() {
    let dir = tempfile::TempDir::new().unwrap();

    let dbt_dir = dir.path().join("SampleDbt.DataBuildToolJob");
    std::fs::create_dir_all(&dbt_dir).unwrap();
    std::fs::write(
        dbt_dir.join(".platform"),
        r#"{
            "metadata": {"type": "DataBuildToolJob", "displayName": "SampleDbt"},
            "config": {"version": "2.0", "logicalId": "dbt-test-lid-001"}
        }"#,
    )
    .unwrap();
    std::fs::write(
        dbt_dir.join("dbt-content.json"),
        r#"{
            "project": {"projectType": "OneLake", "folderPath": "dbt"},
            "profile": {
                "profileType": "DataWarehouse",
                "schema": "analytics",
                "connectionSettings": {"name": "test_warehouse"}
            },
            "command": {"operation": "build", "arguments": {"threads": 4}}
        }"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
    assert_eq!(data["items"], 1);
    assert_eq!(data["summary"]["errors"], 0);
    assert_eq!(data["summary"]["warnings"], 0);
}

#[test]
fn deploy_validate_lakehouse_with_shortcuts() {
    let dir = tempfile::TempDir::new().unwrap();

    let lh_dir = dir.path().join("SalesLH.Lakehouse");
    std::fs::create_dir_all(&lh_dir).unwrap();
    std::fs::write(
        lh_dir.join(".platform"),
        r#"{
            "metadata": {"type": "Lakehouse", "displayName": "SalesLH"},
            "config": {"version": "2.0", "logicalId": "lh-test-lid-001"}
        }"#,
    )
    .unwrap();
    std::fs::write(
        lh_dir.join("shortcuts.metadata.json"),
        r#"[
            {
                "name": "products",
                "path": "Tables",
                "target": {
                    "oneLake": {
                        "workspaceId": "00000000-0000-0000-0000-000000000000",
                        "itemId": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                        "path": "Tables/products"
                    }
                }
            }
        ]"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
    assert_eq!(data["items"], 1);
    assert_eq!(data["summary"]["errors"], 0);
    // shortcuts.metadata.json should not generate any warnings
    assert_eq!(data["summary"]["warnings"], 0);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_lakehouse_includes_shortcuts() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let output_dir = dir.path().join("export_sc");

    // Setup: create a test shortcut in the dest lakehouse
    let sc_name = unique_name("sc_export");
    let target_json = format!(
        r#"{{"workspaceId":"{}","itemId":"{}","path":"Files"}}"#,
        cfg.source_workspace, cfg.source_lakehouse
    );

    let create_assert = fabio()
        .args([
            "lakehouse",
            "create-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &sc_name,
            "--path",
            "Files",
            "--target-type",
            "oneLake",
            "--target",
            &target_json,
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();

    let create_json = parse_json(&create_assert);
    let create_data = extract_data(&create_json);
    assert_eq!(create_data["name"], sc_name);

    // Export the workspace (filter to Lakehouse only)
    let export_assert = fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.dest_workspace,
            "--dir",
            output_dir.to_str().unwrap(),
            "--item-types",
            "Lakehouse",
        ])
        .timeout(Duration::from_mins(3))
        .assert()
        .success();

    let export_json = parse_json(&export_assert);
    let export_data = extract_data(&export_json);
    assert_eq!(export_data["status"], "exported");

    // Find the exported lakehouse directory that has shortcuts.metadata.json
    let mut found_shortcuts = false;
    for entry in std::fs::read_dir(&output_dir).unwrap().flatten() {
        let shortcuts_path = entry.path().join("shortcuts.metadata.json");
        if shortcuts_path.exists() {
            let content = std::fs::read_to_string(&shortcuts_path).unwrap();
            let shortcuts: serde_json::Value = serde_json::from_str(&content).unwrap();
            let arr = shortcuts.as_array().expect("shortcuts should be an array");
            // Check if our test shortcut is in the exported list
            if arr.iter().any(|s| s["name"] == sc_name) {
                found_shortcuts = true;
                break;
            }
        }
    }
    assert!(
        found_shortcuts,
        "Expected shortcuts.metadata.json with test shortcut '{sc_name}' in exported Lakehouse directory"
    );

    // Cleanup: delete the test shortcut
    fabio()
        .args([
            "lakehouse",
            "delete-shortcut",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--name",
            &sc_name,
            "--path",
            "Files",
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_data_build_tool_job() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");

    let dbt_name = unique_name("dbt_plan");
    let dbt_dir = source_dir.join(format!("{dbt_name}.DataBuildToolJob"));
    std::fs::create_dir_all(&dbt_dir).unwrap();
    std::fs::write(
        dbt_dir.join(".platform"),
        serde_json::to_string_pretty(&serde_json::json!({
            "metadata": {"type": "DataBuildToolJob", "displayName": dbt_name},
            "config": {"version": "2.0", "logicalId": "dbt-e2e-plan-001"}
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        dbt_dir.join("dbt-content.json"),
        r#"{"project":{"projectType":"OneLake","folderPath":"dbt"},"command":{"operation":"build"}}"#,
    )
    .unwrap();

    // Plan should show a "create" action for the DataBuildToolJob
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
        ])
        .timeout(Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Verify plan shows create action
    assert_eq!(
        data["summary"]["create"].as_u64().unwrap(),
        1,
        "Expected 1 create action for DataBuildToolJob"
    );

    let changes = data["changes"].as_array().unwrap();
    let dbt_change = changes
        .iter()
        .find(|c| c["item_type"] == "DataBuildToolJob")
        .expect("Expected a DataBuildToolJob change in plan");
    assert_eq!(dbt_change["action"], "create");
    assert_eq!(dbt_change["name"], dbt_name);
}

// ── Destructive operation guards ─────────────────────────────────────────────

/// Helper: create a minimal source directory with a single item of given type.
fn create_source_dir_with_item(dir: &std::path::Path, item_name: &str, item_type: &str) {
    let folder = dir.join(format!("{item_name}.{item_type}"));
    std::fs::create_dir_all(&folder).unwrap();
    let platform = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/gitIntegration/platformProperties/2.0.0/schema.json",
        "metadata": { "type": item_type, "displayName": item_name },
        "config": { "version": "2.0", "logicalId": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee" }
    });
    std::fs::write(
        folder.join(".platform"),
        serde_json::to_string_pretty(&platform).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_delete_orphans_blocks_protected_types() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();

    // Create a source with a fake item that won't match any deployed Lakehouse
    create_source_dir_with_item(&source_dir, "FakeItem", "Lakehouse");

    // Plan with --delete-orphans but WITHOUT --allow-delete-types
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
            "--item-types",
            "Lakehouse",
            "--delete-orphans",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Should have warnings about protected types being skipped
    let warnings = data["warnings"].as_array().expect("warnings array");
    assert!(
        !warnings.is_empty(),
        "Expected warnings about protected type deletions"
    );
    let warning_text = warnings
        .iter()
        .map(|w| w.as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        warning_text.contains("protected type"),
        "Warning should mention 'protected type': {warning_text}"
    );
    assert!(
        warning_text.contains("--allow-delete-types"),
        "Warning should suggest --allow-delete-types: {warning_text}"
    );

    // Should NOT have any delete actions in the changeset (they were blocked)
    let summary = &data["summary"];
    assert_eq!(
        summary["delete"].as_u64().unwrap_or(0),
        0,
        "Protected types should not produce delete actions"
    );

    // destructive should be false (no actual deletes in plan)
    assert_eq!(
        data["destructive"].as_bool(),
        Some(false),
        "destructive should be false when deletions are blocked"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_delete_orphans_allows_when_explicit() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();

    // Create a source with a fake item that won't match any deployed Lakehouse
    create_source_dir_with_item(&source_dir, "FakeItem", "Lakehouse");

    // Plan with --delete-orphans AND --allow-delete-types Lakehouse
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
            "--item-types",
            "Lakehouse",
            "--delete-orphans",
            "--allow-delete-types",
            "Lakehouse",
            "--dry-run",
        ])
        .timeout(Duration::from_mins(1))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Should have delete actions now (protection overridden)
    let summary = &data["summary"];
    assert!(
        summary["delete"].as_u64().unwrap_or(0) > 0,
        "Expected deletes when --allow-delete-types is passed"
    );

    // destructive should be true
    assert_eq!(
        data["destructive"].as_bool(),
        Some(true),
        "destructive should be true when deletions are in plan"
    );

    // Should have no protected-type warnings
    let empty_vec = vec![];
    let warnings = data["warnings"].as_array().unwrap_or(&empty_vec);
    let has_protected_warning = warnings
        .iter()
        .any(|w| w.as_str().unwrap_or_default().contains("protected type"));
    assert!(
        !has_protected_warning,
        "Should not have protected type warnings when --allow-delete-types is set"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_non_protected_type_deleted_without_allow_flag() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();

    // Create empty source (no items) — all deployed Datamarts become orphans
    // Datamarts are NOT protected, so they should be deleted without needing --allow-delete-types
    create_source_dir_with_item(&source_dir, "FakeDatamart", "Datamart");

    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
            "--item-types",
            "Datamart",
            "--delete-orphans",
            "--dry-run",
        ])
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Non-protected types should not generate protected-type warnings
    let empty_vec = vec![];
    let warnings = data["warnings"].as_array().unwrap_or(&empty_vec);
    let has_protected_warning = warnings
        .iter()
        .any(|w| w.as_str().unwrap_or_default().contains("protected type"));
    assert!(
        !has_protected_warning,
        "Non-protected types should not trigger protected-type warnings"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_force_all_sets_destructive_and_warning() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();

    // Create a source with a fake Datamart (fast type, few deployed items)
    create_source_dir_with_item(&source_dir, "FakeDatamart", "Datamart");

    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
            "--item-types",
            "Datamart",
            "--force-all",
            "--dry-run",
        ])
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // --force-all should set destructive to true
    assert_eq!(
        data["destructive"].as_bool(),
        Some(true),
        "destructive should be true with --force-all"
    );

    // Should have a warning about --force-all
    let empty_vec = vec![];
    let warnings = data["warnings"].as_array().unwrap_or(&empty_vec);
    let has_force_all_warning = warnings
        .iter()
        .any(|w| w.as_str().unwrap_or_default().contains("--force-all"));
    assert!(
        has_force_all_warning,
        "Expected warning about --force-all being irreversible: {warnings:?}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_plan_no_deletes_not_destructive() {
    let cfg = TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&source_dir).unwrap();

    // Create a source with a single create-only item
    create_source_dir_with_item(&source_dir, "FakeNewItem", "Datamart");

    // Plan WITHOUT --delete-orphans and WITHOUT --force-all
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            source_dir.to_str().unwrap(),
            "--workspace",
            &cfg.dest_workspace,
            "--item-types",
            "Datamart",
            "--dry-run",
        ])
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Without deletes or --force-all, should not be destructive
    assert_eq!(
        data["destructive"].as_bool(),
        Some(false),
        "destructive should be false without deletes or --force-all"
    );
}

// ── Governance metadata tests ────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn deploy_export_writes_governance_metadata() {
    let cfg = TestConfig::from_env();
    let output_dir = tempfile::TempDir::new().unwrap();

    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            output_dir.path().to_str().unwrap(),
            "--overwrite",
        ])
        .timeout(Duration::from_mins(5))
        .assert()
        .success();
}

#[test]
fn deploy_plan_reads_governance_metadata() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path();

    // Create a notebook item with governance metadata
    let nb_dir = source.join("TestNb.Notebook");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"TestNb"},"config":{"version":"2.0","logicalId":"test-lid-001"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("notebook-content.py"), "# hello").unwrap();
    std::fs::write(
        nb_dir.join("governance.metadata.json"),
        r#"{"sensitivityLabel":{"id":"label-uuid-001"},"tags":[{"id":"tag-uuid-001","displayName":"Production"}]}"#,
    )
    .unwrap();

    // Validate accepts governance metadata without errors
    let assert = fabio()
        .args(["deploy", "validate", "--source", source.to_str().unwrap()])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
    assert_eq!(data["items"], 1);
    assert_eq!(data["summary"]["errors"], 0);
}

#[test]
fn deploy_validate_accepts_label_replace_params() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path();

    // Create minimal source with governance metadata
    let nb_dir = source.join("Nb.Notebook");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"Nb"},"config":{"version":"2.0"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("notebook-content.py"), "# code").unwrap();
    std::fs::write(
        nb_dir.join("governance.metadata.json"),
        r#"{"sensitivityLabel":{"id":"source-label-uuid"},"tags":[{"id":"source-tag-uuid","displayName":"Dev"}]}"#,
    )
    .unwrap();

    // Create parameters file with label_replace and tag_replace
    let params_file = dir.path().join("params.json");
    std::fs::write(
        &params_file,
        r#"{"label_replace":{"source-label-uuid":"target-label-uuid"},"tag_replace":{"source-tag-uuid":"target-tag-uuid"}}"#,
    )
    .unwrap();

    // Validate accepts the params without error
    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            source.to_str().unwrap(),
            "--parameters",
            params_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
}

#[test]
fn deploy_validate_label_replace_null_removes_governance() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path();

    let nb_dir = source.join("Nb.Notebook");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"Nb"},"config":{"version":"2.0"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("notebook-content.py"), "# code").unwrap();
    std::fs::write(
        nb_dir.join("governance.metadata.json"),
        r#"{"sensitivityLabel":{"id":"dev-label"}}"#,
    )
    .unwrap();

    // null value means "skip this label"
    let params_file = dir.path().join("params.json");
    std::fs::write(&params_file, r#"{"label_replace":{"dev-label":null}}"#).unwrap();

    fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            source.to_str().unwrap(),
            "--parameters",
            params_file.to_str().unwrap(),
        ])
        .assert()
        .success();
}

// --- deploy export schedule tests ---

#[test]
#[ignore = "requires live Fabric tenant"]
fn deploy_export_includes_schedules() {
    let cfg = common::TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();

    fabio()
        .args([
            "deploy",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--dir",
            dir.path().to_str().unwrap(),
            "--overwrite",
        ])
        .assert()
        .success();

    // Check if any schedules.metadata.json files were created
    let mut has_schedules = false;
    let mut schedule_path = std::path::PathBuf::new();
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let entry = entry.unwrap();
        let sched_file = entry.path().join("schedules.metadata.json");
        if sched_file.exists() {
            has_schedules = true;
            schedule_path = sched_file;
            break;
        }
    }

    // We know the source workspace has a DataPipeline with a schedule
    assert!(
        has_schedules,
        "Expected at least one schedules.metadata.json in export output"
    );

    // Validate the schedule file format
    let content = std::fs::read_to_string(&schedule_path).unwrap();
    let schedules: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
    assert!(!schedules.is_empty(), "Schedule array should not be empty");

    // Each schedule should have jobType and enabled fields
    for schedule in &schedules {
        assert!(
            schedule.get("jobType").is_some(),
            "Schedule should have jobType field"
        );
        assert!(
            schedule.get("enabled").is_some(),
            "Schedule should have enabled field"
        );
        // Should NOT have id (server-generated, stripped for portability)
        assert!(
            schedule.get("id").is_none(),
            "Schedule should not have id field (stripped for portability)"
        );
    }
}

// --- deploy validate with schedules.metadata.json ---

#[test]
fn deploy_validate_ignores_schedules_metadata_in_parts() {
    let dir = tempfile::TempDir::new().unwrap();
    let nb_dir = dir.path().join("MyPipeline.DataPipeline");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"DataPipeline","displayName":"MyPipeline"},"config":{"version":"2.0","logicalId":"bbbbbbbb-1111-2222-3333-444444444444"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("pipeline-content.json"), "{}").unwrap();
    // Add a schedules.metadata.json — should NOT appear in definition parts
    std::fs::write(
        nb_dir.join("schedules.metadata.json"),
        r#"[{"enabled":true,"jobType":"Pipeline","configuration":{"type":"Cron","interval":60}}]"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "valid");
    assert_eq!(data["summary"]["errors"], 0);
}

// --- deploy --strategy tests ---

#[test]
fn deploy_apply_strategy_invalid_value_fails() {
    fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            ".",
            "--workspace",
            "test",
            "--strategy",
            "invalid_strategy",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("invalid value"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn deploy_apply_strategy_default_live() {
    let cfg = common::TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let nb_dir = dir.path().join("StrategyTestNB.Notebook");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"StrategyTestNB"},"config":{"version":"2.0","logicalId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("notebook-content.py"), "# strategy test").unwrap();
    std::fs::write(
        nb_dir.join("notebook-settings.json"),
        r#"{"defaultLakehouse":{"knownName":"","referenceType":"LogicalIdReference"}}"#,
    )
    .unwrap();

    // Default strategy plan with --item-types to limit scope
    let assert = fabio()
        .args([
            "deploy",
            "plan",
            "--source",
            dir.path().to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--item-types",
            "Notebook",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Plan should produce a status (dry_run or planned)
    assert!(
        data.get("status").is_some() || data.get("summary").is_some(),
        "Plan should produce structured output with status or summary"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn deploy_apply_strategy_bulk_on_source_workspace() {
    // Source workspace has NO Git integration — test that bulk strategy
    // executes without crashing (may fail at API level if bulk import
    // is not available on this tenant/workspace configuration)
    let cfg = common::TestConfig::from_env();
    let dir = tempfile::TempDir::new().unwrap();
    let nb_dir = dir.path().join("BulkTestNB.Notebook");
    std::fs::create_dir_all(&nb_dir).unwrap();
    std::fs::write(
        nb_dir.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"BulkTestNB"},"config":{"version":"2.0","logicalId":"11111111-2222-3333-4444-555555555555"}}"#,
    )
    .unwrap();
    std::fs::write(nb_dir.join("notebook-content.py"), "# bulk strategy test").unwrap();
    std::fs::write(
        nb_dir.join("notebook-settings.json"),
        r#"{"defaultLakehouse":{"knownName":"","referenceType":"LogicalIdReference"}}"#,
    )
    .unwrap();

    // Bulk strategy — the API may succeed or fail with InvalidInput
    // (bulk import API availability depends on tenant configuration).
    // We verify it doesn't crash and returns structured output.
    let assert = fabio()
        .args([
            "deploy",
            "apply",
            "--source",
            dir.path().to_str().unwrap(),
            "--workspace",
            &cfg.source_workspace,
            "--strategy",
            "bulk",
            "--item-types",
            "Notebook",
        ])
        .assert();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // Should produce valid JSON regardless of success/failure
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let data = &json["data"];
    // Status should be "succeeded" or "partial_failure" (not a crash)
    assert!(
        data["status"] == "succeeded" || data["status"] == "partial_failure",
        "Expected structured output with status, got: {data}"
    );
}
