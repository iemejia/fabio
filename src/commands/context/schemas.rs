//! Item definition schemas for AI agents.

use serde_json::{Value, json};

use crate::cli::Cli;
use crate::output;

use super::find_entry;

pub(super) fn execute(cli: &Cli, item_type: &str) {
    let normalized = item_type.to_lowercase().replace(['-', '_'], "");
    if let Some(content) = find_entry(ITEM_SCHEMAS, &normalized) {
        let mut val: Value =
            serde_json::from_str(content).unwrap_or_else(|_| json!({"content": content}));
        merge_definition_requirements(&mut val, item_type);
        output::render_object(cli, &val, "type");
    } else if let Some(spec_type) = crate::definition_spec::canonical_type_name(item_type) {
        // No hand-authored schema, but we have authoritative part requirements.
        let mut val = json!({
            "type": spec_type,
            "note": "No detailed authoring schema yet — showing canonical definition part \
                     requirements. Fetch a real template with: fabio item get-definition \
                     --workspace <WS> --id <ID> (or the type-specific get-definition)."
        });
        merge_definition_requirements(&mut val, item_type);
        output::render_object(cli, &val, "type");
    } else {
        let mut available: Vec<&str> = ITEM_SCHEMAS.iter().map(|(name, _)| *name).collect();
        for t in crate::definition_spec::known_types() {
            if !available.contains(&t) {
                available.push(t);
            }
        }
        available.sort_unstable();
        let result = json!({
            "error": format!("No schema found for item type '{item_type}'"),
            "available_types": available,
            "hint": "Use 'fabio context list' to see all available item types"
        });
        output::render_object(cli, &result, "error");
    }
}

/// Merge the authoritative, live-verified definition part requirements from
/// `definition_spec` into a schema object under `definition_requirements`. This
/// is the canonical source of truth (part paths, `definitionFormat`, aliases),
/// so it never drifts from what Fabric's `getDefinition` returns.
fn merge_definition_requirements(val: &mut Value, item_type: &str) {
    let Some(spec) = crate::definition_spec::spec_for(item_type) else {
        return;
    };
    let mut req = serde_json::Map::new();
    if !spec.required_parts.is_empty() {
        req.insert("requiredParts".into(), json!(spec.required_parts));
    }
    if !spec.required_one_of.is_empty() {
        req.insert("requiredOneOf".into(), json!(spec.required_one_of));
    }
    if !spec.optional_parts.is_empty() {
        req.insert("optionalParts".into(), json!(spec.optional_parts));
    }
    if !spec.alias_parts.is_empty() {
        req.insert("aliasParts".into(), json!(spec.alias_parts));
    }
    if let Some(fmt) = &spec.format {
        req.insert("definitionFormat".into(), json!(fmt));
    }
    if let Some(note) = &spec.note {
        req.insert("note".into(), json!(note));
    }
    req.insert(
        "validateOffline".into(),
        json!(format!(
            "fabio item validate-definition --type {} --dir <folder>",
            crate::definition_spec::canonical_type_name(item_type).unwrap_or(item_type)
        )),
    );
    if let Some(obj) = val.as_object_mut() {
        obj.insert("definition_requirements".into(), Value::Object(req));
    }
}

pub(super) fn list_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = ITEM_SCHEMAS.iter().map(|(name, _)| *name).collect();
    // Include types that have authoritative definition-part requirements even if
    // they lack a hand-authored schema file (served spec-backed by `context schema`).
    for t in crate::definition_spec::known_types() {
        if !names.contains(&t) {
            names.push(t);
        }
    }
    names.sort_unstable();
    names
}

/// Item-definition schema entries, for keyword search via `context find`.
pub(super) const fn entries() -> &'static [(&'static str, &'static str)] {
    ITEM_SCHEMAS
}

const ITEM_SCHEMAS: &[(&str, &str)] = &[
    ("Notebook", include_str!("data/schemas/notebook.json")),
    (
        "DataPipeline",
        include_str!("data/schemas/data_pipeline.json"),
    ),
    (
        "SemanticModel",
        include_str!("data/schemas/semantic_model.json"),
    ),
    ("Lakehouse", include_str!("data/schemas/lakehouse.json")),
    (
        "KQLDatabase",
        include_str!("data/schemas/kql_database.json"),
    ),
    ("Eventhouse", include_str!("data/schemas/eventhouse.json")),
    ("Eventstream", include_str!("data/schemas/eventstream.json")),
    ("Environment", include_str!("data/schemas/environment.json")),
    ("Warehouse", include_str!("data/schemas/warehouse.json")),
    ("Report", include_str!("data/schemas/report.json")),
    ("DataAgent", include_str!("data/schemas/data_agent.json")),
    (
        "SparkJobDefinition",
        include_str!("data/schemas/spark_job_definition.json"),
    ),
    ("GraphQLApi", include_str!("data/schemas/graphql_api.json")),
    ("CopyJob", include_str!("data/schemas/copy_job.json")),
    ("Dataflow", include_str!("data/schemas/dataflow.json")),
    (
        "MirroredDatabase",
        include_str!("data/schemas/mirrored_database.json"),
    ),
    ("Reflex", include_str!("data/schemas/reflex.json")),
    ("MLModel", include_str!("data/schemas/ml_model.json")),
    (
        "MLExperiment",
        include_str!("data/schemas/ml_experiment.json"),
    ),
    ("Ontology", include_str!("data/schemas/ontology.json")),
    (
        "SQLDatabase",
        include_str!("data/schemas/sql_database.json"),
    ),
    ("Connection", include_str!("data/schemas/connection.json")),
    (
        "VariableLibrary",
        include_str!("data/schemas/variable_library.json"),
    ),
    (
        "DigitalTwinBuilder",
        include_str!("data/schemas/digital_twin_builder.json"),
    ),
    (
        "DigitalTwinBuilderFlow",
        include_str!("data/schemas/digital_twin_builder_flow.json"),
    ),
];
