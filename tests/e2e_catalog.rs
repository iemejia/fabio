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
