//! `ontology generate` — generate an ontology from a semantic model or lakehouse.
//!
//! The Fabric portal can "Generate Ontology" from a semantic model, but there is
//! NO public REST API for it. fabio reproduces the feature CLIENT-SIDE from one of
//! two schema sources:
//!
//! * `--semantic-model <id>`: reads the model's schema via the DAX `INFO.VIEW.*`
//!   functions (tables, columns, relationships — the same metadata the portal
//!   uses). Relationship "one"-side columns become entity identifiers.
//! * `--lakehouse <id>` (without `--semantic-model`): reads the lakehouse SQL
//!   analytics endpoint's `INFORMATION_SCHEMA.COLUMNS` (base tables only). Each
//!   table becomes an entity type; each column a typed property. There are no
//!   relationships to infer, so the first column of each table is treated as its
//!   identifier (a reviewable heuristic).
//!
//! Either way, the synthesized OWL runs through the existing `ontology import`
//! path (entity types, typed properties, relationship types, and — with
//! `--lakehouse` — data bindings by table name). As the concepts doc notes,
//! time-series bindings, relationship data bindings, and entity-key review remain
//! manual follow-ups (`ontology bind`, `ontology update-definition`).

use std::collections::HashSet;
use std::fmt::Write as _;

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::semantic_model::operations::fetch_info_view;
use crate::commands::tds_utils::{execute_sql_rows, resolve_lakehouse_sql};
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// Namespace IRI for generated ontology terms (non-network identifier).
const NS: &str = "http://fabric.microsoft.com/ontology/";

#[allow(clippy::too_many_arguments)]
pub(super) async fn generate(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    semantic_model: Option<&str>,
    name: &str,
    lakehouse: Option<&str>,
    lakehouse_workspace: Option<&str>,
    output_owl: Option<&str>,
) -> Result<()> {
    // Resolve the schema source: semantic model (INFO.VIEW) or lakehouse (SQL).
    let (tables, columns, relationships, keys, source) = resolve_schema(
        client,
        workspace,
        semantic_model,
        lakehouse,
        lakehouse_workspace,
    )
    .await?;

    let owl = build_owl(&tables, &columns, &relationships, &keys);
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
            &serde_json::json!({"status": "generated", "owlFile": path, "source": source, "summary": summary}),
            "status",
        );
        return Ok(());
    }

    if output::dry_run_guard(
        cli,
        "ontology generate",
        &serde_json::json!({
            "workspace": workspace,
            "source": source,
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
            "source": source,
            "summary": summary,
            "note": "Follow-ups (matching the portal flow): bind time-series data \
                     (ontology bind --eventhouse ...), review entity keys, and bind \
                     relationship types to data.",
        }),
        "status",
    );
    Ok(())
}

/// Resolve the schema source into `(tables, columns, relationships, keys, source)`.
///
/// With `--semantic-model`, reads INFO.VIEW metadata and derives keys from
/// relationships. Otherwise reads the lakehouse SQL endpoint and derives keys
/// from the first column of each table.
type Schema = (
    Vec<Value>,
    Vec<Value>,
    Vec<Value>,
    HashSet<(String, String)>,
    Value,
);

async fn resolve_schema(
    client: &FabricClient,
    workspace: &str,
    semantic_model: Option<&str>,
    lakehouse: Option<&str>,
    lakehouse_workspace: Option<&str>,
) -> Result<Schema> {
    if let Some(sm) = semantic_model {
        let tables = fetch_info_view(client, workspace, sm, "TABLES").await?;
        let columns = fetch_info_view(client, workspace, sm, "COLUMNS").await?;
        let relationships = fetch_info_view(client, workspace, sm, "RELATIONSHIPS").await?;
        let keys = relationship_keys(&relationships);
        let source = serde_json::json!({"semanticModel": sm});
        return Ok((tables, columns, relationships, keys, source));
    }

    let lh = lakehouse.ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "No schema source specified.",
            "Provide --semantic-model <id> or --lakehouse <id> as the source.",
        )
    })?;
    let lh_ws = lakehouse_workspace.unwrap_or(workspace);
    let (tables, columns, keys) = fetch_lakehouse_schema(client, lh_ws, lh).await?;
    let source = serde_json::json!({"lakehouse": lh});
    Ok((tables, columns, Vec::new(), keys, source))
}

/// Fetch a lakehouse's schema (base tables + columns) from its SQL analytics
/// endpoint and synthesize the `(tables, columns, keys)` triple in the same
/// `Value` shape [`build_owl`] consumes for the semantic-model path.
///
/// Each column carries a pre-resolved `Xsd` local name (from [`map_sql_type`]).
/// With no relationships to infer keys from, the first column (lowest
/// `ORDINAL_POSITION`) of each table is treated as the entity identifier — a
/// reviewable heuristic, since lakehouse Delta tables have no declared PK.
async fn fetch_lakehouse_schema(
    client: &FabricClient,
    workspace: &str,
    lakehouse_id: &str,
) -> Result<(Vec<Value>, Vec<Value>, HashSet<(String, String)>)> {
    let (server, database) = resolve_lakehouse_sql(client, workspace, lakehouse_id).await?;
    let sql = "SELECT c.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE, c.ORDINAL_POSITION \
               FROM INFORMATION_SCHEMA.COLUMNS c \
               JOIN INFORMATION_SCHEMA.TABLES t \
               ON c.TABLE_SCHEMA = t.TABLE_SCHEMA AND c.TABLE_NAME = t.TABLE_NAME \
               WHERE t.TABLE_TYPE = 'BASE TABLE' \
               ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION";
    let (_cols, rows) = execute_sql_rows(client, &server, &database, sql).await?;
    Ok(lakehouse_schema_from_rows(&rows))
}

/// Pure transform: `INFORMATION_SCHEMA.COLUMNS` rows -> `(tables, columns, keys)`.
fn lakehouse_schema_from_rows(
    rows: &[Value],
) -> (Vec<Value>, Vec<Value>, HashSet<(String, String)>) {
    let mut tables: Vec<Value> = Vec::new();
    let mut seen_tables: HashSet<String> = HashSet::new();
    let mut columns: Vec<Value> = Vec::new();
    let mut keys: HashSet<(String, String)> = HashSet::new();
    // The first column encountered per table (rows are ordered by ORDINAL_POSITION)
    // is the identifier heuristic.
    let mut has_key: HashSet<String> = HashSet::new();

    for row in rows {
        let Some(table) = row.get("TABLE_NAME").and_then(Value::as_str) else {
            continue;
        };
        let Some(col) = row.get("COLUMN_NAME").and_then(Value::as_str) else {
            continue;
        };
        let sql_type = row
            .get("DATA_TYPE")
            .and_then(Value::as_str)
            .unwrap_or("varchar");

        if seen_tables.insert(table.to_string()) {
            tables.push(serde_json::json!({"Name": table, "IsHidden": false}));
        }
        if has_key.insert(table.to_string()) {
            // First column for this table -> identifier heuristic.
            keys.insert((table.to_string(), col.to_string()));
        }
        columns.push(serde_json::json!({
            "Name": col,
            "Table": table,
            "Xsd": map_sql_type(sql_type),
            "IsHidden": false,
            "Type": "Data",
        }));
    }
    (tables, columns, keys)
}

/// Map a T-SQL `INFORMATION_SCHEMA` `DATA_TYPE` value to an `xsd` type local name.
fn map_sql_type(sql_type: &str) -> &'static str {
    match sql_type.to_ascii_lowercase().as_str() {
        "bit" => "boolean",
        "tinyint" | "smallint" | "int" | "bigint" => "long",
        "real" | "float" => "double",
        "decimal" | "numeric" | "money" | "smallmoney" => "decimal",
        "date" | "time" | "datetime" | "datetime2" | "smalldatetime" | "datetimeoffset" => {
            "dateTime"
        }
        // char/varchar/nchar/nvarchar/text/uniqueidentifier/binary/... -> string.
        _ => "string",
    }
}

/// Keys from semantic-model relationships: the (`ToTable`, `ToColumn`) "one" side.
fn relationship_keys(relationships: &[Value]) -> HashSet<(String, String)> {
    relationships
        .iter()
        .filter_map(|r| {
            Some((
                r.get("ToTable").and_then(Value::as_str)?.to_string(),
                r.get("ToColumn").and_then(Value::as_str)?.to_string(),
            ))
        })
        .collect()
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

/// Build an RDF/XML OWL document from a schema. Tables become `owl:Class`,
/// columns become `owl:DatatypeProperty` (typed), any (table, column) in `keys`
/// is marked `isIdentifier`, and relationships become `owl:ObjectProperty` from
/// the many-side entity to the one-side entity.
fn build_owl(
    tables: &[Value],
    columns: &[Value],
    relationships: &[Value],
    keys: &HashSet<(String, String)>,
) -> String {
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
        // Lakehouse path pre-resolves the XSD type into an `Xsd` field; the
        // semantic-model path carries a DAX `DataType` mapped on the fly.
        let xsd = c
            .get("Xsd")
            .and_then(Value::as_str)
            .unwrap_or_else(|| map_datatype(dax_type));
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
        let owl = build_owl(
            &tables(),
            &columns(),
            &relationships(),
            &relationship_keys(&relationships()),
        );
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
        let owl = build_owl(
            &tables(),
            &columns(),
            &relationships(),
            &relationship_keys(&relationships()),
        );
        assert!(!owl.contains("RowNumber"));
        // Latitude is Double -> xsd:double.
        assert!(owl.contains("dimstore.Latitude"));
        assert!(owl.contains("XMLSchema#double"));
        // ProductId is Text -> xsd:string.
        assert!(owl.contains("XMLSchema#string"));
    }

    #[test]
    fn owl_marks_relationship_target_keys_as_identifier() {
        let owl = build_owl(
            &tables(),
            &columns(),
            &relationships(),
            &relationship_keys(&relationships()),
        );
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
        let owl = build_owl(
            &tables(),
            &columns(),
            &relationships(),
            &relationship_keys(&relationships()),
        );
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

    // --- Lakehouse (INFORMATION_SCHEMA) source path ---

    fn info_schema_rows() -> Vec<Value> {
        vec![
            json!({"TABLE_NAME": "dimstore", "COLUMN_NAME": "StoreId", "DATA_TYPE": "varchar", "ORDINAL_POSITION": 1}),
            json!({"TABLE_NAME": "dimstore", "COLUMN_NAME": "Latitude", "DATA_TYPE": "float", "ORDINAL_POSITION": 2}),
            json!({"TABLE_NAME": "factsales", "COLUMN_NAME": "SaleId", "DATA_TYPE": "bigint", "ORDINAL_POSITION": 1}),
            json!({"TABLE_NAME": "factsales", "COLUMN_NAME": "RevenueUSD", "DATA_TYPE": "decimal", "ORDINAL_POSITION": 2}),
            json!({"TABLE_NAME": "factsales", "COLUMN_NAME": "SoldAt", "DATA_TYPE": "datetime2", "ORDINAL_POSITION": 3}),
        ]
    }

    #[test]
    fn maps_sql_types_to_xsd() {
        assert_eq!(map_sql_type("varchar"), "string");
        assert_eq!(map_sql_type("NVARCHAR"), "string");
        assert_eq!(map_sql_type("int"), "long");
        assert_eq!(map_sql_type("bigint"), "long");
        assert_eq!(map_sql_type("float"), "double");
        assert_eq!(map_sql_type("decimal"), "decimal");
        assert_eq!(map_sql_type("datetime2"), "dateTime");
        assert_eq!(map_sql_type("bit"), "boolean");
        assert_eq!(map_sql_type("uniqueidentifier"), "string");
    }

    #[test]
    fn lakehouse_rows_build_tables_columns_and_first_column_keys() {
        let (tables, columns, keys) = lakehouse_schema_from_rows(&info_schema_rows());
        assert_eq!(tables.len(), 2);
        assert_eq!(columns.len(), 5);
        // First column of each table is the identifier heuristic.
        assert!(keys.contains(&("dimstore".to_string(), "StoreId".to_string())));
        assert!(keys.contains(&("factsales".to_string(), "SaleId".to_string())));
        // Non-first columns are not keys.
        assert!(!keys.contains(&("factsales".to_string(), "RevenueUSD".to_string())));
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn lakehouse_owl_is_typed_and_keyed() {
        let (tables, columns, keys) = lakehouse_schema_from_rows(&info_schema_rows());
        let owl = build_owl(&tables, &columns, &[], &keys);
        assert_eq!(owl.matches("<owl:Class").count(), 2);
        // No relationships from a lakehouse source.
        assert_eq!(owl.matches("<owl:ObjectProperty").count(), 0);
        // decimal + dateTime + long types resolved from SQL.
        assert!(owl.contains("XMLSchema#decimal"));
        assert!(owl.contains("XMLSchema#dateTime"));
        assert!(owl.contains("XMLSchema#long"));
        // First column is a key.
        let store_key = owl.find("dimstore.StoreId").unwrap();
        assert!(owl[store_key..store_key + 400].contains("isIdentifier"));
    }
}
