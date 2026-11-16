//! Native Iceberg RPCs retain assertions and never broaden scope or replay writes.
mod common;

use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path, query_param},
};

const WS: &str = "00000000-0000-0000-0000-000000000001";
const ITEM: &str = "00000000-0000-0000-0000-000000000002";
const TABLE: &str = "00000000-0000-0000-0000-000000000003";

fn table_route() -> String {
    format!("/iceberg/v1/{WS}/{ITEM}/namespaces/dbo/tables/native")
}
fn metadata() -> Value {
    json!({"metadata-location":format!("abfss://{WS}@onelake.dfs.fabric.microsoft.com/{ITEM}/Tables/dbo/native/metadata/v1.metadata.json"),
    "metadata":{"format-version":2,"table-uuid":TABLE,"current-schema-id":0,
    "location":format!("abfss://{WS}@onelake.dfs.fabric.microsoft.com/{ITEM}/Tables/dbo/native"),"properties":{}},
    "config":{"adls.sas-token":"synthetic-do-not-print"}})
}
fn create_body() -> Value {
    json!({"schema":{"type":"struct","fields":[{"id":1,"name":"id","required":false,"type":"int"}]}})
}
fn commit_body() -> Value {
    json!({"requirements":[{"type":"assert-table-uuid","uuid":TABLE},{"type":"assert-current-schema-id","current-schema-id":0}],
    "updates":[{"action":"set-properties","updates":{"synthetic":"value"}}]})
}

fn command(server: &MockServer, operation: &str) -> assert_cmd::Command {
    let mut command = common::mock_fabio(server);
    command
        .env("FABIO_ONELAKE_TABLE_ENDPOINT", server.uri())
        .env("FABIO_STORAGE_ACCESS_TOKEN", "storage-test-token")
        .env("FABIO_NO_VERSION_CHECK", "1")
        .args([
            "lakehouse",
            operation,
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--namespace",
            "dbo",
            "--table",
            "native",
        ]);
    command
}

async fn config(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/iceberg/v1/config"))
        .and(query_param("warehouse", format!("{WS}/{ITEM}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"overrides":{"prefix":format!("{WS}/{ITEM}")}})),
        )
        .mount(server)
        .await;
}

#[test]
fn create_preview_is_offline_and_invalid_scope_or_payload_never_contacts_catalog() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(MockServer::start());
    let body = create_body().to_string();
    let result = command(&server, "iceberg-create-table")
        .args(["--dry-run", "--content", &body])
        .assert()
        .success();
    let result = common::parse_json(&result);
    assert_eq!(
        result["data"]["would_execute"],
        "lakehouse iceberg-create-table"
    );
    assert_eq!(result["data"]["details"]["request"]["name"], "native");
    let invalid = common::mock_fabio(&server)
        .env("FABIO_ONELAKE_TABLE_ENDPOINT", server.uri())
        .args([
            "--dry-run",
            "--quiet",
            "lakehouse",
            "iceberg-delete-table",
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--table",
            "../all",
            "--expected-table-uuid",
            TABLE,
        ])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&invalid.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
    command(&server, "iceberg-commit-table")
        .args(["--dry-run", "--content", r#"{"updates":[]}"#])
        .assert()
        .failure();
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_uses_storage_auth_exact_scope_and_does_not_print_vended_credentials() {
    let server = MockServer::start().await;
    config(&server).await;
    let mut body = create_body();
    body["name"] = json!("native");
    Mock::given(method("POST"))
        .and(path(format!(
            "/iceberg/v1/{WS}/{ITEM}/namespaces/dbo/tables"
        )))
        .and(header("authorization", "Bearer storage-test-token"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(metadata()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(table_route()))
        .respond_with(ResponseTemplate::new(200).set_body_json(metadata()))
        .expect(1)
        .mount(&server)
        .await;
    let result = command(&server, "iceberg-create-table")
        .args(["--content", &create_body().to_string()])
        .assert()
        .success();
    let value = common::parse_json(&result);
    assert_eq!(value["data"]["observation"]["scopeVerified"], true);
    assert_eq!(value["data"]["observation"]["tableUuid"], TABLE);
    assert!(
        !String::from_utf8_lossy(&result.get_output().stdout).contains("synthetic-do-not-print")
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commit_preview_and_readonly_never_post_and_keep_engine_assertions() {
    let server = MockServer::start().await;
    config(&server).await;
    Mock::given(method("GET"))
        .and(path(table_route()))
        .respond_with(ResponseTemplate::new(200).set_body_json(metadata()))
        .expect(2)
        .mount(&server)
        .await;
    let body = commit_body().to_string();
    let result = command(&server, "iceberg-commit-table")
        .env("OPENCODE", "1")
        .args(["--dry-run", "--content", &body])
        .assert()
        .success();
    let value = common::parse_json(&result);
    assert_eq!(value["data"]["destructive"], true);
    assert!(value["data"]["agentNotice"].is_string());
    assert_eq!(value["data"]["details"]["request"], commit_body());
    let blocked = command(&server, "iceberg-commit-table")
        .args(["--readonly", "--quiet", "--content", &body])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&blocked.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "READONLY_MODE");
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn virtual_delta_shortcuts_and_uuid_changes_are_never_written_or_deleted() {
    for (location, virtual_delta, expected, code) in [
        ("native", true, TABLE, "INVALID_INPUT"),
        ("source", false, TABLE, "INVALID_INPUT"),
        ("native", false, WS, "CONFLICT"),
    ] {
        let server = MockServer::start().await;
        config(&server).await;
        let mut value = metadata();
        value["metadata"]["location"] = json!(format!(
            "abfss://{WS}@onelake.dfs.fabric.microsoft.com/{ITEM}/Tables/dbo/{location}"
        ));
        if virtual_delta {
            value["metadata"]["properties"] =
                json!({"XTABLE_METADATA":"{\"sourceTableFormat\":\"Delta\"}"});
        }
        Mock::given(method("GET"))
            .and(path(table_route()))
            .respond_with(ResponseTemplate::new(200).set_body_json(value))
            .expect(1)
            .mount(&server)
            .await;
        let result = command(&server, "iceberg-delete-table")
            .args(["--quiet", "--dry-run", "--expected-table-uuid", expected])
            .assert()
            .failure();
        let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
        assert_eq!(error["error"]["code"], code);
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_or_uncertain_commits_never_replay_or_drop_requirements() {
    for status in [409, 503, 202] {
        let server = MockServer::start().await;
        config(&server).await;
        Mock::given(method("GET"))
            .and(path(table_route()))
            .respond_with(ResponseTemplate::new(200).set_body_json(metadata()))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path(table_route())).and(body_json(commit_body()))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"error":{"code":"SyntheticConflict","message":"synthetic catalog rejection"}})))
            .expect(1).mount(&server).await;
        let result = command(&server, "iceberg-commit-table")
            .args(["--quiet", "--content", &commit_body().to_string()])
            .assert()
            .failure();
        let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
        assert_eq!(error["error"]["retriable"], false);
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_delete_observes_catalog_absence_without_claiming_physical_purge_or_atomic_guard() {
    let server = MockServer::start().await;
    config(&server).await;
    Mock::given(method("GET"))
        .and(path(table_route()))
        .respond_with(ResponseTemplate::new(200).set_body_json(metadata()))
        .up_to_n_times(1)
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(table_route()))
        .and(header("authorization", "Bearer storage-test-token"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(table_route()))
        .respond_with(ResponseTemplate::new(404).set_body_json(
            json!({"error":{"code":"NoSuchTable","message":"synthetic missing table"}}),
        ))
        .with_priority(2)
        .expect(1)
        .mount(&server)
        .await;
    let result = command(&server, "iceberg-delete-table")
        .args(["--expected-table-uuid", TABLE])
        .assert()
        .success();
    let value = common::parse_json(&result);
    assert_eq!(value["data"]["catalogAbsent"], true);
    assert_eq!(value["data"]["physicalDataRemovalVerified"], false);
    assert_eq!(value["data"]["atomicIdentityGuard"], false);
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_delete_preview_and_readonly_enforce_the_canonical_boundary() {
    let server = MockServer::start().await;
    config(&server).await;
    Mock::given(method("GET"))
        .and(path(table_route()))
        .respond_with(ResponseTemplate::new(200).set_body_json(metadata()))
        .expect(2)
        .mount(&server)
        .await;
    let preview = command(&server, "iceberg-delete-table")
        .env("OPENCODE", "1")
        .args(["--dry-run", "--expected-table-uuid", TABLE])
        .assert()
        .success();
    let value = common::parse_json(&preview);
    assert_eq!(value["data"]["dry_run"], true);
    assert_eq!(
        value["data"]["would_execute"],
        "lakehouse iceberg-delete-table"
    );
    assert_eq!(value["data"]["destructive"], true);
    assert!(value["data"]["agentNotice"].is_string());
    assert_eq!(value["data"]["details"]["atomicIdentityGuard"], false);
    let blocked = command(&server, "iceberg-delete-table")
        .args(["--readonly", "--quiet", "--expected-table-uuid", TABLE])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&blocked.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "READONLY_MODE");
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mismatched_catalog_prefix_is_never_used_for_writes_or_reads() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/iceberg/v1/config"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"overrides":{"prefix":format!("{ITEM}/{WS}")}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let result = command(&server, "iceberg-commit-table")
        .args(["--quiet", "--content", &commit_body().to_string()])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "API_ERROR");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

fn live_native_commit_and_stale_assertion_rejection() {
    let required =
        |name: &str| std::env::var(name).expect("explicit disposable native fixture required");
    let workspace = required("FABIO_TEST_METADATA_WORKSPACE");
    assert!(
        workspace == required("FABIO_BLOG_OWNED_WORKSPACE"),
        "not an owned disposable workspace"
    );
    let item = required("FABIO_TEST_METADATA_ITEM");
    let table = required("FABIO_TEST_ICEBERG_TABLE");
    let uuid = required("FABIO_TEST_ICEBERG_UUID");
    let scope = [
        "--workspace",
        &workspace,
        "--id",
        &item,
        "--namespace",
        "dbo",
        "--table",
        &table,
    ];
    let read = || -> Value {
        let output = common::fabio()
            .args(["--readonly", "lakehouse", "iceberg-table"])
            .args(scope)
            .output()
            .expect("run fabio");
        assert!(
            output.status.success(),
            "native fixture metadata read failed; inspect privately"
        );
        serde_json::from_slice::<Value>(&output.stdout).expect("JSON response")["data"].clone()
    };
    let before = read();
    assert!(
        before["metadata"]["table-uuid"].as_str() == Some(uuid.as_str()),
        "native table identity changed before validation"
    );
    let schema = before["metadata"]["current-schema-id"]
        .as_i64()
        .expect("current schema id");
    let key = format!("fabio.validation.{}", uuid::Uuid::new_v4().simple());
    let request = json!({"requirements":[{"type":"assert-table-uuid","uuid":uuid},{"type":"assert-current-schema-id","current-schema-id":schema}],
        "updates":[{"action":"set-properties","updates":{(key.clone()):"synthetic"}}]});
    let output = common::fabio()
        .args(["lakehouse", "iceberg-commit-table"])
        .args(scope)
        .args(["--content", &request.to_string()])
        .output()
        .expect("run fabio");
    assert!(
        output.status.success(),
        "native catalog commit failed; inspect private diagnostics without replay"
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("JSON acknowledgement");
    assert_eq!(result["data"]["observation"]["scopeVerified"], true);
    let after = read();
    assert!(
        after["metadata"]["properties"][&key] == "synthetic",
        "native property readback mismatch"
    );
    let mut stale = request;
    stale["requirements"][1]["current-schema-id"] = json!(schema + 1_000_000);
    let stale_key = format!("{key}.stale");
    stale["updates"][0]["updates"] = json!({(stale_key.clone()):"must-not-commit"});
    let output = common::fabio()
        .args(["--quiet", "lakehouse", "iceberg-commit-table"])
        .args(scope)
        .args(["--content", &stale.to_string()])
        .output()
        .expect("run fabio");
    assert!(
        !output.status.success(),
        "stale native assertion unexpectedly succeeded"
    );
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON conflict");
    assert_eq!(error["error"]["code"], "CONFLICT");
    assert_eq!(error["error"]["retriable"], false);
    let observed = read();
    assert!(
        observed["metadata"]["properties"].get(&stale_key).is_none(),
        "stale update changed metadata"
    );
}

#[test]
#[ignore = "requires explicit FABIO_TEST_ICEBERG_TABLE/UUID in an owned disposable fixture; commits properties then deletes ONLY that table"]
fn live_native_catalog_commit_conflict_and_delete_lifecycle() {
    let required =
        |name: &str| std::env::var(name).expect("explicit disposable native fixture required");
    let workspace = required("FABIO_TEST_METADATA_WORKSPACE");
    assert!(
        workspace == required("FABIO_BLOG_OWNED_WORKSPACE"),
        "not an owned disposable workspace"
    );
    let item = required("FABIO_TEST_METADATA_ITEM");
    let table = required("FABIO_TEST_ICEBERG_TABLE");
    let uuid = required("FABIO_TEST_ICEBERG_UUID");
    let validation = std::panic::catch_unwind(live_native_commit_and_stale_assertion_rejection);
    let output = common::fabio()
        .args([
            "lakehouse",
            "iceberg-delete-table",
            "--workspace",
            &workspace,
            "--id",
            &item,
            "--namespace",
            "dbo",
            "--table",
            &table,
            "--expected-table-uuid",
            &uuid,
        ])
        .output()
        .expect("delete owned native fixture");
    assert!(
        output.status.success(),
        "native fixture deletion failed; inspect privately without replay"
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("JSON acknowledgement");
    assert_eq!(result["data"]["catalogAbsent"], true);
    assert_eq!(result["data"]["physicalDataRemovalVerified"], false);
    if let Err(panic) = validation {
        std::panic::resume_unwind(panic);
    }
}
