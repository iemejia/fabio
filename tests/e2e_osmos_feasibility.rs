//! Assessment discoverability must not pretend that task mutation support shipped.
mod common;
use serde_json::Value;
use wiremock::MockServer;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn osmos_assessment_and_search_are_offline_and_do_not_launch_a_task() {
    let server = MockServer::start().await;
    let result = common::mock_fabio(&server)
        .args(["--readonly", "context", "best-practices", "project-osmos"])
        .assert()
        .success();
    let value = common::parse_json(&result);
    assert_eq!(
        value["data"]["decision"]["upstreamTaskInterfaceExists"],
        true
    );
    assert_eq!(
        value["data"]["decision"]["builtinLifecycleImplemented"],
        false
    );
    assert_eq!(value["data"]["decision"]["automaticInstall"], false);
    assert_eq!(
        value["data"]["validation"]["autonomousExecutionValidated"],
        false
    );
    let found = common::mock_fabio(&server)
        .args(["--readonly", "context", "find", "Osmos"])
        .assert()
        .success();
    let found = common::parse_json(&found);
    assert_eq!(
        found["data"]["results"][0]["command"],
        "fabio context disambiguate osmos"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[test]
#[ignore = "requires owned disposable workspace/lakehouse; core metadata only, not task execution eligibility"]
fn live_osmos_candidate_metadata_scope_is_readable_without_task_mutations() {
    let workspace = std::env::var("FABIO_BLOG_OWNED_WORKSPACE").expect("owned workspace");
    let item = std::env::var("FABIO_TEST_METADATA_ITEM").expect("owned lakehouse");
    for (group, args) in [
        ("workspace", vec!["show", "--id", &workspace]),
        (
            "lakehouse",
            vec!["show", "--workspace", &workspace, "--id", &item],
        ),
    ] {
        let output = common::fabio()
            .args(["--readonly", group])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "live metadata prerequisite unavailable; inspect privately"
        );
        let value: Value = serde_json::from_slice(&output.stdout).expect("JSON response");
        assert!(
            value["data"]["id"].as_str().is_some_and(|id| id
                == if group == "workspace" {
                    &workspace
                } else {
                    &item
                }),
            "metadata scope mismatch"
        );
        if group == "workspace" {
            assert!(
                value["data"]["capacityId"].as_str().is_some(),
                "capacity assignment unknown, not execution approval"
            );
        }
    }
}
