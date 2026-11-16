//! Known connection-policy failures teach tenant-admin review without extra calls.
mod common;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_errors_remain_non_retriable_without_admin_or_identity_fallbacks() {
    for (status, marker, tab) in [
        (
            403,
            "DMTS_BlockedByAuthenticationKindPolicies",
            "Authentication allowlist",
        ),
        (
            400,
            "This operation was blocked due to inter-tenant data loss prevention policies.",
            "Tenant allowlist",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/connections/00000000-0000-0000-0000-000000000001"))
            .respond_with(ResponseTemplate::new(status).set_body_json(
                json!({"errorCode":"ConnectionFailed", "message":"synthetic connection failure",
                "moreDetails":[{"errorCode":marker, "message":marker}]}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let result = common::mock_fabio(&server)
            .args([
                "--quiet",
                "--readonly",
                "connection",
                "show",
                "--id",
                "00000000-0000-0000-0000-000000000001",
            ])
            .assert()
            .failure();
        let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
        assert_eq!(error["error"]["retriable"], false);
        assert!(error["error"]["hint"].as_str().unwrap().contains(tab));
        assert!(
            error["error"]["hint"]
                .as_str()
                .unwrap()
                .contains("Do not retry automatically")
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[test]
#[ignore = "requires live Fabric credentials; read-only existing connection list, no policy changes or credential probes"]
fn live_connection_metadata_read_does_not_infer_allowlist_state() {
    let output = common::fabio()
        .args(["--readonly", "connection", "list", "--limit", "1"])
        .output()
        .expect("run fabio");
    assert!(
        output.status.success(),
        "live connection read failed; inspect private diagnostics"
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON response");
    assert!(
        value["data"].is_array(),
        "expected bounded connection metadata, not policy readiness"
    );
}
