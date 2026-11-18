mod common;

use common::{fabio, mock_fabio, parse_json};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retrieve_is_a_readonly_post_and_preserves_ranked_context() {
    let server = MockServer::start().await;
    let results = json!([{
        "identifier": "synthetic-context-digest", "rank": 0, "score": null,
        "lexicalMatch": null, "type": "application/agent-instructions", "tokenCount": 3,
        "data": {"summary": "Synthetic context", "uri": null}
    }]);
    Mock::given(method("POST"))
        .and(path("/microsoftIQ/retrieve"))
        .and(body_json(
            json!({"query": {"text": "synthetic context"}, "tokenBudget": 64}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": results})))
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args([
            "--readonly",
            "microsoft-iq",
            "retrieve",
            "--text",
            "synthetic context",
            "--token-budget",
            "64",
        ])
        .assert()
        .success();
    let response = parse_json(&output);
    assert_eq!(response["data"], results);
    assert_eq!(response["count"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_retrieval_and_out_of_scope_are_successful_results() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/microsoftIQ/retrieve"))
        .and(body_json(
            json!({"query": {"text": "no context"}, "tokenBudget": 2048}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/microsoftIQ/amIInScope"))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"inScope": false})))
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args([
            "--readonly",
            "microsoft-iq",
            "retrieve",
            "--text",
            "no context",
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output), json!({"data": [], "count": 0}));
    let output = mock_fabio(&server)
        .args(["--readonly", "microsoft-iq", "am-i-in-scope"])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["inScope"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retrieve_rejects_empty_text_before_network_dispatch() {
    let server = MockServer::start().await;
    let output = mock_fabio(&server)
        .args(["microsoft-iq", "retrieve", "--text", ""])
        .assert()
        .failure();
    let error = common::parse_error(&output);
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retrieve_forbidden_is_not_misreported_as_empty_context() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/microsoftIQ/retrieve"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "errorCode": "InsufficientPrivileges", "message": "Permission is required"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args(["microsoft-iq", "retrieve", "--text", "context"])
        .assert()
        .failure()
        .code(4);
    let error = common::parse_error(&output);
    assert_eq!(error["error"]["code"], "FORBIDDEN");
    assert_eq!(output.get_output().stdout.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_retrieve_response_is_not_misreported_as_empty_context() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/microsoftIQ/retrieve"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args(["microsoft-iq", "retrieve", "--text", "context"])
        .assert()
        .failure();
    assert_eq!(common::parse_error(&output)["error"]["code"], "API_ERROR");
    assert_eq!(output.get_output().stdout.len(), 0);
}

#[test]
fn test_retrieve_rejects_zero_token_budget() {
    fabio()
        .args([
            "microsoft-iq",
            "retrieve",
            "--text",
            "quarterly risks",
            "--token-budget",
            "0",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires a Fabric tenant with Microsoft IQ context"]
fn test_am_i_in_scope_live() {
    let output = fabio()
        .args([
            "--wrap-untrusted",
            "--readonly",
            "microsoft-iq",
            "am-i-in-scope",
        ])
        .assert()
        .success();
    assert!(parse_json(&output)["data"]["inScope"].is_boolean());
}

#[test]
#[ignore = "requires Fabric credentials; permission-filtered retrieval, no context creation"]
fn retrieve_live_allows_empty_permitted_results() {
    let output = fabio()
        .args([
            "--wrap-untrusted",
            "--readonly",
            "microsoft-iq",
            "retrieve",
            "--text",
            "Synthetic validation context",
            "--token-budget",
            "64",
        ])
        .assert()
        .success();
    let response = parse_json(&output);
    let results = response["data"].as_array().unwrap();
    assert_eq!(response["count"], results.len());
    assert!(results.len() <= 50);
    for result in results {
        assert!(result["identifier"].is_string());
        assert!(result["data"]["summary"].is_string());
    }
}
