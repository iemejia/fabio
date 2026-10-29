//! Offline validation and generic-deployer interoperability for native plans.
mod common;

use base64::Engine as _;
use common::{mock_fabio, parse_json};
use serde_json::{Value, json};
use std::path::Path;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const NOTEBOOK: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const PLAN: &str = "cccccccc-1111-2222-3333-444444444444";
const DEPLOYED_NOTEBOOK: &str = "dddddddd-1111-2222-3333-444444444444";
const DEPLOYED_PLAN: &str = "eeeeeeee-1111-2222-3333-444444444444";
const SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/deploymentPlan/definition/plan/1.0.0/schema.json";

fn content() -> String {
    json!({"$schema": SCHEMA, "version": "1.0.0", "groups": [{
        "name": "prepare", "logicalId": NOTEBOOK,
        "postActions": [{"name": "seed", "job": {"type": "Execute", "logicalId": NOTEBOOK}}]
    }]})
    .to_string()
}

fn write_item(root: &Path, name: &str, kind: &str, id: &str, file: &str, content: &str) {
    let directory = root.join(format!("{name}.{kind}"));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(".platform"),
        json!({
            "metadata": {"type": kind, "displayName": name},
            "config": {"version": "2.0", "logicalId": id}
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(directory.join(file), content).unwrap();
}

fn decoded_part(body: &Value, path: &str) -> String {
    let parts = body["definition"]["parts"].as_array().unwrap();
    let part = parts.iter().find(|part| part["path"] == path).unwrap();
    String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(part["payload"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap()
}

fn source_fixture(plan_logical_id: &str) -> tempfile::TempDir {
    let source = tempfile::tempdir().unwrap();
    write_item(
        source.path(),
        "Prepare",
        "Notebook",
        NOTEBOOK,
        "notebook-content.py",
        &format!("ITEM_ID = '{NOTEBOOK}'"),
    );
    write_item(
        source.path(),
        "Release",
        "DeploymentPlan",
        plan_logical_id,
        "plan.yml",
        &content(),
    );
    source
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_validation_is_offline_readonly_and_describes_jobs_without_running_them() {
    let server = MockServer::start().await;
    let output = mock_fabio(&server)
        .args([
            "--readonly",
            "--dry-run",
            "deployment-plan",
            "validate",
            "--content",
            &content(),
        ])
        .assert()
        .success();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["valid"], true);
    assert_eq!(data["groups"], 1);
    assert_eq!(data["actions"], 1);
    assert_eq!(data["executes_jobs"], true);
    assert_eq!(data["source_checked"], false);
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_scope_warnings_are_explicit_and_strict_mode_rejects_them() {
    let server = MockServer::start().await;
    let source = tempfile::tempdir().unwrap();
    write_item(
        source.path(),
        "Other",
        "Notebook",
        PLAN,
        "notebook-content.py",
        "print('fixture')",
    );
    for strict in [false, true] {
        let mut command = mock_fabio(&server);
        command.args([
            "deployment-plan",
            "validate",
            "--content",
            &content(),
            "--source",
            source.path().to_str().unwrap(),
        ]);
        if strict {
            command.arg("--strict");
        }
        let output = if strict {
            command.assert().failure()
        } else {
            command.assert().success()
        };
        let data = parse_json(&output)["data"].clone();
        assert_eq!(data["valid"], !strict);
        assert_eq!(data["source_checked"], true);
        assert_eq!(data["warnings"].as_array().unwrap().len(), 2);
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_plans_fail_with_structured_errors_without_network_calls() {
    let server = MockServer::start().await;
    for invalid in [
        "executableFormat: DeploymentPlanV1",
        "{}",
        "{\"parts\":[]}",
        "{\"parts\":[{\"path\":\"plan.yml\",\"payloadType\":\"InlineBase64\",\"payload\":\"!\"}]}",
    ] {
        let output = mock_fabio(&server)
            .args(["deployment-plan", "validate", "--content", invalid])
            .assert()
            .failure();
        assert_eq!(parse_json(&output)["data"]["valid"], false);
        let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
        assert_eq!(error["error"]["code"], "INVALID_INPUT");
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn item_definition_and_deploy_validation_share_semantic_plan_checks() {
    let server = MockServer::start().await;
    let cyclic = json!({"$schema": SCHEMA, "version": "1.0.0", "groups": [{
        "name": "self", "logicalId": NOTEBOOK, "dependsOn": [{"groupName": "self"}]
    }]})
    .to_string();
    let envelope = json!({"parts": [{"path": "plan.yml", "payloadType": "InlineBase64",
        "payload": base64::engine::general_purpose::STANDARD.encode(cyclic.as_bytes())}]})
    .to_string();
    let output = mock_fabio(&server)
        .args([
            "item",
            "validate-definition",
            "--type",
            "DeploymentPlan",
            "--definition",
            &envelope,
        ])
        .assert()
        .failure();
    assert!(
        String::from_utf8_lossy(&output.get_output().stdout).contains("INVALID_DEPLOYMENT_PLAN")
    );
    let source = tempfile::tempdir().unwrap();
    write_item(
        source.path(),
        "Plan",
        "DeploymentPlan",
        PLAN,
        "plan.yml",
        &cyclic,
    );
    let output = mock_fabio(&server)
        .args([
            "deploy",
            "validate",
            "--source",
            source.path().to_str().unwrap(),
        ])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)] // Compare all three wire-level identity modes in one fixture.
async fn per_item_deploy_preserves_plan_references_and_platform_identities_on_create_and_update() {
    for (update, assigned) in [(false, false), (false, true), (true, true)] {
        let server = MockServer::start().await;
        let source = source_fixture(PLAN);
        let plan_file = source.path().join("changeset.json");
        let changes: Vec<Value> = [("Prepare", "Notebook", NOTEBOOK, DEPLOYED_NOTEBOOK), ("Release", "DeploymentPlan", PLAN, DEPLOYED_PLAN)]
            .into_iter().map(|(name, kind, logical, physical)| {
                let mut value = json!({"name": name, "item_type": kind, "action": if update {"update"} else {"create"}, "reason": "fixture", "logical_id": logical});
                if update { value["deployed_id"] = Value::from(physical); }
                value
            }).collect();
        std::fs::write(&plan_file, json!({"workspace_id": WS, "source_path": source.path(), "changeset": {"changes": changes}}).to_string()).unwrap();
        if update {
            Mock::given(method("GET")).and(path(format!("/workspaces/{WS}/items")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                    {"id": DEPLOYED_NOTEBOOK, "logicalId": NOTEBOOK}, {"id": DEPLOYED_PLAN, "logicalId": PLAN}
                ]}))).expect(1).mount(&server).await;
            for id in [DEPLOYED_NOTEBOOK, DEPLOYED_PLAN] {
                Mock::given(method("POST"))
                    .and(path(format!(
                        "/workspaces/{WS}/items/{id}/updateDefinition"
                    )))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": id})))
                    .expect(1)
                    .mount(&server)
                    .await;
            }
        } else {
            Mock::given(method("POST")).and(path(format!("/workspaces/{WS}/items")))
                .respond_with(move |request: &Request| {
                    let body: Value = serde_json::from_slice(&request.body).unwrap();
                    let mut created = json!({"id": if body["type"] == "Notebook" {DEPLOYED_NOTEBOOK} else {DEPLOYED_PLAN}});
                    if assigned { created["logicalId"] = Value::from(if body["type"] == "Notebook" { NOTEBOOK } else { PLAN }); }
                    ResponseTemplate::new(201).set_body_json(created)
                }).expect(2).mount(&server).await;
            for (id, logical) in [(DEPLOYED_NOTEBOOK, NOTEBOOK), (DEPLOYED_PLAN, PLAN)]
                .into_iter()
                .filter(|_| !assigned)
            {
                Mock::given(method("PATCH"))
                    .and(path(format!("/workspaces/{WS}/items/{id}/logicalId")))
                    .and(body_json(json!({"logicalId": logical})))
                    .respond_with(
                        ResponseTemplate::new(200).set_body_json(json!({"logicalId": logical})),
                    )
                    .expect(1)
                    .mount(&server)
                    .await;
            }
        }
        let output = mock_fabio(&server)
            .args([
                "deploy",
                "apply",
                "--plan",
                plan_file.to_str().unwrap(),
                "--strategy",
                "sequential",
                "--no-post-hooks",
            ])
            .assert()
            .success();
        assert_eq!(parse_json(&output)["data"]["succeeded"], 2);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests.len(),
            if update {
                3
            } else if assigned {
                2
            } else {
                4
            }
        );
        let definitions: Vec<_> = requests
            .iter()
            .filter(|request| request.method.as_str() == "POST")
            .collect();
        let notebook: Value = serde_json::from_slice(&definitions[0].body).unwrap();
        let native_plan: Value = serde_json::from_slice(&definitions[1].body).unwrap();
        let notebook_identity: Value =
            serde_json::from_str(&decoded_part(&notebook, ".platform")).unwrap();
        let plan_identity: Value =
            serde_json::from_str(&decoded_part(&native_plan, ".platform")).unwrap();
        assert_eq!(notebook_identity["config"]["logicalId"], NOTEBOOK);
        assert_eq!(plan_identity["config"]["logicalId"], PLAN);
        assert_eq!(decoded_part(&native_plan, "plan.yml"), content());
        if update {
            assert!(decoded_part(&notebook, "notebook-content.py").contains(DEPLOYED_NOTEBOOK));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn export_uses_assigned_metadata_identity_instead_of_zero_definition_placeholders() {
    let server = MockServer::start().await;
    let output_dir = tempfile::tempdir().unwrap();
    Mock::given(method("GET")).and(path(format!("/workspaces/{WS}/items")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
            {"id": DEPLOYED_NOTEBOOK, "logicalId": NOTEBOOK, "type": "DataPipeline", "displayName": "Prepare"},
            {"id": DEPLOYED_PLAN, "logicalId": PLAN, "type": "DeploymentPlan", "displayName": "Release"}
        ]}))).expect(1).mount(&server).await;
    for (id, name, kind, file, text) in [
        (
            DEPLOYED_NOTEBOOK,
            "Prepare",
            "DataPipeline",
            "pipeline-content.json",
            "{}".to_string(),
        ),
        (
            DEPLOYED_PLAN,
            "Release",
            "DeploymentPlan",
            "plan.yml",
            content(),
        ),
    ] {
        let platform = json!({"metadata": {"displayName": name, "type": kind},
            "config": {"version": "2.0", "logicalId": "00000000-0000-0000-0000-000000000000"}})
        .to_string();
        Mock::given(method("POST")).and(path(format!("/workspaces/{WS}/items/{id}/getDefinition")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"definition": {"parts": [
                {"path": ".platform", "payloadType": "InlineBase64", "payload": base64::engine::general_purpose::STANDARD.encode(platform.as_bytes())},
                {"path": file, "payloadType": "InlineBase64", "payload": base64::engine::general_purpose::STANDARD.encode(text.as_bytes())}
            ]}}))).expect(1).mount(&server).await;
    }
    Mock::given(method("GET"))
        .and(path(format!(
            "/workspaces/{WS}/items/{DEPLOYED_NOTEBOOK}/jobs/Pipeline/schedules"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": []})))
        .expect(1)
        .mount(&server)
        .await;
    mock_fabio(&server)
        .args([
            "deploy",
            "export",
            "--workspace",
            WS,
            "--dir",
            output_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    for (directory, logical) in [
        ("Prepare.DataPipeline", NOTEBOOK),
        ("Release.DeploymentPlan", PLAN),
    ] {
        let platform: Value = serde_json::from_slice(
            &std::fs::read(output_dir.path().join(directory).join(".platform")).unwrap(),
        )
        .unwrap();
        assert_eq!(platform["config"]["logicalId"], logical);
    }
    mock_fabio(&server)
        .args([
            "deploy",
            "validate",
            "--source",
            output_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn existing_identity_mismatches_fail_before_mutation_even_with_override_flags() {
    let server = MockServer::start().await;
    let source = source_fixture(PLAN);
    let plan_file = source.path().join("changeset.json");
    std::fs::write(&plan_file, json!({"workspace_id": WS, "source_path": source.path(), "changeset": {"changes": [
        {"name": "Prepare", "item_type": "Notebook", "action": "update", "reason": "fixture", "logical_id": NOTEBOOK, "deployed_id": DEPLOYED_NOTEBOOK},
        {"name": "Release", "item_type": "DeploymentPlan", "action": "create", "reason": "fixture", "logical_id": PLAN}
    ]}}).to_string()).unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/items")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"value": [{"id": DEPLOYED_NOTEBOOK, "logicalId": DEPLOYED_PLAN}]}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args([
            "--quiet",
            "--force",
            "deploy",
            "apply",
            "--plan",
            plan_file.to_str().unwrap(),
            "--allow-unresolved",
            "--no-post-hooks",
        ])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method.as_str(), "GET");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rename_detection_uses_authoritative_item_metadata_without_definition_placeholders() {
    let server = MockServer::start().await;
    let source = tempfile::tempdir().unwrap();
    write_item(
        source.path(),
        "New",
        "Notebook",
        NOTEBOOK,
        "notebook-content.py",
        "print('fixture')",
    );
    Mock::given(method("GET")).and(path(format!("/workspaces/{WS}/items")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
            {"id": DEPLOYED_NOTEBOOK, "displayName": "Old", "type": "Notebook", "logicalId": NOTEBOOK}
        ]}))).expect(1).mount(&server).await;
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/items/{DEPLOYED_NOTEBOOK}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": DEPLOYED_NOTEBOOK, "logicalId": NOTEBOOK})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args([
            "deploy",
            "plan",
            "--workspace",
            WS,
            "--source",
            source.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["summary"]["rename"], 1);
    assert_eq!(data["summary"]["create"], 0);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_or_duplicate_source_identities_are_rejected_before_target_requests() {
    let server = MockServer::start().await;
    for logical in ["invalid", "00000000-0000-0000-0000-000000000000", NOTEBOOK] {
        let source = source_fixture(logical);
        let output = mock_fabio(&server)
            .args([
                "--quiet",
                "--dry-run",
                "deploy",
                "apply",
                "--workspace",
                "Fixture Workspace",
                "--source",
                source.path().to_str().unwrap(),
                "--no-post-hooks",
            ])
            .assert()
            .failure();
        let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
        assert_eq!(error["error"]["code"], "INVALID_INPUT");
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_create_response_identity_is_not_reported_as_a_successful_plan_deploy() {
    let server = MockServer::start().await;
    let source = source_fixture(PLAN);
    let plan_file = source.path().join("changeset.json");
    std::fs::write(&plan_file, json!({"workspace_id": WS, "source_path": source.path(), "changeset": {"changes": [
        {"name": "Prepare", "item_type": "Notebook", "action": "create", "reason": "fixture", "logical_id": NOTEBOOK},
        {"name": "Release", "item_type": "DeploymentPlan", "action": "create", "reason": "fixture", "logical_id": PLAN}
    ]}}).to_string()).unwrap();
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WS}/items")))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    let output = mock_fabio(&server)
        .args([
            "deploy",
            "apply",
            "--plan",
            plan_file.to_str().unwrap(),
            "--fail-fast",
            "--no-post-hooks",
        ])
        .assert()
        .failure();
    let data = parse_json(&output)["data"].clone();
    assert_eq!(data["succeeded"], 0);
    assert_eq!(data["failed"], 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
