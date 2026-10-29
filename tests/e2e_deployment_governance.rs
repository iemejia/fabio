//! Live regression coverage for the deployment/governance APIs added in PR #188.
//!
//! See tests/README.md for prerequisites and commands. These tests fail (rather
//! than silently pass) when a preview API or required permission is unavailable.
//! Only resources created by the current test are mutated or deleted.

mod common;
#[path = "common/live.rs"]
mod live;

use base64::Engine as _;
use common::unique_name;
use live::{
    CreatedResource, assert_reference, command, contains_id, execute, field, list, run, try_run,
    wait_for_ready,
};
use serde_json::{Value, json};
use std::time::Duration;

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for this live test"))
}

fn create_item(group: &str, workspace: &str, extra: &[&str]) -> (String, CreatedResource) {
    // Lakehouse and other data items reject hyphens in their display names.
    let name = unique_name("fabio_e2e");
    let args = [
        &[group, "create", "--workspace", workspace, "--name", &name][..],
        extra,
    ]
    .concat();
    let result = run(&args);
    let id = field(&result["data"], "id");
    let resource = CreatedResource::new(&[group, "delete", "--workspace", workspace, "--id", &id]);
    (id, resource)
}

fn new_workspace() -> (String, CreatedResource) {
    let capacity = required_env("FABIO_TEST_CAPACITY_ID");
    let name = unique_name("fabio-e2e");
    let result = run(&["workspace", "create", "--name", &name]);
    let id = field(&result["data"], "id");
    let resource = CreatedResource::new(&["workspace", "delete", "--id", &id]);
    run(&[
        "workspace",
        "assign-capacity",
        "--id",
        &id,
        "--capacity",
        &capacity,
    ]);
    // assignToCapacity returns before the assignment necessarily becomes usable.
    // Wait for the read-side state rather than retrying an arbitrary API_ERROR.
    wait_for_ready(
        || try_run(&["workspace", "show", "--id", &id]),
        |current| {
            current["data"]["capacityId"]
                .as_str()
                .is_some_and(|value| value == capacity)
                && current["data"]["capacityAssignmentProgress"] == "Completed"
        },
        Duration::from_mins(2),
    )
    .expect("test workspace capacity assignment did not complete");
    (id, resource)
}

fn assert_not_found(args: &[&str]) {
    let Err(error) = execute(command(args)) else {
        panic!("deleted resource should not be readable; tenant details withheld");
    };
    assert!(
        error.contains("NOT_FOUND"),
        "expected NOT_FOUND; tenant details withheld"
    );
}

fn plan_content() -> String {
    "$schema: https://developer.microsoft.com/json-schemas/fabric/item/deploymentPlan/definition/plan/1.0.0/schema.json\nversion: 1.0.0\ngroups: []\n".to_string()
}

#[test]
#[ignore = "requires live Fabric tenant and FABIO_TEST_SOURCE_WORKSPACE"]
fn live_deployment_plan_lifecycle() {
    let workspace = required_env("FABIO_TEST_SOURCE_WORKSPACE");
    // Use the published YAML schema, not the obsolete executableFormat/id sample.
    let content = plan_content();
    let preview = run(&[
        "deployment-plan",
        "create",
        "--workspace",
        &workspace,
        "--name",
        "preview-only",
        "--content",
        &content,
        "--dry-run",
    ]);
    assert_eq!(preview["data"]["dry_run"], true);
    let (id, mut resource) = create_item("deployment-plan", &workspace, &["--content", &content]);
    let show = [
        "deployment-plan",
        "show",
        "--workspace",
        &workspace,
        "--id",
        &id,
    ];
    assert_reference(&run(&show)["data"]["id"], &id);
    assert!(contains_id(
        &run(&[
            "deployment-plan",
            "list",
            "--workspace",
            &workspace,
            "--all"
        ]),
        &id
    ));

    let name = unique_name("fabio-e2e-updated");
    run(&[
        "deployment-plan",
        "update",
        "--workspace",
        &workspace,
        "--id",
        &id,
        "--name",
        &name,
        "--description",
        "Live regression test",
    ]);
    let updated = run(&show);
    assert!(
        updated["data"]["displayName"]
            .as_str()
            .is_some_and(|value| value.contains(&name))
    );

    let definition_args = [
        "deployment-plan",
        "get-definition",
        "--workspace",
        &workspace,
        "--id",
        &id,
        "--format",
        "DeploymentPlanFormat",
    ];
    let definition = run(&definition_args);
    let parts = definition["data"]["definition"]["parts"]
        .as_array()
        .expect("expected definition parts");
    assert!(parts.iter().any(|part| part["path"] == "plan.yml"));
    // Replace only our own definition, using the server's canonical envelope.
    let body = json!({"definition": definition["data"]["definition"]}).to_string();
    let update = [
        "deployment-plan",
        "update-definition",
        "--workspace",
        &workspace,
        "--id",
        &id,
        "--content",
        &body,
    ];
    let preview = run(&[update.as_slice(), &["--dry-run"]].concat());
    assert_eq!(preview["data"]["destructive"], true);
    run(&update);
    let decoded = run(&[definition_args.as_slice(), &["--decode"]].concat());
    assert!(
        decoded["data"]["definition"]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|part| part["path"] == "plan.yml" && part.get("decodedPayload").is_some())
    );
    resource.delete();
    assert_not_found(&show);
}

#[test]
#[ignore = "requires live Fabric tenant and FABIO_TEST_SOURCE_WORKSPACE"]
fn live_empty_semantic_model_lifecycle() {
    let workspace = required_env("FABIO_TEST_SOURCE_WORKSPACE");
    let (id, mut resource) = create_item("semantic-model", &workspace, &[]);
    let show = [
        "semantic-model",
        "show",
        "--workspace",
        &workspace,
        "--id",
        &id,
    ];
    assert_reference(&run(&show)["data"]["id"], &id);
    assert!(contains_id(
        &run(&["semantic-model", "list", "--workspace", &workspace, "--all"]),
        &id
    ));
    resource.delete();
    assert_not_found(&show);
}

#[test]
#[ignore = "requires live Fabric tenant and FABIO_TEST_SOURCE_WORKSPACE"]
fn live_kql_dashboard_lifecycle() {
    let workspace = required_env("FABIO_TEST_SOURCE_WORKSPACE");
    let (id, mut resource) = create_item("kql-dashboard", &workspace, &[]);
    let show = [
        "kql-dashboard",
        "show",
        "--workspace",
        &workspace,
        "--id",
        &id,
    ];
    assert_reference(&run(&show)["data"]["id"], &id);
    resource.delete();
    assert_not_found(&show);
}

#[test]
#[ignore = "requires live Fabric tenant and FABIO_TEST_SOURCE_WORKSPACE"]
fn live_onelake_semantic_model_lifecycle() {
    let workspace = required_env("FABIO_TEST_SOURCE_WORKSPACE");
    let (table, _lakehouse_resource) = one_lake_fixture(&workspace);
    let (model, mut model_resource) =
        create_item("semantic-model", &workspace, &["--one-lake-table", &table]);
    let show = [
        "semantic-model",
        "show",
        "--workspace",
        &workspace,
        "--id",
        &model,
    ];
    assert_reference(&run(&show)["data"]["id"], &model);
    let definition = run(&[
        "semantic-model",
        "get-definition",
        "--workspace",
        &workspace,
        "--id",
        &model,
        "--decode",
    ]);
    let table_name = urlencoding::decode(table.rsplit('/').next().expect("table URL segment"))
        .expect("encoded table name");
    let parts = definition["data"]["definition"]["parts"]
        .as_array()
        .expect("model definition parts");
    assert!(
        parts.iter().any(|part| part["decodedPayload"]
            .to_string()
            .contains(table_name.as_ref())),
        "native model must contain the selected Delta table; tenant details withheld"
    );
    model_resource.delete();
    assert_not_found(&show);
}

fn one_lake_fixture(workspace: &str) -> (String, Option<CreatedResource>) {
    if let Ok(url) = std::env::var("FABIO_TEST_ONELAKE_TABLE_URL") {
        // An explicitly supplied source is read-only, and never registered for cleanup.
        return (url, None);
    }
    let (lakehouse, resource) = create_item("lakehouse", workspace, &["--enable-schemas"]);
    wait_for_ready(
        || {
            try_run(&[
                "lakehouse",
                "show",
                "--workspace",
                workspace,
                "--id",
                &lakehouse,
            ])
        },
        |current| {
            current["data"]["properties"]["sqlEndpointProperties"]["provisioningStatus"]
                == "Success"
        },
        Duration::from_mins(2),
    )
    .expect("test lakehouse provisioning did not complete");
    let directory = tempfile::tempdir().expect("create CSV fixture directory");
    // NamedTempFile's default dot-prefixed name is not a suitable load source.
    let csv = directory.path().join("regression.csv");
    std::fs::write(&csv, "id,amount\n1,10\n2,20\n").expect("write CSV fixture");
    let source = csv.to_str().expect("UTF-8 fixture path");
    run(&[
        "lakehouse",
        "upload-table",
        "--workspace",
        workspace,
        "--id",
        &lakehouse,
        "--source-path",
        source,
        "--table",
        "regression_table",
        "--schema",
        "dbo",
        "--format",
        "Csv",
    ]);
    let url = format!(
        "https://onelake.dfs.fabric.microsoft.com/{workspace}/{lakehouse}/Tables/dbo/regression_table"
    );
    (url, Some(resource))
}

#[test]
#[ignore = "requires live Fabric tenant and FABIO_TEST_SOURCE_WORKSPACE"]
fn live_report_native_model_reference() {
    let workspace = required_env("FABIO_TEST_SOURCE_WORKSPACE");
    let (model, mut model_resource) = create_item("semantic-model", &workspace, &[]);
    let (report, mut report_resource) = create_item(
        "report",
        &workspace,
        &[
            "--semantic-model",
            &model,
            "--semantic-model-workspace",
            &workspace,
        ],
    );
    let show = ["report", "show", "--workspace", &workspace, "--id", &report];
    assert_reference(&run(&show)["data"]["id"], &report);
    let definition = run(&[
        "report",
        "get-definition",
        "--workspace",
        &workspace,
        "--id",
        &report,
        "--decode",
    ]);
    // Verify the actual binding, not merely that the report API accepted an ID.
    let bound = definition["data"]["definition"]["parts"]
        .as_array()
        .expect("expected report definition")
        .iter()
        .any(|part| {
            part["path"] == "definition.pbir" && part["decodedPayload"].to_string().contains(&model)
        });
    assert!(
        bound,
        "report must reference the test-created semantic model"
    );
    report_resource.delete();
    assert_not_found(&show);
    model_resource.delete();
}

#[test]
#[ignore = "requires capacity administrator permissions and FABIO_TEST_CAPACITY_ID"]
fn live_capacity_blocked_actor_lifecycle() {
    let capacity = required_env("FABIO_TEST_CAPACITY_ID");
    // Preflight before creating anything; never change capacity-wide settings.
    list(&run(&[
        "capacity",
        "list-blocked-actors",
        "--id",
        &capacity,
    ]));
    let (workspace, _workspace_resource) = new_workspace();
    let expiration = (chrono::Utc::now() + chrono::Duration::minutes(10))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let create = [
        "capacity",
        "create-blocked-actor",
        "--id",
        &capacity,
        "--workspace",
        &workspace,
        "--expiration-policy",
        "ExpireAfterDate",
        "--expire-after",
        &expiration,
    ];
    assert_eq!(
        run(&[create.as_slice(), &["--dry-run"]].concat())["data"]["dry_run"],
        true
    );
    let result = run(&create);
    let actor = field(&result["data"], "id");
    let mut blocked = CreatedResource::new(&[
        "capacity",
        "delete-blocked-actor",
        "--id",
        &capacity,
        "--actor-id",
        &actor,
    ]);
    let show = [
        "capacity",
        "show-blocked-actor",
        "--id",
        &capacity,
        "--actor-id",
        &actor,
    ];
    assert_reference(
        &run(&show)["data"]["actorDetails"]["workspaceId"],
        &workspace,
    );
    assert!(contains_id(
        &run(&[
            "capacity",
            "list-blocked-actors",
            "--id",
            &capacity,
            "--all"
        ]),
        &actor
    ));
    let update = [
        "capacity",
        "update-blocked-actor",
        "--id",
        &capacity,
        "--actor-id",
        &actor,
        "--expiration-policy",
        "NeverExpire",
    ];
    run(&update);
    assert_eq!(
        run(&show)["data"]["expirationPolicy"]["policyType"],
        "NeverExpire"
    );
    blocked.delete();
    assert_not_found(&show);
}

#[test]
#[ignore = "requires capacity administrator permissions and FABIO_TEST_CAPACITY_ID"]
fn live_capacity_blocked_actor_duplicate_returns_structured_error() {
    let capacity = required_env("FABIO_TEST_CAPACITY_ID");
    let (workspace, _workspace_resource) = new_workspace();
    let create = [
        "capacity",
        "create-blocked-actor",
        "--id",
        &capacity,
        "--workspace",
        &workspace,
        "--expiration-policy",
        "NeverExpire",
    ];
    let result = run(&create);
    let actor = field(&result["data"], "id");
    let mut resource = CreatedResource::new(&[
        "capacity",
        "delete-blocked-actor",
        "--id",
        &capacity,
        "--actor-id",
        &actor,
    ]);
    // Current service rejects a duplicate POST, despite the spec's upsert claim.
    // This is an explicit negative test, NOT a substitute for the CRUD happy path.
    let Err(error) = execute(command(&create)) else {
        panic!("duplicate POST now succeeds; revisit the documented service discrepancy");
    };
    assert!(
        error.contains("API_ERROR"),
        "expected structured duplicate error; tenant details withheld"
    );
    let shown = run(&[
        "capacity",
        "show-blocked-actor",
        "--id",
        &capacity,
        "--actor-id",
        &actor,
    ]);
    assert_reference(&shown["data"]["id"], &actor);
    resource.delete();
}

#[test]
#[ignore = "requires capacity administrator permissions and FABIO_TEST_CAPACITY_ID"]
fn live_capacity_mission_critical_actor_lifecycle() {
    let capacity = required_env("FABIO_TEST_CAPACITY_ID");
    list(&run(&[
        "capacity",
        "list-mission-critical-actors",
        "--id",
        &capacity,
    ]));
    let (workspace, _workspace_resource) = new_workspace();
    let result = run(&[
        "capacity",
        "create-mission-critical-actor",
        "--id",
        &capacity,
        "--workspace",
        &workspace,
    ]);
    let actor = field(&result["data"], "id");
    let mut critical = CreatedResource::new(&[
        "capacity",
        "delete-mission-critical-actor",
        "--id",
        &capacity,
        "--actor-id",
        &actor,
    ]);
    let show = [
        "capacity",
        "show-mission-critical-actor",
        "--id",
        &capacity,
        "--actor-id",
        &actor,
    ];
    assert_reference(
        &run(&show)["data"]["actorDetails"]["workspaceId"],
        &workspace,
    );
    assert!(contains_id(
        &run(&[
            "capacity",
            "list-mission-critical-actors",
            "--id",
            &capacity,
            "--all"
        ]),
        &actor
    ));
    critical.delete();
    assert_not_found(&show);
}

#[test]
#[ignore = "requires live Fabric tenant and FABIO_TEST_SOURCE_WORKSPACE"]
fn live_environment_pool_activation_history() {
    let workspace = required_env("FABIO_TEST_SOURCE_WORKSPACE");
    let (id, mut resource) = create_item("environment", &workspace, &[]);
    let args = [
        "environment",
        "list-pool-activations",
        "--workspace",
        &workspace,
        "--id",
        &id,
        "--page-size",
        "10",
    ];
    assert_eq!(list(&run(&args)).len(), 0);
    assert_eq!(
        list(&run(&[
            args.as_slice(),
            &["--filter", "state eq 'Succeeded'", "--all"]
        ]
        .concat()))
        .len(),
        0
    );
    let missing = uuid::Uuid::new_v4().to_string();
    assert_not_found(&[
        "environment",
        "show-pool-activation",
        "--workspace",
        &workspace,
        "--id",
        &id,
        "--activation-id",
        &missing,
    ]);
    resource.delete();
}

#[test]
#[ignore = "requires live tenant and explicit environment/activation fixtures"]
fn live_environment_pool_activation_diagnostics() {
    let workspace = required_env("FABIO_TEST_SOURCE_WORKSPACE");
    let environment = required_env("FABIO_TEST_ENVIRONMENT_ID");
    let activation = required_env("FABIO_TEST_POOL_ACTIVATION_ID");
    let result = run(&[
        "environment",
        "show-pool-activation",
        "--workspace",
        &workspace,
        "--id",
        &environment,
        "--activation-id",
        &activation,
    ]);
    assert_reference(&result["data"]["jobInstanceId"], &activation);
    assert!(result["data"]["state"].is_string());
}

fn create_connection() -> (String, CreatedResource) {
    let name = unique_name("fabio-e2e");
    let result = run(&[
        "connection",
        "create",
        "--name",
        &name,
        "--connectivity-type",
        "ShareableCloud",
        "--connection-type",
        "Web",
        "--parameters",
        r#"{"url":"https://example.com"}"#,
        "--credential-type",
        "Anonymous",
        "--skip-test-connection",
    ]);
    let id = field(&result["data"], "id");
    let resource = CreatedResource::new(&["connection", "delete", "--id", &id]);
    (id, resource)
}

#[test]
#[ignore = "requires live Fabric tenant"]
fn live_connection_effective_role() {
    let (id, mut resource) = create_connection();
    let result = run(&["connection", "list", "--all"]);
    let connection = list(&result)
        .iter()
        .find(|item| item["id"].as_str() == Some(&id))
        .expect("created connection must appear in list");
    assert_eq!(connection["myRole"], "Owner");
    resource.delete();
    assert_not_found(&["connection", "show", "--id", &id]);
}

#[test]
#[ignore = "requires tenant Fabric Administrator permissions"]
fn live_admin_connection_lifecycle() {
    // An ordinary workspace user may receive AUTH_REQUIRED on admin APIs.
    list(&run(&["admin", "list-connections", "--limit", "1"]));
    let (id, mut resource) = create_connection();
    assert_reference(
        &run(&["admin", "show-connection", "--id", &id])["data"]["id"],
        &id,
    );
    assert!(contains_id(
        &run(&["admin", "list-connections", "--all"]),
        &id
    ));
    let roles = run(&[
        "admin",
        "list-connection-role-assignments",
        "--id",
        &id,
        "--all",
    ]);
    let role = list(&roles)
        .first()
        .expect("new connection must have an owner");
    let assignment = field(role, "id");
    let shown = run(&[
        "admin",
        "show-connection-role-assignment",
        "--id",
        &id,
        "--assignment-id",
        &assignment,
    ]);
    assert_eq!(shown["data"]["role"], "Owner");
    // Exercise the new admin DELETE, retaining the regular-owner fallback guard.
    let mut admin_delete = CreatedResource::new(&["admin", "delete-connection", "--id", &id]);
    admin_delete.delete();
    resource.disarm();
    assert_not_found(&["connection", "show", "--id", &id]);
}

#[test]
#[ignore = "requires Fabric Administrator and FABIO_TEST_CONNECTION_PRINCIPAL_ID"]
fn live_admin_connection_ownership_lifecycle() {
    // Explicit opt-in recipient: never grant ownership on an existing connection
    // and never discover an arbitrary tenant user to receive access.
    let principal = required_env("FABIO_TEST_CONNECTION_PRINCIPAL_ID");
    let principal_type = required_env("FABIO_TEST_CONNECTION_PRINCIPAL_TYPE");
    assert!(matches!(
        principal_type.as_str(),
        "User" | "ServicePrincipal"
    ));
    list(&run(&["admin", "list-connections", "--limit", "1"]));
    let (id, _resource) = create_connection();
    let result = run(&[
        "admin",
        "add-connection-role-assignment",
        "--id",
        &id,
        "--principal-id",
        &principal,
        "--principal-type",
        &principal_type,
    ]);
    let assignment = field(&result["data"], "id");
    let mut role_resource = CreatedResource::new(&[
        "admin",
        "delete-connection-role-assignment",
        "--id",
        &id,
        "--assignment-id",
        &assignment,
    ]);
    assert_eq!(result["data"]["role"], "PendingOwner");
    run(&[
        "admin",
        "update-connection-role-assignment",
        "--id",
        &id,
        "--assignment-id",
        &assignment,
    ]);
    let result = run(&[
        "admin",
        "show-connection-role-assignment",
        "--id",
        &id,
        "--assignment-id",
        &assignment,
    ]);
    assert_eq!(result["data"]["role"], "PendingOwner");
    role_resource.delete();
}

fn bulk_plan_body(logical_id: &str, content: &str) -> Value {
    let platform = json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/gitIntegration/platformProperties/2.0.0/schema.json",
        "metadata": {"type": "DeploymentPlan", "displayName": "RegressionPlan"},
        "config": {"version": "2.0", "logicalId": logical_id}
    });
    let parts: Vec<Value> = [
        (".platform", platform.to_string()),
        ("plan.yml", content.to_string()),
    ]
    .into_iter()
    .map(|(path, payload)| {
        json!({
            "path": format!("/RegressionPlan.DeploymentPlan/{path}"),
            "payload": base64::engine::general_purpose::STANDARD.encode(payload.as_bytes()),
            "payloadType": "InlineBase64"
        })
    })
    .collect();
    json!({"definitionParts": parts})
}

#[test]
#[ignore = "requires live Fabric tenant and FABIO_TEST_CAPACITY_ID"]
fn live_bulk_import_with_deployment_plan() {
    let (workspace, _workspace_resource) = new_workspace();
    let logical_id = uuid::Uuid::new_v4().to_string();
    let content = bulk_plan_body(&logical_id, &plan_content()).to_string();
    let args = [
        "item",
        "bulk-import-definitions",
        "--workspace",
        &workspace,
        "--content",
        &content,
        "--deployment-plan-logical-id",
        &logical_id,
    ];
    assert_eq!(
        run(&[args.as_slice(), &["--dry-run"]].concat())["data"]["dry_run"],
        true
    );
    let result = run(&args);
    let details = result["data"]["importItemDefinitionsDetails"]
        .as_array()
        .expect("expected import details");
    assert_eq!(details.len(), 1);
    assert_eq!(details[0]["operationStatus"], "Succeeded");
    let plans = run(&["deployment-plan", "list", "--workspace", &workspace]);
    assert_eq!(list(&plans).len(), 1);
}

#[test]
#[ignore = "requires live Fabric tenant, deployment pipeline permissions, and FABIO_TEST_CAPACITY_ID"]
fn live_pipeline_deploy_with_deployment_plan() {
    let (source, _source_resource) = new_workspace();
    let (target, _target_resource) = new_workspace();
    let name = unique_name("fabio-e2e");
    let result = run(&[
        "deployment-pipeline",
        "create",
        "--name",
        &name,
        "--stage",
        "Source",
        "--stage",
        "Target",
    ]);
    let pipeline = field(&result["data"], "id");
    // Drop the pipeline (and its workspace assignments) before either workspace.
    let _pipeline_resource =
        CreatedResource::new(&["deployment-pipeline", "delete", "--id", &pipeline]);
    let stages = run(&["deployment-pipeline", "list-stages", "--id", &pipeline]);
    let stages = list(&stages);
    assert_eq!(stages.len(), 2);
    let source_stage = field(
        stages
            .iter()
            .find(|stage| stage["order"] == 0)
            .expect("source stage"),
        "id",
    );
    let target_stage = field(
        stages
            .iter()
            .find(|stage| stage["order"] == 1)
            .expect("target stage"),
        "id",
    );
    for (stage, workspace) in [(&source_stage, &source), (&target_stage, &target)] {
        run(&[
            "deployment-pipeline",
            "assign-workspace",
            "--id",
            &pipeline,
            "--stage-id",
            stage,
            "--workspace",
            workspace,
        ]);
    }
    let (plan, _plan_resource) =
        create_item("deployment-plan", &source, &["--content", &plan_content()]);
    let (notebook, _notebook_resource) = create_item(
        "notebook",
        &source,
        &["--content", "print('deployment regression')"],
    );
    // An incoming source-stage plan must itself be selected for deployment.
    let items = json!([
        {"sourceItemId": notebook, "itemType": "Notebook"},
        {"sourceItemId": plan, "itemType": "DeploymentPlan"}
    ])
    .to_string();
    let args = [
        "deployment-pipeline",
        "deploy",
        "--id",
        &pipeline,
        "--source-stage-id",
        &source_stage,
        "--target-stage-id",
        &target_stage,
        "--items",
        &items,
        "--deployment-plan-id",
        &plan,
    ];
    assert_eq!(
        run(&[args.as_slice(), &["--dry-run"]].concat())["data"]["dry_run"],
        true
    );
    run(&args);
    let notebooks = run(&["notebook", "list", "--workspace", &target]);
    assert_eq!(list(&notebooks).len(), 1);
    assert_eq!(
        list(&run(&["deployment-plan", "list", "--workspace", &target])).len(),
        1
    );
}

fn action_pipeline(workspace: &str, fail: bool) -> (String, CreatedResource) {
    let activity = if fail {
        json!({"name": "intentional_failure", "type": "Fail", "typeProperties": {
            "message": "Expected isolated deployment-plan test failure", "errorCode": "FABIO_E2E_EXPECTED_FAILURE"
        }})
    } else {
        json!({"name": "wait", "type": "Wait", "typeProperties": {"waitTimeInSeconds": 1}})
    };
    let definition = json!({"parts": [{"path": "pipeline-content.json", "payloadType": "InlineBase64",
        "payload": base64::engine::general_purpose::STANDARD.encode(json!({"properties": {"activities": [activity]}}).to_string().as_bytes())}]}).to_string();
    let (id, resource) = create_item(
        "item",
        workspace,
        &["--type", "DataPipeline", "--definition", &definition],
    );
    assign_fixture_logical_id(workspace, &id);
    (id, resource)
}

fn logical_identity(workspace: &str, id: &str) -> String {
    let result = run(&["item", "list", "--workspace", workspace, "--all"]);
    let item = list(&result)
        .iter()
        .find(|item| item["id"].as_str() == Some(id))
        .expect("item metadata");
    field(item, "logicalId")
}

fn assign_fixture_logical_id(workspace: &str, id: &str) {
    // Only called immediately after this test creates an item, before any plan
    // references it. Never reassign an existing tenant item's identity.
    let logical = uuid::Uuid::new_v4().to_string();
    let args = [
        "item",
        "update-logical-id",
        "--workspace",
        workspace,
        "--id",
        id,
        "--logical-id",
        &logical,
    ];
    assert_eq!(
        run(&[args.as_slice(), &["--dry-run"]].concat())["data"]["dry_run"],
        true
    );
    run(&args);
    wait_for_ready(
        || try_run(&["item", "list", "--workspace", workspace, "--all"]),
        |result| {
            result["data"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["id"].as_str() == Some(id) && item["logicalId"].as_str() == Some(&logical)
                })
            })
        },
        Duration::from_mins(1),
    )
    .expect("fixture logical identity was not visible in item metadata");
    let actual = logical_identity(workspace, id);
    assert!(
        actual == logical,
        "fixture logical-ID assignment must persist in item metadata"
    );
}

fn ordered_plan(first: &str, second: &str) -> String {
    // Deliberately reverse file order: dependencies, not array order, must win.
    json!({"$schema": "https://developer.microsoft.com/json-schemas/fabric/item/deploymentPlan/definition/plan/1.0.0/schema.json",
        "version": "1.0.0", "groups": [
            {"name": "second", "logicalId": second, "dependsOn": [{"groupName": "first"}],
                "postActions": [{"name": "run_second", "job": {"type": "Execute", "logicalId": second}}]},
            {"name": "first", "logicalId": first,
                "postActions": [{"name": "run_first", "job": {"type": "Execute", "logicalId": first}}]}
        ]}).to_string()
}

fn target_pipeline(items: &Value, source_id: &str, source: &str) -> Option<String> {
    let shown = run(&["item", "show", "--workspace", source, "--id", source_id]);
    let name = field(&shown["data"], "displayName");
    list(items)
        .iter()
        .find(|item| item["type"] == "DataPipeline" && item["displayName"].as_str() == Some(&name))
        .and_then(|item| item["id"].as_str())
        .map(str::to_string)
}

fn completed_run(workspace: &str, item: &str, status: &str) -> Value {
    let mut latest = Value::Null;
    wait_for_ready(
        || {
            let result = try_run(&[
                "job-scheduler",
                "list-instances",
                "--workspace",
                workspace,
                "--id",
                item,
            ])?;
            latest = result.clone();
            Ok(result)
        },
        |result| {
            result["data"]
                .as_array()
                .is_some_and(|runs| runs.iter().any(|run| run["status"] == status))
        },
        Duration::from_mins(2),
    )
    .expect("expected terminal action run was not visible");
    list(&latest)
        .iter()
        .find(|run| run["status"] == status)
        .expect("terminal run")
        .clone()
}

fn native_action_deployment(fail: bool) {
    let (source, _source_resource) = new_workspace();
    let (target, _target_resource) = new_workspace();
    let name = unique_name("fabio-e2e");
    let result = run(&[
        "deployment-pipeline",
        "create",
        "--name",
        &name,
        "--stage",
        "Source",
        "--stage",
        "Target",
    ]);
    let pipeline = field(&result["data"], "id");
    let _pipeline_resource =
        CreatedResource::new(&["deployment-pipeline", "delete", "--id", &pipeline]);
    let stages = run(&["deployment-pipeline", "list-stages", "--id", &pipeline]);
    let source_stage = field(
        list(&stages)
            .iter()
            .find(|stage| stage["order"] == 0)
            .expect("source stage"),
        "id",
    );
    let target_stage = field(
        list(&stages)
            .iter()
            .find(|stage| stage["order"] == 1)
            .expect("target stage"),
        "id",
    );
    for (stage, workspace) in [(&source_stage, &source), (&target_stage, &target)] {
        run(&[
            "deployment-pipeline",
            "assign-workspace",
            "--id",
            &pipeline,
            "--stage-id",
            stage,
            "--workspace",
            workspace,
        ]);
    }
    let (first, _first_resource) = action_pipeline(&source, fail);
    let (second, _second_resource) = action_pipeline(&source, false);
    let content = ordered_plan(
        &logical_identity(&source, &first),
        &logical_identity(&source, &second),
    );
    let validation = run(&["deployment-plan", "validate", "--content", &content]);
    assert_eq!(validation["data"]["valid"], true);
    assert_eq!(validation["data"]["actions"], 2);
    let (plan, _plan_resource) = create_item("deployment-plan", &source, &["--content", &content]);
    let items = json!([
        {"sourceItemId": first, "itemType": "DataPipeline"},
        {"sourceItemId": second, "itemType": "DataPipeline"},
        {"sourceItemId": plan, "itemType": "DeploymentPlan"}
    ])
    .to_string();
    let result = try_run(&[
        "deployment-pipeline",
        "deploy",
        "--id",
        &pipeline,
        "--source-stage-id",
        &source_stage,
        "--target-stage-id",
        &target_stage,
        "--items",
        &items,
        "--deployment-plan-id",
        &plan,
    ]);
    if fail {
        let Err(error) = result else {
            panic!("the intentionally failing action must stop deployment");
        };
        assert!(
            error.contains("API_ERROR"),
            "expected a structured action failure; tenant details withheld"
        );
    } else {
        assert!(
            result.is_ok(),
            "native action deployment failed; tenant details withheld"
        );
    }
    verify_native_actions(&source, &target, &first, &second, fail);
}

fn verify_native_actions(source: &str, target: &str, first: &str, second: &str, fail: bool) {
    let target_items = run(&["item", "list", "--workspace", target, "--all"]);
    let first_target = target_pipeline(&target_items, first, source)
        .expect("first action item must have deployed");
    if fail {
        completed_run(target, &first_target, "Failed");
        assert!(
            target_pipeline(&target_items, second, source).is_none(),
            "dependent item must not deploy after the first action fails"
        );
    } else {
        let second_target =
            target_pipeline(&target_items, second, source).expect("second action item must deploy");
        let first_run = completed_run(target, &first_target, "Completed");
        let second_run = completed_run(target, &second_target, "Completed");
        let ended = job_timestamp(&field(&first_run, "endTimeUtc"));
        let started = job_timestamp(&field(&second_run, "startTimeUtc"));
        assert!(
            started >= ended,
            "second action must start only after the first completes"
        );
    }
}

#[test]
#[ignore = "requires live Fabric tenant and FABIO_TEST_CAPACITY_ID; executes isolated Wait jobs"]
fn live_native_plan_orders_and_executes_actions() {
    native_action_deployment(false);
}

fn job_timestamp(value: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&chrono::Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|value| value.and_utc())
        })
        .expect("UTC job timestamp format")
}

#[test]
fn job_history_timestamps_accept_explicit_and_implied_utc() {
    assert_eq!(
        job_timestamp("2026-01-01T12:00:00.1234567Z"),
        job_timestamp("2026-01-01T12:00:00.1234567")
    );
    assert_eq!(
        job_timestamp("2026-01-01T12:00:00Z"),
        job_timestamp("2026-01-01T12:00:00")
    );
}

#[test]
#[ignore = "requires live Fabric tenant and FABIO_TEST_CAPACITY_ID; intentionally fails an isolated job"]
fn live_native_plan_failure_stops_dependent_deployment() {
    native_action_deployment(true);
}

#[test]
#[ignore = "requires live Fabric tenant and FABIO_TEST_CAPACITY_ID"]
#[allow(clippy::too_many_lines)] // One isolated create/update/rename lifecycle with shared cleanup.
fn live_generic_deploy_preserves_plan_identity_without_executing_actions() {
    let (source, _source_resource) = new_workspace();
    let (target, _target_resource) = new_workspace();
    let (first, _first_resource) = action_pipeline(&source, false);
    let (second, _second_resource) = action_pipeline(&source, false);
    let first_logical = logical_identity(&source, &first);
    let second_logical = logical_identity(&source, &second);
    let content = ordered_plan(&first_logical, &second_logical);
    let (plan, _plan_resource) = create_item("deployment-plan", &source, &["--content", &content]);
    assign_fixture_logical_id(&source, &plan);
    let plan_logical = logical_identity(&source, &plan);
    let directory = tempfile::tempdir().expect("export directory");
    let export = directory.path().join("items");
    run(&[
        "deploy",
        "export",
        "--workspace",
        &source,
        "--dir",
        export.to_str().expect("UTF-8 export path"),
    ]);
    run(&[
        "deploy",
        "validate",
        "--source",
        export.to_str().expect("UTF-8 export path"),
    ]);
    let plan_file = std::fs::read_dir(&export)
        .expect("exported item directories")
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("plan.yml"))
        .find(|path| path.is_file())
        .expect("exported plan.yml");
    run(&[
        "deployment-plan",
        "validate",
        "--file",
        plan_file.to_str().expect("UTF-8 plan path"),
        "--source",
        export.to_str().expect("UTF-8 export path"),
        "--strict",
    ]);
    run(&[
        "deploy",
        "apply",
        "--source",
        export.to_str().expect("UTF-8 export path"),
        "--workspace",
        &target,
        "--strategy",
        "sequential",
        "--no-post-hooks",
    ]);
    wait_for_ready(
        || try_run(&["item", "list", "--workspace", &target, "--all"]),
        |result| {
            result["data"].as_array().is_some_and(|items| {
                [&plan_logical, &first_logical, &second_logical]
                    .iter()
                    .all(|logical| {
                        items
                            .iter()
                            .any(|item| item["logicalId"].as_str() == Some(logical.as_str()))
                    })
            })
        },
        Duration::from_mins(1),
    )
    .expect("generic deploy must preserve the plan's assigned logical identity");
    // Exercise the existing-item path too. It must not rewrite plan references
    // or silently reassign identities on an incremental redeployment.
    run(&[
        "deploy",
        "apply",
        "--source",
        export.to_str().expect("UTF-8 export path"),
        "--workspace",
        &target,
        "--strategy",
        "sequential",
        "--no-post-hooks",
    ]);
    let items = run(&["item", "list", "--workspace", &target, "--all"]);
    let target_plan = list(&items)
        .iter()
        .find(|item| item["type"] == "DeploymentPlan")
        .expect("deployed plan");
    let target_id = field(target_plan, "id");
    let actual = logical_identity(&target, &target_id);
    assert!(
        actual == plan_logical,
        "plan assigned metadata identity must remain logical and portable"
    );
    let definition = run(&[
        "deployment-plan",
        "get-definition",
        "--workspace",
        &target,
        "--id",
        &target_id,
        "--decode",
    ]);
    let part = definition["data"]["definition"]["parts"]
        .as_array()
        .expect("definition parts")
        .iter()
        .find(|part| part["path"] == "plan.yml")
        .expect("plan.yml");
    let actual = part["decodedPayload"].to_string();
    assert!(
        actual.contains(&first_logical) && actual.contains(&second_logical),
        "plan references must not be rewritten into physical item IDs"
    );
    for id in [
        target_pipeline(&items, &first, &source).expect("first target"),
        target_pipeline(&items, &second, &source).expect("second target"),
    ] {
        assert_eq!(
            list(&run(&[
                "job-scheduler",
                "list-instances",
                "--workspace",
                &target,
                "--id",
                &id
            ]))
            .len(),
            0
        );
    }
    verify_generic_rename(&export, &target, &first_logical, &items);
}

fn verify_generic_rename(
    export: &std::path::Path,
    target: &str,
    first_logical: &str,
    items: &Value,
) {
    let first_target = field(
        list(items)
            .iter()
            .find(|item| item["logicalId"].as_str() == Some(first_logical))
            .expect("first target before rename"),
        "id",
    );
    let mut renamed = false;
    for entry in std::fs::read_dir(export).expect("exported item directories") {
        let platform_path = entry.expect("item directory").path().join(".platform");
        if !platform_path.is_file() {
            continue;
        }
        let mut metadata: Value =
            serde_json::from_slice(&std::fs::read(&platform_path).expect("platform bytes"))
                .expect("platform JSON");
        if metadata["config"]["logicalId"].as_str() == Some(first_logical) {
            metadata["metadata"]["displayName"] = Value::from(unique_name("fabio_e2e_renamed"));
            std::fs::write(&platform_path, metadata.to_string()).expect("rename exported fixture");
            renamed = true;
            break;
        }
    }
    assert!(renamed, "source fixture for rename must exist");
    run(&[
        "deploy",
        "apply",
        "--source",
        export.to_str().expect("UTF-8 export path"),
        "--workspace",
        target,
        "--strategy",
        "sequential",
        "--no-post-hooks",
    ]);
    let after = run(&["item", "list", "--workspace", target, "--all"]);
    assert_eq!(list(&after).len(), list(items).len());
    let renamed = list(&after)
        .iter()
        .find(|item| item["logicalId"].as_str() == Some(first_logical))
        .expect("renamed target");
    assert_reference(&renamed["id"], &first_target);
}

#[test]
fn live_runner_redacts_unexpected_errors() {
    let result = execute(command(&[
        "deployment-plan",
        "show",
        "--workspace",
        "not-a-uuid",
        "--id",
        "private-resource",
    ]));
    let error = result.unwrap_err();
    assert!(error.contains("INVALID_INPUT"));
    assert!(!error.contains("private-resource"));
    assert!(!error.contains("not-a-uuid"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_runner_reports_permission_failures_without_tenant_messages() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/admin/connections"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "errorCode": "Forbidden", "message": "private-tenant-detail"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut command = common::mock_fabio(&server);
    command.args(["admin", "list-connections"]);
    let error = execute(command).unwrap_err();
    assert!(error.contains("FORBIDDEN"));
    assert!(!error.contains("private-tenant-detail"));
    assert!(!error.contains(&server.uri()));
}

#[test]
fn deployment_plan_fixture_uses_inline_base64() {
    let id = "aaaaaaaa-1111-2222-3333-444444444444";
    let body = bulk_plan_body(id, &plan_content());
    let parts = body["definitionParts"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    let decoded: Vec<Value> = parts
        .iter()
        .map(|part| {
            assert_eq!(part["payloadType"], "InlineBase64");
            let payload = base64::engine::general_purpose::STANDARD
                .decode(part["payload"].as_str().unwrap())
                .unwrap();
            serde_yaml::from_slice(&payload).unwrap()
        })
        .collect();
    assert_eq!(decoded[0]["config"]["logicalId"], id);
    assert_eq!(decoded[0]["metadata"]["type"], "DeploymentPlan");
    assert_eq!(decoded[1]["version"], "1.0.0");
    assert_eq!(decoded[1]["groups"], json!([]));
    let schema = run(&["context", "schema", "DeploymentPlan"]);
    let template: Value = serde_yaml::from_str(
        schema["data"]["definition_content_template"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(template, decoded[1]);
}
