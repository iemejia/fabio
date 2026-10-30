//! Execute published BI examples rather than maintaining parallel test commands.
mod common;

use common::{mock_fabio, parse_json};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const MODEL: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const REPORT: &str = "cccccccc-1111-2222-3333-444444444444";
const TABLE_URL: &str = "https://onelake.dfs.fabric.microsoft.com/aaaaaaaa-1111-2222-3333-444444444444/dddddddd-1111-2222-3333-444444444444/Tables/dbo/sales";

fn example_args(text: &str) -> Vec<String> {
    let tokens = shlex::split(text).expect("published example quotation");
    assert_eq!(tokens[0], "fabio");
    tokens
        .into_iter()
        .skip(1)
        .map(|token| match token.as_str() {
            "$WS_ID" | "<WS>" => WS.to_string(),
            "$SM_ID" | "<MODEL_ID>" => MODEL.to_string(),
            "$ONELAKE_TABLE_URL" => TABLE_URL.to_string(),
            "$MODEL_NAME" => "Example model".to_string(),
            "$REPORT_NAME" | "<NAME>" => "Example report".to_string(),
            _ => token,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn published_native_bi_recipe_sends_definition_free_creation_requests() {
    let server = MockServer::start().await;
    let result = mock_fabio(&server)
        .args(["context", "workflow", "direct-lake-report"])
        .assert()
        .success();
    let recipe = parse_json(&result)["data"].clone();
    for (number, segment, id, body) in [
        (
            3,
            "semanticModels",
            MODEL,
            json!({"displayName": "Example model", "creationPayload": {"oneLakeTables": [TABLE_URL]}}),
        ),
        (
            8,
            "reports",
            REPORT,
            json!({"displayName": "Example report", "creationPayload": {"semanticModelReference": {"referenceType": "ById", "itemId": MODEL, "workspaceId": WS}}}),
        ),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/workspaces/{WS}/{segment}")))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": id})))
            .expect(1)
            .mount(&server)
            .await;
        let step = recipe["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|step| step["step_number"] == number)
            .unwrap();
        let args = example_args(step["command"].as_str().unwrap());
        let preview = mock_fabio(&server)
            .args(&args)
            .arg("--dry-run")
            .assert()
            .success();
        assert_eq!(parse_json(&preview)["data"]["dry_run"], true);
        let output = mock_fabio(&server).args(&args).assert().success();
        assert_eq!(parse_json(&output)["data"]["id"], id);
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn published_report_schema_distinguishes_native_and_compatibility_creation_offline() {
    let server = MockServer::start().await;
    let output = mock_fabio(&server)
        .args(["context", "schema", "Report"])
        .assert()
        .success();
    let schema = parse_json(&output)["data"].clone();
    for mode in ["native_empty", "generated_blank"] {
        assert_eq!(schema["creation_modes"][mode]["definition_required"], false);
        let args = example_args(schema["creation_modes"][mode]["example"].as_str().unwrap());
        let output = mock_fabio(&server)
            .args(args)
            .arg("--dry-run")
            .assert()
            .success();
        let request = parse_json(&output)["data"]["details"]["request"].clone();
        assert_eq!(
            request.get("creationPayload").is_some(),
            mode == "native_empty"
        );
        assert_eq!(
            request.get("definition").is_some(),
            mode == "generated_blank"
        );
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bi_routing_surfaces_safe_mode_selection_without_network_requests() {
    let server = MockServer::start().await;
    for args in [
        ["context", "persona", "bi-developer"],
        ["context", "skill", "bi"],
        ["context", "workflow", "report-authoring"],
    ] {
        let result = mock_fabio(&server).args(args).assert().success();
        let data = parse_json(&result)["data"].to_string();
        assert!(data.contains("bi-creation"));
        assert!(!data.contains("no import/refresh"));
        assert!(!data.contains("Use PBIR-Legacy format when"));
    }
    let result = mock_fabio(&server)
        .args(["context", "best-practices", "bi-creation"])
        .assert()
        .success();
    let guidance = parse_json(&result)["data"].clone();
    assert_eq!(
        guidance["semantic_model_decisions"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(guidance["report_decisions"].as_array().unwrap().len(), 3);
    let result = mock_fabio(&server)
        .args(["context", "find", "bi-creation"])
        .assert()
        .success();
    assert!(
        parse_json(&result)["data"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["command"] == "fabio context best-practices bi-creation")
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
