mod common;

use common::{extract_data, fabio, mock_fabio, parse_json};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WORKSPACE: &str = "00000000-0000-0000-0000-000000000001";
const ITEM: &str = "00000000-0000-0000-0000-000000000002";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgresql_crud_uses_the_case_sensitive_collection_and_body_contract() {
    let server = MockServer::start().await;
    let collection = format!("/workspaces/{WORKSPACE}/postgreSQLDatabases");
    let item = format!("{collection}/{ITEM}");
    for (command, verb, route, extra, body, response) in [
        (
            "create",
            "POST",
            collection.as_str(),
            vec!["--name", "synthetic_db", "--null-creation-payload"],
            Some(json!({"displayName": "synthetic_db", "creationPayload": null})),
            json!({"id": ITEM}),
        ),
        (
            "show",
            "GET",
            item.as_str(),
            vec!["--id", ITEM],
            None,
            json!({"id": ITEM, "properties": {"databaseName": "synthetic_db", "serverFqdn": "example.invalid"}}),
        ),
        (
            "update",
            "PATCH",
            item.as_str(),
            vec!["--id", ITEM, "--description", "Synthetic description"],
            Some(json!({"description": "Synthetic description"})),
            json!({"id": ITEM}),
        ),
        (
            "delete",
            "DELETE",
            item.as_str(),
            vec!["--id", ITEM],
            None,
            json!({}),
        ),
    ] {
        let mut mock = Mock::given(method(verb)).and(path(route));
        if let Some(body) = body {
            mock = mock.and(body_json(body));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&server)
            .await;
        let mut args = vec!["postgresql-database", command, "--workspace", WORKSPACE];
        args.extend(extra);
        mock_fabio(&server).args(args).assert().success();
        server.verify().await;
        server.reset().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgresql_get_definition_preserves_pgschema_parts() {
    let server = MockServer::start().await;
    let definition = json!({"format": "pgschema", "parts": [{"path": "definition.pgschema", "payload": "U0VMRUNUIDE7", "payloadType": "InlineBase64"}]});
    Mock::given(method("POST"))
        .and(path(format!(
            "/workspaces/{WORKSPACE}/postgreSQLDatabases/{ITEM}/getDefinition"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"definition": definition})))
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args([
            "--readonly",
            "postgresql-database",
            "get-definition",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["definition"], definition);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgresql_definition_replacement_preserves_format_and_rebuild_consent() {
    let server = MockServer::start().await;
    let definition = json!({"format": "pgschema", "parts": [{"path": "definition.pgschema", "payload": "U0VMRUNUIDE7", "payloadType": "InlineBase64"}]});
    Mock::given(method("POST"))
        .and(path(format!(
            "/workspaces/{WORKSPACE}/postgreSQLDatabases/{ITEM}/updateDefinition"
        )))
        .and(body_json(
            json!({"definition": definition, "options": {"allowTableRebuild": true}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args([
            "postgresql-database",
            "update-definition",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--content",
            "SELECT 1;",
            "--allow-table-rebuild",
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["destructive"], true);
    assert!(
        parse_json(&output)["data"]["warning"]
            .as_str()
            .unwrap()
            .contains("permanently lose")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgresql_lists_preserve_folder_filters_and_pagination() {
    let server = MockServer::start().await;
    let folder = "00000000-0000-0000-0000-000000000003";
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WORKSPACE}/postgreSQLDatabases")))
        .and(query_param("recursive", "false"))
        .and(query_param("rootFolderId", folder))
        .and(query_param("continuationToken", "page/2+="))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"value": [{"id": ITEM}], "continuationToken": "next"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args([
            "--readonly",
            "postgresql-database",
            "list",
            "--workspace",
            WORKSPACE,
            "--no-recursive",
            "--root-folder-id",
            folder,
            "--continuation-token",
            "page/2+=",
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["count"], 1);
    assert_eq!(parse_json(&output)["continuationToken"], "next");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgresql_mutations_respect_readonly_and_destructive_agent_previews() {
    let server = MockServer::start().await;
    for (command, extra) in [
        ("create", vec!["--name", "synthetic_db"]),
        ("update", vec!["--id", ITEM, "--description", "Changed"]),
        ("delete", vec!["--id", ITEM]),
        (
            "update-definition",
            vec!["--id", ITEM, "--content", "SELECT 1;"],
        ),
    ] {
        let mut args = vec![
            "--readonly",
            "postgresql-database",
            command,
            "--workspace",
            WORKSPACE,
        ];
        args.extend(extra);
        let output = mock_fabio(&server).args(&args).assert().failure();
        let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
        assert_eq!(error["error"]["code"], "READONLY_MODE");
        args.push("--dry-run");
        let output = mock_fabio(&server)
            .env("CLAUDECODE", "1")
            .args(args)
            .assert()
            .success();
        let data = parse_json(&output)["data"].clone();
        assert_eq!(data["dry_run"], true);
        assert_eq!(
            data["would_execute"],
            format!("postgresql-database {command}")
        );
        if matches!(command, "delete" | "update-definition") {
            assert_eq!(data["destructive"], true);
            assert!(data["agentNotice"].is_string());
        }
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgresql_invalid_metadata_is_rejected_before_network_even_in_dry_run() {
    let server = MockServer::start().await;
    for name in [String::new(), "a".repeat(64), "\u{1f600}".repeat(32)] {
        for command in ["create", "update"] {
            let mut args = vec![
                "postgresql-database",
                command,
                "--workspace",
                WORKSPACE,
                "--name",
                &name,
                "--dry-run",
            ];
            if command == "update" {
                args.extend(["--id", ITEM]);
            }
            let output = mock_fabio(&server).args(args).assert().failure();
            let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
            assert_eq!(error["error"]["code"], "INVALID_INPUT");
        }
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[test]
fn postgresql_creation_modes_are_mutually_exclusive() {
    for flags in [
        vec![
            "--originating-product",
            "MigrationTool",
            "--definition",
            "SELECT 1;",
        ],
        vec!["--null-creation-payload", "--definition", "SELECT 1;"],
    ] {
        fabio()
            .args([
                "postgresql-database",
                "create",
                "--workspace",
                WORKSPACE,
                "--name",
                "synthetic_db",
                "--dry-run",
            ])
            .args(flags)
            .assert()
            .failure()
            .code(2);
    }
}

#[test]
fn test_create_dry_run() {
    let assert = fabio()
        .args([
            "postgresql-database",
            "create",
            "--workspace",
            WORKSPACE,
            "--name",
            "dry-run-postgres",
            "--originating-product",
            "MigrationTool",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "postgresql-database create");
    assert_eq!(
        data["details"]["request"]["creationPayload"]["originatingProduct"],
        "MigrationTool"
    );
}

#[test]
fn test_update_definition_dry_run() {
    let assert = fabio()
        .args([
            "postgresql-database",
            "update-definition",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--content",
            "{}",
            "--allow-table-rebuild",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(
        data["would_execute"],
        "postgresql-database update-definition"
    );
    assert_eq!(
        data["details"]["request"]["options"]["allowTableRebuild"],
        true
    );
}
