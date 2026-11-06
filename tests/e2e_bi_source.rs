//! Bounded source checks are shared with creation and never silently switch modes.
mod common;

use common::{mock_fabio, parse_json};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const ITEM: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const MODEL: &str = "cccccccc-1111-2222-3333-444444444444";

fn table_url(name: &str) -> String {
    format!("https://onelake.dfs.fabric.microsoft.com/{WS}/{ITEM}/Tables/dbo/{name}")
}
fn log_name(table: &str, version: u64) -> String {
    format!("{ITEM}/Tables/dbo/{table}/_delta_log/{version:020}.json")
}
fn log_body(kind: &str, reader: u8) -> String {
    let schema=json!({"type":"struct","fields":[{"name":"private_column","type":kind,"nullable":true,"metadata":{}}]}).to_string();
    format!(
        "{}\n{}\n{}\n",
        json!({"protocol":{"minReaderVersion":reader,"minWriterVersion":2}}),
        json!({"metaData":{"format":{"provider":"parquet"},"schemaString":schema,"configuration":{},"partitionColumns":[]}}),
        json!({"add":{"path":"private.parquet","stats":"private row statistics"}})
    )
}
fn command(server: &MockServer) -> assert_cmd::Command {
    let mut command = mock_fabio(server);
    command.env("FABIO_STORAGE_ACCESS_TOKEN", "fake-source-storage-token");
    command.env("FABIO_ONELAKE_DFS_ENDPOINT", server.uri());
    command
}
fn check(server: &MockServer) -> assert_cmd::Command {
    let mut command = command(server);
    command.args([
        "--readonly",
        "semantic-model",
        "check-source",
        "--one-lake-table",
        &table_url("Sales"),
    ]);
    command
}
fn create(server: &MockServer) -> assert_cmd::Command {
    let mut command = command(server);
    command.args([
        "semantic-model",
        "create",
        "--workspace",
        WS,
        "--name",
        "fixture",
        "--one-lake-table",
        &table_url("Sales"),
    ]);
    command
}
async fn metadata(server: &MockServer, schemas: bool) {
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/items/{ITEM}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"id":ITEM,"workspaceId":WS,"type":"Lakehouse","displayName":"private_name"}),
        ))
        .expect(1)
        .mount(server)
        .await;
    let properties = if schemas {
        json!({"defaultSchema":"dbo"})
    } else {
        json!({})
    };
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/lakehouses/{ITEM}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id":ITEM,"properties":properties})),
        )
        .expect(1)
        .mount(server)
        .await;
}
async fn directory(server: &MockServer, table: &str, body: Value, continuation: Option<&str>) {
    let mut response = ResponseTemplate::new(200).set_body_json(body);
    if let Some(token) = continuation {
        response = response.insert_header("x-ms-continuation", token);
    }
    Mock::given(method("GET"))
        .and(path(format!("/{WS}/")))
        .and(query_param("recursive", "false"))
        .and(query_param_is_missing("continuation"))
        .and(query_param("maxResults", "25"))
        .and(query_param(
            "directory",
            format!("{ITEM}/Tables/dbo/{table}/_delta_log"),
        ))
        .respond_with(response)
        .expect(1)
        .mount(server)
        .await;
}
async fn file(server: &MockServer, table: &str, version: u64, body: String) {
    Mock::given(method("GET"))
        .and(path(format!("/{WS}/{}", log_name(table, version))))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(server)
        .await;
}
async fn classic(server: &MockServer) {
    metadata(server, true).await;
    directory(
        server,
        "Sales",
        json!({"paths":[{"name":log_name("Sales",0),"isDirectory":false}]}),
        None,
    )
    .await;
    file(server, "Sales", 0, log_body("long", 1)).await;
}
async fn readonly_requests(server: &MockServer) {
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|request| request.method.as_str() == "GET"
                && !request.url.path().ends_with(".parquet")
                && !request.url.path().contains("/Files/"))
    );
}
fn source_check<'a>(data: &'a Value, name: &str) -> &'a Value {
    data["sources"][0]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["checkId"] == name)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_previews_and_invalid_scope_never_start_source_reads() {
    let server = MockServer::start().await;
    let output = check(&server).arg("--dry-run").assert().success();
    assert_eq!(
        parse_json(&output)["data"]["would_execute"],
        "semantic-model check-source"
    );
    let output = create(&server)
        .args(["--check-source", "--dry-run"])
        .assert()
        .success();
    assert_eq!(
        parse_json(&output)["data"]["details"]["sourceCheck"]["status"],
        "not_run"
    );
    create(&server)
        .arg("--check-source")
        .arg("--readonly")
        .assert()
        .failure();
    create(&server)
        .args(["--source-timeout", "1", "--dry-run"])
        .assert()
        .failure();
    for bad in [
        format!("https://example.org/{WS}/{ITEM}/Tables/T"),
        format!("https://private@onelake.dfs.fabric.microsoft.com/{WS}/{ITEM}/Tables/T"),
        format!("https://onelake.dfs.fabric.microsoft.com/{WS}/{ITEM}/Tables/./T"),
        format!("https://onelake.dfs.fabric.microsoft.com/{WS}/{ITEM}/Tables/dbo/%2fT"),
    ] {
        command(&server)
            .args([
                "semantic-model",
                "check-source",
                "--one-lake-table",
                &bad,
                "--dry-run",
            ])
            .assert()
            .failure();
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn classic_source_metadata_is_bounded_and_excludes_names_stats_and_data_values() {
    let server = MockServer::start().await;
    classic(&server).await;
    let output = check(&server).arg("--strict").assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["status"], "ready");
    assert_eq!(data["usage"]["pages"], 1);
    assert_eq!(data["usage"]["logFiles"], 1);
    assert_eq!(source_check(data, "delta_protocol")["status"], "ready");
    assert_eq!(
        source_check(data, "delta_schema")["evidence"]["columnCount"],
        1
    );
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("private"));
    readonly_requests(&server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn creation_reuses_source_checks_before_unchanged_native_creation_payload() {
    let server = MockServer::start().await;
    classic(&server).await;
    Mock::given(method("POST")).and(path(format!("/workspaces/{WS}/semanticModels")))
        .and(body_json(json!({"displayName":"fixture","creationPayload":{"oneLakeTables":[table_url("Sales")]}})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":MODEL}))).expect(1).mount(&server).await;
    let output = create(&server).arg("--check-source").assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["id"], MODEL);
    assert_eq!(data["sourceCheck"]["status"], "ready");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests.last().unwrap().method.as_str(), "POST");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unrequested_preflight_keeps_legacy_create_network_shape_unchanged() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WS}/semanticModels")))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id":MODEL})))
        .expect(1)
        .mount(&server)
        .await;
    create(&server).assert().success();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsupported_columns_or_new_protocols_stop_opted_in_creation_without_switching_modes() {
    for (kind, reader, expected) in [("binary", 1, "needs_action"), ("long", 3, "unknown")] {
        let server = MockServer::start().await;
        metadata(&server, true).await;
        directory(
            &server,
            "Sales",
            json!({"paths":[{"name":log_name("Sales",0)}]}),
            None,
        )
        .await;
        file(&server, "Sales", 0, log_body(kind, reader)).await;
        let output = create(&server)
            .args(["--check-source", "--force"])
            .assert()
            .failure();
        let data = &parse_json(&output)["data"];
        assert_eq!(data["status"], "not_created");
        assert_eq!(data["sourceCheck"]["status"], expected);
        readonly_requests(&server).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_listing_or_checkpoint_only_evidence_is_unknown_without_reading_older_commits() {
    for continuation in [true, false] {
        let server = MockServer::start().await;
        metadata(&server, true).await;
        let rows = if continuation {
            json!([{"name":log_name("Sales",0)}])
        } else {
            json!([{"name":format!("{ITEM}/Tables/dbo/Sales/_delta_log/00000000000000000010.checkpoint.parquet")}])
        };
        directory(
            &server,
            "Sales",
            json!({"paths":rows}),
            continuation.then_some("opaque?&private"),
        )
        .await;
        let output = check(&server)
            .args(["--all", "--source-page-limit", "1", "--strict"])
            .assert()
            .failure();
        assert_eq!(parse_json(&output)["data"]["status"], "unknown");
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
        readonly_requests(&server).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn raw_continuation_urls_scope_mismatches_and_malformed_pages_are_not_treated_as_empty() {
    for body in [
        json!({}),
        json!({"paths":[],"continuationUri":"https://example.org/private"}),
        json!({"paths":[{"name":"other/private.json"}]}),
    ] {
        let server = MockServer::start().await;
        metadata(&server, true).await;
        directory(&server, "Sales", body, None).await;
        let output = check(&server).arg("--strict").assert().failure();
        assert_eq!(parse_json(&output)["data"]["status"], "unknown");
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
        readonly_requests(&server).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_file_byte_and_time_budgets_never_turn_partial_metadata_into_creation() {
    let server = MockServer::start().await;
    metadata(&server, true).await;
    directory(
        &server,
        "Sales",
        json!({"paths":[{"name":log_name("Sales",0)}]}),
        None,
    )
    .await;
    file(&server, "Sales", 0, "private".repeat(500)).await;
    let output = create(&server)
        .args(["--check-source", "--source-byte-limit", "1024"])
        .assert()
        .failure();
    let data = &parse_json(&output)["data"]["sourceCheck"];
    assert_eq!(data["status"], "unknown");
    assert_eq!(data["usage"]["bodyBytesCharged"], 1024);
    assert_eq!(data["usage"]["byteUsageExact"], false);
    readonly_requests(&server).await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/items/{ITEM}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(3))
                .set_body_json(json!({"id":ITEM,"type":"Lakehouse"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    check(&server)
        .args(["--source-timeout", "1", "--strict"])
        .timeout(std::time::Duration::from_secs(8))
        .assert()
        .failure();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_schema_less_risk_does_not_trigger_a_generator_fallback() {
    for (mode, expected) in [("native", "needs_action"), ("generated", "ready")] {
        let server = MockServer::start().await;
        metadata(&server, false).await;
        directory(
            &server,
            "Sales",
            json!({"paths":[{"name":log_name("Sales",0)}]}),
            None,
        )
        .await;
        file(&server, "Sales", 0, log_body("long", 1)).await;
        let output = check(&server)
            .args(["--creation-mode", mode])
            .assert()
            .success();
        assert_eq!(parse_json(&output)["data"]["status"], expected);
        readonly_requests(&server).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_item_reads_are_deduplicated_and_directory_tokens_are_replayed_only_on_same_scope() {
    let server = MockServer::start().await;
    metadata(&server, true).await;
    let token = "opaque?&private";
    Mock::given(method("GET"))
        .and(path(format!("/{WS}/")))
        .and(query_param("continuation", token))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"paths":[{"name":log_name("Sales",0)}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    directory(&server, "Sales", json!({"paths":[]}), Some(token)).await;
    file(&server, "Sales", 0, log_body("long", 1)).await;
    directory(
        &server,
        "Stores",
        json!({"paths":[{"name":log_name("Stores",0)}]}),
        None,
    )
    .await;
    file(&server, "Stores", 0, log_body("long", 1)).await;
    let output = check(&server)
        .args(["--one-lake-table", &table_url("Stores"), "--strict"])
        .assert()
        .success();
    assert_eq!(
        parse_json(&output)["data"]["sources"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    readonly_requests(&server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generated_creation_check_flag_does_not_bypass_readonly_or_accept_orphan_limits() {
    let server = MockServer::start().await;
    command(&server)
        .args([
            "--readonly",
            "semantic-model",
            "generate",
            "--workspace",
            WS,
            "--name",
            "fixture",
            "--lakehouse",
            ITEM,
            "--check-source",
        ])
        .assert()
        .failure();
    command(&server)
        .args([
            "semantic-model",
            "generate",
            "--workspace",
            WS,
            "--name",
            "fixture",
            "--lakehouse",
            ITEM,
            "--source-timeout",
            "1",
        ])
        .assert()
        .failure();
    command(&server)
        .args([
            "semantic-model",
            "create",
            "--workspace",
            WS,
            "--name",
            "fixture",
            "--check-source",
            "--file",
            "does-not-exist.tmdl",
            "--dry-run",
        ])
        .assert()
        .failure();
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commit_gaps_and_file_caps_stop_before_reading_unjustified_older_metadata() {
    for (latest, limit, reason) in [
        (2, "5", "non_contiguous_commit_suffix"),
        (1, "1", "source_log_limit"),
    ] {
        let server = MockServer::start().await;
        metadata(&server, true).await;
        directory(
            &server,
            "Sales",
            json!({"paths":[{"name":log_name("Sales",latest)},{"name":log_name("Sales",0)}]}),
            None,
        )
        .await;
        file(
            &server,
            "Sales",
            latest,
            json!({"commitInfo":{"private":"secret"}}).to_string(),
        )
        .await;
        let output = check(&server)
            .args(["--source-log-limit", limit, "--strict"])
            .assert()
            .failure();
        let data = &parse_json(&output)["data"];
        assert_eq!(
            source_check(data, "delta_coverage")["evidence"]["reason"],
            reason
        );
        assert_eq!(data["usage"]["logFiles"], 1);
        assert_eq!(server.received_requests().await.unwrap().len(), 4);
        readonly_requests(&server).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forbidden_source_access_is_unknown_and_does_not_trigger_grants_or_inventory() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/items/{ITEM}")))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"errorCode":"Forbidden","message":"private"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = create(&server).arg("--check-source").assert().failure();
    let data = &parse_json(&output)["data"]["sourceCheck"];
    assert_eq!(data["status"], "unknown");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("private"));
    readonly_requests(&server).await;
}
