mod common;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn share_bulk_requests_use_beta_and_preserve_typed_payloads() {
    use serde_json::json;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let ws = "00000000-0000-0000-0000-000000000001";
    let share = "00000000-0000-0000-0000-000000000002";
    let policy = "00000000-0000-0000-0000-000000000003";
    let item = json!({"referenceType": "ById", "itemId": share, "workspaceId": ws});
    let recipient = json!({"type": "User", "email": "synthetic@example.com"});
    let properties = json!({"expirationDateTime": "2027-12-31T23:59:59Z"});
    let asset = json!({"item": item, "shareAs": "Self"});
    let rule = json!({"type": "ShareExpiration", "properties": properties});
    for (command, suffix, request) in [
        (
            "initialize",
            "initializeShare".to_string(),
            json!({"assets": [asset], "recipients": [recipient], "sharePolicies": [rule]}),
        ),
        (
            "bulk-add-assets",
            "assets/bulkAdd".to_string(),
            json!({"assets": [asset]}),
        ),
        (
            "bulk-remove-assets",
            "assets/bulkRemove".to_string(),
            json!({"items": [item]}),
        ),
        (
            "bulk-add-recipients",
            "recipients/bulkAdd".to_string(),
            json!({"recipients": [recipient]}),
        ),
        (
            "bulk-remove-recipients",
            "recipients/bulkRemove".to_string(),
            json!({"recipients": [recipient]}),
        ),
        (
            "bulk-add-policies",
            "sharePolicies/bulkAdd".to_string(),
            json!({"sharePolicies": [rule]}),
        ),
        (
            "update-policy",
            format!("sharePolicies/{policy}/update"),
            json!({"id": policy, "type": "ShareExpiration", "properties": properties}),
        ),
        (
            "bulk-remove-policies",
            "sharePolicies/bulkRemove".to_string(),
            json!({"sharePolicyIds": [policy]}),
        ),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/workspaces/{ws}/shares/{share}/{suffix}")))
            .and(query_param("beta", "true"))
            .and(body_json(&request))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": share})))
            .expect(1)
            .mount(&server)
            .await;
        let raw = request.to_string();
        let mut args = vec![
            "share",
            command,
            "--workspace",
            ws,
            "--id",
            share,
            "--content",
            &raw,
        ];
        if command == "update-policy" {
            args.extend(["--policy-id", policy]);
        }
        let output = common::mock_fabio(&server).args(args).assert().success();
        assert_eq!(common::parse_json(&output)["data"]["id"], share);
        server.verify().await;
        server.reset().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn share_content_lists_and_preview_preserve_scope_and_pagination() {
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let ws = "00000000-0000-0000-0000-000000000001";
    let share = "00000000-0000-0000-0000-000000000002";
    for (command, resource) in [
        ("list-assets", "assets"),
        ("list-recipients", "recipients"),
        ("list-policies", "sharePolicies"),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/workspaces/{ws}/shares/{share}/{resource}")))
            .and(query_param("beta", "true"))
            .and(query_param("continuationToken", "page/2+="))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"value": [{"id": share}], "continuationToken": "next"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let output = common::mock_fabio(&server)
            .args([
                "--readonly",
                "share",
                command,
                "--workspace",
                ws,
                "--id",
                share,
                "--continuation-token",
                "page/2+=",
            ])
            .assert()
            .success();
        assert_eq!(common::parse_json(&output)["continuationToken"], "next");
    }
    let tenant = "00000000-0000-0000-0000-000000000003";
    Mock::given(method("GET"))
        .and(path(format!(
            "/tenants/{tenant}/workspaces/{ws}/shares/{share}/preview"
        )))
        .and(query_param("beta", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"shareId": share})))
        .expect(1)
        .mount(&server)
        .await;
    common::mock_fabio(&server)
        .args([
            "--readonly",
            "share",
            "preview",
            "--producer-tenant",
            tenant,
            "--producer-workspace",
            ws,
            "--producer-share",
            share,
        ])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_share_batches_fail_before_network_including_dry_run() {
    let server = wiremock::MockServer::start().await;
    let ws = "00000000-0000-0000-0000-000000000001";
    let share = "00000000-0000-0000-0000-000000000002";
    for (command, body) in [
        ("bulk-add-assets", r#"{"assets":[]}"#),
        ("bulk-remove-assets", r#"{"items":[]}"#),
        ("bulk-add-recipients", r#"{"recipients":[]}"#),
        (
            "bulk-remove-recipients",
            r#"{"recipients":[{"type":"User"}]}"#,
        ),
        ("bulk-add-policies", r#"{"sharePolicies":[]}"#),
        (
            "bulk-remove-policies",
            r#"{"sharePolicyIds":["not-a-uuid"]}"#,
        ),
        (
            "bulk-add-assets",
            r#"{"assets":[{"item":{"referenceType":"ByName","itemId":"unknown","workspaceId":"unknown"}}]}"#,
        ),
        (
            "bulk-add-recipients",
            r#"{"recipients":[{"type":"User","email":"a@example.com","userPrincipalName":"b@example.com"}]}"#,
        ),
        (
            "bulk-add-policies",
            r#"{"sharePolicies":[{"type":"Unknown","properties":{}}]}"#,
        ),
    ] {
        let output = common::mock_fabio(&server)
            .args([
                "share",
                command,
                "--workspace",
                ws,
                "--id",
                share,
                "--content",
                body,
                "--dry-run",
            ])
            .assert()
            .failure();
        let error: serde_json::Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
        assert_eq!(error["error"]["code"], "INVALID_INPUT", "{command}");
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn share_mutations_honor_readonly_without_dispatch() {
    let server = wiremock::MockServer::start().await;
    for (command, content) in [
        (
            "bulk-add-recipients",
            r#"{"recipients":[{"type":"User","email":"synthetic@example.com"}]}"#,
        ),
        (
            "bulk-remove-recipients",
            r#"{"recipients":[{"type":"User","email":"synthetic@example.com"}]}"#,
        ),
        (
            "bulk-add-policies",
            r#"{"sharePolicies":[{"type":"ShareExpiration","properties":{"expirationDateTime":"2027-12-31T23:59:59Z"}}]}"#,
        ),
        (
            "bulk-remove-policies",
            r#"{"sharePolicyIds":["00000000-0000-0000-0000-000000000003"]}"#,
        ),
    ] {
        let output = common::mock_fabio(&server)
            .args([
                "--readonly",
                "share",
                command,
                "--workspace",
                "00000000-0000-0000-0000-000000000001",
                "--id",
                "00000000-0000-0000-0000-000000000002",
                "--content",
                content,
            ])
            .assert()
            .failure();
        let error: serde_json::Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
        assert_eq!(error["error"]["code"], "READONLY_MODE");
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

use common::{extract_data, fabio, parse_json};

#[test]
fn test_share_create_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "create",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--name",
            "dry-run-share",
            "--definition-content",
            r#"{"assets":[],"recipients":[]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share create");
    assert_eq!(data["dry_run"], true);
}

#[test]
fn test_share_delete_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "delete",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share delete");
    assert_eq!(data["dry_run"], true);
}

#[test]
fn test_share_update_definition_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "update-definition",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--content",
            r#"{"assets":[],"recipients":[]}"#,
            "--update-metadata",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share update-definition");
    assert_eq!(data["details"]["updateMetadata"], true);
}

#[test]
fn test_share_initialize_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "initialize",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"assets":[{"item":{"referenceType":"ById","itemId":"00000000-0000-0000-0000-000000000011","workspaceId":"00000000-0000-0000-0000-000000000001"},"shareAs":"MirroredOneLake"}],"recipients":[{"type":"User","email":"recipient@example.com"}],"sharePolicies":[{"type":"ShareExpiration","properties":{"expirationDateTime":"2027-12-31T23:59:59Z"}}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share initialize");
    assert_eq!(
        data["details"]["request"]["assets"][0]["shareAs"],
        "MirroredOneLake"
    );
    assert_eq!(data["details"]["request"]["recipients"][0]["type"], "User");
}

#[test]
fn test_share_bulk_remove_assets_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-remove-assets",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"items":[{"referenceType":"ById","itemId":"00000000-0000-0000-0000-000000000011","workspaceId":"00000000-0000-0000-0000-000000000001"}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-remove-assets");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["destructive"], true);
}

#[test]
fn test_share_bulk_add_assets_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-add-assets",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"assets":[{"item":{"referenceType":"ById","itemId":"00000000-0000-0000-0000-000000000011","workspaceId":"00000000-0000-0000-0000-000000000001"},"shareAs":"Self"}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-add-assets");
    assert_eq!(data["details"]["request"]["assets"][0]["shareAs"], "Self");
}

#[test]
fn test_share_bulk_add_recipients_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-add-recipients",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"recipients":[{"type":"ServicePrincipal","principalId":"00000000-0000-0000-0000-000000000012","tenantId":"00000000-0000-0000-0000-000000000013"}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-add-recipients");
    assert_eq!(
        data["details"]["request"]["recipients"][0]["type"],
        "ServicePrincipal"
    );
}

#[test]
fn test_share_bulk_remove_recipients_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-remove-recipients",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"recipients":[{"type":"User","email":"recipient@example.com"}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-remove-recipients");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["destructive"], true);
}

#[test]
fn test_share_bulk_add_policies_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-add-policies",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"sharePolicies":[{"type":"ShareExpiration","properties":{"expirationDateTime":"2027-12-31T23:59:59Z"}}]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-add-policies");
    assert_eq!(
        data["details"]["request"]["sharePolicies"][0]["type"],
        "ShareExpiration"
    );
}

#[test]
fn test_share_update_policy_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "update-policy",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--policy-id",
            "00000000-0000-0000-0000-000000000014",
            "--content",
            r#"{"id":"00000000-0000-0000-0000-000000000014","type":"ShareExpiration","properties":{"expirationDateTime":"2027-12-31T23:59:59Z"}}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share update-policy");
    assert_eq!(
        data["details"]["request"]["properties"]["expirationDateTime"],
        "2027-12-31T23:59:59Z"
    );
    assert_eq!(data["destructive"], true);
}

#[test]
fn test_share_bulk_remove_policies_dry_run() {
    let assert = fabio()
        .args([
            "share",
            "bulk-remove-policies",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--content",
            r#"{"sharePolicyIds":["00000000-0000-0000-0000-000000000014"]}"#,
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share bulk-remove-policies");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["destructive"], true);
}
