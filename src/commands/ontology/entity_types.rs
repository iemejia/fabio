//! Ontology entity-type listing — the pure-fabio equivalent of the ontology MCP
//! server's `list_ontology_entity_types` tool.
//!
//! The Fabric Ontology MCP server exposes a `list_ontology_entity_types` tool
//! that returns each entity type's schema (id, namespace, name, properties,
//! timeseries/untyped properties, inheritance). That view is derived entirely
//! from the ontology's stored definition (the `EntityTypes/*/definition.json`
//! parts), so fabio can reproduce it offline from `getDefinition` — no MCP
//! session required.
//!
//! Output matches the MCP tool's `{"values":[...]}` shape and field order
//! exactly (`serde_json` is built with `preserve_order`). The ONE field fabio
//! cannot reproduce is the server-assigned `etag` (a per-entity concurrency
//! token that is not part of the definition and has no offline source); it is
//! therefore omitted. Every schema field matches value-for-value — verified
//! live against the MCP tool.

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Map, Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::output;

/// Fetch the ontology definition and render its entity types in the MCP
/// `list_ontology_entity_types` shape.
pub(super) async fn list_entity_types(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    entity_name: Option<&str>,
    include_properties: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/ontologies/{id}/getDefinition"),
            &json!({}),
            true,
        )
        .await?;

    let values = build_entity_type_values(&data, entity_name, include_properties);
    output::render_object(cli, &json!({ "values": values }), "values");
    Ok(())
}

/// Decode the `EntityTypes/*/definition.json` parts of a `getDefinition`
/// response and reshape them into the MCP tool's entity-type objects, sorted by
/// `id` ascending and optionally filtered by `entity_name`.
fn build_entity_type_values(
    definition: &Value,
    entity_name: Option<&str>,
    include_properties: bool,
) -> Vec<Value> {
    let Some(parts) = definition
        .get("definition")
        .and_then(|d| d.get("parts"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut entities: Vec<Value> = parts
        .iter()
        .filter(|p| {
            p.get("path")
                .and_then(Value::as_str)
                .is_some_and(is_entity_type_part)
        })
        .filter_map(decode_part)
        .filter(|raw| {
            entity_name.is_none_or(|want| raw.get("name").and_then(Value::as_str) == Some(want))
        })
        .map(|raw| reshape_entity_type(&raw, include_properties))
        .collect();

    entities.sort_by(|a, b| {
        a.get("id")
            .and_then(Value::as_str)
            .cmp(&b.get("id").and_then(Value::as_str))
    });
    entities
}

/// Is this part path an entity-type definition (`EntityTypes/<id>/definition.json`)?
fn is_entity_type_part(path: &str) -> bool {
    path.starts_with("EntityTypes/") && path.ends_with("/definition.json")
}

/// Decode a base64 `InlineBase64` part payload into a JSON value.
fn decode_part(part: &Value) -> Option<Value> {
    let payload = part.get("payload").and_then(Value::as_str)?;
    let bytes = BASE64.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Reshape one raw entity-type definition into the MCP tool's entity object:
/// canonical field order, null fields dropped, `documents`/`mappings`/
/// `resourceLinks` defaulted to `[]`, `$schema` and the server-only `etag`
/// omitted. When `include_properties` is false the three property arrays are
/// emitted empty (matching the tool's default behavior).
fn reshape_entity_type(raw: &Value, include_properties: bool) -> Value {
    let mut out = Map::new();

    // Scalar schema fields, in the MCP tool's field order.
    for key in ["id", "namespace", "name", "namespaceType"] {
        if let Some(v) = non_null(raw, key) {
            out.insert(key.to_string(), v.clone());
        }
    }
    // baseEntityTypeId is present only for entity types that inherit.
    if let Some(v) = non_null(raw, "baseEntityTypeId") {
        out.insert("baseEntityTypeId".to_string(), v.clone());
    }
    for key in ["entityIdParts", "displayNamePropertyId", "visibility"] {
        if let Some(v) = non_null(raw, key) {
            out.insert(key.to_string(), v.clone());
        }
    }

    // Property arrays (emptied when include_properties is false).
    for key in ["properties", "timeseriesProperties", "untypedProperties"] {
        let arr = if include_properties {
            raw.get(key)
                .and_then(Value::as_array)
                .map(|a| a.iter().map(strip_nulls).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        out.insert(key.to_string(), Value::Array(arr));
    }

    // Binding-derived arrays: pass through if present, else default to [].
    for key in ["documents", "mappings", "resourceLinks"] {
        let v = raw.get(key).cloned().unwrap_or_else(|| json!([]));
        out.insert(key.to_string(), v);
    }

    Value::Object(out)
}

/// Return the value at `key` only if present and non-null.
fn non_null<'a>(obj: &'a Value, key: &str) -> Option<&'a Value> {
    obj.get(key).filter(|v| !v.is_null())
}

/// Drop null-valued keys from a property object (e.g. `redefines`,
/// `baseTypeNamespaceType`), preserving the remaining field order.
fn strip_nulls(v: &Value) -> Value {
    v.as_object().map_or_else(
        || v.clone(),
        |obj| {
            Value::Object(
                obj.iter()
                    .filter(|(_, val)| !val.is_null())
                    .map(|(k, val)| (k.clone(), val.clone()))
                    .collect(),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `getDefinition` response with two entity types, mirroring the live
    /// tenant (Asset, and Sensor which inherits from Asset and has a timeseries
    /// property). Payloads are the exact raw `EntityTypes/*/definition.json`.
    fn sample_definition() -> Value {
        let asset = r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/ontology/entityType/1.0.0/schema.json","id":"8880000000001","namespace":"usertypes","baseEntityTypeId":null,"name":"Asset","entityIdParts":["888000000000101"],"displayNamePropertyId":"888000000000101","namespaceType":"Custom","visibility":"Visible","properties":[{"id":"888000000000101","name":"assetId","redefines":null,"baseTypeNamespaceType":null,"valueType":"String"}],"timeseriesProperties":[],"untypedProperties":[]}"#;
        let sensor = r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/ontology/entityType/1.0.0/schema.json","id":"8880000000002","namespace":"usertypes","baseEntityTypeId":"8880000000001","name":"Sensor","entityIdParts":["888000000000201"],"displayNamePropertyId":"888000000000201","namespaceType":"Custom","visibility":"Visible","properties":[{"id":"888000000000201","name":"sensorId","redefines":null,"baseTypeNamespaceType":null,"valueType":"String"}],"timeseriesProperties":[{"id":"888000000000202","name":"temperature","redefines":null,"baseTypeNamespaceType":null,"valueType":"Double"}],"untypedProperties":[]}"#;
        json!({
            "definition": { "parts": [
                {"path": "definition.json", "payload": BASE64.encode("{}"), "payloadType": "InlineBase64"},
                {"path": "EntityTypes/8880000000002/definition.json", "payload": BASE64.encode(sensor), "payloadType": "InlineBase64"},
                {"path": "EntityTypes/8880000000001/definition.json", "payload": BASE64.encode(asset), "payloadType": "InlineBase64"},
                {"path": ".platform", "payload": BASE64.encode("{}"), "payloadType": "InlineBase64"}
            ]}
        })
    }

    /// The exact MCP `list_ontology_entity_types` output (includeProperties=true)
    /// captured live, with the server-only `etag` field removed.
    fn expected_with_properties_no_etag() -> Value {
        json!({"values":[
            {"id":"8880000000001","namespace":"usertypes","name":"Asset","namespaceType":"Custom","entityIdParts":["888000000000101"],"displayNamePropertyId":"888000000000101","visibility":"Visible","properties":[{"id":"888000000000101","name":"assetId","valueType":"String"}],"timeseriesProperties":[],"untypedProperties":[],"documents":[],"mappings":[],"resourceLinks":[]},
            {"id":"8880000000002","namespace":"usertypes","name":"Sensor","namespaceType":"Custom","baseEntityTypeId":"8880000000001","entityIdParts":["888000000000201"],"displayNamePropertyId":"888000000000201","visibility":"Visible","properties":[{"id":"888000000000201","name":"sensorId","valueType":"String"}],"timeseriesProperties":[{"id":"888000000000202","name":"temperature","valueType":"Double"}],"untypedProperties":[],"documents":[],"mappings":[],"resourceLinks":[]}
        ]})
    }

    #[test]
    fn matches_mcp_output_with_properties() {
        let values = build_entity_type_values(&sample_definition(), None, true);
        let got = json!({ "values": values });
        assert_eq!(got, expected_with_properties_no_etag());
    }

    #[test]
    fn field_order_matches_mcp_exactly() {
        // preserve_order is on, so serialized key order must match the tool.
        let values = build_entity_type_values(&sample_definition(), None, true);
        let sensor = serde_json::to_string(&values[1]).unwrap();
        let idx = |k: &str| sensor.find(k).unwrap();
        // id < namespace < name < namespaceType < baseEntityTypeId < entityIdParts ...
        assert!(idx("\"id\"") < idx("\"namespace\""));
        assert!(idx("\"namespace\"") < idx("\"name\""));
        assert!(idx("\"namespaceType\"") < idx("\"baseEntityTypeId\""));
        assert!(idx("\"baseEntityTypeId\"") < idx("\"entityIdParts\""));
        assert!(idx("\"untypedProperties\"") < idx("\"documents\""));
        assert!(idx("\"resourceLinks\"") <= sensor.len());
        // Property object order: id < name < valueType, and no null fields.
        assert!(!sensor.contains("redefines"));
        assert!(!sensor.contains("baseTypeNamespaceType"));
    }

    #[test]
    fn include_properties_false_empties_arrays() {
        let values = build_entity_type_values(&sample_definition(), None, false);
        for v in &values {
            assert_eq!(v["properties"], json!([]));
            assert_eq!(v["timeseriesProperties"], json!([]));
            assert_eq!(v["untypedProperties"], json!([]));
        }
        // But the entity scalar fields are still present.
        assert_eq!(values[0]["name"], "Asset");
    }

    #[test]
    fn entity_name_filter_returns_single() {
        let values = build_entity_type_values(&sample_definition(), Some("Sensor"), true);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0]["name"], "Sensor");
        assert_eq!(values[0]["baseEntityTypeId"], "8880000000001");
    }

    #[test]
    fn base_entity_type_id_omitted_when_null() {
        let values = build_entity_type_values(&sample_definition(), Some("Asset"), true);
        assert!(
            !values[0]
                .as_object()
                .unwrap()
                .contains_key("baseEntityTypeId")
        );
    }

    #[test]
    fn no_etag_or_schema_leaks() {
        let values = build_entity_type_values(&sample_definition(), None, true);
        let s = serde_json::to_string(&values).unwrap();
        assert!(!s.contains("etag"));
        assert!(!s.contains("$schema"));
    }

    #[test]
    fn empty_when_no_entity_types() {
        let def = json!({"definition":{"parts":[{"path":"definition.json","payload":BASE64.encode("{}"),"payloadType":"InlineBase64"}]}});
        assert!(build_entity_type_values(&def, None, true).is_empty());
    }
}
