mod common;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provisioned_lists_use_beta_and_preserve_pagination() {
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let workspace = "00000000-0000-0000-0000-000000000001";
    let subscription = "00000000-0000-0000-0000-000000000002";
    for (command, resource) in [
        ("list-assets", "assets"),
        ("list-policies", "sharePolicies"),
    ] {
        Mock::given(method("GET"))
            .and(path(format!(
                "/workspaces/{workspace}/shareSubscriptions/{subscription}/{resource}"
            )))
            .and(query_param("beta", "true"))
            .and(query_param("continuationToken", "page/2+="))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"value": [{"id": subscription}], "continuationToken": "next"}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let output = common::mock_fabio(&server)
            .args([
                "--readonly",
                "share-subscription",
                command,
                "--workspace",
                workspace,
                "--id",
                subscription,
                "--continuation-token",
                "page/2+=",
            ])
            .assert()
            .success();
        assert_eq!(common::parse_json(&output)["count"], 1);
        assert_eq!(common::parse_json(&output)["continuationToken"], "next");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synchronize_uses_beta_and_is_blocked_by_readonly() {
    use serde_json::json;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let workspace = "00000000-0000-0000-0000-000000000001";
    let subscription = "00000000-0000-0000-0000-000000000002";
    let args = [
        "share-subscription",
        "synchronize",
        "--workspace",
        workspace,
        "--id",
        subscription,
    ];
    let output = common::mock_fabio(&server)
        .arg("--readonly")
        .args(args)
        .assert()
        .failure();
    let error: serde_json::Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "READONLY_MODE");
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
    Mock::given(method("POST"))
        .and(path(format!(
            "/workspaces/{workspace}/shareSubscriptions/{subscription}/synchronize"
        )))
        .and(query_param("beta", "true"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": subscription})))
        .expect(1)
        .mount(&server)
        .await;
    common::mock_fabio(&server).args(args).assert().success();
}

use common::{extract_data, fabio, parse_json};

#[test]
fn test_share_subscription_create_dry_run() {
    let assert = fabio()
        .args([
            "share-subscription",
            "create",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--name",
            "dry-run-subscription",
            "--invitation-url",
            "https://app.fabric.microsoft.com/share/invite/example",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share-subscription create");
    assert_eq!(
        data["details"]["request"]["creationPayload"]["invitationUrl"],
        "https://app.fabric.microsoft.com/share/invite/example"
    );
}

#[test]
fn test_share_subscription_delete_dry_run() {
    let assert = fabio()
        .args([
            "share-subscription",
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
    assert_eq!(data["would_execute"], "share-subscription delete");
    assert_eq!(data["dry_run"], true);
}

#[test]
fn test_share_subscription_synchronize_dry_run() {
    let assert = fabio()
        .args([
            "share-subscription",
            "synchronize",
            "--workspace",
            "00000000-0000-0000-0000-000000000001",
            "--id",
            "00000000-0000-0000-0000-000000000010",
            "--dry-run",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "share-subscription synchronize");
    assert_eq!(
        data["details"]["id"],
        "00000000-0000-0000-0000-000000000010"
    );
    assert_eq!(data["destructive"], true);
}
