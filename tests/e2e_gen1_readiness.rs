//! Post-upgrade acceptance discovery and observation primitives, no live fixtures.
mod common;

use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const ITEM: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const JOB: &str = "cccccccc-1111-2222-3333-444444444444";

fn command(server: &MockServer, home: &tempfile::TempDir) -> assert_cmd::Command {
    let mut command = common::mock_fabio(server);
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("FABIO_NO_VERSION_CHECK", "1")
        .arg("--readonly");
    command
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_upgrade_acceptance_is_discoverable_and_never_approves_mutations() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let output = command(&server, &home)
        .args(["context", "workflow", "gen1-upgrade-readiness"])
        .assert()
        .success();
    let value: Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    let workflow = &value["data"];
    assert_eq!(
        workflow["acceptance_gate"]["automatic_mutation_allowed"],
        false
    );
    assert_eq!(workflow["acceptance_gate"]["retry_allowed"], false);
    assert_eq!(workflow["steps"].as_array().unwrap().len(), 8);
    assert!(
        workflow["acceptance_gate"]["rule"]
            .as_str()
            .unwrap()
            .contains("unknown/not_checked")
    );
    command(&server, &home)
        .args(["context", "best-practices", "post-gen1-readiness"])
        .assert()
        .success();
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upgraded_item_definition_and_exact_job_can_be_observed_under_readonly() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/dataflows/{ITEM}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id":ITEM,"type":"Dataflow"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/workspaces/{WS}/dataflows/{ITEM}/getDefinition"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"definition":{"parts":[]}})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/workspaces/{WS}/items/{ITEM}/jobs/instances/{JOB}"
        )))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id":JOB,"status":"Completed"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    for args in [
        vec!["dataflow", "show", "--workspace", WS, "--id", ITEM],
        vec![
            "dataflow",
            "get-definition",
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--decode",
        ],
        vec![
            "job-scheduler",
            "get-instance",
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--job-instance-id",
            JOB,
        ],
    ] {
        command(&server, &home).args(args).assert().success();
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}
