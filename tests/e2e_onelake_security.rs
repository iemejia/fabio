//! E2E integration tests for the `fabio onelake-security` command group.

mod common;

use common::{TestConfig, extract_data, fabio, parse_error, parse_json};
use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
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
    let workspace = std::env::var("FABIO_TEST_SOURCE_WORKSPACE").expect("live workspace binding");
    let lakehouse = std::env::var("FABIO_TEST_SOURCE_LAKEHOUSE").expect("live lakehouse binding");
    let output = fabio()
        .args([
            "--readonly",
            "--wrap-untrusted",
            "onelake-security",
            "get-principal-access",
            "--workspace",
            &workspace,
            "--id",
            &lakehouse,
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

const ACCESS_PATH: &str = "/workspaces/aaaaaaaa-1111-2222-3333-444444444444/items/bbbbbbbb-1111-2222-3333-444444444444/principalDataAccessRecords";

fn access_command(server: &MockServer) -> assert_cmd::Command {
    let mut command = common::mock_fabio(server);
    command.args([
        "--readonly",
        "onelake-security",
        "get-principal-access",
        "--workspace",
        "aaaaaaaa-1111-2222-3333-444444444444",
        "--id",
        "bbbbbbbb-1111-2222-3333-444444444444",
        "--root-path",
        "Tables",
    ]);
    command.timeout(std::time::Duration::from_secs(10));
    command
}

async fn access_page(
    server: &MockServer,
    token: Option<&str>,
    body: serde_json::Value,
    etag: &str,
) {
    let mock = Mock::given(method("GET")).and(path(ACCESS_PATH));
    let mock = match token {
        Some(token) => mock.and(query_param("continuationToken", token)),
        None => mock.and(query_param_is_missing("continuationToken")),
    };
    mock.respond_with(
        ResponseTemplate::new(200)
            .insert_header("ETag", etag)
            .set_body_json(body),
    )
    .expect(1)
    .mount(server)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn principal_access_all_preserves_grouped_constraints_and_metadata() {
    let server = MockServer::start().await;
    let row = serde_json::json!({
        "type":"Table", "path":"/Tables", "effect":"Permit", "access":["Read"],
        "rows":[{"tablePath":"dbo/customer", "rowsConstraint":"[Key] > 10"}],
        "columns":[{"tablePath":"dbo/customer", "columnConstraints":[{"name":"Key","columnEffect":"Permit","columnAction":["Read"]}]}]
    });
    access_page(
        &server,
        None,
        serde_json::json!({"value":[row],"continuationToken":"next token=="}),
        "\"snapshot\"",
    )
    .await;
    access_page(&server, Some("next token=="), serde_json::json!({"value":[{"type":"Folder","path":"/Tables/archive","effect":"Deny","access":["Read"]}],"continuationToken":""}), "\"snapshot\"").await;
    let result = access_command(&server).arg("--all").assert().success();
    let json = parse_json(&result);
    assert_eq!(json["count"], 2);
    assert_eq!(json["etag"], "\"snapshot\"");
    assert!(json.get("continuationToken").is_none());
    assert_eq!(json["data"][0]["rows"][0]["rowsConstraint"], "[Key] > 10");
    assert_eq!(
        json["data"][0]["columns"][0]["columnConstraints"][0]["name"],
        "Key"
    );
    assert_eq!(json["data"][1]["effect"], "Deny");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn principal_access_resumes_and_preserves_etag_with_query_and_limit() {
    for args in [vec!["--limit", "1"], vec!["--query", "[0].path"]] {
        let server = MockServer::start().await;
        access_page(&server, Some("resume=="), serde_json::json!({"value":[{"path":"first"},{"path":"second"}],"continuationToken":"next"}), "\"snapshot\"").await;
        let result = access_command(&server)
            .args(["--continuation-token", "resume=="])
            .args(&args)
            .assert()
            .success();
        let json = parse_json(&result);
        assert_eq!(json["etag"], "\"snapshot\"");
        if args[0] == "--limit" {
            assert_eq!(json["count"], 1);
            assert_eq!(json["truncated"], true);
            assert_eq!(json["total_available"], 2);
            assert_eq!(json["continuationToken"], "next");
        } else {
            assert_eq!(json["data"], "first");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn principal_access_inconsistent_or_repeating_pages_fail_without_partial_output() {
    for changed in [false, true] {
        let server = MockServer::start().await;
        access_page(
            &server,
            None,
            serde_json::json!({"value":[],"continuationToken":"next"}),
            "\"snapshot\"",
        )
        .await;
        access_page(
            &server,
            Some("next"),
            serde_json::json!({"value":[],"continuationToken":"next"}),
            if changed {
                "\"changed\""
            } else {
                "\"snapshot\""
            },
        )
        .await;
        let result = access_command(&server).arg("--all").assert().failure();
        assert_eq!(parse_error(&result)["error"]["code"], "API_ERROR");
        assert_eq!(result.get_output().stdout.len(), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn principal_access_malformed_success_is_not_an_empty_access_report() {
    let server = MockServer::start().await;
    access_page(
        &server,
        None,
        serde_json::json!({"value":null}),
        "\"snapshot\"",
    )
    .await;
    let result = access_command(&server).assert().failure();
    assert_eq!(parse_error(&result)["error"]["code"], "API_ERROR");
    assert_eq!(result.get_output().stdout.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn principal_access_all_requires_snapshot_evidence_on_every_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(ACCESS_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"value":[]})))
        .expect(1)
        .mount(&server)
        .await;
    let result = access_command(&server).arg("--all").assert().failure();
    assert_eq!(parse_error(&result)["error"]["code"], "API_ERROR");
    assert_eq!(result.get_output().stdout.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn principal_access_errors_preserve_codes_without_retry() {
    for (status, code) in [(403, "FORBIDDEN"), (404, "NOT_FOUND"), (412, "CONFLICT")] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(ACCESS_PATH))
            .respond_with(ResponseTemplate::new(status).set_body_json(
                serde_json::json!({"errorCode":"FixtureFailure","message":"Fixture failure"}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let result = access_command(&server)
            .args(["--if-match", "\"snapshot\""])
            .assert()
            .failure();
        assert_eq!(parse_error(&result)["error"]["code"], code);
    }
}

#[test]
fn principal_access_invalid_inputs_fail_before_authentication_even_in_dry_run() {
    for extra in [
        vec!["--principal-id", "not-a-uuid"],
        vec!["--page-size", "0"],
        vec!["--page-size", "1001"],
        vec!["--root-path", "tables"],
    ] {
        fabio()
            .args([
                "--dry-run",
                "onelake-security",
                "get-principal-access",
                "--workspace",
                "aaaaaaaa-1111-2222-3333-444444444444",
                "--id",
                "bbbbbbbb-1111-2222-3333-444444444444",
            ])
            .args(extra)
            .assert()
            .failure();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn role_policy_error_is_taught_for_all_three_mutation_paths() {
    let role_path = "/workspaces/aaaaaaaa-1111-2222-3333-444444444444/items/bbbbbbbb-1111-2222-3333-444444444444/dataAccessRoles";
    for (verb, request_path, args) in [
        (
            "POST",
            role_path.to_string(),
            vec!["create", "--role", r#"{"name":"FixtureRole"}"#],
        ),
        (
            "PUT",
            role_path.to_string(),
            vec!["upsert", "--roles", "[]"],
        ),
        (
            "DELETE",
            format!("{role_path}/FixtureRole"),
            vec!["delete", "--role-name", "FixtureRole"],
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method(verb)).and(path(request_path))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "errorCode":"DataAccessRoleChangeRestrictedByPolicy", "message":"DataAccessRoleChangeRestrictedByPolicy"
            }))).expect(1).mount(&server).await;
        let result = common::mock_fabio(&server)
            .args(["onelake-security"])
            .args(args)
            .args([
                "--workspace",
                "aaaaaaaa-1111-2222-3333-444444444444",
                "--id",
                "bbbbbbbb-1111-2222-3333-444444444444",
            ])
            .assert()
            .failure();
        let error = parse_error(&result);
        assert_eq!(error["error"]["code"], "FORBIDDEN");
        assert!(
            error["error"]["hint"]
                .as_str()
                .unwrap()
                .contains("Manage OneLake data access roles")
        );
    }
}
