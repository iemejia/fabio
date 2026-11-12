//! Specialized mutation senders and `OneLake` move boundaries, entirely local.
mod common;

use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const ITEM: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const OP: &str = "cccccccc-1111-2222-3333-444444444444";

fn command(server: &MockServer, home: &tempfile::TempDir) -> assert_cmd::Command {
    let mut command = common::mock_fabio(server);
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("FABIO_NO_VERSION_CHECK", "1")
        .env("FABIO_POWERBI_ENDPOINT", server.uri())
        .env("FABIO_ARM_ENDPOINT", server.uri())
        .env("FABIO_ONELAKE_DFS_ENDPOINT", server.uri())
        .env("FABIO_ONELAKE_BLOB_ENDPOINT", server.uri());
    command
}

fn error(output: &assert_cmd::assert::Assert) -> Value {
    String::from_utf8_lossy(&output.get_output().stderr)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|value| value.get("error").cloned())
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn raw_binary_multipart_and_arm_failures_preserve_uncertainty_without_replay() {
    for (group, args, service) in [
        (
            "apache-airflow-job",
            vec![
                "deploy-requirements",
                "--workspace",
                WS,
                "--id",
                ITEM,
                "--content",
                "mock==1",
            ],
            "fabric",
        ),
        (
            "environment",
            vec![
                "import-staging-libraries",
                "--workspace",
                WS,
                "--id",
                ITEM,
                "--content",
                "dependencies: []",
            ],
            "fabric",
        ),
        (
            "dataflow",
            vec![
                "execute-query",
                "--workspace",
                WS,
                "--id",
                ITEM,
                "--query-name",
                "Mock",
            ],
            "fabric",
        ),
        (
            "capacity",
            vec![
                "resume",
                "--subscription",
                WS,
                "--resource-group",
                "mock-group",
                "--name",
                "mock-capacity",
            ],
            "arm",
        ),
        (
            "semantic-model",
            vec!["import-pbix", "--workspace", WS, "--name", "Mock"],
            "powerbi",
        ),
    ] {
        for status in [503, 401] {
            let server = MockServer::start().await;
            let home = tempfile::TempDir::new().unwrap();
            let file = home.path().join("mock.pbix");
            std::fs::write(&file, b"mock").unwrap();
            Mock::given(method("POST")).respond_with(ResponseTemplate::new(status).insert_header("x-ms-operation-id",OP)
                .set_body_json(json!({"error":{"message":"mock uncertain","isRetriable":true,"requestId":"mock-trace","parameters":[{"name":"mock","value":"preserved"}]}})))
                .expect(1).mount(&server).await;
            let mut invocation = command(&server, &home);
            invocation.arg(group).args(&args);
            if group == "semantic-model" {
                invocation.arg("--file").arg(&file);
            }
            let output = invocation.assert().failure();
            let error = error(&output);
            assert_eq!(error["retriable"], false, "{group}: {error}");
            assert_eq!(error["submission"]["state"], "unknown");
            assert_eq!(error["submission"]["service"], service);
            assert_eq!(error["submission"]["operationId"], OP);
            assert_eq!(error["parameters"][0]["value"], "preserved");
            assert_eq!(server.received_requests().await.unwrap().len(), 1);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_binary_and_arm_bad_poll_locations_never_receive_a_bearer_request() {
    for group in ["dataflow", "capacity"] {
        let server = MockServer::start().await;
        let untrusted = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(202)
                    .insert_header("location", format!("{}/private", untrusted.uri()))
                    .insert_header("x-ms-operation-id", OP),
            )
            .expect(1)
            .mount(&server)
            .await;
        let args = if group == "dataflow" {
            vec![
                "dataflow",
                "execute-query",
                "--workspace",
                WS,
                "--id",
                ITEM,
                "--query-name",
                "Mock",
            ]
        } else {
            vec![
                "capacity",
                "resume",
                "--subscription",
                WS,
                "--resource-group",
                "mock",
                "--name",
                "mock",
            ]
        };
        let output = command(&server, &home).args(args).assert().failure();
        assert_eq!(error(&output)["submission"]["state"], "accepted");
        assert_eq!(untrusted.received_requests().await.unwrap().len(), 0);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn onelake_move_never_falls_back_after_uncertainty_or_deletes_after_unverified_copy() {
    for (rename_status, copy_verified, expected) in
        [(503, false, 1), (400, false, 2), (400, true, 3)]
    {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        Mock::given(method("PUT"))
            .and(|request: &wiremock::Request| request.headers.contains_key("x-ms-rename-source"))
            .respond_with(
                ResponseTemplate::new(rename_status)
                    .set_body_json(json!({"error":{"message":"UnsupportedHeader"}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let copy = ResponseTemplate::new(202);
        let copy = if copy_verified {
            copy.insert_header("x-ms-copy-status", "success")
        } else {
            copy
        };
        Mock::given(method("PUT"))
            .and(|request: &wiremock::Request| request.headers.contains_key("x-ms-copy-source"))
            .respond_with(copy)
            .expect(u64::from(expected != 1))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .expect(u64::from(copy_verified))
            .mount(&server)
            .await;
        let mut invocation = command(&server, &home);
        invocation.args([
            "lakehouse",
            "move-file",
            "--source-workspace",
            WS,
            "--source-id",
            ITEM,
            "--source-path",
            "Files/source",
            "--dest-workspace",
            WS,
            "--dest-id",
            ITEM,
            "--dest-path",
            "Files/destination",
        ]);
        if copy_verified {
            invocation.assert().success();
        } else {
            let output = invocation.assert().failure();
            assert_eq!(error(&output)["retriable"], false);
        }
        assert_eq!(server.received_requests().await.unwrap().len(), expected);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn onelake_upload_checks_every_step_and_never_continues_after_failure() {
    for failed_step in 0..3 {
        let server = MockServer::start().await;
        let home = tempfile::TempDir::new().unwrap();
        let file = home.path().join("mock.txt");
        std::fs::write(&file, "mock").unwrap();
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(if failed_step == 0 {
                503
            } else {
                201
            }))
            .expect(1)
            .mount(&server)
            .await;
        for (index, action) in [(1, "append"), (2, "flush")] {
            Mock::given(method("PATCH"))
                .and(wiremock::matchers::query_param("action", action))
                .respond_with(ResponseTemplate::new(if failed_step == index {
                    503
                } else {
                    202
                }))
                .expect(u64::from(failed_step >= index))
                .mount(&server)
                .await;
        }
        let output = command(&server, &home)
            .args([
                "lakehouse",
                "upload",
                "--workspace",
                WS,
                "--id",
                ITEM,
                "--source-path",
            ])
            .arg(&file)
            .args(["--dest-path", "Files/mock.txt"])
            .assert()
            .failure();
        let error = error(&output);
        assert_eq!(error["retriable"], false);
        assert_eq!(error["submission"]["service"], "onelake");
        assert_eq!(error["submission"]["targetId"], ITEM);
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            failed_step + 1
        );
    }
}
