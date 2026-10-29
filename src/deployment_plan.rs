//! Offline deployment-plan analysis. Validation never fetches a schema, resolves
//! credentials, or executes a job; the service remains authoritative for runtime
//! permissions, variable values, lineage, and target-workspace readiness.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/deploymentPlan/definition/plan/1.0.0/schema.json";
const MAX_BYTES: usize = 1_048_576;
const MAX_FINDINGS: usize = 100;

/// A source catalog is optional: missing references can be pre-existing target
/// items, so source absence is a warning, not proof the plan cannot execute.
pub type Catalog = HashMap<Uuid, String>;

pub fn catalog<'a>(items: impl IntoIterator<Item = (&'a str, &'a str)>) -> Catalog {
    items
        .into_iter()
        .filter_map(|(id, item_type)| {
            Uuid::parse_str(id)
                .ok()
                .filter(|id| !id.is_nil())
                .map(|id| (id, item_type.to_string()))
        })
        .collect()
}

#[derive(Debug, Default, Serialize)]
#[allow(clippy::struct_excessive_bools)] // Independent JSON report signals, not a workflow state.
pub struct Validation {
    pub valid: bool,
    pub groups: usize,
    pub actions: usize,
    pub executes_jobs: bool,
    pub source_checked: bool,
    pub logical_ids: BTreeSet<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub truncated: bool,
}

impl Validation {
    fn error(&mut self, message: impl Into<String>) {
        if self.errors.len() < MAX_FINDINGS {
            self.errors.push(message.into());
        } else {
            self.truncated = true;
        }
    }

    fn warning(&mut self, message: impl Into<String>) {
        if self.warnings.len() < MAX_FINDINGS {
            self.warnings.push(message.into());
        } else {
            self.truncated = true;
        }
    }

    fn id(&mut self, value: &str, path: &str, catalog: Option<&Catalog>) -> Option<Uuid> {
        let Ok(id) = Uuid::parse_str(value) else {
            self.error(format!(
                "{path} must be a non-zero logical UUID, not a physical item ID or a name."
            ));
            return None;
        };
        if id.is_nil() {
            self.error(format!("{path} must not be the zero UUID."));
            return None;
        }
        self.logical_ids.insert(id.to_string());
        if catalog.is_some_and(|items| !items.contains_key(&id)) {
            self.warning(format!("{path} is absent from the source catalog; ensure the item already exists in the target or is included in the operation."));
        }
        Some(id)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    #[serde(rename = "$schema")]
    schema: String,
    version: String,
    groups: Option<Vec<Group>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Group {
    name: String,
    logical_id: String,
    depends_on: Option<Vec<GroupDependency>>,
    pre_actions: Option<Vec<Action>>,
    post_actions: Option<Vec<Action>>,
    options: Option<Vec<BTreeMap<String, String>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GroupDependency {
    group_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Action {
    name: String,
    job: Option<Job>,
    depends_on: Option<Vec<ActionDependency>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActionDependency {
    action_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Job {
    logical_id: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    parameters: Option<Vec<Parameter>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameter {
    name: String,
    value: Value,
}

/// Stable identity/reference fields must never undergo the generic
/// logical-ID-to-deployed-object-ID substitution used for runtime bindings.
#[inline]
pub fn preserves_logical_ids(item_type: &str, part_path: &str) -> bool {
    part_path == ".platform"
        || (item_type.eq_ignore_ascii_case("DeploymentPlan") && part_path == "plan.yml")
}

pub fn validate(content: &[u8], catalog: Option<&Catalog>) -> Validation {
    let mut result = Validation {
        source_checked: catalog.is_some(),
        ..Validation::default()
    };
    if content.len() > MAX_BYTES {
        result.error("Deployment plan exceeds the 1 MiB definition limit.");
        return result;
    }
    let plan = match serde_yaml::from_slice::<Plan>(content) {
        Ok(plan) => plan,
        Err(error) => {
            let mut message = error.to_string();
            message.truncate(message.floor_char_boundary(500));
            result.error(format!("Invalid plan.yml: {message}. Use the current $schema/version/groups format, not executableFormat/id."));
            return result;
        }
    };
    if plan.schema != SCHEMA {
        result.error(format!(
            "$schema must be {SCHEMA}; validation never downloads user-supplied schemas."
        ));
    }
    if plan.version != "1.0.0" {
        result.error("Unsupported deployment plan version; expected 1.0.0.");
    }
    let groups = plan.groups.unwrap_or_default();
    result.groups = groups.len();
    if groups.len() > 1000 {
        result.error("A deployment plan supports at most 1,000 groups.");
        return result;
    }
    let mut names = HashMap::new();
    let mut items = HashMap::new();
    for (index, group) in groups.iter().enumerate() {
        if !valid_name(&group.name) {
            result.error(format!("groups[{index}].name must contain 1-60 characters without control characters or surrounding whitespace."));
        }
        if names.insert(group.name.to_lowercase(), index).is_some() {
            result.error(format!(
                "groups[{index}] repeats a group name (case-insensitive)."
            ));
        }
        if let Some(id) = result.id(
            &group.logical_id,
            &format!("groups[{index}].logicalId"),
            catalog,
        ) && items.insert(id, index).is_some()
        {
            result.error(format!(
                "groups[{index}] deploys an item already used by another group."
            ));
        }
        validate_options(
            group.options.as_deref().unwrap_or_default(),
            index,
            &mut result,
        );
    }
    let mut edges = HashSet::new();
    let mut dataflows = HashSet::new();
    for (index, group) in groups.iter().enumerate() {
        let dependencies = group.depends_on.as_deref().unwrap_or_default();
        if dependencies.len() > 1000 {
            result.error(format!("groups[{index}] has more than 1,000 dependencies."));
        }
        for dependency in dependencies {
            if let Some(&target) = names.get(&dependency.group_name.to_lowercase()) {
                edges.insert((index, target));
            } else {
                result.error(format!(
                    "groups[{index}].dependsOn references an unknown group."
                ));
            }
        }
        validate_actions(
            group,
            index,
            catalog,
            &items,
            &mut edges,
            &mut dataflows,
            &mut result,
        );
    }
    if has_cycle(groups.len(), &edges) {
        result.error("Deployment groups contain a dependency cycle or self-dependency, including dependencies through action items.");
    }
    result.executes_jobs = result.actions > 0;
    result.valid = result.errors.is_empty();
    result
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 60
        && name.trim() == name
        && !name.chars().any(char::is_control)
}

fn validate_options(options: &[BTreeMap<String, String>], index: usize, result: &mut Validation) {
    if options.len() > 20 {
        result.error(format!("groups[{index}] has more than 20 options."));
    }
    for option in options {
        if option.len() != 1
            || option.iter().any(|(key, value)| {
                key.is_empty()
                    || key.len() > 64
                    || !key.chars().all(|c| c.is_ascii_alphabetic())
                    || value.chars().count() > 256
                    || value.contains(['\r', '\n'])
            })
        {
            result.error(format!("groups[{index}].options must contain single-name objects with ASCII-letter keys (1-64 characters) and single-line values of at most 256 characters."));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_actions(
    group: &Group,
    group_index: usize,
    catalog: Option<&Catalog>,
    group_items: &HashMap<Uuid, usize>,
    group_edges: &mut HashSet<(usize, usize)>,
    dataflows: &mut HashSet<Uuid>,
    result: &mut Validation,
) {
    let pre = group.pre_actions.as_deref().unwrap_or_default();
    let post = group.post_actions.as_deref().unwrap_or_default();
    if pre.len() > 20 || post.len() > 20 {
        result.error(format!(
            "groups[{group_index}] supports at most 20 pre-actions and 20 post-actions."
        ));
        return;
    }
    result.actions += pre.len() + post.len();
    let actions: Vec<&Action> = pre.iter().chain(post).collect();
    let names = index_actions(&actions, group_index, result);
    let mut edges = HashSet::new();
    for (index, action) in actions.iter().enumerate() {
        let path = format!("groups[{group_index}].actions[{index}]");
        let dependencies = action.depends_on.as_deref().unwrap_or_default();
        if dependencies.len() > 20 {
            result.error(format!("{path} has more than 20 action dependencies."));
        }
        for dependency in dependencies {
            if let Some(&target) = names.get(&dependency.action_name.to_lowercase()) {
                if index < pre.len() && target >= pre.len() {
                    result.error(format!(
                        "{path}: a pre-action cannot depend on a post-action."
                    ));
                }
                edges.insert((index, target));
            } else {
                result.error(format!(
                    "{path}.dependsOn references an unknown action in this group."
                ));
            }
        }
        let Some(job) = &action.job else {
            result.warning(format!(
                "{path}.job is missing: this draft action cannot execute yet."
            ));
            continue;
        };
        if job.kind != "Execute" {
            result.warning(format!(
                "{path}.job.type must be Execute (case-sensitive) before this action can run."
            ));
        }
        let id = job
            .logical_id
            .as_deref()
            .and_then(|value| result.id(value, &format!("{path}.job.logicalId"), catalog));
        if job.logical_id.is_none() {
            result.warning(format!(
                "{path}.job.logicalId is missing: this draft action cannot execute yet."
            ));
        }
        if let Some(id) = id {
            if let Some(&target) = group_items.get(&id) {
                if target == group_index {
                    if index < pre.len() {
                        result.error(format!(
                            "{path}: a pre-action cannot run the item its own group deploys."
                        ));
                    }
                } else {
                    group_edges.insert((group_index, target));
                }
            }
            if let Some(item_type) = catalog.and_then(|items| items.get(&id)) {
                if ![
                    "Notebook",
                    "DataPipeline",
                    "Dataflow",
                    "CopyJob",
                    "UserDataFunction",
                ]
                .iter()
                .any(|allowed| item_type.eq_ignore_ascii_case(allowed))
                {
                    result.warning(format!(
                        "{path} targets an item type that cannot run as a deployment action."
                    ));
                }
                if item_type.eq_ignore_ascii_case("Dataflow") && !dataflows.insert(id) {
                    result.warning(format!(
                        "{path} repeats a Dataflow Gen2 item in the plan's actions."
                    ));
                }
            }
        }
        validate_parameters(job.parameters.as_deref().unwrap_or_default(), &path, result);
    }
    if has_cycle(actions.len(), &edges) {
        result.error(format!(
            "groups[{group_index}] contains an action dependency cycle or self-dependency."
        ));
    }
}

fn index_actions(
    actions: &[&Action],
    group_index: usize,
    result: &mut Validation,
) -> HashMap<String, usize> {
    let mut names = HashMap::new();
    for (index, action) in actions.iter().enumerate() {
        if !valid_name(&action.name) {
            result.error(format!("groups[{group_index}].actions[{index}].name is invalid (1-60 characters, no surrounding whitespace/control characters)."));
        }
        if names.insert(action.name.to_lowercase(), index).is_some() {
            result.error(format!(
                "groups[{group_index}] repeats an action name (case-insensitive)."
            ));
        }
    }
    names
}

fn validate_parameters(parameters: &[Parameter], path: &str, result: &mut Validation) {
    if parameters.len() > 20 {
        result.error(format!("{path}.job supports at most 20 parameters."));
    }
    let mut names = HashSet::new();
    for parameter in parameters {
        let mut chars = parameter.name.chars();
        if !chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            || parameter.name.len() > 255
            || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        {
            result.error(format!("{path}.job has an invalid parameter name."));
        }
        if !names.insert(&parameter.name) {
            result.error(format!("{path}.job repeats a parameter name."));
        }
        if parameter.value.is_object() {
            result.warning(format!("{path}.job: object/variable-reference parameter values require service-side resolution; offline validation does not check active value sets or permissions."));
        }
    }
}

fn has_cycle(nodes: usize, edges: &HashSet<(usize, usize)>) -> bool {
    let mut indegree = vec![0; nodes];
    let mut dependents = vec![Vec::new(); nodes];
    for &(node, dependency) in edges {
        indegree[node] += 1;
        dependents[dependency].push(node);
    }
    let mut ready: VecDeque<usize> = indegree
        .iter()
        .enumerate()
        .filter_map(|(i, count)| (*count == 0).then_some(i))
        .collect();
    let mut visited = 0;
    while let Some(node) = ready.pop_front() {
        visited += 1;
        for &dependent in &dependents[node] {
            indegree[dependent] -= 1;
            if indegree[dependent] == 0 {
                ready.push_back(dependent);
            }
        }
    }
    visited != nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const A: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const B: &str = "bbbbbbbb-1111-2222-3333-444444444444";
    const C: &str = "cccccccc-1111-2222-3333-444444444444";

    fn plan(groups: Value) -> Value {
        let mut value = json!({"$schema": SCHEMA, "version": "1.0.0"});
        value["groups"] = groups;
        value
    }

    fn check(plan: &Value, source: Option<&Catalog>) -> Validation {
        validate(plan.to_string().as_bytes(), source)
    }

    fn action(name: &str, id: &str) -> Value {
        json!({"name": name, "job": {"type": "Execute", "logicalId": id}})
    }

    #[test]
    fn minimal_and_null_groups_are_valid_but_obsolete_and_unsupported_formats_are_not() {
        for groups in [Value::Null, json!([])] {
            let result = check(&plan(groups), None);
            assert!(result.valid, "{:?}", result.errors);
            assert_eq!(result.actions, 0);
            assert!(!result.executes_jobs);
            assert!(!result.source_checked);
        }
        for content in [
            "executableFormat: DeploymentPlanV1\nid: old",
            "{}",
            "[invalid",
            "",
        ] {
            assert!(!validate(content.as_bytes(), None).valid);
        }
        for (field, value) in [
            ("$schema", "https://example.com/schema.json"),
            ("version", "2.0.0"),
        ] {
            let mut value_plan = plan(json!([]));
            value_plan[field] = Value::from(value);
            assert!(!check(&value_plan, None).valid);
        }
    }

    #[test]
    fn ordered_actions_and_groups_validate_with_a_source_catalog() {
        let value = plan(json!([
            {"name": "prepare", "logicalId": A, "postActions": [action("seed", A)]},
            {"name": "consume", "logicalId": B, "dependsOn": [{"groupName": "PREPARE"}], "postActions": [action("verify", B)]}
        ]));
        let source = catalog([(A, "Notebook"), (B, "DataPipeline")]);
        let result = check(&value, Some(&source));
        assert!(result.valid, "{:?}", result.errors);
        assert_eq!(result.groups, 2);
        assert_eq!(result.actions, 2);
        assert!(result.executes_jobs);
        assert!(result.source_checked);
        assert_eq!(result.warnings, Vec::<String>::new());
        assert_eq!(
            result.logical_ids,
            BTreeSet::from([A.to_string(), B.to_string()])
        );
    }

    #[test]
    fn group_dependencies_reject_missing_self_duplicate_and_cyclic_references() {
        for groups in [
            json!([{"name": "one", "logicalId": A, "dependsOn": [{"groupName": "missing"}]}]),
            json!([{"name": "one", "logicalId": A, "dependsOn": [{"groupName": "one"}]}]),
            json!([{"name": "one", "logicalId": A}, {"name": "ONE", "logicalId": B}]),
            json!([{"name": "one", "logicalId": A}, {"name": "two", "logicalId": A}]),
            json!([{"name": "one", "logicalId": A, "dependsOn": [{"groupName": "two"}]}, {"name": "two", "logicalId": B, "dependsOn": [{"groupName": "one"}]}]),
            json!([{"name": "one", "logicalId": A, "postActions": [action("run-two", B)]}, {"name": "two", "logicalId": B, "postActions": [action("run-one", A)]}]),
        ] {
            assert!(!check(&plan(groups), None).valid);
        }
    }

    #[test]
    fn actions_reject_missing_dependencies_cycles_duplicates_and_invalid_preconditions() {
        for group in [
            json!({"name": "one", "logicalId": A, "preActions": [action("self", A)]}),
            json!({"name": "one", "logicalId": A, "postActions": [action("same", A), action("SAME", A)]}),
            json!({"name": "one", "logicalId": A, "postActions": [{"name": "self", "job": {"type": "Execute", "logicalId": A}, "dependsOn": [{"actionName": "self"}]}]}),
            json!({"name": "one", "logicalId": A, "preActions": [{"name": "pre", "job": {"type": "Execute", "logicalId": B}, "dependsOn": [{"actionName": "post"}]}], "postActions": [action("post", A)]}),
            json!({"name": "one", "logicalId": A, "postActions": [{"name": "run", "job": {"type": "Execute", "logicalId": A}, "dependsOn": [{"actionName": "missing"}]}]}),
        ] {
            assert!(!check(&plan(json!([group])), None).valid);
        }
        let mut first = action("first", A);
        first["dependsOn"] = json!([{"actionName": "second"}]);
        let mut second = action("second", A);
        second["dependsOn"] = json!([{"actionName": "first"}]);
        assert!(
            !check(
                &plan(json!([{"name": "one", "logicalId": A, "postActions": [first, second]}])),
                None
            )
            .valid
        );
    }

    #[test]
    fn chained_actions_can_depend_on_earlier_pre_actions() {
        let mut post = action("post", A);
        post["dependsOn"] = json!([{"actionName": "PRE"}]);
        let result = check(
            &plan(
                json!([{"name": "one", "logicalId": A, "preActions": [action("pre", B)], "postActions": [post]}]),
            ),
            None,
        );
        assert!(result.valid, "{:?}", result.errors);
    }

    #[test]
    fn action_jobs_require_nonzero_logical_ids_and_execute_type() {
        for invalid in ["", "not-a-uuid", "00000000-0000-0000-0000-000000000000"] {
            assert!(!check(&plan(json!([{"name": "one", "logicalId": invalid}])), None).valid);
            assert!(!check(&plan(json!([{"name": "one", "logicalId": A, "postActions": [action("run", invalid)]}])), None).valid);
        }
        for incomplete in [
            json!({"name": "run"}),
            json!({"name": "run", "job": {"type": "Execute"}}),
            json!({"name": "run", "job": {"type": "Pipeline", "logicalId": A}}),
        ] {
            let result = check(
                &plan(json!([{"name": "one", "logicalId": A, "postActions": [incomplete]}])),
                None,
            );
            assert!(
                result.valid,
                "draft artifact storage is distinct from execution readiness"
            );
            assert_ne!(result.warnings, Vec::<String>::new());
        }
    }

    #[test]
    fn source_availability_and_action_readiness_warn_without_blocking_storage() {
        let value =
            plan(json!([{"name": "one", "logicalId": A, "postActions": [action("run", B)]}]));
        let missing = catalog([(A, "Lakehouse")]);
        let result = check(&value, Some(&missing));
        assert!(result.valid);
        assert_eq!(result.warnings.len(), 1);
        let unsupported = catalog([(A, "Lakehouse"), (B, "SemanticModel")]);
        let result = check(&value, Some(&unsupported));
        assert!(result.valid);
        assert_ne!(result.warnings, Vec::<String>::new());
        let supported = catalog([(A, "Lakehouse"), (B, "UserDataFunction")]);
        assert!(check(&value, Some(&supported)).valid);
    }

    #[test]
    fn dataflow_actions_cannot_be_repeated_across_groups() {
        let value = plan(json!([
            {"name": "one", "logicalId": A, "postActions": [action("first", B)]},
            {"name": "two", "logicalId": C, "postActions": [action("second", B)]}
        ]));
        let result = check(
            &value,
            Some(&catalog([
                (A, "Notebook"),
                (B, "Dataflow"),
                (C, "Notebook"),
            ])),
        );
        assert!(result.valid);
        assert_ne!(result.warnings, Vec::<String>::new());
        assert!(
            check(
                &value,
                Some(&catalog([(A, "Notebook"), (B, "CopyJob"), (C, "Notebook")]))
            )
            .valid
        );
    }

    #[test]
    fn parameters_and_options_validate_limits_names_and_duplicates() {
        let mut run = action("run", A);
        run["job"]["parameters"] = json!([{"name": "Environment", "value": "test"}, {"name": "Environment", "value": "prod"}]);
        assert!(
            !check(
                &plan(json!([{"name": "one", "logicalId": A, "postActions": [run]}])),
                None
            )
            .valid
        );
        for name in ["", "2bad", "has space", "bad.name"] {
            let mut run = action("run", A);
            run["job"]["parameters"] = json!([{"name": name, "value": 1}]);
            assert!(
                !check(
                    &plan(json!([{"name": "one", "logicalId": A, "postActions": [run]}])),
                    None
                )
                .valid
            );
        }
        for options in [
            json!([{}]),
            json!([{"A": "x", "B": "y"}]),
            json!([{"bad_key": "x"}]),
            json!([{"Valid": "line\nbreak"}]),
        ] {
            assert!(
                !check(
                    &plan(json!([{"name": "one", "logicalId": A, "options": options}])),
                    None
                )
                .valid
            );
        }
        assert!(check(&plan(json!([{"name": "one", "logicalId": A, "options": [{"ActiveValueSet": "Default"}]}])), None).valid);
    }

    #[test]
    fn limits_and_findings_are_bounded_and_invalid_utf8_is_rejected() {
        assert!(!validate(&vec![b' '; MAX_BYTES + 1], None).valid);
        assert!(!validate(&[0xff], None).valid);
        let groups: Vec<Value> = (0..1001)
            .map(|id| json!({"name": format!("group{id}"), "logicalId": A}))
            .collect();
        assert!(!check(&plan(json!(groups)), None).valid);
        let groups: Vec<Value> = (0..200)
            .map(|_| json!({"name": "", "logicalId": "invalid"}))
            .collect();
        let result = check(&plan(json!(groups)), None);
        assert_eq!(result.errors.len(), MAX_FINDINGS);
        assert!(result.truncated);
        for name in [
            " leading".to_string(),
            "trailing ".to_string(),
            "x".repeat(61),
        ] {
            assert!(!check(&plan(json!([{"name": name, "logicalId": A}])), None).valid);
        }
    }

    #[test]
    fn identity_parts_and_plan_references_are_not_runtime_bindings() {
        assert!(preserves_logical_ids("Notebook", ".platform"));
        assert!(preserves_logical_ids("deploymentplan", "plan.yml"));
        assert!(!preserves_logical_ids("Notebook", "notebook-content.py"));
        assert!(!preserves_logical_ids("Plan", "plan.yml"));
        let source = catalog([
            (A, "Notebook"),
            ("not-a-uuid", "Report"),
            ("00000000-0000-0000-0000-000000000000", "Report"),
        ]);
        assert_eq!(source.len(), 1);
    }
}
