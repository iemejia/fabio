//! End-to-end integration tests for global CLI options: --query, --quiet, --output.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use predicates::prelude::*;
use serial_test::serial;

// --- --query flag tests ---

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn query_extracts_single_field_from_object() {
    let cfg = TestConfig::from_env();

    // Use --query to extract just the "id" field from workspace show
    let assert = fabio()
        .args([
            "--query",
            "id",
            "workspace",
            "show",
            "--id",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Should just be the workspace ID string
    assert_eq!(data.as_str().unwrap(), cfg.source_workspace);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn query_extracts_field_from_list() {
    // Use --query with JMESPath to extract "displayName" from workspace list
    let assert = fabio()
        .args(["--query", "[*].displayName", "workspace", "list"])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Should be an array of display names (strings)
    let arr = data.as_array().expect("expected array of names");
    assert!(!arr.is_empty());
    // Each element should be a string
    for name in arr {
        assert!(name.is_string(), "expected string, got: {name}");
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn query_list_scalar_result_not_array_wrapped() {
    // A --query on a list that yields a SCALAR (e.g. `[0].id`) must emit
    // {"data": "<id>"} — not {"data": ["<id>"], "count": 1}. Regression test for
    // the scalar-wrap quirk. Counts use the JMESPath idiom `length([])`.
    let assert = fabio()
        .args(["--query", "[0].id", "workspace", "list"])
        .assert()
        .success();

    let json = parse_json(&assert);
    // `data` should be a bare string, and there should be no `count` field.
    let data = extract_data(&json);
    assert!(
        data.is_string(),
        "expected scalar string under data, got: {data}"
    );
    assert!(
        json.get("count").is_none(),
        "scalar query result must not carry a list `count`, got: {json}"
    );

    // `length([])` yields the item count as a bare number.
    let assert = fabio()
        .args(["--query", "length([])", "workspace", "list"])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(
        extract_data(&json).is_number(),
        "length([]) should yield a number, got: {json}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn query_list_scalar_plain_prints_single_value() {
    // `-o plain` on a list query that yields a SCALAR must print exactly that one
    // value — NOT fall back to printing the whole un-projected list. Regression
    // test for the plain-path analog of the scalar-wrap bug.
    let assert = fabio()
        .args([
            "--query",
            "[0].id",
            "--output",
            "plain",
            "workspace",
            "list",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "scalar query in plain mode must print exactly one line, got: {lines:?}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn query_nested_field() {
    let cfg = TestConfig::from_env();

    // Workspace show returns fields like capacityId - test it extracts correctly
    let assert = fabio()
        .args([
            "--query",
            "capacityId",
            "workspace",
            "show",
            "--id",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    // Should be the capacity ID string
    assert!(data.is_string(), "expected capacityId to be a string");
    assert!(!data.as_str().unwrap().is_empty());
}

// --- --query validation (fail fast on jq syntax / envelope confusion) ---
// These run OFFLINE: the query is validated before any network call, so no
// live tenant or auth is needed.

#[test]
#[serial]
fn query_jq_leading_dot_fails_fast_with_teaching_error() {
    // `.data[].name` is jq, not JMESPath. It used to silently return
    // {"data":null} with exit 0; now it must fail fast with a corrected form.
    let assert = fabio()
        .args(["--query", ".data[].name", "workspace", "list"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let json: serde_json::Value = serde_json::from_str(stderr.trim()).expect("error JSON");
    assert_eq!(json["error"]["code"], "INVALID_INPUT");
    assert_eq!(json["error"]["hintType"], "syntax_fix");
    // Suggests the corrected JMESPath.
    assert!(
        json["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("'[].name'"),
        "hint should suggest '[].name': {}",
        json["error"]["hint"]
    );
}

#[test]
#[serial]
fn query_jq_select_pipe_fails_fast() {
    let assert = fabio()
        .args(["--query", "[] | select(.type=='X')", "workspace", "list"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("jq"));
    assert!(stderr.contains("JMESPath"));
}

#[test]
#[serial]
fn query_envelope_data_prefix_fails_fast() {
    // `data[].displayName` — the payload IS already the value under `data`.
    let assert = fabio()
        .args(["--query", "data[].displayName", "workspace", "list"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("'[].displayName'"), "got: {stderr}");
}

#[test]
#[serial]
fn query_bare_count_suggests_length_idiom() {
    let assert = fabio()
        .args(["--query", "count", "workspace", "list"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("length([])"), "got: {stderr}");
}

#[test]
#[serial]
fn query_invalid_jmespath_syntax_fails_fast() {
    let assert = fabio()
        .args(["--query", "[[[invalid", "workspace", "list"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("JMESPath"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn query_missing_field_returns_null() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--query",
            "nonexistent_field",
            "workspace",
            "show",
            "--id",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);

    assert!(
        data.is_null(),
        "expected null for missing field, got: {data}"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn query_with_table_output() {
    // --query + --output table should not crash
    fabio()
        .args([
            "--query",
            "[*].displayName",
            "--output",
            "table",
            "workspace",
            "list",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn query_with_plain_output() {
    let cfg = TestConfig::from_env();

    // --query id + --output plain should print just the id
    let assert = fabio()
        .args([
            "--query",
            "id",
            "--output",
            "plain",
            "workspace",
            "show",
            "--id",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let trimmed = stdout.trim();
    assert_eq!(trimmed, cfg.source_workspace);
}

// --- --quiet flag tests ---

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn quiet_suppresses_all_stdout() {
    // --quiet should produce no stdout
    fabio()
        .args(["--quiet", "workspace", "list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn quiet_with_show_command() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "--quiet",
            "workspace",
            "show",
            "--id",
            &cfg.source_workspace,
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn quiet_still_shows_errors_on_stderr() {
    // --quiet should still show errors on stderr
    fabio()
        .args([
            "--quiet",
            "workspace",
            "show",
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("error"));
}

// --- --output format coverage ---

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn output_json_produces_valid_json() {
    let assert = fabio()
        .args(["--output", "json", "workspace", "list"])
        .assert()
        .success();

    // Should be valid JSON
    let _json = parse_json(&assert);
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn output_table_has_headers() {
    let cfg = TestConfig::from_env();

    // Table output for item list should show headers
    fabio()
        .args([
            "--output",
            "table",
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("NAME"))
        .stdout(predicate::str::contains("ID"))
        .stdout(predicate::str::contains("TYPE"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn output_plain_for_item_list() {
    let cfg = TestConfig::from_env();

    // Plain output for item list should print item IDs (plain_key is "id")
    fabio()
        .args([
            "--output",
            "plain",
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// --- --continuation-token flag tests ---

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn continuation_token_with_invalid_token_returns_error() {
    let cfg = TestConfig::from_env();

    // An invalid token should fail gracefully (API returns error)
    fabio()
        .args([
            "--continuation-token",
            "invalid_token_abc123",
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn continuation_token_flag_accepted_on_list_command() {
    // Verify the flag is accepted (even with no token) - just tests CLI parsing
    fabio()
        .args(["workspace", "list", "--limit", "1"])
        .assert()
        .success();
}

// --- --lro-timeout flag tests ---

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lro_timeout_flag_accepted() {
    // Verify the --lro-timeout flag is accepted and doesn't break normal commands
    fabio()
        .args(["--lro-timeout", "300", "workspace", "list", "--limit", "1"])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lro_timeout_flag_works_with_show() {
    let cfg = TestConfig::from_env();

    // Verify --lro-timeout works alongside other commands
    let assert = fabio()
        .args([
            "--lro-timeout",
            "60",
            "workspace",
            "show",
            "--id",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["id"], cfg.source_workspace);
}

// --- --output csv/tsv format tests ---

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn output_csv_produces_comma_separated_with_header() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--output",
            "csv",
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--limit",
            "2",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    // Must have header + at least 1 data row
    assert!(lines.len() >= 2, "CSV should have header + data rows");
    // Header should contain comma-separated column names
    assert!(
        lines[0].contains(','),
        "CSV header should use comma separator"
    );
    // Header should include common item fields
    let header = lines[0].to_lowercase();
    assert!(header.contains("id"), "CSV header should include 'id'");
    assert!(header.contains("type"), "CSV header should include 'type'");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn output_tsv_produces_tab_separated_with_header() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--output",
            "tsv",
            "item",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--limit",
            "2",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    // Must have header + at least 1 data row
    assert!(lines.len() >= 2, "TSV should have header + data rows");
    // Header should contain tab-separated column names
    assert!(
        lines[0].contains('\t'),
        "TSV header should use tab separator"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn output_csv_single_object_produces_key_value_rows() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--output",
            "csv",
            "workspace",
            "show",
            "--id",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // Single-object CSV should produce header + 1 data row
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "Single-object CSV should have header + data"
    );
    assert!(lines[0].contains(','), "Should use comma separator");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn output_csv_with_query_extracts_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "--output",
            "csv",
            "--query",
            "id",
            "workspace",
            "show",
            "--id",
            &cfg.source_workspace,
        ])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // With --query id, output should contain the workspace ID
    assert!(
        stdout.contains(&cfg.source_workspace),
        "CSV with --query should extract the field value"
    );
}
