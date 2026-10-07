//! Integration tests for `fabio policy-set`.

mod common;

use common::{extract_data, fabio, parse_json};
use serial_test::serial;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WORKSPACE: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const POLICY_SET: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const POLICY_RULE: &str = "cccccccc-1111-2222-3333-444444444444";
const CAPACITY: &str = "dddddddd-1111-2222-3333-444444444444";

#[test]
fn test_create_dry_run_serializes_creation_payload() {
    let output = fabio()
        .args([
            "--dry-run",
            "policy-set",
            "create",
            "--workspace",
            WORKSPACE,
            "--name",
            "Tenant governance",
            "--description",
            "Tenant item-creation controls",
            "--scope",
            "Tenant",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "policy-set create");
    assert_eq!(data["details"]["displayName"], "Tenant governance");
    assert_eq!(
        data["details"]["creationPayload"]["scope"]["type"],
        "Tenant"
    );
}

#[test]
fn test_create_rule_dry_run_serializes_discriminated_unions() {
    let conditions = r#"[{"type":"Dynamic","targetProperty":"item.type","predicate":{"operator":"AnyOf","values":["Lakehouse","Warehouse"]}}]"#;
    let output = fabio()
        .args([
            "--dry-run",
            "policy-set",
            "create-rule",
            "--workspace",
            WORKSPACE,
            "--id",
            POLICY_SET,
            "--name",
            "Approved item types",
            "--policy",
            "ItemCreation",
            "--conditions",
            conditions,
            "--effects",
            r#"[{"type":"Allow"}]"#,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "policy-set create-rule");
    assert_eq!(data["details"]["policy"], "ItemCreation");
    assert_eq!(
        data["details"]["conditions"][0]["predicate"]["operator"],
        "AnyOf"
    );
    assert_eq!(data["details"]["effects"][0]["type"], "Allow");
}

#[test]
fn test_update_definition_dry_run_uses_beta_format() {
    let output = fabio()
        .args([
            "--dry-run",
            "policy-set",
            "update-definition",
            "--workspace",
            WORKSPACE,
            "--id",
            POLICY_SET,
            "--content",
            r#"{"scope":{"type":"Tenant"},"policyRules":[{"displayName":"Keep rule"}]}"#,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "policy-set update-definition");
    assert_eq!(data["destructive"], true);
    assert_eq!(data["details"]["request"]["definition"]["format"], "Beta");
    assert_eq!(
        data["details"]["request"]["definition"]["parts"][0]["path"],
        "policySet.json"
    );
}

#[test]
fn test_update_definition_rejects_empty_rules_without_force() {
    let output = fabio()
        .args([
            "--dry-run",
            "policy-set",
            "update-definition",
            "--workspace",
            WORKSPACE,
            "--id",
            POLICY_SET,
            "--content",
            r#"{"scope":{"type":"Tenant"},"policyRules":[]}"#,
        ])
        .assert()
        .failure();

    let error: serde_json::Value =
        serde_json::from_slice(&output.get_output().stderr).expect("JSON error");
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
    assert!(error["error"]["hint"].as_str().unwrap().contains("--force"));
}

#[test]
fn test_update_definition_force_allows_empty_rules_in_encoded_envelope() {
    use base64::Engine;

    let policy_set = base64::engine::general_purpose::STANDARD
        .encode(r#"{"scope":{"type":"Tenant"},"policyRules":[]}"#);
    let envelope = serde_json::json!({
        "definition": {
            "format": "Beta",
            "parts": [{
                "path": "policySet.json",
                "payload": policy_set,
                "payloadType": "InlineBase64"
            }]
        }
    })
    .to_string();
    let output = fabio()
        .args([
            "--force",
            "--dry-run",
            "policy-set",
            "update-definition",
            "--workspace",
            WORKSPACE,
            "--id",
            POLICY_SET,
            "--content",
            &envelope,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["would_execute"], "policy-set update-definition");
}

#[test]
fn test_policy_set_ids_are_validated_before_dispatch() {
    let cases = [
        vec!["policy-set", "list", "--workspace", "../../invalid"],
        vec![
            "policy-set",
            "show",
            "--workspace",
            WORKSPACE,
            "--id",
            "../../invalid",
        ],
        vec![
            "policy-set",
            "show-rule",
            "--workspace",
            WORKSPACE,
            "--id",
            POLICY_SET,
            "--rule-id",
            "../../invalid",
        ],
    ];

    for arguments in cases {
        let output = fabio()
            .env("FABIO_ACCESS_TOKEN", "fake-test-token")
            .args(arguments)
            .assert()
            .failure();
        let error: serde_json::Value =
            serde_json::from_slice(&output.get_output().stderr).expect("JSON error");
        assert_eq!(error["error"]["code"], "INVALID_INPUT");
    }
}

#[test]
fn test_array_flags_accept_json_files() {
    let dir = tempfile::tempdir().unwrap();
    let conditions = dir.path().join("conditions.json");
    let effects = dir.path().join("effects.json");
    let policy_rules = dir.path().join("policy-rules.json");
    std::fs::write(&conditions, r#"[{"type":"Static","value":true}]"#).unwrap();
    std::fs::write(&effects, r#"[{"type":"Allow"}]"#).unwrap();
    std::fs::write(
        &policy_rules,
        r#"[{"displayName":"Allow items","conditions":[{"type":"Static","value":true}],"effects":[{"type":"Allow"}]}]"#,
    )
    .unwrap();

    let condition_arg = format!("@{}", conditions.display());
    let effect_arg = format!("@{}", effects.display());
    let output = fabio()
        .args([
            "--dry-run",
            "policy-set",
            "create-rule",
            "--workspace",
            WORKSPACE,
            "--id",
            POLICY_SET,
            "--name",
            "Allow items",
            "--policy",
            "ItemCreation",
            "--conditions",
            &condition_arg,
            "--effects",
            &effect_arg,
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["details"]["conditions"][0]["value"], true);
    assert_eq!(data["details"]["effects"][0]["type"], "Allow");

    let policy_rules_arg = format!("@{}", policy_rules.display());
    let output = fabio()
        .args([
            "--dry-run",
            "policy-set",
            "replace-rules-by-policy",
            "--workspace",
            WORKSPACE,
            "--id",
            POLICY_SET,
            "--policy",
            "ItemCreation",
            "--policy-rules",
            &policy_rules_arg,
        ])
        .assert()
        .success();
    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(
        data["details"]["policyRules"][0]["displayName"],
        "Allow items"
    );
}

#[test]
fn test_replace_rules_rejects_empty_array_without_force() {
    let output = fabio()
        .args([
            "--dry-run",
            "policy-set",
            "replace-rules-by-policy",
            "--workspace",
            WORKSPACE,
            "--id",
            POLICY_SET,
            "--policy",
            "ItemCreation",
            "--policy-rules",
            "[]",
        ])
        .assert()
        .failure();

    let error: serde_json::Value =
        serde_json::from_slice(&output.get_output().stderr).expect("JSON error");
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
    assert!(error["error"]["hint"].as_str().unwrap().contains("--force"));
}

#[test]
fn test_activate_allow_replace_dry_run_is_destructive() {
    let output = fabio()
        .args([
            "--dry-run",
            "policy-set",
            "activate",
            "--workspace",
            WORKSPACE,
            "--id",
            POLICY_SET,
            "--capacity-id",
            CAPACITY,
            "--allow-replace",
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    let data = extract_data(&json);
    assert_eq!(data["details"]["request"]["scopeType"], "Capacity");
    assert_eq!(data["details"]["request"]["capacityId"], CAPACITY);
    assert_eq!(data["destructive"], true);
}

#[test]
fn test_remaining_mutations_support_dry_run() {
    let cases = [
        (
            "policy-set update",
            vec![
                "--dry-run",
                "policy-set",
                "update",
                "--workspace",
                WORKSPACE,
                "--id",
                POLICY_SET,
                "--name",
                "Updated governance",
            ],
        ),
        (
            "policy-set delete",
            vec![
                "--dry-run",
                "policy-set",
                "delete",
                "--workspace",
                WORKSPACE,
                "--id",
                POLICY_SET,
            ],
        ),
        (
            "policy-set update-rule",
            vec![
                "--dry-run",
                "policy-set",
                "update-rule",
                "--workspace",
                WORKSPACE,
                "--id",
                POLICY_SET,
                "--rule-id",
                POLICY_RULE,
                "--name",
                "Updated rule",
            ],
        ),
        (
            "policy-set delete-rule",
            vec![
                "--dry-run",
                "policy-set",
                "delete-rule",
                "--workspace",
                WORKSPACE,
                "--id",
                POLICY_SET,
                "--rule-id",
                POLICY_RULE,
            ],
        ),
        (
            "policy-set replace-rules-by-policy",
            vec![
                "--dry-run",
                "policy-set",
                "replace-rules-by-policy",
                "--workspace",
                WORKSPACE,
                "--id",
                POLICY_SET,
                "--policy",
                "WorkspaceSettingsEditing",
                "--policy-rules",
                r#"[{"displayName":"Allow workspace settings","conditions":[{"type":"Static","value":true}],"effects":[{"type":"Allow"}]}]"#,
            ],
        ),
        (
            "policy-set deactivate",
            vec![
                "--dry-run",
                "policy-set",
                "deactivate",
                "--workspace",
                WORKSPACE,
                "--id",
                POLICY_SET,
            ],
        ),
    ];

    for (operation, arguments) in cases {
        let output = fabio().args(arguments).assert().success();
        let json = parse_json(&output);
        assert_eq!(
            extract_data(&json)["would_execute"],
            operation,
            "wrong dry-run operation for {operation}"
        );
    }
}

#[test]
#[serial]
fn test_list_sends_folder_filters() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (server_uri, _server) = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/workspaces/{WORKSPACE}/policySets")))
            .and(query_param("recursive", "false"))
            .and(query_param("rootFolderId", POLICY_RULE))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [{
                    "id": POLICY_SET,
                    "displayName": "Governance",
                    "type": "PolicySet",
                    "properties": {"scope": {"type": "Tenant"}, "status": "Inactive"}
                }]
            })))
            .mount(&server)
            .await;
        (server.uri(), server)
    });

    let output = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server_uri)
        .args([
            "policy-set",
            "list",
            "--workspace",
            WORKSPACE,
            "--no-recursive",
            "--root-folder-id",
            POLICY_RULE,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    assert_eq!(json["count"], 1);
    assert_eq!(json["data"][0]["type"], "PolicySet");
}

#[test]
#[serial]
fn test_create_rule_sends_beta_request() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (server_uri, _server) = runtime.block_on(async {
        let server = MockServer::start().await;
        let request = serde_json::json!({
            "displayName": "Allow lakehouses",
            "policy": "ItemCreation",
            "conditions": [{"type": "Static", "value": true}],
            "effects": [{"type": "Allow"}]
        });
        Mock::given(method("POST"))
            .and(path(format!(
                "/workspaces/{WORKSPACE}/policySets/{POLICY_SET}/policyRules"
            )))
            .and(query_param("beta", "true"))
            .and(body_json(request.clone()))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "id": POLICY_RULE,
                "displayName": "Allow lakehouses",
                "description": "",
                "policy": "ItemCreation",
                "conditions": [{"type": "Static", "value": true}],
                "effects": [{"type": "Allow"}]
            })))
            .mount(&server)
            .await;
        (server.uri(), server)
    });

    let output = fabio()
        .env("FABIO_ACCESS_TOKEN", "fake-test-token")
        .env("FABIO_FABRIC_API_ENDPOINT", server_uri)
        .args([
            "policy-set",
            "create-rule",
            "--workspace",
            WORKSPACE,
            "--id",
            POLICY_SET,
            "--name",
            "Allow lakehouses",
            "--policy",
            "ItemCreation",
            "--conditions",
            r#"[{"type":"Static","value":true}]"#,
            "--effects",
            r#"[{"type":"Allow"}]"#,
        ])
        .assert()
        .success();

    let json = parse_json(&output);
    assert_eq!(json["data"]["id"], POLICY_RULE);
}

#[test]
#[ignore = "requires a live Fabric tenant with PolicySet preview enabled"]
fn test_get_active_for_capacity_live() {
    let capacity_id =
        std::env::var("FABIO_TEST_CAPACITY_ID").expect("FABIO_TEST_CAPACITY_ID required");
    let output = fabio()
        .args([
            "policy-set",
            "get-active-for-capacity",
            "--capacity-id",
            &capacity_id,
        ])
        .output()
        .expect("fabio command");
    if output.status.success() {
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON output");
        assert!(json["data"]["id"].is_string());
    } else {
        let error: serde_json::Value = serde_json::from_slice(&output.stderr).expect("JSON error");
        assert!(matches!(
            error["error"]["code"].as_str(),
            Some("NOT_FOUND" | "FORBIDDEN" | "API_ERROR")
        ));
    }
}
