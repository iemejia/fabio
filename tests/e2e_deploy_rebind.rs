//! End-to-end integration tests for `fabio deploy rebind` and
//! `fabio deploy validate --pr-ready`.
//!
//! These are fully OFFLINE (no Fabric tenant, no network) — they operate on local
//! `.platform` definition trees in a temp directory — so they run in CI without
//! `#[ignore]`.

mod common;

use common::{extract_data, fabio, parse_json};

/// Build a minimal branched-out repo tree:
/// - a `SemanticModel` whose Direct Lake connection embeds the dev lakehouse + workspace GUIDs
/// - a Notebook whose META block embeds the dev lakehouse GUID
/// - a `VariableLibrary` with a `dev` value set
///
/// Returns the temp dir and the path to a params file mapping dev <-> feature-x values.
fn make_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();

    let sm = root.join("MySM.SemanticModel/definition");
    std::fs::create_dir_all(&sm).unwrap();
    std::fs::write(
        root.join("MySM.SemanticModel/.platform"),
        r#"{"metadata":{"type":"SemanticModel","displayName":"MySM"}}"#,
    )
    .unwrap();
    std::fs::write(
        sm.join("expressions.tmdl"),
        r#"let Database = Sql.Database("dev-lake-1111", "dev-ws-2222")"#,
    )
    .unwrap();

    let nb = root.join("MyNb.Notebook");
    std::fs::create_dir_all(&nb).unwrap();
    std::fs::write(
        nb.join(".platform"),
        r#"{"metadata":{"type":"Notebook","displayName":"MyNb"}}"#,
    )
    .unwrap();
    std::fs::write(
        nb.join("notebook-content.py"),
        "# META default_lakehouse: dev-lake-1111\nprint(1)",
    )
    .unwrap();

    let vs = root.join("MyVL.VariableLibrary/valueSets");
    std::fs::create_dir_all(&vs).unwrap();
    std::fs::write(
        root.join("MyVL.VariableLibrary/.platform"),
        r#"{"metadata":{"type":"VariableLibrary","displayName":"MyVL"}}"#,
    )
    .unwrap();
    std::fs::write(vs.join("dev.json"), r#"{"lakehouse":"dev-lake-1111"}"#).unwrap();

    let params = root.join("params.json");
    std::fs::write(
        &params,
        r#"{"find_replace":[
            {"find_value":"ph1","replace_value":{"dev":"dev-lake-1111","feature-x":"feat-lake-9999"}},
            {"find_value":"ph2","replace_value":{"dev":"dev-ws-2222","feature-x":"feat-ws-8888"}}
        ]}"#,
    )
    .unwrap();

    (dir, params)
}

#[test]
fn rebind_dry_run_does_not_write_files() {
    let (dir, params) = make_repo();
    let sm = dir
        .path()
        .join("MySM.SemanticModel/definition/expressions.tmdl");
    let before = std::fs::read_to_string(&sm).unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "rebind",
            "--source",
            dir.path().to_str().unwrap(),
            "--parameters",
            params.to_str().unwrap(),
            "--from-env",
            "dev",
            "--to-env",
            "feature-x",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["files_changed"], 3);
    assert_eq!(data["replacements"], 4);

    // Files must be unchanged after a dry run.
    assert_eq!(std::fs::read_to_string(&sm).unwrap(), before);
}

#[test]
fn rebind_rewrites_all_files_and_is_reversible() {
    let (dir, params) = make_repo();
    let root = dir.path();
    let sm = root.join("MySM.SemanticModel/definition/expressions.tmdl");
    let nb = root.join("MyNb.Notebook/notebook-content.py");
    let vs = root.join("MyVL.VariableLibrary/valueSets/dev.json");

    // Rebind dev -> feature-x.
    fabio()
        .args([
            "deploy",
            "rebind",
            "--source",
            root.to_str().unwrap(),
            "--parameters",
            params.to_str().unwrap(),
            "--from-env",
            "dev",
            "--to-env",
            "feature-x",
        ])
        .assert()
        .success();

    assert!(
        std::fs::read_to_string(&sm)
            .unwrap()
            .contains("feat-lake-9999")
    );
    assert!(
        std::fs::read_to_string(&sm)
            .unwrap()
            .contains("feat-ws-8888")
    );
    assert!(
        std::fs::read_to_string(&nb)
            .unwrap()
            .contains("feat-lake-9999")
    );
    assert!(
        std::fs::read_to_string(&vs)
            .unwrap()
            .contains("feat-lake-9999")
    );

    // Reverse: feature-x -> dev.
    fabio()
        .args([
            "deploy",
            "rebind",
            "--source",
            root.to_str().unwrap(),
            "--parameters",
            params.to_str().unwrap(),
            "--from-env",
            "feature-x",
            "--to-env",
            "dev",
        ])
        .assert()
        .success();

    assert!(
        std::fs::read_to_string(&sm)
            .unwrap()
            .contains("dev-lake-1111")
    );
    assert!(
        std::fs::read_to_string(&sm)
            .unwrap()
            .contains("dev-ws-2222")
    );
    assert!(
        !std::fs::read_to_string(&nb)
            .unwrap()
            .contains("feat-lake-9999")
    );
}

#[test]
fn rebind_same_env_fails() {
    let (dir, params) = make_repo();
    fabio()
        .args([
            "deploy",
            "rebind",
            "--source",
            dir.path().to_str().unwrap(),
            "--parameters",
            params.to_str().unwrap(),
            "--from-env",
            "dev",
            "--to-env",
            "dev",
        ])
        .assert()
        .failure();
}

#[test]
fn rebind_skips_deploy_time_dynamic_values_with_warning() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let sm = root.join("MySM.SemanticModel");
    std::fs::create_dir_all(&sm).unwrap();
    std::fs::write(
        sm.join(".platform"),
        r#"{"metadata":{"type":"SemanticModel","displayName":"MySM"}}"#,
    )
    .unwrap();
    std::fs::write(sm.join("model.tmdl"), "dev-lake-1111").unwrap();

    // The feature-x value is a deploy-time dynamic — cannot resolve offline.
    let params = root.join("params.json");
    std::fs::write(
        &params,
        r#"{"find_replace":[{"find_value":"ph","replace_value":{"dev":"dev-lake-1111","feature-x":"$items.Lakehouse.LH.id"}}]}"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "rebind",
            "--source",
            root.to_str().unwrap(),
            "--parameters",
            params.to_str().unwrap(),
            "--from-env",
            "dev",
            "--to-env",
            "feature-x",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    // Nothing rewritten (the only rule was skipped), and a warning emitted.
    assert_eq!(data["replacements"], 0);
    let warnings = data["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("dynamic")),
        "expected a dynamic-skip warning, got {warnings:?}"
    );
}

#[test]
fn pr_ready_fails_when_bound_to_foreign_env() {
    let (dir, params) = make_repo();
    let root = dir.path();

    // Bind to feature-x first.
    fabio()
        .args([
            "deploy",
            "rebind",
            "--source",
            root.to_str().unwrap(),
            "--parameters",
            params.to_str().unwrap(),
            "--from-env",
            "dev",
            "--to-env",
            "feature-x",
        ])
        .assert()
        .success();
    // Add a stray feature value set.
    std::fs::write(
        root.join("MyVL.VariableLibrary/valueSets/feature-x.json"),
        r#"{"lakehouse":"feat-lake-9999"}"#,
    )
    .unwrap();

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            root.to_str().unwrap(),
            "--pr-ready",
            "--parameters",
            params.to_str().unwrap(),
            "--expect-env",
            "dev",
            "--allow-value-set",
            "dev",
        ])
        .assert()
        .failure();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "not_pr_ready");
    assert_eq!(data["pr_ready"], false);
    let errors = data["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e.as_str().unwrap().contains("foreign env 'feature-x'")),
        "expected foreign-env error, got {errors:?}"
    );
    assert!(
        errors.iter().any(|e| e.as_str().unwrap().contains("stray")),
        "expected stray value-set error, got {errors:?}"
    );
}

#[test]
fn pr_ready_passes_when_clean() {
    let (dir, params) = make_repo();
    let root = dir.path();

    let assert = fabio()
        .args([
            "deploy",
            "validate",
            "--source",
            root.to_str().unwrap(),
            "--pr-ready",
            "--parameters",
            params.to_str().unwrap(),
            "--expect-env",
            "dev",
            "--allow-value-set",
            "dev",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "pr_ready");
    assert_eq!(data["pr_ready"], true);
    assert_eq!(data["summary"]["errors"], 0);
}
