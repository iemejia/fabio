//! Admin hygiene is a bounded review plan, not a deletion/ownership executor.
mod common;

use common::{mock_fabio, parse_json};
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const A: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const B: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const PRINCIPAL: &str = "cccccccc-1111-2222-3333-444444444444";
const ASSIGNMENT: &str = "dddddddd-1111-2222-3333-444444444444";
const GATEWAY: &str = "eeeeeeee-1111-2222-3333-444444444444";

fn command(server: &MockServer) -> assert_cmd::Command {
    let mut command = mock_fabio(server);
    command.args(["--readonly", "admin", "connection-hygiene"]);
    command
}
fn metadata(connection: &str, events: bool) -> Value {
    let recency = if events {
        json!({"createdDateTime":"2026-06-01T00:00:00Z","lastBoundDateTime":"2026-10-01T00:00:00Z","lastCredentialUsedDateTime":"2026-11-01T00:00:00Z"})
    } else {
        json!({"createdDateTime":"2026-06-01T00:00:00Z","lastBoundDateTime":null,"lastCredentialUsedDateTime":null})
    };
    json!({"id":connection,"displayName":"private_name","connectivityType":"ShareableCloud","gatewayId":GATEWAY,
        "connectionDetails":{"type":"Web","path":"https://example.org/private?secret=never-emit"},
        "credentialDetails":{"credentialType":"Anonymous","privateCredential":"never-emit"},"myRole":"Owner","connectionRecency":recency})
}
fn role(kind: &str, role: &str) -> Value {
    json!({"id":ASSIGNMENT,"role":role,"principal":{"id":PRINCIPAL,"type":kind,"displayName":"private_principal"}})
}
async fn get(server: &MockServer, route: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}
async fn connection(server: &MockServer, connection: &str, body: Value, roles: Value) {
    get(server, &format!("/admin/connections/{connection}"), body).await;
    get(
        server,
        &format!("/admin/connections/{connection}/roleAssignments"),
        json!({"value":roles}),
    )
    .await;
}
async fn reads_only(server: &MockServer, count: usize) {
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), count);
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET"
                && request.url.path().starts_with("/admin/connections"))
    );
}
fn finding<'a>(data: &'a Value, key: &str) -> &'a Value {
    data["connections"][0]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["checkId"] == key)
        .unwrap()
}
fn actions(data: &Value) -> &[Value] {
    data["reviewPlan"]["actions"].as_array().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_scope_and_offline_preview_are_validated_without_inventory_or_mutations() {
    let server = MockServer::start().await;
    let output = command(&server)
        .args(["--id", A, "--dry-run"])
        .assert()
        .success();
    assert_eq!(
        parse_json(&output)["data"]["would_execute"],
        "admin connection-hygiene"
    );
    assert_eq!(
        parse_json(&output)["data"]["details"]["scope"]["mutates"],
        false
    );
    for args in [
        vec![],
        vec!["--id", "../connections"],
        vec!["--id", "00000000-0000-0000-0000-000000000000"],
        vec!["--id", A, "--id", A],
        vec!["--id", A, "--inventory"],
        vec!["--id", A, "--id", B, "--connection-limit", "1"],
        vec!["--inventory", "--continuation-token", "private"],
        vec!["--id", A, "--created-after", "2026-01-01"],
        vec!["--id", A, "--unused-days", "4294967295"],
    ] {
        command(&server)
            .args(args)
            .arg("--dry-run")
            .assert()
            .failure();
    }
    reads_only(&server, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recency_and_single_owner_signals_produce_only_guarded_review_previews() {
    let server = MockServer::start().await;
    connection(
        &server,
        A,
        metadata(A, false),
        json!([role("User", "Owner")]),
    )
    .await;
    let output = command(&server).args(["--id", A]).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "needs_action");
    assert_eq!(data["mutates"], false);
    assert_eq!(data["destructive"], true);
    assert_eq!(data["reviewPlan"]["applied"], false);
    assert_eq!(data["reviewPlan"]["readyToApply"], false);
    assert_eq!(
        finding(data, "connection_ownership")["evidence"]["reportedOwnerPrincipalCount"],
        1
    );
    assert_eq!(actions(data).len(), 2);
    for action in actions(data) {
        assert_eq!(action["requiresApproval"], true);
        assert_eq!(action["readyToApply"], false);
        let preview = &action["preview"];
        assert_eq!(preview["requiresApproval"], true);
        let text = preview["command"]
            .as_str()
            .or_else(|| preview["commandTemplate"].as_str())
            .unwrap();
        assert!(text.contains("--readonly"));
        assert!(text.contains("--dry-run"));
        let args = shlex::split(&text.replace("$DURABLE_OWNER_SP_ID", PRINCIPAL)).unwrap();
        let output = mock_fabio(&server).args(&args[1..]).assert().success();
        let preview = &parse_json(&output)["data"];
        assert_eq!(preview["dry_run"], true);
        assert_eq!(preview["details"]["connectionId"], A);
        assert_eq!(
            preview["would_execute"],
            if action["destructive"] == true {
                "admin delete-connection"
            } else {
                "admin add-connection-role-assignment"
            }
        );
    }
    let text = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(!text.contains("private_name"));
    assert!(!text.contains("example.org"));
    assert!(!text.contains("never-emit"));
    assert!(!text.contains("private_principal"));
    reads_only(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_malformed_and_old_recency_is_unknown_not_unused_or_retirement_approval() {
    for recency in [
        json!({}),
        json!({"createdDateTime":"2026-06-01T00:00:00Z"}),
        json!({"createdDateTime":"2026-06-01T00:00:00Z","lastBoundDateTime":"private-bad","lastCredentialUsedDateTime":null}),
        json!({"createdDateTime":"2024-01-01T00:00:00Z","lastBoundDateTime":null,"lastCredentialUsedDateTime":null}),
    ] {
        let server = MockServer::start().await;
        let mut body = metadata(A, false);
        body["connectionRecency"] = recency;
        connection(&server, A, body, json!([role("Group", "Owner")])).await;
        let output = command(&server)
            .args(["--id", A, "--strict"])
            .assert()
            .failure();
        let data = &parse_json(&output)["data"];
        assert_eq!(finding(data, "connection_recency")["status"], "unknown");
        assert_eq!(actions(data).len(), 0);
        assert_eq!(data["partial"], true);
        assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("private-bad"));
        reads_only(&server, 2).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_owner_suppresses_deletion_and_grant_previews_even_with_recency_review_signals() {
    let server = MockServer::start().await;
    let mut body = metadata(A, false);
    body["myRole"] = json!("PendingOwner");
    connection(&server, A, body, json!([role("User", "PendingOwner")])).await;
    let output = command(&server).args(["--id", A]).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(
        finding(data, "connection_ownership")["evidence"]["reportedOwnerPrincipalCount"],
        0
    );
    assert_eq!(
        finding(data, "connection_ownership")["evidence"]["pendingOwnersObserved"],
        1
    );
    assert_eq!(actions(data).len(), 1);
    assert_eq!(actions(data)[0]["kind"], "review_pending_ownership");
    assert_eq!(actions(data)[0]["preview"], Value::Null);
    assert_eq!(data["destructive"], false);
    reads_only(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_or_unrecognized_role_data_never_establishes_owners_or_permission_change_previews()
{
    for (rows, token) in [
        (json!([role("User", "FutureOwner")]), None),
        (json!([role("User", "Owner")]), Some("more")),
    ] {
        let server = MockServer::start().await;
        get(
            &server,
            &format!("/admin/connections/{A}"),
            metadata(A, false),
        )
        .await;
        let body = token.map_or_else(
            || json!({"value":rows}),
            |token| json!({"value":rows,"continuationToken":token}),
        );
        get(
            &server,
            &format!("/admin/connections/{A}/roleAssignments"),
            body,
        )
        .await;
        let output = command(&server)
            .args(["--id", A, "--page-limit", "1", "--role-limit", "1", "--all"])
            .assert()
            .success();
        let data = &parse_json(&output)["data"];
        assert_eq!(finding(data, "connection_ownership")["status"], "unknown");
        assert_eq!(
            finding(data, "connection_ownership")["evidence"]["reportedOwnerPrincipalCount"],
            Value::Null
        );
        assert_eq!(actions(data).len(), 0);
        reads_only(&server, 2).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_inventory_uses_admin_tokens_not_returned_regular_or_arbitrary_urls() {
    let server = MockServer::start().await;
    let token = "opaque?&private";
    Mock::given(method("GET")).and(path("/admin/connections")).and(query_param_is_missing("continuationToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value":[metadata(A,true)],"continuationToken":token,"continuationUri":"https://example.org/private"}))).expect(1).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/admin/connections"))
        .and(query_param("continuationToken", token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value":[metadata(B,true)]})))
        .expect(1)
        .mount(&server)
        .await;
    for id in [A, B] {
        get(
            &server,
            &format!("/admin/connections/{id}/roleAssignments"),
            json!({"value":[role("Group","Owner")]}),
        )
        .await;
    }
    let output = command(&server)
        .args(["--inventory", "--connection-limit", "2"])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["connections"].as_array().unwrap().len(), 2);
    assert_eq!(data["status"], "needs_action");
    let similarity = data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["checkId"] == "target_similarity")
        .unwrap();
    assert_eq!(
        similarity["evidence"]["groups"][0]["winner"],
        "not_selected"
    );
    assert_eq!(
        similarity["evidence"]["groups"][0]["credentialEquivalence"],
        "not_checked"
    );
    assert_eq!(actions(data).len(), 0);
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("keepId"));
    reads_only(&server, 4).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_inventory_stays_partial_despite_all_or_force() {
    let server = MockServer::start().await;
    get(
        &server,
        "/admin/connections",
        json!({"value":[metadata(A,true),metadata(B,true)],"continuationToken":"more"}),
    )
    .await;
    get(
        &server,
        &format!("/admin/connections/{A}/roleAssignments"),
        json!({"value":[role("Group","Owner")]}),
    )
    .await;
    let output = command(&server)
        .args([
            "--inventory",
            "--connection-limit",
            "1",
            "--all",
            "--force",
            "--strict",
        ])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["partial"], true);
    assert_eq!(data["connections"].as_array().unwrap().len(), 1);
    reads_only(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forbidden_or_wrong_scope_admin_metadata_does_not_fallback_or_probe_roles() {
    for status in [200, 403] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/admin/connections/{A}")))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(if status == 200 {
                    metadata(B, false)
                } else {
                    json!({"errorCode":"Forbidden","message":"private"})
                }),
            )
            .expect(1)
            .mount(&server)
            .await;
        let output = command(&server)
            .args(["--id", A, "--strict"])
            .assert()
            .failure();
        let data = &parse_json(&output)["data"];
        assert_eq!(data["status"], "unknown");
        assert_eq!(actions(data).len(), 0);
        reads_only(&server, 1).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn error_bearing_or_wrong_scope_role_projection_cannot_produce_a_retirement_preview() {
    for body in [
        json!({"value":[],"error":{"message":"private"}}),
        json!({"value":[role("User", "Owner")],"errorCode":"private"}),
        json!({"connectionId":B,"value":[role("User","Owner")]}),
        json!({"value":[{"id":ASSIGNMENT,"role":"Owner","principal":{"id":PRINCIPAL,"type":"User"},"connectionId":B}]}),
        json!({"value":[{"id":ASSIGNMENT,"role":"Owner","principal":{"id":PRINCIPAL,"type":"User"},"errorCode":"private"}]}),
    ] {
        let server = MockServer::start().await;
        get(
            &server,
            &format!("/admin/connections/{A}"),
            metadata(A, false),
        )
        .await;
        get(
            &server,
            &format!("/admin/connections/{A}/roleAssignments"),
            body,
        )
        .await;
        let output = command(&server).args(["--id", A]).assert().success();
        let data = &parse_json(&output)["data"];
        assert_eq!(finding(data, "connection_ownership")["status"], "unknown");
        assert_eq!(actions(data).len(), 0);
        reads_only(&server, 2).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn total_deadline_includes_inflight_admin_reads_and_skips_later_probes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/admin/connections/{A}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(3))
                .set_body_json(metadata(A, false)),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&server)
        .args(["--id", A, "--timeout", "1", "--strict"])
        .timeout(std::time::Duration::from_secs(8))
        .assert()
        .failure();
    assert_eq!(parse_json(&output)["data"]["status"], "unknown");
    reads_only(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn healthy_admin_metadata_can_be_review_ready_without_caller_role_or_credential_claims() {
    let server = MockServer::start().await;
    let mut body = metadata(A, true);
    body.as_object_mut().unwrap().remove("myRole");
    connection(&server, A, body, json!([role("Group", "Owner")])).await;
    let output = command(&server)
        .args(["--id", A, "--strict"])
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "ready");
    assert_eq!(data["connections"][0]["credentialHealth"], "not_checked");
    assert_eq!(actions(data).len(), 0);
    assert_eq!(data["reviewPlan"]["readyToApply"], false);
    assert_eq!(data["destructive"], false);
    reads_only(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_inventory_identity_never_produces_ownership_or_retirement_previews() {
    let server = MockServer::start().await;
    get(
        &server,
        "/admin/connections",
        json!({"value":[metadata(A,false),metadata(A,true)]}),
    )
    .await;
    let output = command(&server)
        .args(["--inventory", "--strict"])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "unknown");
    assert_eq!(actions(data).len(), 0);
    reads_only(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conflicting_role_identity_is_unknown_and_suppresses_permission_previews() {
    let server = MockServer::start().await;
    let mut second = role("User", "Owner");
    second["principal"]["id"] = json!(B);
    connection(
        &server,
        A,
        metadata(A, false),
        json!([role("User", "Owner"), second]),
    )
    .await;
    let output = command(&server).args(["--id", A]).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(finding(data, "connection_ownership")["status"], "unknown");
    assert_eq!(actions(data).len(), 0);
    reads_only(&server, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cyclic_role_tokens_and_uri_only_inventory_do_not_become_complete() {
    let server = MockServer::start().await;
    get(
        &server,
        &format!("/admin/connections/{A}"),
        metadata(A, false),
    )
    .await;
    let route = format!("/admin/connections/{A}/roleAssignments");
    Mock::given(method("GET"))
        .and(path(&route))
        .and(query_param_is_missing("continuationToken"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"value":[role("User","Owner")],"continuationToken":"again"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(&route))
        .and(query_param("continuationToken", "again"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"value":[],"continuationToken":"again"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&server)
        .args(["--id", A, "--strict"])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(
        finding(data, "connection_ownership")["evidence"]["scan"]["reason"],
        "repeated_continuation"
    );
    assert_eq!(actions(data).len(), 0);
    reads_only(&server, 3).await;

    let server = MockServer::start().await;
    get(
        &server,
        "/admin/connections",
        json!({"value":[],"continuationUri":"https://example.org/private"}),
    )
    .await;
    let output = command(&server)
        .args(["--inventory", "--strict"])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "unknown");
    assert_eq!(
        data["checks"][0]["evidence"]["reason"],
        "uri_only_continuation"
    );
    assert_eq!(data["checks"][1]["status"], "unknown");
    assert_eq!(actions(data).len(), 0);
    reads_only(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conflicting_caller_owner_and_unknown_principal_never_certify_an_owner_count() {
    for rows in [json!([]), json!([role("FuturePrincipal", "Owner")])] {
        let server = MockServer::start().await;
        connection(&server, A, metadata(A, false), rows).await;
        let output = command(&server).args(["--id", A]).assert().success();
        let data = &parse_json(&output)["data"];
        assert_eq!(finding(data, "connection_ownership")["status"], "unknown");
        assert_eq!(
            finding(data, "connection_ownership")["evidence"]["reportedOwnerPrincipalCount"],
            Value::Null
        );
        assert_eq!(actions(data).len(), 0);
        reads_only(&server, 2).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn personal_and_unknown_types_do_not_offer_runnable_owner_templates() {
    for kind in ["PersonalCloud", "FutureConnectivity"] {
        let server = MockServer::start().await;
        let mut body = metadata(A, true);
        body["connectivityType"] = json!(kind);
        connection(&server, A, body, json!([role("User", "Owner")])).await;
        let output = command(&server).args(["--id", A]).assert().success();
        let data = &parse_json(&output)["data"];
        assert_eq!(actions(data).len(), 1);
        assert_eq!(actions(data)[0]["kind"], "review_owner_continuity");
        assert_eq!(actions(data)[0]["preview"], Value::Null);
        reads_only(&server, 2).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conflicting_principal_roles_with_different_assignments_suppress_previews() {
    let server = MockServer::start().await;
    let mut second = role("User", "UserWithReshare");
    second["id"] = json!(B);
    connection(
        &server,
        A,
        metadata(A, false),
        json!([role("User", "Owner"), second]),
    )
    .await;
    let output = command(&server).args(["--id", A]).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(finding(data, "connection_ownership")["status"], "unknown");
    assert_eq!(
        finding(data, "connection_ownership")["evidence"]["reportedOwnerPrincipalCount"],
        Value::Null
    );
    assert_eq!(actions(data).len(), 0);
    reads_only(&server, 2).await;
}
