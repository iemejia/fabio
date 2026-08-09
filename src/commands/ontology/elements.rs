//! Granular ontology definition editing — add/delete entity types, relationship
//! types, and report (resource) links on an existing ontology.
//!
//! The Fabric ontology REST API is definition-based (CRUD + `getDefinition`/
//! `updateDefinition`); there are no per-element endpoints. These commands
//! therefore implement client-side **read-modify-write**: fetch the current
//! definition parts, add/remove/edit exactly one element, and push the whole
//! set back via `updateDefinition`. Part shapes match `import`/`generate`
//! (`EntityTypes/{id}/definition.json`, `RelationshipTypes/{id}/definition.json`,
//! `EntityTypes/{id}/ResourceLinks/definition.json`).

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

const ENTITY_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/ontology/entityType/1.0.0/schema.json";
const REL_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/ontology/relationshipType/1.0.0/schema.json";

/// Canonical `valueType` values accepted by the ontology entity-type schema.
///
/// These are the ONLY values the Fabric `entityType/1.0.0` schema's
/// `EntityTypeProperty.valueType` enum accepts (verified against the published
/// schema AND live): `String`, `Boolean`, `DateTime`, `Object`, `BigInt`,
/// `Double`. NOTE `Long`/`Decimal`/`Any` are NOT valid typed-property types —
/// the API rejects them with a generic `ALMOperationImportFailed`, so common
/// integer/decimal/any inputs are normalized to `BigInt`/`Double`/`Object`.
/// (`Any` is valid ONLY inside `untypedProperties`, which this path never
/// writes.)
const VALUE_TYPES: &[&str] = &[
    "String", "BigInt", "Double", "Boolean", "DateTime", "Object",
];

// ─── Pure helpers (unit-tested) ──────────────────────────────────────────────

/// Normalize a user-supplied property type to a canonical `valueType`.
///
/// Accepts the canonical `PascalCase` values and common lowercase aliases,
/// mapping every reasonable input to a schema-valid `valueType`:
/// `string`/`text` → `String`; `long`/`int`/`integer`/`bigint` → `BigInt`;
/// `double`/`float`/`number`/`decimal`/`currency` → `Double`;
/// `bool`/`boolean` → `Boolean`; `date`/`datetime`/`timestamp` → `DateTime`;
/// `object`/`any` → `Object`. Errors (with the valid set) on anything else so a
/// typo never silently produces a `String` column.
fn normalize_value_type(input: &str) -> Result<String> {
    let v = input.trim();
    let canon = match v.to_ascii_lowercase().as_str() {
        "string" | "text" | "str" => "String",
        "long" | "int" | "integer" | "bigint" => "BigInt",
        "double" | "float" | "number" | "decimal" | "currency" => "Double",
        "boolean" | "bool" => "Boolean",
        "datetime" | "date" | "timestamp" => "DateTime",
        "object" | "any" => "Object",
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Unknown property type '{input}'"),
                format!("Valid types: {}", VALUE_TYPES.join(", ")),
            )
            .into());
        }
    };
    Ok(canon.to_string())
}

/// Parse a `name:type` property spec into `(name, valueType)`.
fn parse_property_spec(spec: &str) -> Result<(String, String)> {
    let (name, ty) = spec.split_once(':').ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid property spec '{spec}'"),
            "Use name:type, e.g. --property \"ProductId:String\" --property \"Price:Double\"",
        )
    })?;
    let name = name.trim();
    if name.is_empty() {
        return Err(
            FabioError::invalid_input(format!("Property name is empty in '{spec}'")).into(),
        );
    }
    Ok((name.to_string(), normalize_value_type(ty)?))
}

/// Collect every id already used in the definition (entity/relationship type
/// ids and property ids) so newly-generated ids never collide.
fn collect_ids(parts: &[Value]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for part in parts {
        let Some(obj) = part_json(part) else { continue };
        if let Some(id) = obj.get("id").and_then(Value::as_str) {
            ids.insert(id.to_string());
        }
        if let Some(props) = obj.get("properties").and_then(Value::as_array) {
            for p in props {
                if let Some(pid) = p.get("id").and_then(Value::as_str) {
                    ids.insert(pid.to_string());
                }
            }
        }
    }
    ids
}

/// Yield a numeric id string not already present in `existing`, starting from
/// `seed` and incrementing. Deterministic given the inputs (testable).
fn gen_unique_id(existing: &HashSet<String>, seed: u64) -> String {
    let mut n = seed;
    loop {
        let s = n.to_string();
        if !existing.contains(&s) {
            return s;
        }
        n = n.wrapping_add(1);
    }
}

fn now_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_nanos()).ok())
        .unwrap_or(1_000_000_000_000)
}

/// Build a single entity-type `properties[]` entry.
fn build_property(id: &str, name: &str, value_type: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "redefines": Value::Null,
        "baseTypeNamespaceType": Value::Null,
        "valueType": value_type,
    })
}

/// Build an `EntityTypes/{id}/definition.json` payload.
fn build_entity_type_def(id: &str, name: &str, properties: &[Value], key_ids: &[String]) -> Value {
    json!({
        "$schema": ENTITY_SCHEMA,
        "id": id,
        "namespace": "usertypes",
        "baseEntityTypeId": Value::Null,
        "name": name,
        "namespaceType": "Custom",
        "visibility": "Visible",
        "displayNamePropertyId": Value::Null,
        "entityIdParts": key_ids,
        "properties": properties,
        "timeseriesProperties": [],
        "untypedProperties": [],
    })
}

/// Build a `RelationshipTypes/{id}/definition.json` payload.
fn build_relationship_type_def(id: &str, name: &str, source_id: &str, target_id: &str) -> Value {
    json!({
        "$schema": REL_SCHEMA,
        "id": id,
        "namespace": "usertypes",
        "name": name,
        "namespaceType": "Custom",
        "source": { "entityTypeId": source_id },
        "target": { "entityTypeId": target_id },
    })
}

/// Build one `resourceLinks[]` entry (a Power BI report link).
fn build_report_link(workspace_id: &str, report_id: &str) -> Value {
    json!({
        "type": "PowerBIReport",
        "workspaceId": workspace_id,
        "itemId": report_id,
    })
}

/// Decode a definition part's base64 `payload` into JSON.
fn part_json(part: &Value) -> Option<Value> {
    let payload = part.get("payload").and_then(Value::as_str)?;
    let bytes = BASE64.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Build a definition part object from a path + JSON payload.
fn encode_part(path: &str, payload: &Value) -> Value {
    let s = serde_json::to_string(payload).unwrap_or_default();
    json!({
        "path": path,
        "payload": BASE64.encode(s.as_bytes()),
        "payloadType": "InlineBase64",
    })
}

/// The `EntityTypes/<id>/definition.json` path (used to identify entity parts).
fn entity_def_path(id: &str) -> String {
    format!("EntityTypes/{id}/definition.json")
}

/// Resolve an entity type by id or (case-insensitive) name → its id.
fn resolve_entity_id(parts: &[Value], name_or_id: &str) -> Option<String> {
    for part in parts {
        let path = part.get("path").and_then(Value::as_str).unwrap_or("");
        if !path.starts_with("EntityTypes/") || !path.ends_with("/definition.json") {
            continue;
        }
        let Some(obj) = part_json(part) else { continue };
        let id = obj.get("id").and_then(Value::as_str).unwrap_or("");
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
        if id == name_or_id || name.eq_ignore_ascii_case(name_or_id) {
            return Some(id.to_string());
        }
    }
    None
}

/// Resolve a relationship type by id or (case-insensitive) name → its id.
fn resolve_relationship_id(parts: &[Value], name_or_id: &str) -> Option<String> {
    for part in parts {
        let path = part.get("path").and_then(Value::as_str).unwrap_or("");
        if !path.starts_with("RelationshipTypes/") || !path.ends_with("/definition.json") {
            continue;
        }
        let Some(obj) = part_json(part) else { continue };
        let id = obj.get("id").and_then(Value::as_str).unwrap_or("");
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
        if id == name_or_id || name.eq_ignore_ascii_case(name_or_id) {
            return Some(id.to_string());
        }
    }
    None
}

/// The list of entity-type names present (for error hints).
fn entity_type_names(parts: &[Value]) -> Vec<String> {
    parts
        .iter()
        .filter_map(|p| {
            let path = p.get("path").and_then(Value::as_str)?;
            if path.starts_with("EntityTypes/") && path.ends_with("/definition.json") {
                part_json(p)?
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                None
            }
        })
        .collect()
}

// ─── Definition read-modify-write plumbing ───────────────────────────────────

/// Fetch the ontology's definition parts (each `{path, payload, payloadType}`).
async fn fetch_parts(client: &FabricClient, workspace: &str, id: &str) -> Result<Vec<Value>> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/ontologies/{id}/getDefinition"),
            &json!({}),
            true,
        )
        .await?;
    Ok(data
        .get("definition")
        .and_then(|d| d.get("parts"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// Push a full set of parts back via `updateDefinition` (LRO).
async fn push_parts(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    parts: Vec<Value>,
) -> Result<Value> {
    client
        .post(
            &format!("/workspaces/{workspace}/ontologies/{id}/updateDefinition"),
            &json!({ "definition": { "parts": parts } }),
            true,
        )
        .await
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// Add an entity type (with typed properties and optional key properties).
pub(super) async fn add_entity_type(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    properties: &[String],
    keys: &[String],
) -> Result<()> {
    let parsed: Vec<(String, String)> = properties
        .iter()
        .map(|s| parse_property_spec(s))
        .collect::<Result<_>>()?;

    let mut parts = fetch_parts(client, workspace, id).await?;

    if resolve_entity_id(&parts, name).is_some() {
        return Err(FabioError::invalid_input(format!(
            "Entity type '{name}' already exists in this ontology"
        ))
        .into());
    }

    let mut ids = collect_ids(&parts);
    let mut seed = now_seed();
    let entity_id = gen_unique_id(&ids, seed);
    ids.insert(entity_id.clone());
    seed = seed.wrapping_add(1);

    let mut prop_values = Vec::with_capacity(parsed.len());
    let mut key_ids = Vec::new();
    for (pname, ptype) in &parsed {
        let pid = gen_unique_id(&ids, seed);
        ids.insert(pid.clone());
        seed = seed.wrapping_add(1);
        if keys.iter().any(|k| k.eq_ignore_ascii_case(pname)) {
            key_ids.push(pid.clone());
        }
        prop_values.push(build_property(&pid, pname, ptype));
    }

    // Validate every --key names a real property.
    for k in keys {
        if !parsed.iter().any(|(pn, _)| pn.eq_ignore_ascii_case(k)) {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("--key '{k}' does not match any --property name"),
                "Keys must reference a property you also declared with --property name:type.",
            )
            .into());
        }
    }

    let def = build_entity_type_def(&entity_id, name, &prop_values, &key_ids);

    if output::dry_run_guard(
        cli,
        "ontology add-entity-type",
        &json!({ "ontology": id, "entityType": name, "entityTypeId": entity_id, "properties": parsed.len(), "keys": key_ids.len() }),
    ) {
        return Ok(());
    }

    parts.push(encode_part(&entity_def_path(&entity_id), &def));
    push_parts(client, workspace, id, parts).await?;

    output::render_object(
        cli,
        &json!({
            "status": "entity_type_added",
            "entityType": name,
            "entityTypeId": entity_id,
            "properties": parsed.len(),
            "keys": key_ids,
        }),
        "status",
    );
    Ok(())
}

/// Delete an entity type (and any relationship types referencing it).
pub(super) async fn delete_entity_type(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    entity: &str,
) -> Result<()> {
    let parts = fetch_parts(client, workspace, id).await?;
    let entity_id = resolve_entity_id(&parts, entity).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Entity type '{entity}' not found in ontology '{id}'"),
            format!(
                "Existing entity types: {}",
                entity_type_names(&parts).join(", ")
            ),
        )
    })?;

    // Relationship types that reference the entity would dangle → remove them.
    let removed_rels: Vec<String> = parts
        .iter()
        .filter_map(|p| {
            let path = p.get("path").and_then(Value::as_str)?;
            if !path.starts_with("RelationshipTypes/") || !path.ends_with("/definition.json") {
                return None;
            }
            let obj = part_json(p)?;
            let src = obj.pointer("/source/entityTypeId").and_then(Value::as_str);
            let tgt = obj.pointer("/target/entityTypeId").and_then(Value::as_str);
            if src == Some(entity_id.as_str()) || tgt == Some(entity_id.as_str()) {
                obj.get("name").and_then(Value::as_str).map(str::to_string)
            } else {
                None
            }
        })
        .collect();

    if output::dry_run_guard(
        cli,
        "ontology delete-entity-type",
        &json!({ "ontology": id, "entityType": entity, "entityTypeId": entity_id, "cascadedRelationshipTypes": removed_rels }),
    ) {
        return Ok(());
    }

    let entity_prefix = format!("EntityTypes/{entity_id}/");
    let rel_ids: HashSet<String> = parts
        .iter()
        .filter_map(|p| {
            let path = p.get("path").and_then(Value::as_str)?;
            if !path.starts_with("RelationshipTypes/") || !path.ends_with("/definition.json") {
                return None;
            }
            let obj = part_json(p)?;
            let src = obj.pointer("/source/entityTypeId").and_then(Value::as_str);
            let tgt = obj.pointer("/target/entityTypeId").and_then(Value::as_str);
            (src == Some(entity_id.as_str()) || tgt == Some(entity_id.as_str()))
                .then(|| obj.get("id").and_then(Value::as_str).map(str::to_string))
                .flatten()
        })
        .collect();

    let kept: Vec<Value> = parts
        .into_iter()
        .filter(|p| {
            let path = p.get("path").and_then(Value::as_str).unwrap_or("");
            if path.starts_with(&entity_prefix) {
                return false;
            }
            !rel_ids
                .iter()
                .any(|rid| path.starts_with(&format!("RelationshipTypes/{rid}/")))
        })
        .collect();

    push_parts(client, workspace, id, kept).await?;

    output::render_object(
        cli,
        &json!({
            "status": "entity_type_deleted",
            "entityType": entity,
            "entityTypeId": entity_id,
            "cascadedRelationshipTypes": removed_rels,
        }),
        "status",
    );
    Ok(())
}

/// Rename an entity type's `name` in the definition parts.
///
/// Relationship types and data bindings reference an entity by its stable
/// `entityTypeId`, NOT its name, so a rename only rewrites the entity's own
/// `EntityTypes/{id}/definition.json` `name` field. Returns the updated parts
/// (or `None` if the entity id was not found).
fn rename_entity_in_parts(parts: &[Value], entity_id: &str, new_name: &str) -> Option<Vec<Value>> {
    let target = entity_def_path(entity_id);
    let mut found = false;
    let out: Vec<Value> = parts
        .iter()
        .map(|p| {
            let path = p.get("path").and_then(Value::as_str).unwrap_or("");
            if path == target
                && let Some(mut obj) = part_json(p)
            {
                obj["name"] = Value::from(new_name);
                found = true;
                return encode_part(path, &obj);
            }
            p.clone()
        })
        .collect();
    found.then_some(out)
}

/// Rename an entity type (updates its `name`; relationship/binding references
/// use the stable id, so they are unaffected).
pub(super) async fn rename_entity_type(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    entity: &str,
    new_name: &str,
) -> Result<()> {
    let parts = fetch_parts(client, workspace, id).await?;
    let entity_id = resolve_entity_id(&parts, entity).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Entity type '{entity}' not found in ontology '{id}'"),
            format!(
                "Existing entity types: {}",
                entity_type_names(&parts).join(", ")
            ),
        )
    })?;

    // A different entity already using the new name → conflict.
    if let Some(existing) = resolve_entity_id(&parts, new_name)
        && existing != entity_id
    {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("Entity type '{new_name}' already exists in this ontology"),
            "Choose a different --new-name.",
        )
        .into());
    }

    if output::dry_run_guard(
        cli,
        "ontology rename-entity-type",
        &json!({ "ontology": id, "entityType": entity, "entityTypeId": entity_id, "newName": new_name }),
    ) {
        return Ok(());
    }

    let updated = rename_entity_in_parts(&parts, &entity_id, new_name).ok_or_else(|| {
        FabioError::invalid_input("Failed to locate the entity type definition part")
    })?;
    push_parts(client, workspace, id, updated).await?;

    output::render_object(
        cli,
        &json!({
            "status": "entity_type_renamed",
            "entityType": entity,
            "newName": new_name,
            "entityTypeId": entity_id,
        }),
        "status",
    );
    Ok(())
}

/// Add a relationship type between two entity types.
#[allow(clippy::too_many_arguments)]
pub(super) async fn add_relationship_type(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    source: &str,
    target: &str,
) -> Result<()> {
    let parts = fetch_parts(client, workspace, id).await?;

    let source_id = resolve_entity_id(&parts, source).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Source entity type '{source}' not found"),
            format!(
                "Existing entity types: {}",
                entity_type_names(&parts).join(", ")
            ),
        )
    })?;
    let target_id = resolve_entity_id(&parts, target).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Target entity type '{target}' not found"),
            format!(
                "Existing entity types: {}",
                entity_type_names(&parts).join(", ")
            ),
        )
    })?;

    if resolve_relationship_id(&parts, name).is_some() {
        return Err(FabioError::invalid_input(format!(
            "Relationship type '{name}' already exists in this ontology"
        ))
        .into());
    }

    let ids = collect_ids(&parts);
    let rel_id = gen_unique_id(&ids, now_seed());
    let def = build_relationship_type_def(&rel_id, name, &source_id, &target_id);

    if output::dry_run_guard(
        cli,
        "ontology add-relationship-type",
        &json!({ "ontology": id, "relationshipType": name, "relationshipTypeId": rel_id, "source": source_id, "target": target_id }),
    ) {
        return Ok(());
    }

    let mut parts = parts;
    parts.push(encode_part(
        &format!("RelationshipTypes/{rel_id}/definition.json"),
        &def,
    ));
    push_parts(client, workspace, id, parts).await?;

    output::render_object(
        cli,
        &json!({
            "status": "relationship_type_added",
            "relationshipType": name,
            "relationshipTypeId": rel_id,
            "sourceEntityTypeId": source_id,
            "targetEntityTypeId": target_id,
        }),
        "status",
    );
    Ok(())
}

/// Delete a relationship type.
pub(super) async fn delete_relationship_type(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    relationship: &str,
) -> Result<()> {
    let parts = fetch_parts(client, workspace, id).await?;
    let rel_id = resolve_relationship_id(&parts, relationship).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Relationship type '{relationship}' not found in ontology '{id}'"),
            "List the definition: fabio ontology get-definition -w <ws> --id <id> --decode",
        )
    })?;

    if output::dry_run_guard(
        cli,
        "ontology delete-relationship-type",
        &json!({ "ontology": id, "relationshipType": relationship, "relationshipTypeId": rel_id }),
    ) {
        return Ok(());
    }

    let prefix = format!("RelationshipTypes/{rel_id}/");
    let kept: Vec<Value> = parts
        .into_iter()
        .filter(|p| {
            !p.get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .starts_with(&prefix)
        })
        .collect();

    push_parts(client, workspace, id, kept).await?;

    output::render_object(
        cli,
        &json!({
            "status": "relationship_type_deleted",
            "relationshipType": relationship,
            "relationshipTypeId": rel_id,
        }),
        "status",
    );
    Ok(())
}

/// Add a Power BI report link (`ResourceLinks`) to an entity type.
pub(super) async fn add_report_link(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    entity: &str,
    report_workspace: &str,
    report_id: &str,
) -> Result<()> {
    let mut parts = fetch_parts(client, workspace, id).await?;
    let entity_id = resolve_entity_id(&parts, entity).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Entity type '{entity}' not found in ontology '{id}'"),
            format!(
                "Existing entity types: {}",
                entity_type_names(&parts).join(", ")
            ),
        )
    })?;

    if output::dry_run_guard(
        cli,
        "ontology add-report-link",
        &json!({ "ontology": id, "entityType": entity, "reportWorkspaceId": report_workspace, "reportId": report_id }),
    ) {
        return Ok(());
    }

    let link_path = format!("EntityTypes/{entity_id}/ResourceLinks/definition.json");
    let link = build_report_link(report_workspace, report_id);

    // Merge into the existing ResourceLinks part, or create it.
    if let Some(part) = parts
        .iter_mut()
        .find(|p| p.get("path").and_then(Value::as_str) == Some(link_path.as_str()))
    {
        let mut obj = part_json(part).unwrap_or_else(|| json!({ "resourceLinks": [] }));
        let links = obj
            .get_mut("resourceLinks")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| FabioError::invalid_input("Malformed ResourceLinks part"))?;
        if links
            .iter()
            .any(|l| l.get("itemId").and_then(Value::as_str) == Some(report_id))
        {
            return Err(FabioError::invalid_input(format!(
                "Report '{report_id}' is already linked to entity type '{entity}'"
            ))
            .into());
        }
        links.push(link);
        *part = encode_part(&link_path, &obj);
    } else {
        let obj = json!({ "resourceLinks": [link] });
        parts.push(encode_part(&link_path, &obj));
    }

    push_parts(client, workspace, id, parts).await?;

    output::render_object(
        cli,
        &json!({
            "status": "report_link_added",
            "entityType": entity,
            "entityTypeId": entity_id,
            "reportWorkspaceId": report_workspace,
            "reportId": report_id,
        }),
        "status",
    );
    Ok(())
}

/// List report (resource) links, optionally scoped to one entity type.
pub(super) async fn list_report_links(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    entity: Option<&str>,
) -> Result<()> {
    let parts = fetch_parts(client, workspace, id).await?;

    let scope_id = match entity {
        Some(e) => Some(resolve_entity_id(&parts, e).ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                format!("Entity type '{e}' not found in ontology '{id}'"),
                format!(
                    "Existing entity types: {}",
                    entity_type_names(&parts).join(", ")
                ),
            )
        })?),
        None => None,
    };

    // Map entity id → name for readable output.
    let mut out = Vec::new();
    for part in &parts {
        let path = part.get("path").and_then(Value::as_str).unwrap_or("");
        if !path.starts_with("EntityTypes/") || !path.ends_with("/ResourceLinks/definition.json") {
            continue;
        }
        let ent_id = path
            .strip_prefix("EntityTypes/")
            .and_then(|s| s.split('/').next())
            .unwrap_or("");
        if scope_id.as_deref().is_some_and(|s| s != ent_id) {
            continue;
        }
        let ent_name = resolve_entity_name(&parts, ent_id).unwrap_or_else(|| ent_id.to_string());
        if let Some(links) = part_json(part)
            .as_ref()
            .and_then(|o| o.get("resourceLinks"))
            .and_then(Value::as_array)
        {
            for l in links {
                out.push(json!({
                    "entityType": ent_name,
                    "entityTypeId": ent_id,
                    "type": l.get("type").cloned().unwrap_or(Value::Null),
                    "workspaceId": l.get("workspaceId").cloned().unwrap_or(Value::Null),
                    "itemId": l.get("itemId").cloned().unwrap_or(Value::Null),
                }));
            }
        }
    }

    output::render_list_with_token(
        cli,
        &out,
        &["entityType", "type", "workspaceId", "itemId"],
        &["ENTITY TYPE", "TYPE", "WORKSPACE ID", "ITEM ID"],
        "entityType",
        None,
    );
    Ok(())
}

/// Delete a report link from an entity type by report item id.
pub(super) async fn delete_report_link(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    entity: &str,
    report_id: &str,
) -> Result<()> {
    let mut parts = fetch_parts(client, workspace, id).await?;
    let entity_id = resolve_entity_id(&parts, entity).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Entity type '{entity}' not found in ontology '{id}'"),
            format!(
                "Existing entity types: {}",
                entity_type_names(&parts).join(", ")
            ),
        )
    })?;

    if output::dry_run_guard(
        cli,
        "ontology delete-report-link",
        &json!({ "ontology": id, "entityType": entity, "reportId": report_id }),
    ) {
        return Ok(());
    }

    let link_path = format!("EntityTypes/{entity_id}/ResourceLinks/definition.json");
    let part = parts
        .iter_mut()
        .find(|p| p.get("path").and_then(Value::as_str) == Some(link_path.as_str()))
        .ok_or_else(|| {
            FabioError::not_found(format!("Entity type '{entity}' has no report links"))
        })?;
    let mut obj = part_json(part).unwrap_or_else(|| json!({ "resourceLinks": [] }));
    let links = obj
        .get_mut("resourceLinks")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::invalid_input("Malformed ResourceLinks part"))?;
    let before = links.len();
    links.retain(|l| l.get("itemId").and_then(Value::as_str) != Some(report_id));
    if links.len() == before {
        return Err(FabioError::not_found(format!(
            "Report '{report_id}' is not linked to entity type '{entity}'"
        ))
        .into());
    }
    let remaining = links.is_empty();
    let link_path_owned = link_path.clone();
    if remaining {
        // No links left: drop the whole part.
        parts.retain(|p| p.get("path").and_then(Value::as_str) != Some(link_path_owned.as_str()));
    } else {
        *part = encode_part(&link_path, &obj);
    }

    push_parts(client, workspace, id, parts).await?;

    output::render_object(
        cli,
        &json!({
            "status": "report_link_deleted",
            "entityType": entity,
            "entityTypeId": entity_id,
            "reportId": report_id,
        }),
        "status",
    );
    Ok(())
}

/// Resolve an entity id → its display name (for output).
fn resolve_entity_name(parts: &[Value], entity_id: &str) -> Option<String> {
    parts.iter().find_map(|p| {
        let path = p.get("path").and_then(Value::as_str)?;
        if path == entity_def_path(entity_id) {
            part_json(p)?
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_value_type_maps_aliases_and_rejects_unknown() {
        assert_eq!(normalize_value_type("string").unwrap(), "String");
        // Every integer alias -> BigInt (Fabric has no "Long" valueType).
        assert_eq!(normalize_value_type("Int").unwrap(), "BigInt");
        assert_eq!(normalize_value_type("integer").unwrap(), "BigInt");
        assert_eq!(normalize_value_type("Long").unwrap(), "BigInt");
        assert_eq!(normalize_value_type("bigint").unwrap(), "BigInt");
        assert_eq!(normalize_value_type("float").unwrap(), "Double");
        // Decimal is NOT a Fabric ontology valueType -> Double.
        assert_eq!(normalize_value_type("Decimal").unwrap(), "Double");
        assert_eq!(normalize_value_type("BOOL").unwrap(), "Boolean");
        assert_eq!(normalize_value_type("datetime").unwrap(), "DateTime");
        // "any"/"object" -> Object (the only valid catch-all typed value).
        assert_eq!(normalize_value_type("any").unwrap(), "Object");
        assert_eq!(normalize_value_type("Object").unwrap(), "Object");
        // No normalized output may fall outside the schema enum.
        for t in [
            "string", "int", "long", "decimal", "float", "bool", "date", "any", "object",
        ] {
            let out = normalize_value_type(t).unwrap();
            assert!(
                VALUE_TYPES.contains(&out.as_str()),
                "{t} normalized to {out}, not in the schema enum"
            );
        }
        assert!(normalize_value_type("frobnicate").is_err());
    }

    #[test]
    fn parse_property_spec_splits_name_and_type() {
        assert_eq!(
            parse_property_spec("Price:Double").unwrap(),
            ("Price".to_string(), "Double".to_string())
        );
        assert_eq!(
            parse_property_spec("Name:string").unwrap(),
            ("Name".to_string(), "String".to_string())
        );
        assert!(parse_property_spec("NoType").is_err());
        assert!(parse_property_spec(":Double").is_err());
    }

    #[test]
    fn gen_unique_id_avoids_collisions() {
        let mut existing = HashSet::new();
        existing.insert("100".to_string());
        existing.insert("101".to_string());
        assert_eq!(gen_unique_id(&existing, 100), "102");
        assert_eq!(gen_unique_id(&existing, 200), "200");
    }

    #[test]
    fn build_entity_type_def_shape() {
        let props = vec![build_property("p1", "ProductId", "String")];
        let def = build_entity_type_def("e1", "DimProducts", &props, &["p1".to_string()]);
        assert_eq!(def["namespaceType"], "Custom");
        assert_eq!(def["visibility"], "Visible");
        assert_eq!(def["name"], "DimProducts");
        assert_eq!(def["entityIdParts"][0], "p1");
        assert_eq!(def["properties"][0]["valueType"], "String");
        assert_eq!(def["properties"][0]["name"], "ProductId");
        assert!(def["displayNamePropertyId"].is_null());
    }

    #[test]
    fn build_relationship_type_def_shape() {
        let def = build_relationship_type_def("r1", "sells", "e1", "e2");
        assert_eq!(def["source"]["entityTypeId"], "e1");
        assert_eq!(def["target"]["entityTypeId"], "e2");
        assert_eq!(def["namespaceType"], "Custom");
        assert_eq!(def["name"], "sells");
    }

    #[test]
    fn build_report_link_is_powerbireport() {
        let l = build_report_link("ws1", "rep1");
        assert_eq!(l["type"], "PowerBIReport");
        assert_eq!(l["workspaceId"], "ws1");
        assert_eq!(l["itemId"], "rep1");
    }

    #[test]
    fn resolve_entity_id_by_name_or_id() {
        let def = build_entity_type_def("888", "DimStore", &[], &[]);
        let parts = vec![encode_part(&entity_def_path("888"), &def)];
        assert_eq!(
            resolve_entity_id(&parts, "DimStore").as_deref(),
            Some("888")
        );
        assert_eq!(
            resolve_entity_id(&parts, "dimstore").as_deref(),
            Some("888")
        );
        assert_eq!(resolve_entity_id(&parts, "888").as_deref(), Some("888"));
        assert_eq!(resolve_entity_id(&parts, "Nope"), None);
    }

    #[test]
    fn collect_ids_gathers_entity_and_property_ids() {
        let props = vec![
            build_property("p1", "A", "String"),
            build_property("p2", "B", "Long"),
        ];
        let def = build_entity_type_def("e1", "E", &props, &[]);
        let parts = vec![encode_part(&entity_def_path("e1"), &def)];
        let ids = collect_ids(&parts);
        assert!(ids.contains("e1"));
        assert!(ids.contains("p1"));
        assert!(ids.contains("p2"));
    }

    #[test]
    fn encode_then_part_json_roundtrips() {
        let v = json!({"hello": "world", "n": 5});
        let part = encode_part("EntityTypes/1/definition.json", &v);
        assert_eq!(part["payloadType"], "InlineBase64");
        assert_eq!(part_json(&part).unwrap(), v);
    }

    #[test]
    fn rename_entity_in_parts_updates_name_only_and_preserves_others() {
        let def = build_entity_type_def(
            "e1",
            "dimproducts",
            &[build_property("p1", "ProductId", "String")],
            &["p1".to_string()],
        );
        let other = build_entity_type_def("e2", "dimstore", &[], &[]);
        let rel = build_relationship_type_def("r1", "sells", "e2", "e1");
        let parts = vec![
            encode_part(&entity_def_path("e1"), &def),
            encode_part(&entity_def_path("e2"), &other),
            encode_part("RelationshipTypes/r1/definition.json", &rel),
        ];
        let out = rename_entity_in_parts(&parts, "e1", "Products").expect("renamed");
        // e1's name is updated, its properties/keys preserved.
        let e1 = part_json(&out[0]).unwrap();
        assert_eq!(e1["name"], "Products");
        assert_eq!(e1["id"], "e1");
        assert!(
            e1["properties"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "ProductId")
        );
        // e2 and the relationship (which references e1 by id) are untouched.
        assert_eq!(part_json(&out[1]).unwrap()["name"], "dimstore");
        let r = part_json(&out[2]).unwrap();
        assert_eq!(r["source"]["entityTypeId"], "e2");
        assert_eq!(r["target"]["entityTypeId"], "e1");
    }

    #[test]
    fn rename_entity_in_parts_returns_none_for_unknown_id() {
        let parts = vec![encode_part(
            &entity_def_path("e1"),
            &build_entity_type_def("e1", "A", &[], &[]),
        )];
        assert!(rename_entity_in_parts(&parts, "nope", "X").is_none());
    }
}
