//! End-to-end integration tests for `fabio scorecard` (Power BI Goals).

mod common;

use common::{TestConfig, extract_data, fabio, parse_json, unique_name};
use serial_test::serial;

// ── Hermetic (no tenant) ─────────────────────────────────────────────────────

#[test]
fn scorecard_create_dry_run() {
    let assert = fabio()
        .args([
            "scorecard",
            "create",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--name",
            "sc_dry",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(extract_data(&json)["would_execute"], "scorecard create");
    assert_eq!(extract_data(&json)["details"]["name"], "sc_dry");
}

#[test]
fn scorecard_delete_dry_run_is_destructive() {
    let assert = fabio()
        .args([
            "scorecard",
            "delete",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "scorecard delete");
    // Destructive dry-run preview must carry the confirm-with-user signal.
    assert_eq!(data["destructive"], true);
}

#[test]
fn scorecard_delete_goal_requires_goal_id() {
    // Empty goal id must fail fast with a teaching hint (before any network call).
    fabio()
        .args([
            "scorecard",
            "delete-goal",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--goal-id",
            "   ",
        ])
        .assert()
        .failure();
}

// ── Live (requires tenant) ───────────────────────────────────────────────────

/// Full lifecycle against the Power BI Goals API: create a scorecard, add a
/// goal, expand it via show/list-goals, then delete (which cascades the goal).
#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn scorecard_full_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = unique_name("sc");

    // Create.
    let created = fabio()
        .args([
            "scorecard",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
            "--description",
            "fabio e2e",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let created_json = parse_json(&created);
    let sc = extract_data(&created_json);
    assert_eq!(sc["name"], name);
    let sc_id = sc["id"].as_str().unwrap().to_string();

    // Create a goal.
    let goal = fabio()
        .args([
            "scorecard",
            "create-goal",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &sc_id,
            "--name",
            "Revenue",
            "--rank",
            "1",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&goal))["name"], "Revenue");

    // Show with goals expanded.
    let shown = fabio()
        .args([
            "scorecard",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &sc_id,
            "--goals",
        ])
        .assert()
        .success();
    let shown_data = parse_json(&shown);
    let goals = extract_data(&shown_data)["goals"].as_array().unwrap();
    assert!(!goals.is_empty(), "expanded scorecard must list its goal");

    // List goals.
    let listed = fabio()
        .args([
            "scorecard",
            "list-goals",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &sc_id,
        ])
        .assert()
        .success();
    assert!(extract_data(&parse_json(&listed)).is_array());

    // Delete the scorecard (cascades goals).
    let deleted = fabio()
        .args([
            "scorecard",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &sc_id,
        ])
        .assert()
        .success();
    assert_eq!(extract_data(&parse_json(&deleted))["status"], "deleted");
}
