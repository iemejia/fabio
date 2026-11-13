//! Consolidated November API sync contracts: real request shapes, not only previews.

mod common;

use common::{mock_fabio, parse_json};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "00000000-0000-0000-0000-000000000001";
const ITEM: &str = "00000000-0000-0000-0000-000000000002";
const SCHEDULE: &str = "00000000-0000-0000-0000-000000000003";

#[test]
#[allow(clippy::too_many_lines)]
fn new_item_families_cover_crud_definitions_and_readonly() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for (group, collection, part, format) in [
        (
            "custom-stream-connector",
            "customStreamConnectors",
            Some("connector.json"),
            Some("CustomStreamConnectorDefinitionV1"),
        ),
        ("share", "shares", Some("share.json"), None),
        ("share-subscription", "shareSubscriptions", None, None),
    ] {
        let server = runtime.block_on(MockServer::start());
        let base = format!("/workspaces/{WS}/{collection}");
        let item_path = format!("{base}/{ITEM}");
        let mut create = json!({"displayName": "synthetic-item"});
        let mut extra = Vec::new();
        if part.is_none() {
            create["creationPayload"] =
                json!({"invitationUrl": "https://app.fabric.microsoft.com/share/invite/synthetic"});
            extra.extend([
                "--invitation-url",
                "https://app.fabric.microsoft.com/share/invite/synthetic",
            ]);
        }
        runtime.block_on(async {
            for (verb, route, body, response) in [
                ("POST", base.as_str(), Some(create), json!({"id": ITEM})),
                ("GET", base.as_str(), None, json!({"value": [{"id": ITEM}]})),
                ("GET", item_path.as_str(), None, json!({"id": ITEM})),
                (
                    "PATCH",
                    item_path.as_str(),
                    Some(json!({"displayName": "updated"})),
                    json!({"id": ITEM}),
                ),
                ("DELETE", item_path.as_str(), None, json!(null)),
            ] {
                let mut mock = Mock::given(method(verb)).and(path(route));
                if let Some(body) = body {
                    mock = mock.and(body_json(body));
                }
                mock.respond_with(ResponseTemplate::new(200).set_body_json(response))
                    .expect(1)
                    .mount(&server)
                    .await;
            }
        });
        mock_fabio(&server)
            .args([
                group,
                "create",
                "--workspace",
                WS,
                "--name",
                "synthetic-item",
            ])
            .args(extra)
            .assert()
            .success();
        mock_fabio(&server)
            .args(["--readonly", group, "list", "--workspace", WS])
            .assert()
            .success();
        mock_fabio(&server)
            .args(["--readonly", group, "show", "--workspace", WS, "--id", ITEM])
            .assert()
            .success();
        mock_fabio(&server)
            .args([
                group,
                "update",
                "--workspace",
                WS,
                "--id",
                ITEM,
                "--name",
                "updated",
            ])
            .assert()
            .success();
        if let Some(part) = part {
            let mut definition = json!({"parts": [{"path": part, "payload": "e30=", "payloadType": "InlineBase64"}]});
            if let Some(format) = format {
                definition["format"] = json!(format);
            }
            runtime.block_on(async {
                Mock::given(method("POST"))
                    .and(path(format!("{item_path}/getDefinition")))
                    .respond_with(
                        ResponseTemplate::new(200).set_body_json(json!({"definition": definition})),
                    )
                    .expect(1)
                    .mount(&server)
                    .await;
                Mock::given(method("POST"))
                    .and(path(format!("{item_path}/updateDefinition")))
                    .and(query_param("updateMetadata", "true"))
                    .and(body_json(json!({"definition": definition})))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                    .expect(1)
                    .mount(&server)
                    .await;
            });
            mock_fabio(&server)
                .args([
                    "--readonly",
                    group,
                    "get-definition",
                    "--workspace",
                    WS,
                    "--id",
                    ITEM,
                ])
                .assert()
                .success();
            mock_fabio(&server)
                .args([
                    group,
                    "update-definition",
                    "--workspace",
                    WS,
                    "--id",
                    ITEM,
                    "--content",
                    "{}",
                    "--update-metadata",
                ])
                .assert()
                .success();
            mock_fabio(&server)
                .args([
                    "--quiet",
                    "--readonly",
                    group,
                    "update-definition",
                    "--workspace",
                    WS,
                    "--id",
                    ITEM,
                    "--content",
                    "{}",
                ])
                .assert()
                .failure()
                .stderr(predicates::str::contains("READONLY_MODE"));
        }
        mock_fabio(&server)
            .args([
                "--quiet",
                "--readonly",
                group,
                "delete",
                "--workspace",
                WS,
                "--id",
                ITEM,
            ])
            .assert()
            .failure()
            .stderr(predicates::str::contains("READONLY_MODE"));
        mock_fabio(&server)
            .args([group, "delete", "--workspace", WS, "--id", ITEM])
            .assert()
            .success();
        let count = runtime.block_on(server.received_requests()).unwrap().len();
        assert_eq!(count, if part.is_some() { 7 } else { 5 });
    }
}

#[test]
fn destructive_previews_reject_unscoped_ids_and_carry_agent_notices() {
    for group in ["custom-stream-connector", "share", "share-subscription"] {
        let result = common::fabio()
            .env("CLAUDECODE", "1")
            .args([
                "--dry-run",
                group,
                "delete",
                "--workspace",
                WS,
                "--id",
                ITEM,
            ])
            .assert()
            .success();
        let value = parse_json(&result);
        assert_eq!(value["data"]["destructive"], true);
        assert!(value["data"]["agentNotice"].is_string());
        for id in ["", "..", "../items/other", "*"] {
            common::fabio()
                .args(["--dry-run", group, "delete", "--workspace", WS, "--id", id])
                .assert()
                .failure()
                .stderr(predicates::str::contains("INVALID_INPUT"));
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn copy_job_schedules_support_files_disabled_creation_and_crud() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(MockServer::start());
    let base = format!("/workspaces/{WS}/copyJobs/{ITEM}/jobs/execute/schedules");
    let configuration = json!({"type": "Cron", "interval": 60, "startDateTime": "2026-01-01T00:00:00", "endDateTime": "2027-01-01T00:00:00", "localTimeZoneId": "UTC"});
    runtime.block_on(async {
        Mock::given(method("POST"))
            .and(path(&base))
            .and(body_json(
                json!({"enabled": false, "configuration": configuration}),
            ))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": SCHEDULE})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(&base))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"value": [{"id": SCHEDULE}]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{base}/{SCHEDULE}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": SCHEDULE})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path(format!("{base}/{SCHEDULE}")))
            .and(body_json(json!({"enabled": true})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": SCHEDULE})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(format!("{base}/{SCHEDULE}")))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
    });
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("schedule.json");
    std::fs::write(&file, configuration.to_string()).unwrap();
    mock_fabio(&server)
        .args([
            "copy-job",
            "create-schedule",
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--enabled",
            "false",
            "--config",
            &format!("@{}", file.display()),
        ])
        .assert()
        .success();
    mock_fabio(&server)
        .args([
            "--readonly",
            "copy-job",
            "list-schedules",
            "--workspace",
            WS,
            "--id",
            ITEM,
        ])
        .assert()
        .success();
    for command in ["show-schedule", "update-schedule", "delete-schedule"] {
        let mut cmd = mock_fabio(&server);
        cmd.args([
            "copy-job",
            command,
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--schedule-id",
            SCHEDULE,
        ]);
        if command == "update-schedule" {
            cmd.args(["--enabled", "true"]);
        }
        cmd.assert().success();
    }
    mock_fabio(&server)
        .args([
            "--dry-run",
            "copy-job",
            "create-schedule",
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--config",
            "[]",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("INVALID_INPUT"));
}

#[test]
fn feedback_continuations_are_bodyless_and_attachments_never_clobber_files() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(MockServer::start());
    let base = format!("/workspaces/{WS}/dataAgents/{ITEM}/feedback");
    runtime.block_on(async {
        Mock::given(method("POST"))
            .and(path(format!("{base}/search")))
            .and(query_param("beta", "true"))
            .and(query_param("continuationToken", "opaque/+="))
            .and(|request: &wiremock::Request| request.body.is_empty())
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"value": [{"id": "synthetic"}]})),
            )
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("{base}/search")))
            .and(query_param("pageSize", "1"))
            .and(body_json(json!({})))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"value": [{"id": "first"}], "continuationToken": "opaque/+="}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{base}/synthetic/attachments")))
            .and(query_param("allowDownloadWithoutSensitivityLabel", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"synthetic-zip"))
            .expect(1)
            .mount(&server)
            .await;
    });
    mock_fabio(&server)
        .args([
            "--readonly",
            "--continuation-token",
            "opaque/+=",
            "data-agent",
            "query-feedback",
            "--workspace",
            WS,
            "--id",
            ITEM,
        ])
        .assert()
        .success();
    let listed = mock_fabio(&server)
        .args([
            "--readonly",
            "--all",
            "data-agent",
            "query-feedback",
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--page-size",
            "1",
        ])
        .assert()
        .success();
    assert_eq!(parse_json(&listed)["count"], 2);
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("attachment.zip");
    for success in [true, false] {
        let mut command = mock_fabio(&server);
        command.args([
            "--readonly",
            "data-agent",
            "download-feedback-attachments",
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--feedback-id",
            "synthetic",
            "--output-file",
            file.to_str().unwrap(),
            "--allow-download-without-sensitivity-label",
        ]);
        if success {
            command.assert().success();
        } else {
            command.assert().failure().stderr(predicates::str::contains(
                "existing files are never overwritten",
            ));
        }
    }
    assert_eq!(std::fs::read(&file).unwrap(), b"synthetic-zip");
}

#[test]
fn existing_raw_text_uploads_keep_their_content_type() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(MockServer::start());
    let route = format!("/workspaces/{WS}/apacheAirflowJobs/{ITEM}/files/dags/synthetic.py");
    runtime.block_on(async {
        Mock::given(method("PUT"))
            .and(path(route))
            .and(header("Content-Type", "text/plain"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
    });
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("synthetic.py");
    std::fs::write(&file, "# synthetic").unwrap();
    mock_fabio(&server)
        .args([
            "apache-airflow-job",
            "upload-file",
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--path",
            "dags/synthetic.py",
            "--file",
            file.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn topic_text_lros_fail_closed_on_untrusted_results_or_unverified_completion() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for (state, result_location) in [
        ("Succeeded", Some("https://example.org/result")),
        ("Succeeded", None),
        ("Unexpected", None),
    ] {
        let server = runtime.block_on(MockServer::start());
        let base = format!("/workspaces/{WS}/dataAgents/{ITEM}/staging/datasources");
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path(&base))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    json!({"value": [{"id": SCHEDULE, "displayName": "synthetic"}]}),
                ))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path(format!("{base}/{SCHEDULE}/topics/format")))
                .and(header("Accept", "text/markdown"))
                .and(|request: &wiremock::Request| request.body.is_empty())
                .respond_with(
                    ResponseTemplate::new(202)
                        .insert_header("Location", format!("{}/operations/synthetic", server.uri()))
                        .insert_header("Retry-After", "0"),
                )
                .expect(1)
                .mount(&server)
                .await;
            let mut response = ResponseTemplate::new(200).set_body_json(json!({"status": state}));
            if let Some(location) = result_location {
                response = response.insert_header("Location", location);
            }
            Mock::given(method("GET"))
                .and(path("/operations/synthetic"))
                .respond_with(response)
                .expect(1)
                .mount(&server)
                .await;
        });
        mock_fabio(&server)
            .args([
                "--readonly",
                "data-agent",
                "format-topic",
                "--workspace",
                WS,
                "--id",
                ITEM,
                "--datasource",
                "synthetic",
                "--file-name",
                "topic.md",
            ])
            .assert()
            .failure();
        assert_eq!(
            runtime.block_on(server.received_requests()).unwrap().len(),
            3
        );
    }
}

#[test]
fn soft_delete_mutations_are_scoped_guarded_and_report_bodyless_success() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(async {
        Mock::given(method("POST"))
            .and(path(format!(
                "/workspaces/{WS}/onelake/settings/modifyFileSoftDelete"
            )))
            .and(body_json(
                json!({"status": "Enabled", "retentionPeriodInDays": 14}),
            ))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
    });
    let args = [
        "workspace",
        "modify-file-soft-delete",
        "--workspace",
        WS,
        "--status",
        "Enabled",
        "--retention-days",
        "14",
    ];
    let preview = mock_fabio(&server)
        .env("CLAUDECODE", "1")
        .args(["--dry-run"])
        .args(args)
        .assert()
        .success();
    let preview = parse_json(&preview);
    assert_eq!(preview["data"]["destructive"], true);
    assert_eq!(preview["data"]["details"]["workspaceId"], WS);
    assert!(preview["data"]["agentNotice"].is_string());
    mock_fabio(&server)
        .args(["--readonly"])
        .args(args)
        .assert()
        .failure()
        .stderr(predicates::str::contains("READONLY_MODE"));
    let result = mock_fabio(&server).args(args).assert().success();
    assert_eq!(parse_json(&result)["data"]["status"], "updated");
    mock_fabio(&server)
        .args([
            "--dry-run",
            "workspace",
            "modify-file-soft-delete",
            "--workspace",
            WS,
            "--status",
            "Disabled",
            "--retention-days",
            "14",
        ])
        .assert()
        .failure();
}
