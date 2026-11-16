//! Mandatory value-set activation before publishing reference-bearing Eventstreams.
use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};

use super::changeset::{ChangeAction, Changeset, DeployFailure};
use super::platform::SourceWorkspace;
use crate::client::{FabricClient, validate_uuid};
use crate::commands::eventstream::variable_references;
use crate::errors::{ErrorCode, FabioError, HintType};

pub(super) type References = BTreeMap<String, BTreeSet<String>>;

pub(super) fn selected_references(
    source: &SourceWorkspace,
    changeset: &Changeset,
) -> Result<References> {
    let mut result = References::new();
    for item in &source.items {
        if !item.metadata.item_type.eq_ignore_ascii_case("Eventstream") {
            continue;
        }
        if changeset.changes.iter().any(|change| {
            change.item_type.eq_ignore_ascii_case("Eventstream")
                && change.name == item.metadata.display_name
                && change.action == ChangeAction::Delete
        }) {
            continue;
        }
        let changed = changeset.changes.iter().any(|change| {
            change.item_type.eq_ignore_ascii_case("Eventstream")
                && change.name == item.metadata.display_name
                && matches!(
                    change.action,
                    ChangeAction::Create | ChangeAction::Update | ChangeAction::Rename
                )
        });
        let Some(part) = item
            .parts
            .iter()
            .find(|part| part.path == "eventstream.json")
        else {
            continue;
        };
        let definition: Value =
            serde_json::from_slice(&BASE64.decode(&part.payload).map_err(|_| {
                FabioError::invalid_input("Invalid Eventstream definition encoding")
            })?)
            .map_err(|_| FabioError::invalid_input("Invalid Eventstream definition JSON"))?;
        for (library, variable) in variable_references::references(&definition)? {
            let library_changed = changeset.changes.iter().any(|change| {
                change.item_type.eq_ignore_ascii_case("VariableLibrary")
                    && change.name == library
                    && matches!(
                        change.action,
                        ChangeAction::Create | ChangeAction::Update | ChangeAction::Rename
                    )
            });
            if changed || library_changed {
                result.entry(library).or_default().insert(variable);
            }
        }
    }
    Ok(result)
}

pub(super) fn validate_mode(
    references: &References,
    environment: Option<&str>,
    strategy: &str,
    no_post_hooks: bool,
) -> Result<()> {
    if references.is_empty() {
        return Ok(());
    }
    if environment.is_none_or(|value| value.trim().is_empty())
        || strategy == "bulk"
        || no_post_hooks
    {
        return Err(FabioError::with_typed_hint(ErrorCode::InvalidInput,
            "Eventstream variable references require a reviewed value set and pre-stream activation",
            "Review --env <VALUE_SET> with the default/sequential strategy and hooks enabled, or deploy/configure the library and dependencies explicitly before publishing the stream. Bulk cannot activate a library between imports. Do not guess the environment, switch strategy or retry automatically; review a new scoped plan.",
            HintType::SemanticCorrection).into());
    }
    Ok(())
}

pub(super) fn validate_scope(
    references: &References,
    workspace: &str,
    changeset: &Changeset,
) -> Result<()> {
    if references.is_empty() {
        return Ok(());
    }
    validate_uuid(workspace, "workspace")?;
    let mut ids = BTreeSet::new();
    for library in references.keys() {
        let matches: Vec<_> = changeset
            .changes
            .iter()
            .filter(|change| {
                change.item_type.eq_ignore_ascii_case("VariableLibrary") && change.name == *library
            })
            .collect();
        if matches.len() != 1 {
            return Err(FabioError::invalid_input(
                "Referenced variable library must have exactly one scoped plan entry",
            )
            .into());
        }
        let change=matches.first().copied().filter(|change|change.action!=ChangeAction::Delete)
            .ok_or_else(||FabioError::with_hint(ErrorCode::InvalidInput,"Referenced variable library is absent or deleted in the scoped plan",
                "Include the intended target library and its dependencies in a reviewed plan. No tenant-wide lookup or automatic library selection is performed."))?;
        if change.action != ChangeAction::Create {
            let id = change.deployed_id.as_deref().ok_or_else(|| {
                FabioError::invalid_input(
                    "Existing variable library requires an explicit deployed ID",
                )
            })?;
            validate_uuid(id, "variable library")?;
            if !ids.insert(uuid::Uuid::parse_str(id).expect("validated UUID")) {
                return Err(FabioError::invalid_input(
                    "Different referenced libraries cannot share a deployed ID",
                )
                .into());
            }
        }
    }
    for change in changeset
        .changes
        .iter()
        .filter(|change| change.item_type.eq_ignore_ascii_case("Eventstream"))
    {
        if let Some(id) = &change.deployed_id {
            validate_uuid(id, "Eventstream")?;
        }
        if matches!(change.action, ChangeAction::Update | ChangeAction::Rename)
            && change.deployed_id.is_none()
        {
            return Err(FabioError::invalid_input(
                "Existing Eventstream requires an explicit deployed ID",
            )
            .into());
        }
    }
    Ok(())
}

fn validate_library_definition(
    definition: &Value,
    environment: &str,
    variables: &BTreeSet<String>,
) -> Result<()> {
    let parts = definition
        .pointer("/definition/parts")
        .and_then(Value::as_array)
        .ok_or_else(|| FabioError::api_error("Variable library definition is unavailable"))?;
    let decode = |name: &str| -> Result<Value> {
        let matches: Vec<_> = parts
            .iter()
            .filter(|part| part["path"].as_str() == Some(name))
            .collect();
        if matches.len() != 1 {
            return Err(FabioError::api_error(
                "Variable library prerequisite part missing or duplicated",
            )
            .into());
        }
        let bytes = BASE64
            .decode(
                matches[0]["payload"]
                    .as_str()
                    .ok_or_else(|| FabioError::api_error("Missing variable library payload"))?,
            )
            .map_err(|_| FabioError::api_error("Invalid variable library encoding"))?;
        serde_json::from_slice(&bytes)
            .map_err(|_| FabioError::api_error("Invalid variable library JSON").into())
    };
    let defaults = decode("variables.json")?;
    let variable_list = defaults["variables"]
        .as_array()
        .ok_or_else(|| FabioError::api_error("Variable list missing"))?;
    let names: BTreeSet<_> = variable_list
        .iter()
        .filter_map(|value| value["name"].as_str())
        .collect();
    if names.len() != variable_list.len() {
        return Err(
            FabioError::api_error("Variable library names are missing or duplicated").into(),
        );
    }
    if !variables.iter().all(|name| names.contains(name.as_str())) {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,"A referenced variable is absent from the target library",
            "Inspect the library definition and requested value set before publishing the Eventstream. Do not invent a variable, credential or target identifier.").into());
    }
    if environment != "Default value set" && environment != "Default" {
        let value_set = decode(&format!("valueSets/{environment}.json"))?;
        if value_set["name"].as_str() != Some(environment) {
            return Err(FabioError::invalid_input(
                "Target value set name does not match its definition",
            )
            .into());
        }
    }
    Ok(())
}

pub(super) async fn activate(
    client: &FabricClient,
    workspace: &str,
    references: &References,
    environment: &str,
    created_ids: &HashMap<(String, String), String>,
    failures: &[DeployFailure],
) -> Result<Vec<Value>> {
    validate_uuid(workspace, "workspace")?;
    let mut results = Vec::new();
    for (library, variables) in references {
        match activate_one(
            client,
            workspace,
            library,
            variables,
            environment,
            created_ids,
            failures,
        )
        .await
        {
            Ok(record) => {
                let failed = record["retryAllowed"] == false;
                results.push(record);
                if failed {
                    break;
                }
            }
            Err(error) => {
                let mut record =
                    super::failure::prerequisite_failure("activate_value_set", library, &error);
                record["phase"] = json!("pre_stream");
                record["workspace_id"] = json!(workspace);
                if let Some(id) = created_ids.iter().find_map(|((kind, name), id)| {
                    (kind.eq_ignore_ascii_case("VariableLibrary") && name == library).then_some(id)
                }) {
                    record["item_id"] = json!(id);
                }
                results.push(record);
                break;
            }
        }
    }
    Ok(results)
}

async fn activate_one(
    client: &FabricClient,
    workspace: &str,
    library: &str,
    variables: &BTreeSet<String>,
    environment: &str,
    created_ids: &HashMap<(String, String), String>,
    failures: &[DeployFailure],
) -> Result<Value> {
    if failures.iter().any(|failure| {
        failure
            .change
            .item_type
            .eq_ignore_ascii_case("VariableLibrary")
            && failure.change.name == *library
    }) {
        return Err(FabioError::api_error(
            "A required variable library failed deployment; no dependent stream may be published",
        )
        .into());
    }
    let id=created_ids.iter().find_map(|((kind,name),id)|(kind.eq_ignore_ascii_case("VariableLibrary") && name==library).then_some(id))
            .ok_or_else(||FabioError::with_hint(ErrorCode::InvalidInput,"Referenced variable library is not in the scoped deployment plan",
                "Include the target VariableLibrary (unchanged/skipped is supported) and its dependencies in a reviewed plan. No tenant-wide inventory or implicit library selection is performed."))?;
    validate_uuid(id, "variable library")?;
    let route = format!("/workspaces/{workspace}/variableLibraries/{id}");
    let definition = client
        .post_readonly_with_lro(&format!("{route}/getDefinition"), &json!({}))
        .await?;
    validate_library_definition(&definition, environment, variables)?;
    let current = client.get(&route).await?;
    if !current["id"]
        .as_str()
        .is_some_and(|value| value.eq_ignore_ascii_case(id))
        || current["displayName"].as_str() != Some(library)
    {
        return Err(FabioError::api_error("Variable library metadata scope mismatch").into());
    }
    let requested = if environment == "Default" {
        "Default value set"
    } else {
        environment
    };
    let already_active = current
        .pointer("/properties/activeValueSetName")
        .and_then(Value::as_str)
        == Some(requested);
    let verified = if already_active {
        current
    } else {
        client
            .patch(
                &route,
                &json!({"properties":{"activeValueSetName":requested}}),
            )
            .await?;
        match client.get(&route).await {
            Ok(value) => value,
            Err(error) => {
                let mut record = activation_record(library, id, workspace, requested, "failed");
                record["activationAcknowledged"] = json!(true);
                record["verified"] = json!(false);
                return Ok(super::failure::hook_failure(record, &error));
            }
        }
    };
    if !verified["id"]
        .as_str()
        .is_some_and(|value| value.eq_ignore_ascii_case(id))
        || verified["displayName"].as_str() != Some(library)
        || verified
            .pointer("/properties/activeValueSetName")
            .and_then(Value::as_str)
            != Some(requested)
    {
        let error = FabioError::with_typed_hint(ErrorCode::ApiError,"Variable library activation readback is unverified",
                "Inspect the target library read-only. Do not replay activation or publish dependent streams until the reviewed value set is confirmed.",HintType::SemanticCorrection)
                .set_retriable(Some(false));
        let mut record = activation_record(library, id, workspace, requested, "failed");
        record["activationAcknowledged"] = json!(!already_active);
        record["verified"] = json!(false);
        return Ok(super::failure::hook_failure(record, &error.into()));
    }
    let mut record = activation_record(
        library,
        id,
        workspace,
        requested,
        if already_active {
            "already_active"
        } else {
            "activated"
        },
    );
    record["verified"] = json!(true);
    Ok(record)
}

fn activation_record(
    library: &str,
    id: &str,
    workspace: &str,
    environment: &str,
    status: &str,
) -> Value {
    json!({"hook":"activate_value_set","phase":"pre_stream","item_type":"VariableLibrary","item_name":library,"item_id":id,"workspace_id":workspace,"value_set":environment,"status":status})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_modes_require_review_instead_of_implicit_environment_or_strategy_selection() {
        let refs = References::from([(
            "Library".to_string(),
            BTreeSet::from(["Target".to_string()]),
        )]);
        for (environment, strategy, hooks) in [
            (None, "default", false),
            (Some("test"), "bulk", false),
            (Some("test"), "default", true),
        ] {
            assert!(validate_mode(&refs, environment, strategy, hooks).is_err());
        }
        assert!(validate_mode(&refs, Some("test"), "sequential", false).is_ok());
        assert!(validate_mode(&References::new(), None, "bulk", true).is_ok());
    }

    #[test]
    fn target_definitions_must_contain_referenced_variables_and_the_exact_value_set() {
        let part = |path: &str, value: Value| json!({"path":path,"payload":BASE64.encode(value.to_string())});
        let definition = json!({"definition":{"parts":[part("variables.json",json!({"variables":[{"name":"Target","type":"ItemReference","value":{}}]})),
            part("valueSets/test.json",json!({"name":"test","variableOverrides":[]}))]}});
        let vars = BTreeSet::from(["Target".to_string()]);
        assert!(validate_library_definition(&definition, "test", &vars).is_ok());
        assert!(validate_library_definition(&definition, "other", &vars).is_err());
        assert!(
            validate_library_definition(
                &definition,
                "test",
                &BTreeSet::from(["Missing".to_string()])
            )
            .is_err()
        );
    }

    #[test]
    fn references_cover_changed_consumers_or_libraries_but_not_noops_or_deleted_consumers() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("Stream.Eventstream");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join(".platform"),
            json!({"metadata":{"type":"Eventstream","displayName":"Stream"}}).to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("eventstream.json"),json!({"sources":[],"streams":[],"destinations":[{"type":"Lakehouse","properties":{"itemReference":"$(/**/Settings/Target)"}}]}).to_string()).unwrap();
        let source = super::super::platform::parse_source_directory(home.path()).unwrap();
        for (stream, library, count) in [
            ("create", "skip", 1),
            ("skip", "update", 1),
            ("skip", "skip", 0),
            ("delete", "update", 0),
        ] {
            let changeset:Changeset=serde_json::from_value(json!({"changes":[{"name":"Stream","item_type":"Eventstream","action":stream,"reason":"test"},
                {"name":"Settings","item_type":"VariableLibrary","action":library,"reason":"test"}]})).unwrap();
            assert_eq!(
                selected_references(&source, &changeset).unwrap().len(),
                count
            );
        }
        let record = activation_record("Settings", "item", "workspace", "test", "activated");
        assert_eq!(record["phase"], "pre_stream");
        assert_eq!(record["item_type"], "VariableLibrary");
        assert_eq!(record["item_id"], "item");
        assert_eq!(record["workspace_id"], "workspace");
    }

    #[test]
    fn scoped_plan_guards_precede_any_activation_or_item_write() {
        let refs = References::from([(
            "Settings".to_string(),
            BTreeSet::from(["Target".to_string()]),
        )]);
        let ws = uuid::Uuid::nil().to_string();
        let mut plan:Changeset=serde_json::from_value(json!({"changes":[{"name":"Settings","item_type":"VariableLibrary","action":"skip","reason":"test","deployed_id":ws}]})).unwrap();
        assert!(validate_scope(&refs, &ws, &plan).is_ok());
        assert!(validate_scope(&refs, "../workspace", &plan).is_err());
        plan.changes.push(plan.changes[0].clone());
        assert!(validate_scope(&refs, &ws, &plan).is_err());
        plan.changes.pop();
        let mut other = plan.changes[0].clone();
        other.name = "Other".to_string();
        plan.changes.push(other);
        let mut two_refs = refs.clone();
        two_refs.insert("Other".to_string(), BTreeSet::from(["Target".to_string()]));
        assert!(validate_scope(&two_refs, &ws, &plan).is_err());
        plan.changes.pop();
        plan.changes[0].deployed_id = None;
        assert!(validate_scope(&refs, &ws, &plan).is_err());
        plan.changes[0].action = ChangeAction::Delete;
        assert!(validate_scope(&refs, &ws, &plan).is_err());
    }
}
