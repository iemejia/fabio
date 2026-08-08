//! End-to-end integration tests for `fabio catalog` commands.

mod common;

use common::{fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn catalog_search_succeeds() {
    let assert = fabio()
        .args(["catalog", "search", "--search", "test"])
        .assert()
        .success();
    let json = parse_json(&assert);
    // Result may be null (no matches), an array, or an object
    assert!(json.get("data").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn catalog_search_with_type_filter() {
    let assert = fabio()
        .args([
            "catalog",
            "search",
            "--search",
            "Sales",
            "--type",
            "Lakehouse",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(json.get("data").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn catalog_search_with_top() {
    let assert = fabio()
        .args(["catalog", "search", "--search", "test", "--top", "2"])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(json.get("data").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn catalog_search_with_exclude_type() {
    let assert = fabio()
        .args([
            "catalog",
            "search",
            "--search",
            "test",
            "--exclude-type",
            "Dashboard",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(json.get("data").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn catalog_search_with_multiple_types() {
    let assert = fabio()
        .args([
            "catalog",
            "search",
            "--search",
            "test",
            "--type",
            "Notebook,Lakehouse",
            "--top",
            "5",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(json.get("data").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn catalog_search_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "catalog",
            "search",
            "--search",
            "test",
            "--type",
            "Notebook",
            "--top",
            "3",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = json.get("data").expect("missing data");
    assert_eq!(data["dry_run"], true);
    // Verify the search body uses the correct CatalogQueryRequest fields
    // (search / pageSize / filter — NOT searchString / top / itemTypes).
    let details = &data["details"];
    assert_eq!(details["search"], "test");
    assert_eq!(details["pageSize"], 3);
    assert_eq!(details["filter"], "Type eq 'Notebook'");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn catalog_search_content_flag_override() {
    // --content should override convenience flags
    let assert = fabio()
        .args([
            "catalog",
            "search",
            "--content",
            r#"{"search":"Sales","pageSize":1}"#,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(json.get("data").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn catalog_search_all_paginates_and_flattens() {
    // Regression for 113fc01: `--all` must auto-paginate the body-token pages
    // and flatten the `{value:[…]}` envelope to the standard `{data:[…],count}`
    // list shape (so agents can iterate `data`). The empty-string last-page
    // token must terminate cleanly (no InvalidContinuationToken).
    let assert = fabio()
        .args(["catalog", "search", "--search", "a", "--all"])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert!(
        json.get("data").and_then(|d| d.as_array()).is_some(),
        "--all must return a flattened data array, got: {json}"
    );
    // count must be present on the list envelope.
    assert!(
        json.get("count").is_some(),
        "list envelope must carry count"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn catalog_search_exposes_workspace_column_path() {
    // Regression for 113fc01: the WORKSPACE column is projected from the nested
    // `hierarchy.workspace.displayName` path. Verify a matched item carries the
    // hierarchy.workspace object so the column resolves (was empty before).
    let assert = fabio()
        .args([
            "catalog",
            "search",
            "--search",
            "Lakehouse",
            "--type",
            "Lakehouse",
            "--top",
            "5",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    if let Some(rows) = json.get("data").and_then(|d| d.as_array())
        && let Some(first) = rows.first()
    {
        // The source rows must expose hierarchy.workspace (the WORKSPACE column
        // source). Not all items have it, but a Lakehouse always lives in a
        // workspace, so at least the hierarchy key must be present.
        assert!(
            first.get("hierarchy").is_some(),
            "catalog row must carry the hierarchy object, got: {first}"
        );
    }
}
