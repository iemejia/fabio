//! `semantic-model generate` — generate a Direct Lake semantic model from a
//! lakehouse or warehouse, the way the Fabric portal's "New semantic model" does.
//!
//! The portal action (open a lakehouse/warehouse/SQL analytics endpoint → "New
//! semantic model" → pick tables) has NO public REST API. fabio reproduces it
//! CLIENT-SIDE:
//!
//! 1. Resolve the source's SQL analytics endpoint `(server, database)`.
//! 2. Read `INFORMATION_SCHEMA.COLUMNS` for the base tables (optionally filtered
//!    to `--tables`).
//! 3. Map each SQL type to a Power BI data type; columns whose type can't be
//!    mapped are DROPPED (matching Fabric's own sync behavior).
//! 4. Synthesize a Direct Lake `model.bim` (TMSL, compatibilityLevel 1604,
//!    `defaultMode: directLake`, one `directLake` entity partition per table, and
//!    a shared `DatabaseQuery = Sql.Database(server, database)` expression).
//! 5. Create the semantic model and frame it with a `Full` refresh so DAX works.
//!
//! Relationships and measures are NOT inferred (same as the portal — you add
//! them afterward with `update-definition`). This mirrors `ontology generate`,
//! which likewise reproduces a portal-only, no-REST feature.

use std::collections::HashSet;
use std::fmt::Write as _;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{
    execute_sql_rows, parse_connection_string, resolve_lakehouse_sql,
};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

/// A table planned for the generated model: its name + the columns that survived
/// type mapping (each carrying its resolved Power BI `dataType`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GenColumn {
    pub name: String,
    pub data_type: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GenTable {
    pub name: String,
    pub columns: Vec<GenColumn>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn generate(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: Option<&str>,
    warehouse: Option<&str>,
    name: &str,
    tables: Option<&str>,
    schema: &str,
    no_refresh: bool,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    // Resolve the schema source (exactly one of --lakehouse / --warehouse).
    let (server, database, source) =
        resolve_source_sql(client, workspace, lakehouse, warehouse).await?;

    // Optional table allow-list (case-insensitive, comma-separated).
    let filter: Option<HashSet<String>> = tables.map(|t| {
        t.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    });

    // Read the source schema over the SQL analytics endpoint.
    let sql = build_schema_query(schema);
    let (_cols, rows) = execute_sql_rows(client, &server, &database, &sql).await?;
    let (gen_tables, dropped) = plan_tables(&rows, filter.as_ref());

    if gen_tables.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "No usable tables found in schema '{schema}' (after type mapping) for the selected source."
            ),
            "Verify the lakehouse/warehouse has BASE TABLEs with mappable column types, and that --tables (if given) matches real table names. The SQL analytics endpoint can lag ~30-60s behind a freshly loaded table.".to_string(),
        )
        .into());
    }

    let model_bim = build_direct_lake_bim(&server, &database, schema, &gen_tables);
    let summary = summarize(&gen_tables, &dropped, schema);

    if output::dry_run_guard(
        cli,
        "semantic-model generate",
        &serde_json::json!({
            "workspace": workspace,
            "name": name,
            "source": source,
            "storageMode": "directLake",
            "summary": summary,
        }),
    ) {
        return Ok(());
    }

    let (id, framed, note) = create_and_frame(
        client,
        workspace,
        name,
        &model_bim,
        description,
        sensitivity_label,
        no_refresh,
    )
    .await?;

    output::render_object(
        cli,
        &serde_json::json!({
            "status": "generated",
            "id": id,
            "name": name,
            "source": source,
            "storageMode": "directLake",
            "framed": framed,
            "summary": summary,
            "note": note,
        }),
        "status",
    );
    Ok(())
}

/// Create the semantic model from the synthesized `model.bim` (+ `definition.pbism`)
/// and frame it with a `Full` refresh (unless `no_refresh`). Returns
/// `(id, framed, note)`. A freshly created Direct Lake model errors on DAX until
/// framed, so framing is triggered by default but is NON-FATAL — the note
/// records what happened so the caller can retry with `semantic-model refresh`.
async fn create_and_frame(
    client: &FabricClient,
    workspace: &str,
    name: &str,
    model_bim: &Value,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
    no_refresh: bool,
) -> Result<(String, bool, String)> {
    let parts = vec![
        serde_json::json!({
            "path": "model.bim",
            "payload": BASE64.encode(model_bim.to_string().as_bytes()),
            "payloadType": "InlineBase64"
        }),
        serde_json::json!({
            "path": "definition.pbism",
            "payload": BASE64.encode(pbism().to_string().as_bytes()),
            "payloadType": "InlineBase64"
        }),
    ];
    let mut body = serde_json::json!({
        "displayName": name,
        "definition": { "parts": parts }
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({ "sensitivityLabelId": label_id });
    }

    let created = client
        .post(
            &format!("/workspaces/{workspace}/semanticModels"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model generate", "Member"))?;
    let id = created
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| FabioError::new(ErrorCode::ApiError, "Create returned no id"))?
        .to_string();

    let mut framed = false;
    let mut note = String::from(
        "Direct Lake model created. Relationships/measures are not generated (like the portal) — add them via update-definition. Query with: fabio semantic-model query.",
    );
    if no_refresh {
        note.push_str(
            " Framing skipped (--no-refresh): run `fabio semantic-model refresh` before querying.",
        );
    } else if let Err(e) = client
        .post_powerbi(
            &format!("/groups/{workspace}/datasets/{id}/refreshes"),
            &serde_json::json!({ "type": "Full" }),
        )
        .await
    {
        let _ = write!(
            note,
            " Framing refresh could not be triggered ({e}); run `fabio semantic-model refresh --workspace {workspace} --id {id}` before querying."
        );
    } else {
        framed = true;
        note.push_str(" Framed (allow ~15-30s before the first DAX query).");
    }
    Ok((id, framed, note))
}

/// Resolve the source item to `(sql_server_host, database, source_json)`.
///
/// Exactly one of `--lakehouse` / `--warehouse` must be set. The server is the
/// SQL analytics endpoint host and the database is the item's catalog name
/// (both needed to read the schema over TDS AND to bind the Direct Lake
/// `Sql.Database(...)` expression).
async fn resolve_source_sql(
    client: &FabricClient,
    workspace: &str,
    lakehouse: Option<&str>,
    warehouse: Option<&str>,
) -> Result<(String, String, Value)> {
    match (lakehouse, warehouse) {
        (Some(_), Some(_)) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Provide exactly one source: --lakehouse OR --warehouse.".to_string(),
            "e.g. --lakehouse <LH_ID>  or  --warehouse <WH_ID>".to_string(),
        )
        .into()),
        (Some(lh), None) => {
            let (server, database) = resolve_lakehouse_sql(client, workspace, lh).await?;
            Ok((server, database, serde_json::json!({ "lakehouse": lh })))
        }
        (None, Some(wh)) => {
            let data = client
                .get(&format!("/workspaces/{workspace}/warehouses/{wh}"))
                .await
                .map_err(|e| enrich_forbidden(e, "warehouse", "Viewer"))?;
            let conn = data
                .get("properties")
                .and_then(|p| p.get("connectionString"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    FabioError::with_hint(
                        ErrorCode::NotFound,
                        "Warehouse SQL connection string not available.",
                        "Wait for provisioning to complete, then retry.",
                    )
                })?;
            let (server, _parsed_db) = parse_connection_string(conn);
            let database = data
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok((server, database, serde_json::json!({ "warehouse": wh })))
        }
        (None, None) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "No source specified.".to_string(),
            "Provide --lakehouse <id> or --warehouse <id> as the model's data source.".to_string(),
        )
        .into()),
    }
}

/// The `INFORMATION_SCHEMA` query for base-table columns in a given schema,
/// ordered so a table's columns arrive together in ordinal order.
fn build_schema_query(schema: &str) -> String {
    // Escape single quotes in the schema name (defense-in-depth; schema names
    // are simple identifiers but the value is caller-supplied).
    let safe = schema.replace('\'', "''");
    format!(
        "SELECT c.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE, c.ORDINAL_POSITION \
         FROM INFORMATION_SCHEMA.COLUMNS c \
         JOIN INFORMATION_SCHEMA.TABLES t \
         ON c.TABLE_SCHEMA = t.TABLE_SCHEMA AND c.TABLE_NAME = t.TABLE_NAME \
         WHERE t.TABLE_TYPE = 'BASE TABLE' AND c.TABLE_SCHEMA = '{safe}' \
         ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION"
    )
}

/// Map a T-SQL `INFORMATION_SCHEMA.DATA_TYPE` to a Power BI TMSL column
/// `dataType`. Returns `None` for types Power BI cannot represent — the column
/// is then DROPPED, matching Fabric's "unmappable types are dropped" sync rule.
fn map_sql_type_to_powerbi(sql_type: &str) -> Option<&'static str> {
    match sql_type.to_ascii_lowercase().as_str() {
        "bit" => Some("boolean"),
        "tinyint" | "smallint" | "int" | "bigint" => Some("int64"),
        "real" | "float" => Some("double"),
        "decimal" | "numeric" | "money" | "smallmoney" => Some("decimal"),
        "date" | "datetime" | "datetime2" | "smalldatetime" | "datetimeoffset" | "time" => {
            Some("dateTime")
        }
        "char" | "varchar" | "nchar" | "nvarchar" | "text" | "ntext" | "uniqueidentifier" => {
            Some("string")
        }
        // varbinary/binary/image/geography/geometry/hierarchyid/sql_variant/xml/... are
        // not representable as a Power BI column type -> drop (like Fabric).
        _ => None,
    }
}

/// Pure transform: `INFORMATION_SCHEMA.COLUMNS` rows -> `(tables, dropped)`.
///
/// `dropped` lists `"table.column (sqlType)"` for each column skipped because
/// its SQL type is unmappable. A table with zero mappable columns is omitted.
fn plan_tables(rows: &[Value], filter: Option<&HashSet<String>>) -> (Vec<GenTable>, Vec<String>) {
    let mut tables: Vec<GenTable> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();

    for row in rows {
        let Some(table) = row.get("TABLE_NAME").and_then(Value::as_str) else {
            continue;
        };
        if let Some(f) = filter
            && !f.contains(&table.to_ascii_lowercase())
        {
            continue;
        }
        let Some(col) = row.get("COLUMN_NAME").and_then(Value::as_str) else {
            continue;
        };
        let sql_type = row
            .get("DATA_TYPE")
            .and_then(Value::as_str)
            .unwrap_or("varchar");

        match map_sql_type_to_powerbi(sql_type) {
            Some(dt) => {
                if let Some(t) = tables.iter_mut().find(|t| t.name == table) {
                    t.columns.push(GenColumn {
                        name: col.to_string(),
                        data_type: dt,
                    });
                } else {
                    tables.push(GenTable {
                        name: table.to_string(),
                        columns: vec![GenColumn {
                            name: col.to_string(),
                            data_type: dt,
                        }],
                    });
                }
            }
            None => dropped.push(format!("{table}.{col} ({sql_type})")),
        }
    }

    // Drop tables that ended up with no mappable columns.
    tables.retain(|t| !t.columns.is_empty());
    (tables, dropped)
}

/// The `definition.pbism` for a model.bim (TMSL v3) model.
fn pbism() -> Value {
    serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/semanticModel/definitionProperties/1.0.0/schema.json",
        "version": "3.0"
    })
}

/// Build a Direct Lake `model.bim` (TMSL) from the planned tables.
///
/// Key requirements (see AGENTS.md / `direct_lake_report` workflow):
/// `compatibilityLevel 1604`, `defaultPowerBIDataSourceVersion: powerBI_V3`,
/// `defaultMode: directLake`, one `directLake` entity partition per table
/// bound to the shared `DatabaseQuery` expression, which is
/// `Sql.Database("<server>", "<database>")`.
fn build_direct_lake_bim(server: &str, database: &str, schema: &str, tables: &[GenTable]) -> Value {
    let table_specs: Vec<Value> = tables
        .iter()
        .map(|t| {
            let columns: Vec<Value> = t
                .columns
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "dataType": c.data_type,
                        "sourceColumn": c.name,
                        "summarizeBy": "none"
                    })
                })
                .collect();
            serde_json::json!({
                "name": t.name,
                "columns": columns,
                "partitions": [{
                    "name": t.name,
                    "mode": "directLake",
                    "source": {
                        "type": "entity",
                        "entityName": t.name,
                        "schemaName": schema,
                        "expressionSource": "DatabaseQuery"
                    }
                }]
            })
        })
        .collect();

    // The shared M expression that binds Direct Lake to the SQL analytics
    // endpoint. Sql.Database(server, database) — the server FQDN + catalog name.
    let expression =
        format!("let\n    database = Sql.Database(\"{server}\", \"{database}\")\nin\n    database");

    serde_json::json!({
        "compatibilityLevel": 1604,
        "model": {
            "culture": "en-US",
            "defaultPowerBIDataSourceVersion": "powerBI_V3",
            "defaultMode": "directLake",
            "tables": table_specs,
            "expressions": [{
                "name": "DatabaseQuery",
                "kind": "m",
                "expression": expression
            }]
        }
    })
}

/// A compact summary of the generated model.
fn summarize(tables: &[GenTable], dropped: &[String], schema: &str) -> Value {
    let table_names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    let column_count: usize = tables.iter().map(|t| t.columns.len()).sum();
    serde_json::json!({
        "schema": schema,
        "tables": table_names,
        "tableCount": tables.len(),
        "columnCount": column_count,
        "droppedColumns": dropped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn info_schema_rows() -> Vec<Value> {
        vec![
            json!({"TABLE_NAME": "dimstore", "COLUMN_NAME": "StoreId", "DATA_TYPE": "bigint", "ORDINAL_POSITION": 1}),
            json!({"TABLE_NAME": "dimstore", "COLUMN_NAME": "StoreName", "DATA_TYPE": "varchar", "ORDINAL_POSITION": 2}),
            json!({"TABLE_NAME": "dimstore", "COLUMN_NAME": "Location", "DATA_TYPE": "geography", "ORDINAL_POSITION": 3}),
            json!({"TABLE_NAME": "factsales", "COLUMN_NAME": "SaleId", "DATA_TYPE": "bigint", "ORDINAL_POSITION": 1}),
            json!({"TABLE_NAME": "factsales", "COLUMN_NAME": "Amount", "DATA_TYPE": "decimal", "ORDINAL_POSITION": 2}),
            json!({"TABLE_NAME": "factsales", "COLUMN_NAME": "SoldAt", "DATA_TYPE": "datetime2", "ORDINAL_POSITION": 3}),
        ]
    }

    #[test]
    fn maps_sql_types_and_drops_unmappable() {
        assert_eq!(map_sql_type_to_powerbi("bit"), Some("boolean"));
        assert_eq!(map_sql_type_to_powerbi("INT"), Some("int64"));
        assert_eq!(map_sql_type_to_powerbi("bigint"), Some("int64"));
        assert_eq!(map_sql_type_to_powerbi("float"), Some("double"));
        assert_eq!(map_sql_type_to_powerbi("decimal"), Some("decimal"));
        assert_eq!(map_sql_type_to_powerbi("datetime2"), Some("dateTime"));
        assert_eq!(map_sql_type_to_powerbi("nvarchar"), Some("string"));
        assert_eq!(map_sql_type_to_powerbi("uniqueidentifier"), Some("string"));
        // Unmappable -> dropped.
        assert_eq!(map_sql_type_to_powerbi("geography"), None);
        assert_eq!(map_sql_type_to_powerbi("varbinary"), None);
    }

    #[test]
    fn plan_tables_groups_columns_and_records_drops() {
        let (tables, dropped) = plan_tables(&info_schema_rows(), None);
        assert_eq!(tables.len(), 2);
        let dimstore = tables.iter().find(|t| t.name == "dimstore").unwrap();
        // geography column dropped -> only 2 columns survive.
        assert_eq!(dimstore.columns.len(), 2);
        assert_eq!(dimstore.columns[0].data_type, "int64");
        assert_eq!(dimstore.columns[1].data_type, "string");
        let factsales = tables.iter().find(|t| t.name == "factsales").unwrap();
        assert_eq!(factsales.columns.len(), 3);
        assert!(
            dropped
                .iter()
                .any(|d| d.contains("dimstore.Location (geography)"))
        );
    }

    #[test]
    fn plan_tables_honors_case_insensitive_filter() {
        let filter: HashSet<String> = std::iter::once("FACTSALES".to_ascii_lowercase()).collect();
        let (tables, _) = plan_tables(&info_schema_rows(), Some(&filter));
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "factsales");
    }

    #[test]
    fn plan_tables_drops_table_with_no_mappable_columns() {
        let rows = vec![
            json!({"TABLE_NAME": "blobs", "COLUMN_NAME": "data", "DATA_TYPE": "varbinary", "ORDINAL_POSITION": 1}),
        ];
        let (tables, dropped) = plan_tables(&rows, None);
        assert!(tables.is_empty());
        assert_eq!(dropped.len(), 1);
    }

    #[test]
    fn direct_lake_bim_has_required_fields() {
        let (tables, _) = plan_tables(&info_schema_rows(), None);
        let bim = build_direct_lake_bim(
            "srv.datawarehouse.fabric.microsoft.com",
            "MyLH",
            "dbo",
            &tables,
        );
        assert_eq!(bim["compatibilityLevel"], 1604);
        assert_eq!(bim["model"]["defaultMode"], "directLake");
        assert_eq!(
            bim["model"]["defaultPowerBIDataSourceVersion"],
            "powerBI_V3"
        );
        // One partition per table, directLake entity source referencing DatabaseQuery.
        let t0 = &bim["model"]["tables"][0];
        let part = &t0["partitions"][0];
        assert_eq!(part["mode"], "directLake");
        assert_eq!(part["source"]["type"], "entity");
        assert_eq!(part["source"]["schemaName"], "dbo");
        assert_eq!(part["source"]["expressionSource"], "DatabaseQuery");
        // The shared expression binds Sql.Database(server, database).
        let expr = bim["model"]["expressions"][0]["expression"]
            .as_str()
            .unwrap();
        assert!(
            expr.contains("Sql.Database(\"srv.datawarehouse.fabric.microsoft.com\", \"MyLH\")")
        );
    }

    #[test]
    fn schema_query_filters_base_tables_and_schema() {
        let sql = build_schema_query("dbo");
        assert!(sql.contains("INFORMATION_SCHEMA.COLUMNS"));
        assert!(sql.contains("TABLE_TYPE = 'BASE TABLE'"));
        assert!(sql.contains("TABLE_SCHEMA = 'dbo'"));
        assert!(sql.contains("ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION"));
    }

    #[test]
    fn schema_query_escapes_single_quotes() {
        let sql = build_schema_query("we'ird");
        assert!(sql.contains("TABLE_SCHEMA = 'we''ird'"));
    }

    #[test]
    fn summary_counts_tables_columns_and_drops() {
        let (tables, dropped) = plan_tables(&info_schema_rows(), None);
        let s = summarize(&tables, &dropped, "dbo");
        assert_eq!(s["tableCount"], 2);
        // 2 (dimstore) + 3 (factsales) mappable columns.
        assert_eq!(s["columnCount"], 5);
        assert_eq!(s["schema"], "dbo");
        assert_eq!(s["droppedColumns"].as_array().unwrap().len(), 1);
    }
}
