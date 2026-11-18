mod common;

use common::{extract_data, fabio, mock_fabio, parse_json};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WORKSPACE: &str = "00000000-0000-0000-0000-000000000001";
const ITEM: &str = "00000000-0000-0000-0000-000000000002";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playbook_item_and_definition_contracts_preserve_the_public_format() {
    let server = MockServer::start().await;
    let base = format!("/workspaces/{WORKSPACE}/playbooks");
    let item = format!("{base}/{ITEM}");
    let definition = json!({"format": "PlaybookV1", "parts": [{"path": "PlaybookV1.json", "payload": "e30=", "payloadType": "InlineBase64"}]});
    let get_definition = format!("{item}/getDefinition");
    let update_definition = format!("{item}/updateDefinition");
    for (command, verb, route, extra, body, response) in [
        (
            "create",
            "POST",
            base.as_str(),
            vec!["--name", "Handbook", "--definition", "{}"],
            Some(json!({"displayName": "Handbook", "definition": definition})),
            json!({"id": ITEM}),
        ),
        (
            "list",
            "GET",
            base.as_str(),
            vec![],
            None,
            json!({"value": [{"id": ITEM}]}),
        ),
        (
            "show",
            "GET",
            item.as_str(),
            vec!["--id", ITEM],
            None,
            json!({"id": ITEM}),
        ),
        (
            "update",
            "PATCH",
            item.as_str(),
            vec!["--id", ITEM, "--description", "Revised"],
            Some(json!({"description": "Revised"})),
            json!({"id": ITEM}),
        ),
        (
            "get-definition",
            "POST",
            get_definition.as_str(),
            vec!["--id", ITEM, "--readonly"],
            None,
            json!({"definition": definition}),
        ),
        (
            "update-definition",
            "POST",
            update_definition.as_str(),
            vec!["--id", ITEM, "--content", "{}"],
            Some(json!({"definition": definition})),
            json!({}),
        ),
        (
            "get-user-preference",
            "GET",
            format!("{item}/userPreference").as_str(),
            vec!["--id", ITEM],
            None,
            json!({"active": true}),
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
        let mut args = vec!["playbook", command, "--workspace", WORKSPACE];
        args.extend(extra);
        mock_fabio(&server).args(args).assert().success();
        server.verify().await;
        server.reset().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playbook_creation_server_error_preserves_unknown_submission_and_never_replays() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WORKSPACE}/playbooks")))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "errorCode": "MiqArtifactLifecycleForwardingFailed", "message": "An error occurred while processing the operation"
        })))
        .expect(1).mount(&server).await;
    let output = mock_fabio(&server)
        .args([
            "playbook",
            "create",
            "--workspace",
            WORKSPACE,
            "--name",
            "synthetic_playbook",
        ])
        .assert()
        .failure();
    let error = common::parse_error(&output);
    assert_eq!(error["error"]["retriable"], false);
    assert_eq!(error["error"]["submission"]["state"], "unknown");
    assert_eq!(error["error"]["submission"]["workspaceId"], WORKSPACE);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[test]
#[ignore = "requires Playbook provisioning; creates only synthetic context in an owned fixture"]
fn playbook_section_group_context_lifecycle_live() {
    let fixture = common::ScopedTestItem::create("playbook", "playbook_test");
    let section = format!("{}/Section", fixture.name);
    let group = format!("{section}/_context/Group");
    let context = format!("{group}/context.md");
    fixture
        .command("put-section")
        .args(["--path", &section, "--description", "Synthetic section"])
        .assert()
        .success();
    let output = fixture
        .command("get-section")
        .args(["--path", &section, "--readonly"])
        .assert()
        .success();
    assert_eq!(
        parse_json(&output)["data"]["description"]
            .as_str()
            .unwrap()
            .trim_prefix("<<<UNTRUSTED>>>")
            .trim_suffix("<<<END_UNTRUSTED>>>"),
        "Synthetic section"
    );
    fixture
        .command("put-group")
        .args(["--path", &group])
        .assert()
        .success();
    fixture
        .command("put-context")
        .args([
            "--path",
            &context,
            "--summary",
            "Synthetic summary",
            "--content-type",
            "text/markdown",
            "--data",
            "IyBUZXN0",
        ])
        .assert()
        .success();
    let output = fixture
        .command("get-context")
        .args(["--path", &context, "--include-content", "--readonly"])
        .assert()
        .success();
    assert_eq!(
        parse_json(&output)["data"]["summary"]
            .as_str()
            .unwrap()
            .trim_prefix("<<<UNTRUSTED>>>")
            .trim_suffix("<<<END_UNTRUSTED>>>"),
        "Synthetic summary"
    );
    fixture
        .command("update-context")
        .args(["--path", &context, "--summary", "Revised summary"])
        .assert()
        .success();
    fixture
        .command("delete-context")
        .args(["--path", &context])
        .assert()
        .success();
    fixture
        .command("delete-group")
        .args(["--path", &group])
        .assert()
        .success();
    fixture
        .command("delete-section")
        .args(["--path", &section])
        .assert()
        .success();
    fixture.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn section_group_and_context_requests_match_the_resource_contract() {
    let server = MockServer::start().await;
    let base = format!("/workspaces/{WORKSPACE}/playbooks/{ITEM}/sections/Handbook/Section%20A");
    for (command, verb, kind, extra, body) in [
        (
            "put-section",
            "PUT",
            "section",
            vec!["--description", "Section description"],
            Some(json!({"description": "Section description"})),
        ),
        ("get-section", "GET", "section", vec![], None),
        (
            "put-group",
            "PUT",
            "group",
            vec![],
            Some(json!({"description": ""})),
        ),
        ("get-group", "GET", "group", vec![], None),
        (
            "put-context",
            "PUT",
            "context",
            vec![
                "--summary",
                "Synthetic summary",
                "--content-type",
                "text/markdown",
                "--data",
                "IyBUZXN0",
            ],
            Some(
                json!({"summary": "Synthetic summary", "content": {"type": "text/markdown", "data": "IyBUZXN0"}}),
            ),
        ),
        (
            "update-context",
            "PATCH",
            "context",
            vec!["--summary", "Revised summary"],
            Some(json!({"summary": "Revised summary"})),
        ),
        (
            "get-context",
            "GET",
            "context",
            vec!["--include-content"],
            None,
        ),
        ("delete-context", "DELETE", "context", vec![], None),
        ("delete-group", "DELETE", "group", vec!["--recursive"], None),
        (
            "delete-section",
            "DELETE",
            "section",
            vec!["--recursive"],
            None,
        ),
    ] {
        let mut mock = Mock::given(method(verb))
            .and(path(&base))
            .and(query_param("resource", kind));
        if let Some(body) = body {
            mock = mock.and(body_json(body));
        }
        if extra.contains(&"--recursive") {
            mock = mock.and(query_param("recursive", "true"));
        }
        if extra.contains(&"--include-content") {
            mock = mock.and(query_param("include", "content"));
        }
        mock.respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"summary": "Synthetic summary"})),
        )
        .expect(1)
        .mount(&server)
        .await;
        let mut command_args = vec![
            "playbook",
            command,
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--path",
            "Handbook/Section A",
        ];
        command_args.extend(extra);
        mock_fabio(&server).args(command_args).assert().success();
        server.verify().await;
        server.reset().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rename_encodes_the_source_header_and_sends_no_json_body() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path(format!(
            "/workspaces/{WORKSPACE}/playbooks/{ITEM}/sections/Handbook/New%20section"
        )))
        .and(query_param("resource", "section"))
        .and(header("x-ms-rename-source", "Handbook/Old%20section"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    mock_fabio(&server)
        .args([
            "playbook",
            "put-section",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--path",
            "Handbook/New section",
            "--rename-source",
            "Handbook/Old section",
        ])
        .assert()
        .success();
    assert_eq!(server.received_requests().await.unwrap()[0].body.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn section_roles_send_principal_and_role_without_changing_scope() {
    let server = MockServer::start().await;
    let principal = "00000000-0000-0000-0000-000000000003";
    let url = format!(
        "/workspaces/{WORKSPACE}/playbooks/{ITEM}/sections/roleAssignments/Handbook/Section"
    );
    for (command, verb, extra, body) in [
        (
            "add-section-role-assignment",
            "POST",
            vec![
                "--principal-id",
                principal,
                "--principal-type",
                "User",
                "--role",
                "curator",
            ],
            Some(json!({"principal": {"id": principal, "type": "User"}, "role": "Curator"})),
        ),
        (
            "update-section-role-assignment",
            "PATCH",
            vec!["--principal-id", principal, "--role", "user"],
            Some(json!({"role": "User"})),
        ),
        (
            "delete-section-role-assignment",
            "DELETE",
            vec!["--principal-id", principal],
            None,
        ),
    ] {
        let mut mock = Mock::given(method(verb)).and(path(&url));
        if verb != "POST" {
            mock = mock.and(query_param("principalId", principal));
        }
        if let Some(body) = body {
            mock = mock.and(body_json(body));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(json!({"role": "User"})))
            .expect(1)
            .mount(&server)
            .await;
        let mut args = vec![
            "playbook",
            command,
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--path",
            "Handbook/Section",
        ];
        args.extend(extra);
        mock_fabio(&server).args(args).assert().success();
        server.verify().await;
        server.reset().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_lists_preserve_continuation_tokens() {
    let server = MockServer::start().await;
    for (command, suffix) in [
        (
            "list-groups",
            "sections/Handbook/Section/_context?resource=contextRoot",
        ),
        (
            "list-section-role-assignments",
            "sections/roleAssignments/Handbook/Section/_context",
        ),
    ] {
        let (route, _) = suffix.split_once('?').unwrap_or((suffix, ""));
        Mock::given(method("GET"))
            .and(path(format!("/workspaces/{WORKSPACE}/playbooks/{ITEM}/{route}")))
            .and(query_param("continuationToken", "page/2+="))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [{"path": "Handbook/Section/_context/Group"}], "continuationToken": "page3"})))
            .expect(1).mount(&server).await;
        let output = mock_fabio(&server)
            .args([
                "--readonly",
                "playbook",
                command,
                "--workspace",
                WORKSPACE,
                "--id",
                ITEM,
                "--path",
                "Handbook/Section/_context",
                "--continuation-token",
                "page/2+=",
            ])
            .assert()
            .success();
        let response = parse_json(&output);
        assert_eq!(response["count"], 1);
        assert_eq!(response["continuationToken"], "page3");
        server.verify().await;
        server.reset().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preference_and_binary_content_roundtrip() {
    let server = MockServer::start().await;
    let base = format!("/workspaces/{WORKSPACE}/playbooks/{ITEM}");
    Mock::given(method("PATCH"))
        .and(path(format!("{base}/userPreference")))
        .and(body_json(json!({"active": false})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"active": false})))
        .expect(1)
        .mount(&server)
        .await;
    mock_fabio(&server)
        .args([
            "playbook",
            "update-user-preference",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--active",
            "false",
        ])
        .assert()
        .success();
    let bytes = b"PK\x03\x04\x00\xffsynthetic zip bytes";
    Mock::given(method("GET"))
        .and(path(format!(
            "{base}/sections/Handbook/Section/_context/Skill"
        )))
        .and(query_param("resource", "content"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.as_slice()))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("skill.zip");
    let output = mock_fabio(&server)
        .args([
            "--readonly",
            "playbook",
            "get-context-content",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--path",
            "Handbook/Section/_context/Skill",
            "--output-file",
            destination.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(std::fs::read(destination).unwrap(), bytes);
    assert_eq!(parse_json(&output)["data"]["bytes"], bytes.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_paths_summaries_and_cross_parent_renames_never_dispatch() {
    let server = MockServer::start().await;
    for invalid in [
        "",
        "/",
        "Handbook",
        "Handbook/../Other",
        "Handbook//Section",
    ] {
        for command in ["delete-section", "delete-group", "delete-context"] {
            let output = mock_fabio(&server)
                .args([
                    "playbook",
                    command,
                    "--workspace",
                    WORKSPACE,
                    "--id",
                    ITEM,
                    "--path",
                    invalid,
                    "--dry-run",
                ])
                .assert()
                .failure();
            let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
            assert_eq!(error["error"]["code"], "INVALID_INPUT");
        }
    }
    for summary in [String::new(), "é".repeat(1025)] {
        for command in ["put-context", "update-context"] {
            mock_fabio(&server)
                .args([
                    "playbook",
                    command,
                    "--workspace",
                    WORKSPACE,
                    "--id",
                    ITEM,
                    "--path",
                    "Handbook/Section/_context/Skill",
                    "--summary",
                    &summary,
                    "--dry-run",
                ])
                .assert()
                .failure();
        }
    }
    mock_fabio(&server)
        .args([
            "playbook",
            "put-section",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--path",
            "Handbook/Other/New",
            "--rename-source",
            "Handbook/Section/Old",
            "--dry-run",
        ])
        .assert()
        .failure();
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_mutations_are_blocked_by_readonly_and_destructive_previews_warn_agents() {
    let server = MockServer::start().await;
    for command in [
        "put-section",
        "delete-section",
        "delete-group",
        "delete-context",
    ] {
        let mut args = vec![
            "--readonly",
            "playbook",
            command,
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--path",
            "Handbook/Section",
        ];
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
        assert_eq!(data["would_execute"], format!("playbook {command}"));
        if command.starts_with("delete") {
            assert_eq!(data["destructive"], true);
            assert!(data["agentNotice"].is_string());
        }
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[test]
fn test_create_dry_run() {
    let assert = fabio()
        .args([
            "playbook",
            "create",
            "--workspace",
            WORKSPACE,
            "--name",
            "dry-run-playbook",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "playbook create");
    assert_eq!(data["dry_run"], true);
}

#[test]
fn test_delete_section_rejects_root_path() {
    fabio()
        .args([
            "playbook",
            "delete-section",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--path",
            "/",
            "--recursive",
            "--dry-run",
        ])
        .assert()
        .failure();
}

#[test]
fn test_put_context_dry_run() {
    let assert = fabio()
        .args([
            "playbook",
            "put-context",
            "--workspace",
            WORKSPACE,
            "--id",
            ITEM,
            "--path",
            "Sales/_context/overview",
            "--summary",
            "Sales context",
            "--content-type",
            "text/markdown",
            "--data",
            "IyBTYWxlcw==",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "playbook put-context");
    assert_eq!(
        data["details"]["request"]["content"]["type"],
        "text/markdown"
    );
}
