//! E2E integration tests for the `fabio onelake-security` command group.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
#[ignore = "requires live Fabric tenant"]
fn onelake_security_list() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "onelake-security",
            "list",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    assert!(json.get("data").is_some());
    assert!(json.get("count").is_some());
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn onelake_security_upsert_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "onelake-security",
            "upsert",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--roles",
            r#"[{"name":"TestRole","decisionRules":[{"effect":"Permit","permission":[{"attributeName":"Path","attributeValueIncludedIn":["/Tables/*"]}]}],"members":{"fabricItemMembers":[]}}]"#,
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn onelake_security_delete_dry_run() {
    let cfg = TestConfig::from_env();

    let output = fabio()
        .args([
            "onelake-security",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--role-name",
            "NonExistentRole",
            "--dry-run",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["status"], "dry_run");
}

// ─── Create ─────────────────────────────────────────────────────────────────

#[test]
fn onelake_security_create_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "onelake-security",
            "create",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--role",
            r#"{"name":"TestRole","decisionRules":[{"effect":"Permit","permission":[{"attributeName":"Path","attributeValueIncludedIn":["/Tables/*"]}]}],"members":{"fabricItemMembers":[]}}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "onelake-security create");
}

#[test]
fn onelake_security_create_with_conflict_policy_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "onelake-security",
            "create",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--role",
            r#"{"name":"TestRole","decisionRules":[{"effect":"Permit","permission":[{"attributeName":"Path","attributeValueIncludedIn":["/Tables/*"]}]}],"members":{"fabricItemMembers":[]}}"#,
            "--conflict-policy",
            "Overwrite",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
}

// ─── Show (native endpoint) ─────────────────────────────────────────────────

#[test]
#[ignore = "requires live Fabric tenant"]
fn onelake_security_show_not_found() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "onelake-security",
            "show",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--role-name",
            "NonExistentRoleName12345",
        ])
        .assert()
        .failure();
}

// ─── Effective principal access ─────────────────────────────────────────────

#[test]
fn onelake_security_get_principal_access_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "onelake-security",
            "get-principal-access",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--principal-id",
            "cccccccc-1111-2222-3333-444444444444",
            "--root-path",
            "Tables",
            "--expand-tables",
            "--page-size",
            "1000",
            "--if-match",
            "\"access-etag\"",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["would_execute"],
        "onelake-security get-principal-access"
    );
    assert_eq!(data["details"]["rootPath"], "Tables");
    assert_eq!(data["details"]["expandTables"], true);
    assert_eq!(data["details"]["pageSize"], 1000);
    assert_eq!(data["details"]["ifMatch"], "\"access-etag\"");
}

#[test]
fn onelake_security_get_principal_access_rejects_expand_for_files() {
    fabio()
        .args([
            "onelake-security",
            "get-principal-access",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--root-path",
            "Files",
            "--expand-tables",
        ])
        .assert()
        .failure();
}

#[test]
fn onelake_security_get_principal_access_rejects_unquoted_etag() {
    fabio()
        .args([
            "onelake-security",
            "get-principal-access",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--if-none-match",
            "unquoted-etag",
        ])
        .assert()
        .failure();
}

#[test]
fn onelake_security_get_principal_access_forwards_query_headers_and_etag() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (server_uri, _server) = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/workspaces/aaaaaaaa-1111-2222-3333-444444444444/items/bbbbbbbb-1111-2222-3333-444444444444/principalDataAccessRecords",
            ))
            .and(query_param(
                "principalId",
                "cccccccc-1111-2222-3333-444444444444",
            ))
            .and(query_param("rootPath", "Tables"))
            .and(query_param("expandTables", "true"))
            .and(query_param("pageSize", "25"))
            .and(header("If-Match", "\"access-etag\""))
            .and(header("If-None-Match", "\"older-etag\""))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ETag", "\"computed-etag\"")
                    .set_body_json(serde_json::json!({
                        "value": [{
                            "type": "ExpandedTable",
                            "path": "/Tables/dbo/dimension_customer",
                            "access": ["Read"],
                            "effect": "Permit",
                            "columnConstraints": [{
                                "name": "CustomerKey",
                                "columnEffect": "Permit",
                                "columnAction": ["Read"]
                            }],
                            "rowsConstraint": "[CustomerKey] > 10"
                        }],
                        "continuationToken": "next-token"
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;
        (server.uri(), server)
    });

    let output = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
        .args([
            "onelake-security",
            "get-principal-access",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--principal-id",
            "cccccccc-1111-2222-3333-444444444444",
            "--root-path",
            "Tables",
            "--expand-tables",
            "--page-size",
            "25",
            "--if-match",
            "\"access-etag\"",
            "--if-none-match",
            "\"older-etag\"",
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    assert_eq!(json["etag"], "\"computed-etag\"");
    assert_eq!(json["continuationToken"], "next-token");
    assert_eq!(json["count"], 1);
    assert_eq!(json["data"][0]["type"], "ExpandedTable");
    assert_eq!(
        json["data"][0]["columnConstraints"][0]["columnAction"][0],
        "Read"
    );
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn onelake_security_get_principal_access_live() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "onelake-security",
            "get-principal-access",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--root-path",
            "Tables",
            "--page-size",
            "1",
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    assert!(json["data"].is_array());
    assert!(json["etag"].is_string());
}
