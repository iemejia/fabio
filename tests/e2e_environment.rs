//! End-to-end integration tests for `fabio environment` commands.

mod common;

use common::{TestConfig, extract_data, fabio, mock_fabio, parse_json};
use serial_test::serial;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MOCK_WORKSPACE: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const MOCK_ENVIRONMENT: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const MOCK_ACTIVATION: &str = "cccccccc-1111-2222-3333-444444444444";
const MOCK_SCHEDULE: &str = "dddddddd-1111-2222-3333-444444444444";

#[test]
fn test_run_custom_live_pool_dry_run() {
    let output = fabio()
        .args([
            "--dry-run",
            "environment",
            "run-custom-live-pool",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&output)).clone();
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["environmentId"], MOCK_ENVIRONMENT);
}

#[test]
fn test_create_custom_live_pool_schedule_dry_run() {
    let output = fabio()
        .args([
            "--dry-run",
            "environment",
            "create-custom-live-pool-schedule",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
            "--content",
            r#"{"enabled":true,"configuration":{"type":"Cron","startDateTime":"2026-10-01T00:00:00Z","endDateTime":"2027-10-01T00:00:00Z","localTimeZoneId":"Central Standard Time","interval":1440}}"#,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&output)).clone();
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["request"]["enabled"], true);
    assert_eq!(
        data["details"]["request"]["configuration"]["interval"],
        1440
    );
}

#[test]
fn test_update_custom_live_pool_schedule_dry_run() {
    let output = fabio()
        .args([
            "--dry-run",
            "environment",
            "update-custom-live-pool-schedule",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
            "--schedule-id",
            MOCK_SCHEDULE,
            "--content",
            r#"{"enabled":false,"configuration":{"type":"Cron","startDateTime":"2026-10-01T00:00:00Z","endDateTime":"2027-10-01T00:00:00Z","localTimeZoneId":"Central Standard Time","interval":720}}"#,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&output)).clone();
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["details"]["scheduleId"], MOCK_SCHEDULE);
    assert_eq!(data["details"]["request"]["enabled"], false);
}

#[test]
fn test_delete_custom_live_pool_schedule_dry_run() {
    let output = fabio()
        .args([
            "--dry-run",
            "environment",
            "delete-custom-live-pool-schedule",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
            "--schedule-id",
            MOCK_SCHEDULE,
        ])
        .assert()
        .success();
    let data = extract_data(&parse_json(&output)).clone();
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["destructive"], true);
    assert_eq!(data["details"]["scheduleId"], MOCK_SCHEDULE);
}

#[test]
#[serial]
fn environment_get_publish_log_preserves_download_metadata() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/staging/publish/log"
            )))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header(
                        "Content-Disposition",
                        "attachment; filename=\"publish-log.log\"",
                    )
                    .insert_header("x-ms-log-truncated", "true")
                    .set_body_string("partial\n--- LOG TRUNCATED AT 50000000 BYTES ---"),
            )
            .expect(1)
            .mount(&server)
            .await;
        server
    });
    let output = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server.uri())
        .args([
            "environment",
            "get-publish-log",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
        ])
        .assert()
        .success();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["truncated"], true);
    assert_eq!(
        data["contentDisposition"],
        "attachment; filename=\"publish-log.log\""
    );
    assert!(
        data["content"]
            .as_str()
            .unwrap()
            .ends_with("--- LOG TRUNCATED AT 50000000 BYTES ---")
    );
}

#[test]
#[serial]
fn environment_staging_spark_compute_explicitly_requests_beta() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (server_uri, _server) = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/staging/sparkcompute"
            )))
            .and(query_param("beta", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "runtimeVersion": "1.3",
                "sparkProperties": {}
            })))
            .mount(&server)
            .await;
        (server.uri(), server)
    });

    let output = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server_uri)
        .args([
            "environment",
            "get-staging-spark-settings",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["runtimeVersion"], "1.3");
}

#[test]
#[serial]
fn environment_update_staging_spark_compute_explicitly_requests_beta() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let body = serde_json::json!({"runtimeVersion": "2.0"});
    let (server_uri, _server) = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path(format!(
                "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/staging/sparkcompute"
            )))
            .and(query_param("beta", "true"))
            .and(body_json(body.clone()))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        (server.uri(), server)
    });

    let output = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server_uri)
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
            "--content",
            r#"{"runtimeVersion":"2.0"}"#,
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["runtimeVersion"], "2.0");
}

#[test]
#[serial]
fn environment_transition_endpoints_explicitly_request_beta() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (server_uri, _server) = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!(
                "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/staging/publish"
            )))
            .and(query_param("beta", "true"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/sparkcompute"
            )))
            .and(query_param("beta", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sparkProperties": {}
            })))
            .mount(&server)
            .await;
        for endpoint_path in [
            format!("/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/libraries"),
            format!(
                "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/staging/libraries"
            ),
        ] {
            Mock::given(method("GET"))
                .and(path(endpoint_path))
                .and(query_param("beta", "true"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({"customLibraries": []})),
                )
                .mount(&server)
                .await;
        }

        (server.uri(), server)
    });

    for arguments in [
        vec!["publish"],
        vec!["get-spark-settings"],
        vec!["list-libraries"],
        vec!["list-staging-libraries"],
    ] {
        fabio()
            .env("FABIO_ACCESS_TOKEN", "fake-test-token")
            .env("FABIO_FABRIC_API_ENDPOINT", &server_uri)
            .args([
                "environment",
                arguments[0],
                "--workspace",
                MOCK_WORKSPACE,
                "--id",
                MOCK_ENVIRONMENT,
            ])
            .assert()
            .success();
    }
}

#[test]
#[serial]
fn test_list_pool_activations_sends_pagination_filters() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (server_uri, _server) = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/poolActivations"
            )))
            .and(query_param("pageSize", "25"))
            .and(query_param("filter", "state eq 'InProgress'"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [{"jobInstanceId": MOCK_ACTIVATION, "state": "InProgress"}]
            })))
            .mount(&server)
            .await;
        (server.uri(), server)
    });
    let output = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server_uri)
        .args([
            "environment",
            "list-pool-activations",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
            "--page-size",
            "25",
            "--filter",
            "state eq 'InProgress'",
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"][0]["state"], "InProgress");
}

#[test]
#[serial]
fn test_show_pool_activation_returns_diagnostics() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (server_uri, _server) = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/poolActivations/{MOCK_ACTIVATION}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jobInstanceId": MOCK_ACTIVATION,
                "state": "Succeeded",
                "runtimeVersion": "2.0",
                "sparkConfiguration": {"driverCores": 4},
                "resourceStatusSummary": {"ready": 2}
            })))
            .mount(&server)
            .await;
        (server.uri(), server)
    });
    let output = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server_uri)
        .args([
            "environment",
            "show-pool-activation",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
            "--activation-id",
            MOCK_ACTIVATION,
        ])
        .assert()
        .success();
    assert_eq!(
        parse_json(&output)["data"]["resourceStatusSummary"]["ready"],
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_live_pool_run_returns_the_accepted_job_without_replaying() {
    let server = MockServer::start().await;
    let route = format!(
        "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/jobs/runCustomLivePool/instances"
    );
    let location = format!(
        "https://api.fabric.microsoft.com/v1/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/jobs/instances/{MOCK_ACTIVATION}"
    );
    Mock::given(method("POST"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(202).insert_header("Location", location))
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args([
            "environment",
            "run-custom-live-pool",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
        ])
        .assert()
        .success();
    assert_eq!(
        parse_json(&output)["data"]["jobInstanceId"],
        MOCK_ACTIVATION
    );
    assert_eq!(server.received_requests().await.unwrap()[0].body.len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_live_pool_schedule_crud_uses_the_background_job_route() {
    use wiremock::matchers::body_json;
    let server = MockServer::start().await;
    let base = format!(
        "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/jobs/runCustomLivePool/schedules"
    );
    let schedule = "dddddddd-1111-2222-3333-444444444444";
    let item = format!("{base}/{schedule}");
    let body =
        serde_json::json!({"enabled": false, "configuration": {"type": "Cron", "interval": 1440}});
    let raw = body.to_string();
    for (command, verb, route, extra, response) in [
        (
            "create-custom-live-pool-schedule",
            "POST",
            base.as_str(),
            vec!["--content", raw.as_str()],
            serde_json::json!({"id": schedule, "enabled": false}),
        ),
        (
            "list-custom-live-pool-schedules",
            "GET",
            base.as_str(),
            vec![],
            serde_json::json!({"value": [{"id": schedule}], "continuationToken": "next"}),
        ),
        (
            "show-custom-live-pool-schedule",
            "GET",
            item.as_str(),
            vec!["--schedule-id", schedule],
            serde_json::json!({"id": schedule}),
        ),
        (
            "update-custom-live-pool-schedule",
            "PATCH",
            item.as_str(),
            vec!["--schedule-id", schedule, "--content", raw.as_str()],
            serde_json::json!({"id": schedule, "enabled": false}),
        ),
        (
            "delete-custom-live-pool-schedule",
            "DELETE",
            item.as_str(),
            vec!["--schedule-id", schedule],
            serde_json::json!({}),
        ),
    ] {
        let mut mock = Mock::given(method(verb)).and(path(route));
        if matches!(verb, "POST" | "PATCH") {
            mock = mock.and(body_json(&body));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&server)
            .await;
        let mut args = vec![
            "environment",
            command,
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
        ];
        args.extend(extra);
        let output = mock_fabio(&server).args(args).assert().success();
        let response = parse_json(&output);
        if command == "list-custom-live-pool-schedules" {
            assert_eq!(response["count"], 1);
            assert_eq!(response["continuationToken"], "next");
        } else {
            assert_eq!(response["data"]["id"], schedule);
        }
        server.verify().await;
        server.reset().await;
    }
}

#[test]
#[ignore = "creates an owned environment in FABIO_TEST_SOURCE_WORKSPACE; schedules stay disabled"]
fn custom_live_pool_disabled_schedule_lifecycle_live() {
    let fixture = common::ScopedTestItem::create("environment", "pool_schedule");
    let start = chrono::Utc::now() + chrono::Duration::days(1);
    let end = start + chrono::Duration::days(1);
    let body = serde_json::json!({"enabled": false, "configuration": {
        "type": "Cron", "interval": 1440, "localTimeZoneId": "UTC",
        "startDateTime": start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "endDateTime": end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }});
    let output = fixture
        .command("create-custom-live-pool-schedule")
        .args(["--content", &body.to_string()])
        .assert()
        .success();
    let response = parse_json(&output);
    let schedule = response["data"]["id"].as_str().unwrap();
    assert_eq!(response["data"]["enabled"], false);
    let output = fixture
        .command("show-custom-live-pool-schedule")
        .args(["--schedule-id", schedule, "--readonly"])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["id"], schedule);
    let mut revised = body;
    revised["configuration"]["interval"] = serde_json::json!(720);
    let output = fixture
        .command("update-custom-live-pool-schedule")
        .args(["--schedule-id", schedule, "--content", &revised.to_string()])
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"]["enabled"], false);
    assert_eq!(
        parse_json(&output)["data"]["configuration"]["interval"],
        720
    );
    fixture
        .command("delete-custom-live-pool-schedule")
        .args(["--schedule-id", schedule])
        .assert()
        .success();
    let output = fixture
        .command("list-custom-live-pool-schedules")
        .arg("--readonly")
        .assert()
        .success();
    assert_eq!(parse_json(&output)["data"], serde_json::json!([]));
    fixture.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_live_pool_schedule_invalid_inputs_and_readonly_never_dispatch() {
    let server = MockServer::start().await;
    for body in [
        "[]",
        "{}",
        r#"{"enabled":"yes","configuration":{}}"#,
        r#"{"enabled":false,"configuration":null}"#,
    ] {
        mock_fabio(&server)
            .args([
                "environment",
                "create-custom-live-pool-schedule",
                "--workspace",
                MOCK_WORKSPACE,
                "--id",
                MOCK_ENVIRONMENT,
                "--content",
                body,
                "--dry-run",
            ])
            .assert()
            .failure();
    }
    for (command, extra) in [
        ("run-custom-live-pool", vec![]),
        (
            "create-custom-live-pool-schedule",
            vec!["--content", r#"{"enabled":false,"configuration":{}}"#],
        ),
        (
            "update-custom-live-pool-schedule",
            vec![
                "--schedule-id",
                MOCK_ACTIVATION,
                "--content",
                r#"{"enabled":false,"configuration":{}}"#,
            ],
        ),
        (
            "delete-custom-live-pool-schedule",
            vec!["--schedule-id", MOCK_ACTIVATION],
        ),
    ] {
        let mut args = vec![
            "--readonly",
            "environment",
            command,
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
        ];
        args.extend(extra);
        let output = mock_fabio(&server).args(args).assert().failure();
        let error: serde_json::Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
        assert_eq!(error["error"]["code"], "READONLY_MODE", "{command}");
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pool_activation_pagination_preserves_state_and_page_size() {
    let server = MockServer::start().await;
    let url =
        format!("/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/poolActivations");
    for (token, response) in [
        (
            "",
            serde_json::json!({"value": [{"jobInstanceId": MOCK_ACTIVATION}], "continuationToken": "next-page"}),
        ),
        (
            "next-page",
            serde_json::json!({"value": [{"jobInstanceId": MOCK_ENVIRONMENT}]}),
        ),
    ] {
        let mut mock = Mock::given(method("GET"))
            .and(path(&url))
            .and(query_param("pageSize", "10"))
            .and(query_param("filter", "state eq 'Succeeded'"));
        if !token.is_empty() {
            mock = mock.and(query_param("continuationToken", token));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(response))
            .with_priority(if token.is_empty() { 2 } else { 1 })
            .expect(1)
            .mount(&server)
            .await;
    }
    let output = mock_fabio(&server)
        .args([
            "environment",
            "list-pool-activations",
            "--workspace",
            MOCK_WORKSPACE,
            "--id",
            MOCK_ENVIRONMENT,
            "--page-size",
            "10",
            "--filter",
            "state eq 'Succeeded'",
            "--all",
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    assert_eq!(json["count"], 2);
    assert_eq!(json["data"][0]["jobInstanceId"], MOCK_ACTIVATION);
    assert_eq!(json["data"][1]["jobInstanceId"], MOCK_ENVIRONMENT);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pool_activation_published_query_examples_execute() {
    let output = fabio()
        .args(["context", "examples", "environment", "show-pool-activation"])
        .assert()
        .success();
    let example = parse_json(&output)["data"].clone();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/workspaces/{MOCK_WORKSPACE}/environments/{MOCK_ENVIRONMENT}/poolActivations/{MOCK_ACTIVATION}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(&example["response"]["data"]))
        .expect(2).mount(&server).await;
    let queries = example["query_examples"].as_array().unwrap();
    assert_eq!(queries.len(), 2);
    for (entry, expected) in queries
        .iter()
        .zip([serde_json::json!(2), serde_json::json!("2.0")])
    {
        let query = entry["query"].as_str().unwrap();
        let output = mock_fabio(&server)
            .args([
                "environment",
                "show-pool-activation",
                "--workspace",
                MOCK_WORKSPACE,
                "--id",
                MOCK_ENVIRONMENT,
                "--activation-id",
                MOCK_ACTIVATION,
                "--query",
                query,
            ])
            .assert()
            .success();
        assert_eq!(parse_json(&output)["data"], expected, "{query}");
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["environment", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("env_test");

    // Create
    let assert = fabio()
        .args([
            "environment",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let env_id = data["id"].as_str().unwrap().to_string();

    // Delete
    let assert = fabio()
        .args([
            "environment",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_update_name() {
    let cfg = TestConfig::from_env();
    let original = common::unique_name("env_upd_o");
    let updated = common::unique_name("env_upd_n");

    // Create
    let assert = fabio()
        .args([
            "environment",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &original,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let env_id = data["id"].as_str().unwrap().to_string();

    // Update
    let assert = fabio()
        .args([
            "environment",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &env_id,
            "--name",
            &updated,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], updated);

    // Cleanup
    fabio()
        .args([
            "environment",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "environment",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "environment",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-dry-run",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "environment create");
}

// ─── Upload Staging Library ─────────────────────────────────────────────────

#[test]
fn environment_upload_staging_library_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "upload-staging-library",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--file",
            "Cargo.toml",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "environment upload-staging-library");
    assert_eq!(data["details"]["libraryName"], "Cargo.toml");
    assert!(data["details"]["sizeBytes"].as_u64().unwrap() > 0);
}

#[test]
fn environment_upload_staging_library_custom_name_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "upload-staging-library",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--file",
            "Cargo.toml",
            "--library-name",
            "my_lib-1.0.0.whl",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["details"]["libraryName"], "my_lib-1.0.0.whl");
}

#[test]
fn environment_upload_staging_library_missing_file() {
    let assert = fabio()
        .args([
            "environment",
            "upload-staging-library",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--file",
            "/nonexistent/path/lib.whl",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("Failed to read file"));
}

// ─── Staging Spark Compute (runtime version + spark properties) ──────────────

#[test]
fn environment_update_staging_spark_compute_typed_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--runtime-version",
            "2.0",
            "--spark-property",
            "spark.native.enabled=true",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["would_execute"],
        "environment update-staging-spark-compute"
    );
    assert_eq!(data["details"]["runtimeVersion"], "2.0");
    assert_eq!(
        data["details"]["sparkProperties"]["spark.native.enabled"],
        "true"
    );
}

#[test]
fn environment_update_staging_spark_compute_live_pool_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--custom-live-pool-support",
            "Enabled",
            "--max-clusters-to-hydrate",
            "4",
            "--cluster-idle-timeout",
            "PT20M",
            "--custom-live-pool-lifespan",
            "PT1H",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["details"]["customLivePoolSupport"], "Enabled");
    assert_eq!(
        data["details"]["customLivePoolSettings"]["maxClustersToHydrate"],
        4
    );
    assert_eq!(
        data["details"]["customLivePoolSettings"]["clusterIdleTimeout"],
        "PT20M"
    );
    assert_eq!(
        data["details"]["customLivePoolSettings"]["customLivePoolLifespan"],
        "PT1H"
    );
}

#[test]
fn environment_update_staging_spark_compute_clear_live_pool_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--clear-custom-live-pool-settings",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data["details"]["customLivePoolSettings"].is_null());
}

#[test]
fn environment_update_staging_spark_compute_requires_input() {
    // Neither --file/--content nor --runtime-version/--spark-property provided.
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("typed Spark compute flag"));
}

#[test]
fn environment_update_staging_spark_compute_content_conflicts_with_typed() {
    // clap should reject combining raw JSON with typed override flags.
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--content",
            "{}",
            "--runtime-version",
            "2.0",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("cannot be used with"));
}

#[test]
fn environment_update_staging_spark_compute_rejects_bad_property() {
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--spark-property",
            "no-equals-sign",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("KEY=VALUE"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_staging_spark_compute_runtime_and_properties_lifecycle() {
    struct EnvironmentCleanup {
        workspace: String,
        id: String,
    }

    impl Drop for EnvironmentCleanup {
        fn drop(&mut self) {
            let _ = fabio()
                .args([
                    "environment",
                    "delete",
                    "--workspace",
                    &self.workspace,
                    "--id",
                    &self.id,
                ])
                .assert();
        }
    }

    let cfg = TestConfig::from_env();
    let name = common::unique_name("env_spark_rt");

    // Create a throwaway environment.
    let assert = fabio()
        .args([
            "environment",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let env_id = extract_data(&json)["id"].as_str().unwrap().to_string();
    let _cleanup = EnvironmentCleanup {
        workspace: cfg.source_workspace.clone(),
        id: env_id.clone(),
    };

    // Set runtime 2.0 + a spark property (read-merge-write).
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
            "--runtime-version",
            "2.0",
            "--spark-property",
            "spark.native.enabled=true",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["runtimeVersion"], "2.0");
    assert_eq!(data["sparkProperties"]["spark.native.enabled"], "true");

    // Add a second property; runtime + first property must be preserved (merge).
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
            "--spark-property",
            "spark.remote.shuffle.enabled=true",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["runtimeVersion"], "2.0");
    assert_eq!(data["sparkProperties"]["spark.native.enabled"], "true");
    assert_eq!(
        data["sparkProperties"]["spark.remote.shuffle.enabled"],
        "true"
    );

    // Configure custom live-pool hydration and verify the new response fields.
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
            "--custom-live-pool-support",
            "Enabled",
            "--max-clusters-to-hydrate",
            "1",
            "--cluster-idle-timeout",
            "PT20M",
            "--custom-live-pool-lifespan",
            "PT30M",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["customLivePoolSupport"], "Enabled");
    assert_eq!(data["customLivePoolSettings"]["maxClustersToHydrate"], 1);
    assert_eq!(
        data["customLivePoolSettings"]["clusterIdleTimeout"],
        "PT20M"
    );
    assert_eq!(
        data["customLivePoolSettings"]["customLivePoolLifespan"],
        "PT30M"
    );
    assert!(
        data["instancePool"]["maxClustersToHydrateLimit"]
            .as_i64()
            .is_some_and(|limit| limit >= 1)
    );

    // Read back via get-staging-spark-settings.
    let assert = fabio()
        .args([
            "environment",
            "get-staging-spark-settings",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["runtimeVersion"], "2.0");
    assert_eq!(data["customLivePoolSupport"], "Enabled");

    // Exercise null removal semantics and retain the explicit disabled state.
    fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
            "--custom-live-pool-support",
            "Disabled",
            "--clear-custom-live-pool-settings",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
}

// ─── External libraries (import/export environment.yml) ──────────────────────

#[test]
fn environment_import_staging_libraries_dry_run() {
    let dir = tempfile::tempdir().unwrap();
    let yml = dir.path().join("environment.yml");
    std::fs::write(&yml, "dependencies:\n  - pip:\n    - requests==2.32.3\n").unwrap();

    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "import-staging-libraries",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--file",
            yml.to_str().unwrap(),
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["would_execute"],
        "environment import-staging-libraries"
    );
    assert!(data["details"]["contentLength"].as_u64().unwrap() > 0);
}

#[test]
fn environment_import_staging_libraries_requires_input() {
    let assert = fabio()
        .args([
            "environment",
            "import-staging-libraries",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("--file") || stderr.contains("--content"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_external_libraries_roundtrip() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("env_extlibs");

    // Create a throwaway environment.
    let assert = fabio()
        .args([
            "environment",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let env_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Import an environment.yml (octet-stream, not JSON).
    let yml = "dependencies:\n  - pip:\n    - requests==2.32.3\n";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("environment.yml");
    std::fs::write(&path, yml).unwrap();
    let assert = fabio()
        .args([
            "environment",
            "import-staging-libraries",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
            "--file",
            path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    assert_eq!(
        extract_data(&parse_json(&assert))["status"],
        "libraries_imported"
    );

    // Export it back and confirm the content round-trips (raw text, not JSON).
    let assert = fabio()
        .args([
            "environment",
            "export-staging-libraries",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let exported = data["externalLibraries"].as_str().unwrap();
    assert!(
        exported.contains("dependencies") && exported.contains("requests"),
        "expected the imported environment.yml back, got: {exported}"
    );

    // Clean up.
    fabio()
        .args([
            "environment",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();
}

#[test]
fn environment_cancel_publish_dry_run_is_guarded() {
    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "cancel-publish",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(
        extract_data(&json)["would_execute"],
        "environment cancel-publish"
    );
}

// remove-staging-library requires BOTH name and version (the API rejects the
// old {libraryToRemove} body). Offline dry-run regression.
#[test]
fn environment_remove_staging_library_sends_name_and_version() {
    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "remove-staging-library",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
            "--library-name",
            "fuzzywuzzy",
            "--library-version",
            "0.18.0",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["details"]["name"], "fuzzywuzzy");
    assert_eq!(data["details"]["version"], "0.18.0");
}

#[test]
fn environment_remove_staging_library_requires_version() {
    fabio()
        .args([
            "environment",
            "remove-staging-library",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
            "--library-name",
            "fuzzywuzzy",
        ])
        .assert()
        .failure();
}
