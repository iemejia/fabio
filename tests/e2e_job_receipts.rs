//! Local-only receipt persistence, exact observation and cancellation regressions.
mod common;

use common::mock_fabio;
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const ITEM: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const JOB: &str = "cccccccc-1111-2222-3333-444444444444";

fn command(server: &MockServer, home: &tempfile::TempDir, group: &str) -> assert_cmd::Command {
    let mut command = mock_fabio(server);
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("FABIO_NO_VERSION_CHECK", "1");
    command.args([
        group,
        if group == "job-scheduler" {
            "run-on-demand"
        } else {
            "run"
        },
        "--workspace",
        WS,
        "--id",
        ITEM,
    ]);
    if group == "job-scheduler" {
        command.args(["--job-type", "RunNotebook"]);
    }
    command
}

fn data(output: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice::<Value>(&output.get_output().stdout).unwrap()["data"].clone()
}

fn trigger() -> ResponseTemplate {
    ResponseTemplate::new(202).insert_header(
        "location",
        format!(
            "https://api.fabric.microsoft.com/v1/workspaces/{WS}/items/{ITEM}/jobs/instances/{JOB}"
        ),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_runner_preserves_acknowledgement_when_local_storage_is_unavailable() {
    for group in [
        "notebook",
        "dataflow",
        "data-build-tool-job",
        "job-scheduler",
        "copy-job",
        "data-pipeline",
        "spark-job-definition",
    ] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        std::fs::write(home.path().join(".fabio"), "blocked").unwrap();
        Mock::given(method("POST"))
            .respond_with(trigger())
            .expect(1)
            .mount(&server)
            .await;
        let output = command(&server, &home, group).assert().success();
        let result = data(&output);
        assert_eq!(result["receiptPersisted"], false, "{group}");
        assert_eq!(result["retryAllowed"], false);
        assert_eq!(result["jobReceipt"]["id"], JOB);
        assert_eq!(result["jobReceipt"]["workspace"], WS);
        assert_eq!(result["jobReceipt"]["item_id"], ITEM);
        assert_eq!(result["jobReceipt"]["submission_state"], "accepted");
        assert_eq!(result["warnings"][0]["code"], "RECEIPT_PERSISTENCE_FAILED");
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn observation_failure_retains_the_exact_job_without_suggesting_replay() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    Mock::given(method("POST"))
        .respond_with(trigger())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403).set_body_json(
            json!({"errorCode":"Denied","message":"mock denied","requestId":"mock-trace"}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&server, &home, "notebook")
        .arg("--wait")
        .assert()
        .failure();
    assert_eq!(data(&output)["jobReceipt"]["id"], JOB);
    assert_eq!(data(&output)["receiptPersisted"], true);
    let error = String::from_utf8_lossy(&output.get_output().stderr)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value.get("error").is_some())
        .unwrap();
    assert_eq!(error["error"]["requestId"], "mock-trace");
    assert_eq!(error["error"]["retriable"], false);
    assert!(
        error["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("Do not resubmit")
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_acknowledgement_or_failure_never_marks_a_job_cancelled() {
    for status in [200, 503] {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        Mock::given(method("POST"))
            .respond_with(trigger())
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wiremock::matchers::path_regex("/cancel$"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(json!({"error":{"message":"mock cancel failure"}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let output = command(&server, &home, "notebook")
            .args(["--wait", "--timeout", "0", "--cancel-on-timeout"])
            .assert()
            .failure();
        let result = data(&output);
        assert_eq!(
            result["jobReceipt"]["status"],
            if status == 200 {
                "cancel_requested"
            } else {
                "timeout"
            }
        );
        assert!(result["jobReceipt"].get("completed_at").is_none());
        assert_eq!(result["retryAllowed"], false);
        if status == 503 {
            assert_eq!(result["cancelError"]["submission"]["state"], "unknown");
        }
        let ledger =
            std::fs::read_to_string(home.path().join(".fabio").join("jobs.jsonl")).unwrap();
        assert!(!ledger.contains("\"status\":\"cancelled\""));
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_observation_preserves_remote_result_when_receipt_update_fails() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let ledger = home.path().join(".fabio").join("jobs.jsonl");
    Mock::given(method("POST"))
        .respond_with(trigger())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(move |_: &wiremock::Request| {
            std::fs::write(&ledger, "corrupt-private-evidence").unwrap();
            ResponseTemplate::new(200).set_body_json(json!({"status":"Completed"}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&server, &home, "notebook")
        .arg("--wait")
        .assert()
        .success();
    let result = data(&output);
    assert_eq!(result["status"], "Completed");
    assert_eq!(result["receiptPersisted"], false);
    assert_eq!(result["jobReceipt"]["status"], "completed");
    assert_eq!(result["warnings"][0]["code"], "RECEIPT_PERSISTENCE_FAILED");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}
