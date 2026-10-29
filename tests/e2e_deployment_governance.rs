//! Live regression coverage for the deployment/governance APIs added in PR #188.
//!
//! See tests/README.md for prerequisites and commands. These tests fail (rather
//! than silently pass) when a preview API or required permission is unavailable.
//! Only resources created by the current test are mutated or deleted.

mod common;

use assert_cmd::Command;
use base64::Engine as _;
use common::{fabio, unique_name};
use serde_json::{Value, json};
use std::time::Duration;

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for this live test"))
}

/// Do not include commands, resource IDs, names, or raw responses in failures.
/// `assert_cmd`'s default failure report includes all of them.
fn execute(mut command: Command) -> Result<Value, String> {
    let output = command
        .timeout(Duration::from_mins(5))
        .output()
        .map_err(|_| "failed to execute fabio".to_string())?;
    if output.status.success() {
        serde_json::from_slice(&output.stdout)
            .map_err(|_| "successful command returned invalid JSON".to_string())
    } else {
        let error = String::from_utf8_lossy(&output.stderr)
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str::<Value>(line).ok());
        // Only known machine-readable codes are safe to publish. Never print an
        // arbitrary API message, hint, or string supplied by the tenant.
        let code = error.as_ref().and_then(|v| v["error"]["code"].as_str());
        let code = code
            .filter(|code| {
                matches!(
                    *code,
                    "AUTH_REQUIRED"
                        | "FORBIDDEN"
                        | "NOT_FOUND"
                        | "API_ERROR"
                        | "INVALID_INPUT"
                        | "CONFLICT"
                        | "TIMEOUT"
                        | "CAPACITY_INACTIVE"
                        | "READONLY_MODE"
                        | "RATE_LIMITED"
                        | "NETWORK_ERROR"
                )
            })
            .unwrap_or("UNKNOWN_ERROR");
        Err(format!(
            "fabio failed with {code} (tenant details withheld)"
        ))
    }
}

fn command(args: &[&str]) -> Command {
    let mut command = fabio();
    command.args(["--wrap-untrusted", "--lro-timeout", "240"]);
    command.args(args);
    command
}

fn run(args: &[&str]) -> Value {
    execute(command(args)).unwrap_or_else(|error| panic!("{} {}: {error}", args[0], args[1]))
}

fn field(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("expected string field {key} (tenant details withheld)"))
        .to_string()
}

fn list(value: &Value) -> &[Value] {
    let items = value["data"].as_array().expect("expected list envelope");
    assert_eq!(value["count"].as_u64(), Some(items.len() as u64));
    items
}

fn contains_id(value: &Value, id: &str) -> bool {
    list(value)
        .iter()
        .any(|item| item["id"].as_str() == Some(id))
}

fn assert_reference(actual: &Value, expected: &str) {
    assert!(
        actual.as_str().is_some_and(|id| id == expected),
        "resource reference mismatch; tenant details withheld"
    );
}

/// Register cleanup immediately after creation, before assertions can unwind.
/// The guard is never constructed for an existing tenant resource.
struct CreatedResource {
    delete_args: Vec<String>,
    active: bool,
}

impl CreatedResource {
    fn new(args: &[&str]) -> Self {
        Self {
            delete_args: args.iter().map(|arg| (*arg).to_string()).collect(),
            active: true,
        }
    }

    fn delete(&mut self) {
        let args: Vec<&str> = self.delete_args.iter().map(String::as_str).collect();
        let preview = run(&[args.as_slice(), &["--dry-run"]].concat());
        assert_eq!(preview["data"]["dry_run"], true);
        assert_eq!(preview["data"]["destructive"], true);
        let result = run(&args);
        self.active = false;
        assert_eq!(result["data"]["status"], "deleted");
    }
}

impl Drop for CreatedResource {
    fn drop(&mut self) {
        if self.active {
            let args: Vec<&str> = self.delete_args.iter().map(String::as_str).collect();
            let result = execute(command(&args));
            if result.is_err() {
                if std::thread::panicking() {
                    eprintln!(
                        "cleanup failed for a test-created resource; tenant details withheld"
                    );
                } else {
                    panic!("cleanup failed for a test-created resource; tenant details withheld");
                }
            }
        }
    }
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
    let deadline = std::time::Instant::now() + Duration::from_mins(2);
    loop {
        let current = run(&["workspace", "show", "--id", &id]);
        if current["data"]["capacityId"]
            .as_str()
            .is_some_and(|value| value == capacity)
            && current["data"]["capacityAssignmentProgress"] == "Completed"
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "test workspace capacity assignment did not complete"
        );
        std::thread::sleep(Duration::from_secs(2));
    }
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
    let deadline = std::time::Instant::now() + Duration::from_mins(2);
    loop {
        let current = run(&[
            "lakehouse",
            "show",
            "--workspace",
            workspace,
            "--id",
            &lakehouse,
        ]);
        if current["data"]["properties"]["sqlEndpointProperties"]["provisioningStatus"] == "Success"
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "test lakehouse provisioning did not complete"
        );
        std::thread::sleep(Duration::from_secs(2));
    }
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
    resource.active = false;
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
