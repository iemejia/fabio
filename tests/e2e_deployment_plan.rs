mod common;

use common::{TestConfig, extract_data, fabio, mock_fabio, parse_json};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WORKSPACE: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const PLAN: &str = "bbbbbbbb-1111-2222-3333-444444444444";

#[test]
#[ignore = "requires live Fabric tenant"]
fn test_list_live() {
    let cfg = TestConfig::from_env();
    let output = fabio()
        .args([
            "deployment-plan",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    assert!(json["data"].is_array());
    assert!(json["count"].is_number());
}

#[test]
fn test_create_dry_run_serializes_definition() {
    let output = fabio()
        .args([
            "--dry-run",
            "deployment-plan",
            "create",
            "--workspace",
            WORKSPACE,
            "--name",
            "Ordered deployment",
            "--content",
            "executableFormat: DeploymentPlanV1",
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "deployment-plan create");
    assert_eq!(
        data["details"]["request"]["definition"]["parts"][0]["path"],
        "plan.yml"
    );
}

#[test]
fn test_update_definition_dry_run_includes_metadata_flag() {
    let output = fabio()
        .args([
            "--dry-run",
            "deployment-plan",
            "update-definition",
            "--workspace",
            WORKSPACE,
            "--id",
            PLAN,
            "--content",
            "executableFormat: DeploymentPlanV1",
            "--update-metadata",
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "deployment-plan update-definition");
    assert_eq!(data["destructive"], true);
    assert_eq!(data["details"]["updateMetadata"], true);
}

#[test]
fn test_delete_dry_run_is_destructive() {
    let output = fabio()
        .args([
            "--dry-run",
            "deployment-plan",
            "delete",
            "--workspace",
            WORKSPACE,
            "--id",
            PLAN,
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "deployment-plan delete");
    assert_eq!(data["destructive"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deployment_plan_update_preview_identifies_the_target_without_requests() {
    let server = MockServer::start().await;
    for (workspace, plan) in [(WORKSPACE, PLAN), (PLAN, WORKSPACE)] {
        let output = mock_fabio(&server)
            .args([
                "--dry-run",
                "deployment-plan",
                "update",
                "--workspace",
                workspace,
                "--id",
                plan,
                "--name",
                "Updated name",
            ])
            .assert()
            .success();
        let data = parse_json(&output)["data"].clone();
        assert_eq!(data["dry_run"], true);
        assert_eq!(data["would_execute"], "deployment-plan update");
        assert_eq!(data["details"]["workspace"], workspace);
        assert_eq!(data["details"]["id"], plan);
        assert_eq!(data["details"]["displayName"], "Updated name");
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deployment_plan_delete_rejects_invalid_scope_before_dry_run() {
    let server = MockServer::start().await;
    for invalid in [
        "",
        "..",
        "../../../workspaces/other",
        "%2e%2e",
        "bad?id=other",
    ] {
        for (workspace, id, flag) in [(invalid, PLAN, "--workspace"), (WORKSPACE, invalid, "--id")]
        {
            for dry_run in [false, true] {
                let mut cmd = mock_fabio(&server);
                cmd.args([
                    "deployment-plan",
                    "delete",
                    "--workspace",
                    workspace,
                    "--id",
                    id,
                ]);
                if dry_run {
                    cmd.arg("--dry-run");
                }
                let output = cmd.assert().failure();
                let json: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
                assert_eq!(json["error"]["code"], "INVALID_INPUT");
                assert!(json["error"]["message"].as_str().unwrap().contains(flag));
                assert_eq!(output.get_output().stdout.len(), 0);
            }
        }
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

fn definition() -> Value {
    json!({
        "format": "DeploymentPlanFormat",
        "parts": [{"path": "plan.yml", "payload": "e30=", "payloadType": "InlineBase64"}]
    })
}

/// Exercise both operation polling and fetching its final resource/definition.
async fn accepted_response(server: &MockServer, result: &Value) -> ResponseTemplate {
    Mock::given(method("GET"))
        .and(path("/operations/plan-operation"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    "location",
                    format!("{}/operations/plan-operation/result", server.uri()),
                )
                .set_body_json(json!({"status": "Succeeded"})),
        )
        .expect(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/operations/plan-operation/result"))
        .respond_with(ResponseTemplate::new(200).set_body_json(result))
        .expect(1)
        .mount(server)
        .await;
    ResponseTemplate::new(202)
        .insert_header("x-ms-operation-id", "plan-operation")
        .insert_header("retry-after", "0")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deployment_plan_create_posts_definition_and_handles_lro() {
    for lro in [false, true] {
        let server = MockServer::start().await;
        let plan = json!({"id": PLAN, "displayName": "Mock deployment"});
        let response = if lro {
            accepted_response(&server, &plan).await
        } else {
            ResponseTemplate::new(201).set_body_json(&plan)
        };
        Mock::given(method("POST"))
            .and(path(format!("/workspaces/{WORKSPACE}/deploymentPlans")))
            .and(body_json(json!({
                "displayName": "Mock deployment", "description": "Mock description", "folderId": PLAN,
                "sensitivityLabelSettings": {"sensitivityLabelId": PLAN}, "definition": definition()
            })))
            .respond_with(response).expect(1).mount(&server).await;
        let content = json!({"definition": definition()}).to_string();
        let output = mock_fabio(&server)
            .args([
                "deployment-plan",
                "create",
                "--workspace",
                WORKSPACE,
                "--name",
                "Mock deployment",
                "--description",
                "Mock description",
                "--folder-id",
                PLAN,
                "--sensitivity-label",
                PLAN,
                "--content",
                &content,
            ])
            .assert()
            .success();
        assert_eq!(parse_json(&output)["data"], plan);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deployment_plan_get_definition_passes_format_and_decodes_lro_result() {
    for lro in [false, true] {
        let server = MockServer::start().await;
        let result = json!({"definition": definition()});
        let response = if lro {
            accepted_response(&server, &result).await
        } else {
            ResponseTemplate::new(200).set_body_json(&result)
        };
        Mock::given(method("POST"))
            .and(path(format!(
                "/workspaces/{WORKSPACE}/deploymentPlans/{PLAN}/getDefinition"
            )))
            .and(query_param("format", "DeploymentPlanFormat"))
            .and(body_json(json!({})))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let mut cmd = mock_fabio(&server);
        cmd.args([
            "deployment-plan",
            "get-definition",
            "--workspace",
            WORKSPACE,
            "--id",
            PLAN,
            "--format",
            "DeploymentPlanFormat",
        ]);
        if lro {
            cmd.arg("--decode");
        }
        let output = cmd.assert().success();
        let data = parse_json(&output)["data"].clone();
        if lro {
            assert_eq!(data["definition"]["parts"][0]["decodedPayload"], json!({}));
            assert_eq!(data["definition"]["parts"][0]["payload"], "e30=");
            assert_eq!(data["definition"]["parts"][0]["path"], "plan.yml");
        } else {
            assert_eq!(data, result);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deployment_plan_update_definition_passes_metadata_and_handles_lro() {
    for lro in [false, true] {
        for update_metadata in [false, true] {
            let server = MockServer::start().await;
            let response = if lro {
                accepted_response(&server, &json!({})).await
            } else {
                ResponseTemplate::new(200)
            };
            Mock::given(method("POST"))
                .and(path(format!(
                    "/workspaces/{WORKSPACE}/deploymentPlans/{PLAN}/updateDefinition"
                )))
                .and(query_param("updateMetadata", update_metadata.to_string()))
                .and(body_json(json!({"definition": definition()})))
                .respond_with(response)
                .expect(1)
                .mount(&server)
                .await;
            let mut cmd = mock_fabio(&server);
            let content = json!({"definition": definition()}).to_string();
            cmd.args([
                "deployment-plan",
                "update-definition",
                "--workspace",
                WORKSPACE,
                "--id",
                PLAN,
                "--content",
                &content,
            ]);
            if update_metadata {
                cmd.arg("--update-metadata");
            }
            let output = cmd.assert().success();
            assert_eq!(
                parse_json(&output)["data"],
                json!({"id": PLAN, "status": "definition_updated"})
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deployment_plan_delete_uses_scoped_path_and_hard_delete_query() {
    for hard_delete in [false, true] {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path(format!(
                "/workspaces/{WORKSPACE}/deploymentPlans/{PLAN}"
            )))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let mut cmd = mock_fabio(&server);
        cmd.args([
            "deployment-plan",
            "delete",
            "--workspace",
            WORKSPACE,
            "--id",
            PLAN,
        ]);
        if hard_delete {
            cmd.arg("--hard-delete");
        }
        let output = cmd.assert().success();
        assert_eq!(
            parse_json(&output)["data"],
            json!({"id": PLAN, "status": "deleted"})
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests[0].url.query(),
            hard_delete.then_some("hardDelete=true")
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deployment_plan_http_errors_remain_structured() {
    for (status, code) in [(403, "FORBIDDEN"), (404, "NOT_FOUND")] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!(
                "/workspaces/{WORKSPACE}/deploymentPlans/{PLAN}/getDefinition"
            )))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                "errorCode": "RequestFailed", "message": "Mock request failed"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let output = mock_fabio(&server)
            .args([
                "--quiet",
                "deployment-plan",
                "get-definition",
                "--workspace",
                WORKSPACE,
                "--id",
                PLAN,
            ])
            .assert()
            .failure();
        let json: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
        assert_eq!(json["error"]["code"], code);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deployment_plan_delete_respects_readonly() {
    let server = MockServer::start().await;
    let output = mock_fabio(&server)
        .args([
            "--readonly",
            "deployment-plan",
            "delete",
            "--workspace",
            WORKSPACE,
            "--id",
            PLAN,
        ])
        .assert()
        .failure();
    let json: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
    assert_eq!(json["error"]["code"], "READONLY_MODE");
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
