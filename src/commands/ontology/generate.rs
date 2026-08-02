//! `ontology generate` — generate an ontology from a Power BI semantic model.
//!
//! The Fabric portal can "Generate Ontology" from a semantic model, but there is
//! NO public REST API for it. fabio reproduces the feature CLIENT-SIDE: it reads
//! the model's schema via the DAX `INFO.VIEW.*` functions (tables, columns,
//! relationships — the same metadata the portal uses), synthesizes an OWL model,
//! and runs it through the existing `ontology import` path (which builds entity
//! types, typed properties, relationship types, and — with `--lakehouse` — data
//! bindings by table name).
//!
//! This mirrors the portal's automatic output (entity types + static properties +
//! bindings + relationship definitions). As the concepts doc notes, time-series
//! bindings, relationship data bindings, and entity-key review remain manual
//! follow-ups (`ontology bind`, `ontology update-definition`).

use std::collections::HashSet;
use std::fmt::Write as _;

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::semantic_model::operations::fetch_info_view;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// Namespace IRI for generated ontology terms (non-network identifier).
const NS: &str = "http://fabric.microsoft.com/ontology/";

#[allow(clippy::too_many_arguments)]
pub(super) async fn generate(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    semantic_model: &str,
    name: &str,
    lakehouse: Option<&str>,
    lakehouse_workspace: Option<&str>,
    output_owl: Option<&str>,
) -> Result<()> {
    // Fetch the model schema (tables, columns, relationships) via INFO.VIEW.
    let tables = fetch_info_view(client, workspace, semantic_model, "TABLES").await?;
    let columns = fetch_info_view(client, workspace, semantic_model, "COLUMNS").await?;
    let relationships = fetch_info_view(client, workspace, semantic_model, "RELATIONSHIPS").await?;

    let owl = build_owl(&tables, &columns, &relationships);
    let summary = summarize(&tables, &columns, &relationships);

    // --output-owl: write the generated OWL and stop (composable / inspectable).
    if let Some(path) = output_owl {
        std::fs::write(path, &owl).map_err(|e| {
            FabioError::new(
                ErrorCode::InvalidInput,
                format!("Failed to write OWL to '{path}': {e}"),
            )
        })?;
        output::render_object(
            cli,
            &serde_json::json!({"status": "generated", "owlFile": path, "summary": summary}),
            "status",
        );
        return Ok(());
    }

    if output::dry_run_guard(
        cli,
        "ontology generate",
        &serde_json::json!({
            "workspace": workspace,
            "semanticModel": semantic_model,
            "name": name,
            "lakehouse": lakehouse,
            "summary": summary,
            "owl": owl,
        }),
    ) {
        return Ok(());
    }

    // Create the ontology item, then import the synthesized OWL into it. With
    // --lakehouse, entity types convention-bind to same-named lakehouse tables.
    let created = client
        .post(
            &format!("/workspaces/{workspace}/ontologies"),
            &serde_json::json!({"displayName": name}),
            true,
        )
        .await?;
    let ontology_id = created
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| FabioError::new(ErrorCode::ApiError, "Ontology create returned no id"))?
        .to_string();

    let owl_path = std::env::temp_dir().join(format!("fabio-ontology-{ontology_id}.owl"));
    std::fs::write(&owl_path, &owl)
        .map_err(|e| FabioError::new(ErrorCode::InvalidInput, e.to_string()))?;

    // Reuse the tested import path (parses OWL -> entity types + bindings).
    let import_result = super::import::import_owl(
        cli,
        client,
        Some(workspace),
        Some(&ontology_id),
        owl_path.to_str().unwrap(),
        None,
        lakehouse,
        lakehouse_workspace,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        true,
    )
    .await;
    let _ = std::fs::remove_file(&owl_path);
    import_result?;

    output::render_object(
        cli,
        &serde_json::json!({
            "status": "generated",
            "id": ontology_id,
            "name": name,
            "semanticModel": semantic_model,
            "summary": summary,
            "note": "Follow-ups (matching the portal flow): bind time-series data \
                     (ontology bind --eventhouse ...), review entity keys, and bind \
                     relationship types to data.",
        }),
        "status",
    );
    Ok(())
}

/// Map a DAX `INFO.VIEW.COLUMNS.DataType` to an XSD type local name.
fn map_datatype(dax_type: &str) -> &'static str {
    match dax_type {
        "Integer" | "Int64" | "Whole Number" => "long",
        // Power BI's floating-point "Decimal Number" surfaces as "Number" in INFO.VIEW.
        "Number" | "Double" | "Decimal Number" => "double",
        "Currency" | "Fixed Decimal Number" | "Decimal" => "decimal",
        "DateTime" | "Date" | "Time" | "Date/Time" => "dateTime",
        "Boolean" | "True/False" => "boolean",
        // Text and anything unrecognized map to string.
        _ => "string",
    }
}

/// Is this column a synthetic/hidden column that must not become a property?
fn is_synthetic_column(col: &Value) -> bool {
    let hidden = col
        .get("IsHidden")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ty = col.get("Type").and_then(Value::as_str).unwrap_or("");
    let cat = col
        .get("DataCategory")
        .and_then(Value::as_str)
        .unwrap_or("");
    let name = col.get("Name").and_then(Value::as_str).unwrap_or("");
    hidden || ty == "RowNumber" || cat == "RowNumber" || name.starts_with("RowNumber-")
}

/// Build an RDF/XML OWL document from the semantic-model schema. Tables become
/// `owl:Class`, columns become `owl:DatatypeProperty` (typed), the "one"-side key
/// of each relationship is marked `isIdentifier`, and relationships become
/// `owl:ObjectProperty` from the many-side entity to the one-side entity.
fn build_owl(tables: &[Value], columns: &[Value], relationships: &[Value]) -> String {
    // Keys = (ToTable, ToColumn) pairs (the dimension/"one" side of a relationship).
    let keys: HashSet<(String, String)> = relationships
        .iter()
        .filter_map(|r| {
            Some((
                r.get("ToTable").and_then(Value::as_str)?.to_string(),
                r.get("ToColumn").and_then(Value::as_str)?.to_string(),
            ))
        })
        .collect();

    let mut owl = String::from(
        "<?xml version=\"1.0\"?>\n\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"\n\
         \x20        xmlns:rdfs=\"http://www.w3.org/2000/01/rdf-schema#\"\n\
         \x20        xmlns:owl=\"http://www.w3.org/2002/07/owl#\"\n\
         \x20        xmlns:ont=\"http://fabric.microsoft.com/ontology/\">\n",
    );

    for t in tables {
        if t.get("IsHidden").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let Some(name) = t.get("Name").and_then(Value::as_str) else {
            continue;
        };
        let _ = writeln!(
            owl,
            "  <owl:Class rdf:about=\"{NS}{name}\"><rdfs:label>{name}</rdfs:label></owl:Class>"
        );
    }

    for c in columns {
        if is_synthetic_column(c) {
            continue;
        }
        let (Some(table), Some(name)) = (
            c.get("Table").and_then(Value::as_str),
            c.get("Name").and_then(Value::as_str),
        ) else {
            continue;
        };
        let dax_type = c.get("DataType").and_then(Value::as_str).unwrap_or("Text");
        let xsd = map_datatype(dax_type);
        let is_key = keys.contains(&(table.to_string(), name.to_string()));
        // Property IRI is per-(table,column) to avoid collisions across entities;
        // the label (the ontology property name) stays the column name.
        let _ = write!(
            owl,
            "  <owl:DatatypeProperty rdf:about=\"{NS}{table}.{name}\">\n\
             \x20   <rdfs:label>{name}</rdfs:label>\n\
             \x20   <rdfs:domain rdf:resource=\"{NS}{table}\"/>\n\
             \x20   <rdfs:range rdf:resource=\"http://www.w3.org/2001/XMLSchema#{xsd}\"/>\n"
        );
        if is_key {
            owl.push_str(
                "    <ont:isIdentifier rdf:datatype=\"http://www.w3.org/2001/XMLSchema#boolean\">true</ont:isIdentifier>\n",
            );
        }
        owl.push_str("  </owl:DatatypeProperty>\n");
    }

    for r in relationships {
        let (Some(from), Some(to)) = (
            r.get("FromTable").and_then(Value::as_str),
            r.get("ToTable").and_then(Value::as_str),
        ) else {
            continue;
        };
        let rel = format!("{from}_has_{to}");
        let _ = write!(
            owl,
            "  <owl:ObjectProperty rdf:about=\"{NS}{rel}\">\n\
             \x20   <rdfs:label>{rel}</rdfs:label>\n\
             \x20   <rdfs:domain rdf:resource=\"{NS}{from}\"/>\n\
             \x20   <rdfs:range rdf:resource=\"{NS}{to}\"/>\n\
             \x20 </owl:ObjectProperty>\n"
        );
    }

    owl.push_str("</rdf:RDF>\n");
    owl
}

/// A compact summary of what would be generated.
fn summarize(tables: &[Value], columns: &[Value], relationships: &[Value]) -> Value {
    let entity_types: Vec<&str> = tables
        .iter()
        .filter(|t| !t.get("IsHidden").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|t| t.get("Name").and_then(Value::as_str))
        .collect();
    let property_count = columns.iter().filter(|c| !is_synthetic_column(c)).count();
    serde_json::json!({
        "entityTypes": entity_types,
        "propertyCount": property_count,
        "relationshipCount": relationships.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Metadata mirroring the live INFO.VIEW output for a 3-table retail model.
    fn tables() -> Vec<Value> {
        vec![
            json!({"Name": "dimproducts", "IsHidden": false}),
            json!({"Name": "dimstore", "IsHidden": false}),
            json!({"Name": "factsales", "IsHidden": false}),
        ]
    }
    fn columns() -> Vec<Value> {
        vec![
            json!({"Name": "RowNumber-2662", "Table": "dimproducts", "DataType": "Integer", "IsHidden": true, "Type": "RowNumber", "DataCategory": "RowNumber"}),
            json!({"Name": "ProductId", "Table": "dimproducts", "DataType": "Text", "IsHidden": false, "Type": "Data"}),
            json!({"Name": "StoreId", "Table": "dimstore", "DataType": "Text", "IsHidden": false, "Type": "Data"}),
            json!({"Name": "Latitude", "Table": "dimstore", "DataType": "Number", "IsHidden": false, "Type": "Data"}),
            json!({"Name": "SaleId", "Table": "factsales", "DataType": "Integer", "IsHidden": false, "Type": "Data"}),
            json!({"Name": "StoreId", "Table": "factsales", "DataType": "Text", "IsHidden": false, "Type": "Data"}),
            json!({"Name": "RevenueUSD", "Table": "factsales", "DataType": "Number", "IsHidden": false, "Type": "Data"}),
        ]
    }
    fn relationships() -> Vec<Value> {
        vec![
            json!({"FromTable": "factsales", "FromColumn": "StoreId", "ToTable": "dimstore", "ToColumn": "StoreId"}),
            json!({"FromTable": "factsales", "FromColumn": "ProductId", "ToTable": "dimproducts", "ToColumn": "ProductId"}),
        ]
    }

    #[test]
    fn maps_dax_types_to_xsd() {
        assert_eq!(map_datatype("Text"), "string");
        assert_eq!(map_datatype("Integer"), "long");
        assert_eq!(map_datatype("Number"), "double");
        assert_eq!(map_datatype("DateTime"), "dateTime");
        assert_eq!(map_datatype("Boolean"), "boolean");
        assert_eq!(map_datatype("Whatever"), "string");
    }

    #[test]
    fn filters_synthetic_rownumber_columns() {
        assert!(is_synthetic_column(
            &json!({"Name": "RowNumber-x", "IsHidden": true, "Type": "RowNumber"})
        ));
        assert!(!is_synthetic_column(
            &json!({"Name": "ProductId", "IsHidden": false, "Type": "Data"})
        ));
    }

    #[test]
    fn owl_has_class_per_visible_table() {
        let owl = build_owl(&tables(), &columns(), &relationships());
        assert!(
            owl.contains("<owl:Class rdf:about=\"http://fabric.microsoft.com/ontology/dimstore\">")
        );
        assert!(
            owl.contains(
                "<owl:Class rdf:about=\"http://fabric.microsoft.com/ontology/factsales\">"
            )
        );
        assert_eq!(owl.matches("<owl:Class").count(), 3);
    }

    #[test]
    fn owl_skips_rownumber_and_types_properties() {
        let owl = build_owl(&tables(), &columns(), &relationships());
        assert!(!owl.contains("RowNumber"));
        // Latitude is Double -> xsd:double.
        assert!(owl.contains("dimstore.Latitude"));
        assert!(owl.contains("XMLSchema#double"));
        // ProductId is Text -> xsd:string.
        assert!(owl.contains("XMLSchema#string"));
    }

    #[test]
    fn owl_marks_relationship_target_keys_as_identifier() {
        let owl = build_owl(&tables(), &columns(), &relationships());
        // dimstore.StoreId is the "one" side of a relationship -> isIdentifier.
        let store_key = owl.find("dimstore.StoreId").unwrap();
        let after = &owl[store_key..store_key + 400];
        assert!(
            after.contains("isIdentifier"),
            "dimstore.StoreId should be a key"
        );
        // factsales.StoreId (many side) is NOT a key.
        let fact_key = owl.find("factsales.StoreId").unwrap();
        let after2 = &owl[fact_key..fact_key + 400];
        assert!(
            !after2.contains("isIdentifier"),
            "factsales.StoreId should not be a key"
        );
    }

    #[test]
    fn owl_emits_object_property_per_relationship() {
        let owl = build_owl(&tables(), &columns(), &relationships());
        assert!(owl.contains("factsales_has_dimstore"));
        assert!(owl.contains("factsales_has_dimproducts"));
        assert_eq!(owl.matches("<owl:ObjectProperty").count(), 2);
    }

    #[test]
    fn summary_counts_visible_entities_and_properties() {
        let s = summarize(&tables(), &columns(), &relationships());
        assert_eq!(s["entityTypes"].as_array().unwrap().len(), 3);
        assert_eq!(s["relationshipCount"], 2);
        // 6 real columns (RowNumber excluded).
        assert_eq!(s["propertyCount"], 6);
    }
}
