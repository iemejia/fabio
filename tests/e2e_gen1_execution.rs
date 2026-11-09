//! Gen1 mutation/observation guardrails with isolated local receipt ledgers.
mod common;

use common::{mock_fabio, parse_json};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const A: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const B: &str = "cccccccc-1111-2222-3333-444444444444";
const OP: &str = "dddddddd-1111-2222-3333-444444444444";

fn base(server: &MockServer, home: &tempfile::TempDir) -> assert_cmd::Command {
    let mut command = mock_fabio(server);
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path());
    command
}
fn submit(server: &MockServer, home: &tempfile::TempDir) -> assert_cmd::Command {
    let mut command = base(server, home);
    command.args([
        "dataflow",
        "upgrade-gen1",
        "--workspace",
        WS,
        "--id",
        A,
        "--allow-in-place-upgrade",
    ]);
    command
}
async fn ready(server: &MockServer, ids: &[&str]) {
    Mock::given(method("GET")).and(path(format!("/workspaces/{WS}/dataflows/gen1UpgradeReadinessResults")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value":ids.iter().map(|id|json!({"id":id,"displayName":"private_name","status":"ReadyToMigrate"})).collect::<Vec<_>>()}))).expect(1).mount(server).await;
}
fn migration(id: &str, status: &str) -> Value {
    let mut row = json!({"dataflowId":id,"workspaceId":WS,"dataflowName":"private_name","workspaceName":"private_workspace","targetDataflowName":"private_target","assessmentStatus":"ReadyToMigrate","migrationStatus":status});
    if status == "Failed" {
        row["migrationError"] = json!({"errorCode":"UpgradeFailed","message":"private endpoint secret","isRetriable":true});
    }
    row
}
fn result(rows: Vec<Value>, succeeded: u64, failed: u64) -> Value {
    let count = rows.len();
    let rows = Value::Array(rows);
    json!({"summary":{"totalCount":count,"succeededCount":succeeded,"failedCount":failed},"migrations":rows})
}
async fn response(server: &MockServer, template: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WS}/dataflows/gen1Upgrade")))
        .respond_with(template)
        .expect(1)
        .mount(server)
        .await;
}
async fn reads_result(server: &MockServer, body: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/operations/{OP}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status":"Succeeded"})))
        .expect(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/operations/{OP}/result")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}
async fn mutation_count(server: &MockServer, expected: usize) {
    assert_eq!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.method.as_str() == "POST")
            .count(),
        expected
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn approval_readonly_and_scope_guards_precede_all_network_requests() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let output = base(&server, &home)
        .args(["dataflow", "upgrade-gen1", "--workspace", WS, "--id", A])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
    assert!(
        error["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("--allow-in-place-upgrade")
    );
    submit(&server, &home).arg("--readonly").assert().failure();
    submit(&server, &home).args(["--id", A]).assert().failure();
    let dry = submit(&server, &home)
        .args(["--readonly", "--dry-run"])
        .assert()
        .success();
    assert_eq!(parse_json(&dry)["data"]["destructive"], true);
    assert_eq!(
        parse_json(&dry)["data"]["details"]["singleSubmission"],
        true
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
    assert!(!home.path().join(".fabio").join("jobs.jsonl").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovery_examples_match_workflow_and_preview_without_network_or_receipts() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let workflow: Value = serde_json::from_str(include_str!(
        "../src/commands/context/data/workflows/gen1_upgrade_execution.json"
    ))
    .unwrap();
    let expected: Vec<_> = workflow["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|step| step["discovery_example"] == true)
        .map(|step| step["command"].as_str().unwrap())
        .collect();
    assert_eq!(expected.len(), 2);

    let describe = base(&server, &home)
        .args([
            "--readonly",
            "context",
            "describe",
            "dataflow",
            "upgrade-gen1",
        ])
        .assert()
        .success();
    let group = base(&server, &home)
        .args(["--readonly", "context", "agent", "--group", "dataflow"])
        .assert()
        .success();
    for metadata in [
        &parse_json(&describe)["data"],
        &parse_json(&group)["data"]["group_details"]["subcommands"]["upgrade-gen1"],
    ] {
        assert_eq!(metadata["examples"], json!(expected));
        assert_eq!(metadata["mutates"], true);
        assert_eq!(metadata["destructive"], true);
        assert_eq!(metadata["async"], true);
        assert_eq!(metadata["returns"], "object");
    }

    let preview = expected[0]
        .replace("$WORKSPACE_ID", WS)
        .replace("$GEN1_ID", A);
    let args = shlex::split(&preview).unwrap();
    assert!(args.iter().any(|arg| arg == "--readonly"));
    assert!(args.iter().any(|arg| arg == "--dry-run"));
    let output = base(&server, &home).args(&args[1..]).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "dataflow upgrade-gen1");
    assert_eq!(data["destructive"], true);
    assert_eq!(data["details"]["workspace"], WS);
    assert_eq!(data["details"]["dataflows"], json!([{"id": A}]));
    assert_eq!(data["details"]["approvalProvided"], false);

    // Never submit the execution example: without its explicit approval flag,
    // the same invocation must still fail before readiness or receipt creation.
    let execution = expected[1]
        .replace("$WORKSPACE_ID", WS)
        .replace("$GEN1_ID", A);
    let mut args = shlex::split(&execution).unwrap();
    assert!(args.iter().any(|arg| arg == "--allow-in-place-upgrade"));
    assert!(
        !args
            .iter()
            .any(|arg| arg == "--readonly" || arg == "--dry-run")
    );
    args.retain(|arg| arg != "--allow-in-place-upgrade");
    let blocked = base(&server, &home).args(&args[1..]).assert().failure();
    let error: Value = serde_json::from_slice(&blocked.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
    assert!(!home.path().join(".fabio").join("jobs.jsonl").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn damaged_receipt_storage_blocks_submission_and_follow_without_repair_or_post() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let directory = home.path().join(".fabio");
    std::fs::create_dir(&directory).unwrap();
    let original = b"{\"private-secret\":";
    std::fs::write(directory.join("jobs.jsonl"), original).unwrap();
    ready(&server, &[A]).await;
    let output = submit(&server, &home).arg("--quiet").assert().failure();
    assert_eq!(output.get_output().stdout.len(), 0);
    let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("No upgrade submitted")
    );
    let output = base(&server, &home)
        .args([
            "--readonly",
            "dataflow",
            "follow-upgrade",
            "--receipt-id",
            OP,
        ])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("ledger validation failed")
    );
    assert!(!String::from_utf8_lossy(&output.get_output().stderr).contains("private-secret"));
    assert_eq!(
        std::fs::read(directory.join("jobs.jsonl")).unwrap(),
        original
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    mutation_count(&server, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn storage_failure_after_acceptance_retains_stdout_receipt_and_never_resubmits() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    ready(&server, &[A]).await;
    let ledger = home.path().join(".fabio").join("jobs.jsonl");
    let damaged_path = ledger.clone();
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WS}/dataflows/gen1Upgrade")))
        .respond_with(move |_: &wiremock::Request| {
            // External disk corruption between the pre-POST durable receipt and
            // the accepted response: the client must warn, never repair/replay.
            std::fs::write(&damaged_path, b"damaged-local-receipt").unwrap();
            ResponseTemplate::new(202).insert_header("x-ms-operation-id", OP)
        })
        .expect(1)
        .mount(&server)
        .await;
    let output = submit(&server, &home).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["submission"]["state"], "accepted");
    assert_eq!(data["submission"]["operationId"], OP);
    assert!(data["receiptId"].as_str().is_some());
    assert_eq!(data["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(data["retryAllowed"], false);
    assert_eq!(std::fs::read(&ledger).unwrap(), b"damaged-local-receipt");
    mutation_count(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incomplete_or_nonready_gate_never_posts_despite_approval_all_or_force() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    Mock::given(method("GET")).and(path(format!("/workspaces/{WS}/dataflows/gen1UpgradeReadinessResults")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value":[{"id":A,"displayName":"mock","status":"ReadyToMigrate"}],"continuationToken":"more"}))).expect(1).mount(&server).await;
    let output = submit(&server, &home)
        .args(["--assessment-page-limit", "1", "--all", "--force"])
        .assert()
        .failure();
    assert_eq!(
        parse_json(&output)["data"]["submission"]["state"],
        "not_submitted"
    );
    mutation_count(&server, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synchronous_exact_scope_result_is_cached_without_names_or_credential_messages() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    ready(&server, &[A]).await;
    response(
        &server,
        ResponseTemplate::new(200).set_body_json(result(vec![migration(A, "Succeeded")], 1, 0)),
    )
    .await;
    let output = submit(&server, &home).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["submission"]["state"], "acknowledged");
    assert_eq!(data["completionStatus"], "succeeded");
    let receipt = data["receiptId"].as_str().unwrap();
    let follow = base(&server, &home)
        .args([
            "--readonly",
            "dataflow",
            "follow-upgrade",
            "--receipt-id",
            receipt,
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&follow)["data"]["completionStatus"], "succeeded");
    assert!(!String::from_utf8_lossy(&output.get_output().stdout).contains("private_"));
    mutation_count(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_headers_survive_unreadable_body_and_follow_uses_only_gets() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    ready(&server, &[A]).await;
    response(
        &server,
        ResponseTemplate::new(202)
            .insert_header("x-ms-operation-id", OP)
            .set_body_raw("not-json", "application/json"),
    )
    .await;
    reads_result(&server, result(vec![migration(A, "Succeeded")], 1, 0)).await;
    let output = submit(&server, &home).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["submission"]["state"], "accepted");
    assert_eq!(data["submission"]["responseBodyStatus"], "unreadable");
    assert_eq!(data["completionStatus"], "not_checked");
    let follow = base(&server, &home)
        .args([
            "--readonly",
            "dataflow",
            "follow-upgrade",
            "--receipt-id",
            data["receiptId"].as_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&follow)["data"]["completionStatus"], "succeeded");
    mutation_count(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn errors_never_retry_submission_and_unknown_receipts_cannot_invent_an_operation() {
    for status in [401, 429, 502, 503, 504] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        ready(&server, &[A]).await;
        response(
            &server,
            ResponseTemplate::new(status)
                .insert_header("retry-after", "0")
                .set_body_json(
                    json!({"errorCode":"RetryableError","message":"private","isRetriable":true}),
                ),
        )
        .await;
        let output = submit(&server, &home).assert().failure();
        let data = &parse_json(&output)["data"];
        assert_eq!(data["submission"]["httpStatus"], status);
        assert_eq!(data["retryAllowed"], false);
        let follow = base(&server, &home)
            .args([
                "--readonly",
                "dataflow",
                "follow-upgrade",
                "--receipt-id",
                data["receiptId"].as_str().unwrap(),
            ])
            .assert()
            .failure();
        assert_eq!(
            parse_json(&follow)["data"]["observationReason"],
            "no_verified_operation_identity"
        );
        let prune = base(&server, &home)
            .args(["jobs", "prune"])
            .assert()
            .success();
        assert_eq!(parse_json(&prune)["data"]["remaining"], 1);
        mutation_count(&server, 1).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_success_fails_without_losing_verified_rows_or_authorizing_retries() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    ready(&server, &[A, B]).await;
    response(
        &server,
        ResponseTemplate::new(200).set_body_json(result(
            vec![migration(A, "Succeeded"), migration(B, "Failed")],
            1,
            1,
        )),
    )
    .await;
    let output = submit(&server, &home).args(["--id", B]).assert().failure();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["completionStatus"], "partial_success");
    assert_eq!(
        data["result"]["migrations"][0]["migrationStatus"],
        "Succeeded"
    );
    assert_eq!(
        data["result"]["migrations"][1]["migrationError"]["isRetriable"],
        true
    );
    assert_eq!(data["retryAllowed"], false);
    mutation_count(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authoritative_header_survives_foreign_location_without_following_that_url() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    ready(&server, &[A]).await;
    response(
        &server,
        ResponseTemplate::new(202)
            .insert_header("x-ms-operation-id", OP)
            .insert_header("location", format!("https://example.org/operations/{OP}")),
    )
    .await;
    reads_result(&server, result(vec![migration(A, "Succeeded")], 1, 0)).await;
    let output = submit(&server, &home).args(["--wait"]).assert().success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["submission"]["state"], "accepted");
    assert_eq!(data["submission"]["operationId"], OP);
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
    mutation_count(&server, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_header_recovery_observes_existing_receipt_without_posting() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let id = "eeeeeeee-1111-2222-3333-444444444444";
    let directory = home.path().join(".fabio");
    std::fs::create_dir_all(&directory).unwrap();
    let entry = json!({"id":id,"kind":"gen1-upgrade","status":"accepted","workspace":WS,"item_id":A,
        "target_ids":[A],"started_at":"2026-11-09T00:00:00Z","submission_state":"accepted","http_status":202,"futureEvidence":{"retained":true},
        "fabric_endpoint":server.uri(),"correlation_evidence":{"reason":"invalid_location","operationHeaderPresent":true,
        "operationHeaderLength":36,"locationSameOrigin":false,"unverifiedOperationCandidate":OP}});
    std::fs::write(directory.join("jobs.jsonl"), format!("{entry}\n")).unwrap();
    reads_result(&server, result(vec![migration(A, "Succeeded")], 1, 0)).await;
    let output = base(&server, &home)
        .args([
            "--readonly",
            "dataflow",
            "follow-upgrade",
            "--receipt-id",
            id,
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["completionStatus"], "succeeded");
    let stored = base(&server, &home)
        .args(["--readonly", "jobs", "get", "--id", id])
        .assert()
        .success();
    assert_eq!(
        parse_json(&stored)["data"]["futureEvidence"],
        json!({"retained":true})
    );
    mutation_count(&server, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cached_success_status_without_valid_result_is_unknown_not_offline_success() {
    for cached in [
        Value::Null,
        json!({"summary":{"totalCount":1,"succeededCount":1,"failedCount":0},"migrations":[]}),
    ] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        let id = "eeeeeeee-1111-2222-3333-444444444444";
        let directory = home.path().join(".fabio");
        std::fs::create_dir_all(&directory).unwrap();
        let entry = json!({"id":id,"kind":"gen1-upgrade","status":"succeeded","workspace":WS,"item_id":A,
            "target_ids":[A],"started_at":"2026-11-09T00:00:00Z","submission_state":"acknowledged","http_status":200,
            "fabric_endpoint":server.uri(),"result":cached});
        std::fs::write(directory.join("jobs.jsonl"), format!("{entry}\n")).unwrap();
        let output = base(&server, &home)
            .args([
                "--readonly",
                "dataflow",
                "follow-upgrade",
                "--receipt-id",
                id,
            ])
            .assert()
            .failure();
        assert_eq!(parse_json(&output)["data"]["completionStatus"], "unknown");
        assert_eq!(parse_json(&output)["data"]["result"], Value::Null);
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_scope_counts_or_missing_rows_never_establish_migration_success() {
    for body in [
        result(vec![migration(B, "Succeeded")], 1, 0),
        json!({"summary":{"totalCount":1,"succeededCount":1,"failedCount":0},"migrations":[]}),
        result(vec![migration(A, "Succeeded")], 0, 1),
    ] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        ready(&server, &[A]).await;
        response(&server, ResponseTemplate::new(200).set_body_json(body)).await;
        let output = submit(&server, &home).assert().failure();
        assert_eq!(parse_json(&output)["data"]["completionStatus"], "unknown");
        mutation_count(&server, 1).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_deadline_after_accepted_headers_preserves_the_receipt_without_replay() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let seed = MockServer::start().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut endpoint = reqwest::Url::parse(&seed.uri()).unwrap();
    endpoint.set_host(Some("127.0.0.1")).unwrap();
    endpoint
        .set_port(Some(listener.local_addr().unwrap().port()))
        .unwrap();
    let task = tokio::spawn(async move {
        for index in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let count = socket.read(&mut buffer).await.unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            if index == 0 {
                assert!(request.starts_with(b"GET "));
                let body =
                    json!({"value":[{"id":A,"displayName":"mock","status":"ReadyToMigrate"}]})
                        .to_string();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            } else {
                assert!(request.starts_with(b"POST "));
                socket.write_all(format!("HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nContent-Length: 20\r\nx-ms-operation-id: {OP}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    });
    let home = tempfile::TempDir::new().unwrap();
    let output = submit(&seed, &home)
        .env(
            "FABIO_FABRIC_API_ENDPOINT",
            endpoint.as_str().trim_end_matches('/'),
        )
        .args(["--timeout", "1"])
        .timeout(std::time::Duration::from_secs(8))
        .assert()
        .success();
    let data = &parse_json(&output)["data"];
    assert_eq!(data["submission"]["state"], "accepted");
    assert_eq!(data["submission"]["responseBodyStatus"], "unreadable");
    assert_eq!(data["submission"]["operationId"], OP);
    assert_eq!(data["completionStatus"], "not_checked");
    task.await.unwrap();
}
