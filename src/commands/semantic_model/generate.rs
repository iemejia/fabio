//! `semantic-model generate` — generate a Direct Lake semantic model from a
//! lakehouse or warehouse, the way the Fabric portal's "New semantic model" does.
//!
//! This is the client-side, definition-backed alternative to API-native
//! `semantic-model create --one-lake-table`. It remains useful for schema-aware
//! generation (including schema-less lakehouses) and explicit storage binding:
//!
//! 1. Resolve the source's SQL analytics endpoint `(server, database)`.
//! 2. Read `INFORMATION_SCHEMA.COLUMNS` for the base tables (optionally filtered
//!    to `--tables`).
//! 3. Map each SQL type to a Power BI data type; columns whose type can't be
//!    mapped are DROPPED (matching Fabric's own sync behavior).
//! 4. Synthesize a multi-file TMDL model with SQL-endpoint or `OneLake` binding.
//! 5. Create the model and request `Full` framing unless `--no-refresh` is set.
//!    Refresh submission is not proof of completion or DAX readiness.
//!
//! Relationships and measures are NOT inferred (same as the portal — you add
//! them afterward with `update-definition`). Native creation is a separate mode,
//! not a silent fallback that changes this generator's source/binding semantics.

use std::collections::HashSet;
use std::fmt::Write as _;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;

use super::refresh::{self, Submission};
use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{execute_sql_rows, parse_connection_string};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

/// The Direct Lake storage mode of the generated model.
///
/// Both modes read the source schema over the SQL analytics endpoint (like the
/// Fabric portal / Power BI Desktop). They differ ONLY in the M expression that
/// binds the `directLake` partitions to storage:
///
/// * [`StorageMode::Sql`] — binds through the SQL analytics endpoint with
///   `Sql.Database(server, sqlEndpointId)`. Supports SQL-endpoint security and
///   `DirectQuery` fallback, but a single source only.
/// * [`StorageMode::Onelake`] — binds directly to `OneLake` Delta with
///   `AzureStorage.DataLake("https://onelake.dfs.fabric.microsoft.com/{ws}/{item}")`.
///   The recommended mode (GA March 2026): `OneLake` security, more modeling
///   features, faster queries, and tables from multiple sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum StorageMode {
    /// Direct Lake on SQL — bind via the SQL analytics endpoint (`Sql.Database`).
    #[default]
    Sql,
    /// Direct Lake on `OneLake` — bind directly to `OneLake` Delta (`AzureStorage.DataLake`).
    Onelake,
}

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

/// The resolved source of a `generate` — everything needed to BOTH read the
/// schema (over SQL) AND bind the partitions (SQL endpoint or `OneLake` path).
#[derive(Debug, Clone)]
struct ResolvedSource {
    /// SQL analytics endpoint host (used to read `INFORMATION_SCHEMA`).
    server: String,
    /// SQL catalog used to read the schema over TDS (the item display name).
    database: String,
    /// SQL analytics endpoint item id — the `Sql.Database(...)` catalog for
    /// Direct Lake on SQL.
    catalog: String,
    /// The lakehouse/warehouse item id — the `{item}` in the `OneLake` `DFS` path
    /// for Direct Lake on `OneLake`.
    item_id: String,
    /// Whether the source exposes SQL schemas. Warehouses always do; a lakehouse
    /// does only when schema-enabled (`properties.defaultSchema` is present).
    /// Direct Lake on `OneLake` drops `schemaName` for schema-less lakehouses.
    schema_enabled: bool,
    /// `{ "lakehouse": id }` or `{ "warehouse": id }` for output.
    source: Value,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn generate(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: Option<&str>,
    warehouse: Option<&str>,
    name: &str,
    tables: Option<&str>,
    schema: &str,
    storage_mode: StorageMode,
    no_refresh: bool,
    check_source: bool,
    source_limits: &super::source::Limits,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    source_limits.validate_creation(check_source)?;
    if check_source {
        crate::commands::diagnostics::uuid_target(workspace, "--workspace")?;
        if let Some(id) = lakehouse {
            crate::commands::diagnostics::uuid_target(id, "--lakehouse")?;
        }
        if let Some(id) = warehouse {
            crate::commands::diagnostics::uuid_target(id, "--warehouse")?;
        }
    }
    if check_source && client.is_readonly() && !cli.dry_run {
        return Err(FabioError::with_hint(ErrorCode::ReadonlyMode,"Generated-model creation is blocked in readonly mode.","Run semantic-model check-source for read-only inspection; generator --dry-run retains its existing SQL schema discovery without creating a model.").into());
    }
    // Resolve the schema source (exactly one of --lakehouse / --warehouse).
    // `database` is the SQL catalog used to READ the schema over TDS (the display
    // name works); `catalog` is the SQL analytics endpoint item id that the
    // portal embeds in the Direct Lake `Sql.Database(...)` expression; `item_id`
    // is the lakehouse/warehouse item id used in the OneLake DFS path.
    let src = resolve_source_sql(client, workspace, lakehouse, warehouse).await?;

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
    let (_cols, rows) = execute_sql_rows(client, &src.server, &src.database, &sql).await?;
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

    // Direct Lake on OneLake drops `schemaName` from partitions for a schema-less
    // lakehouse (per the Fabric migration guidance); SQL mode always keeps it.
    let emit_schema = match storage_mode {
        StorageMode::Sql => true,
        StorageMode::Onelake => src.schema_enabled,
    };

    // Byte-for-byte the shape the Fabric portal's "New semantic model" produces:
    // a TMDL definition folder (model/database/expressions + one file per table).
    let parts = build_tmdl_parts(
        &src,
        workspace,
        storage_mode,
        schema,
        emit_schema,
        &gen_tables,
    );
    let summary = summarize(&gen_tables, &dropped, schema);
    let storage_label = storage_mode_label(storage_mode);
    let sources = if check_source {
        if gen_tables.len() > 100 {
            return Err(FabioError::with_hint(ErrorCode::InvalidInput,"Checked generation supports at most 100 selected source tables.","Use --tables to select an explicit bounded subset. No tables are dropped from the requested model automatically.").into());
        }
        let urls: Vec<_> = gen_tables
            .iter()
            .map(|table| {
                let table = urlencoding::encode(&table.name);
                let path = if src.schema_enabled {
                    format!("{}/{}", urlencoding::encode(schema), table)
                } else {
                    table.into_owned()
                };
                format!(
                    "https://onelake.dfs.fabric.microsoft.com/{workspace}/{}/Tables/{path}",
                    src.item_id
                )
            })
            .collect();
        Some(super::source::selected(&urls)?)
    } else {
        None
    };

    if output::dry_run_guard(
        cli,
        "semantic-model generate",
        &serde_json::json!({
            "workspace": workspace,
            "name": name,
            "source": src.source,
            "storageMode": storage_label,
            "summary": summary,
            "sourceCheck":sources.as_ref().map(|sources|super::source::preview(sources,source_limits,super::source::Mode::Generated)),
        }),
    ) {
        return Ok(());
    }

    let source_report = if let Some(sources) = &sources {
        Some(
            super::source::required(
                cli,
                client,
                sources,
                source_limits,
                super::source::Mode::Generated,
            )
            .await?,
        )
    } else {
        None
    };

    let (id, framing, note) = create_and_frame(
        client,
        workspace,
        name,
        &parts,
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
            "source": src.source,
            "storageMode": storage_label,
            "framed": framing.acknowledged(),
            "refreshId":framing.refresh_id,
            "framing":framing,
            "summary": summary,
            "note": note,
            "sourceCheck":source_report,
        }),
        "status",
    );
    Ok(())
}

/// The human/JSON label for a storage mode (matches the Fabric UI terminology).
const fn storage_mode_label(mode: StorageMode) -> &'static str {
    match mode {
        StorageMode::Sql => "directLakeOnSql",
        StorageMode::Onelake => "directLakeOnOneLake",
    }
}

/// Create the semantic model from the synthesized TMDL definition parts and
/// frame it with a `Full` refresh (unless `no_refresh`). Returns
/// `(id, framing receipt, note)`. Submission is NON-FATAL so a model is not lost
/// when acknowledgement/correlation is unavailable. An unknown transport
/// outcome must never invite an automatic resubmission.
async fn create_and_frame(
    client: &FabricClient,
    workspace: &str,
    name: &str,
    parts: &[(String, String)],
    description: Option<&str>,
    sensitivity_label: Option<&str>,
    no_refresh: bool,
) -> Result<(String, Submission, String)> {
    let definition_parts: Vec<Value> = parts
        .iter()
        .map(|(path, content)| {
            serde_json::json!({
                "path": path,
                "payload": BASE64.encode(content.as_bytes()),
                "payloadType": "InlineBase64"
            })
        })
        .collect();
    let mut body = serde_json::json!({
        "displayName": name,
        "definition": { "parts": definition_parts }
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

    let framing = if no_refresh {
        Submission::skipped()
    } else {
        match refresh::submit(client, workspace, &id, &serde_json::json!({"type":"Full"})).await {
            Ok(submission) => submission,
            Err(error) => Submission::failed(&error),
        }
    };
    let note = framing_note(&framing);
    Ok((id, framing, note))
}

fn framing_note(framing: &Submission) -> String {
    let outcome = match framing.state {
        "skipped" => "Framing was deliberately skipped (--no-refresh); submit only when intended.",
        "accepted" | "acknowledged" => {
            "Framing was acknowledged, not completed. Use refreshId only when correlation is verified, then perform bounded readiness checks. Never resubmit accepted work or rely on a fixed sleep."
        }
        "rejected" => {
            "Framing submission was rejected. Inspect framing.error before deciding on a new submission; no automatic retry was made."
        }
        _ => {
            "Framing outcome is unknown and may already be running. Do not retry automatically; inspect existing refresh evidence and establish correlation first."
        }
    };
    format!(
        "Direct Lake model created; relationships/measures are not inferred. {outcome} The legacy framed flag records acknowledgement only, not completion; false does not prove an unknown submission was rejected."
    )
}

/// Resolve the source item to a [`ResolvedSource`].
///
/// Exactly one of `--lakehouse` / `--warehouse` must be set.
/// * `server` — the SQL analytics endpoint host.
/// * `database` — the SQL catalog used to READ the schema over TDS (the item's
///   display name works reliably for this).
/// * `catalog` — the **SQL analytics endpoint item id**, which is what the
///   Fabric portal embeds as the second argument of `Sql.Database(...)` in the
///   Direct Lake on SQL expression (a GUID is rename-stable; the display name is
///   not). For a lakehouse it is `properties.sqlEndpointProperties.id`; for a
///   warehouse the warehouse item is itself the SQL endpoint, so it is `wh`.
/// * `item_id` — the lakehouse/warehouse item id (the `{item}` in the `OneLake`
///   `DFS` path for Direct Lake on `OneLake`).
/// * `schema_enabled` — warehouses always expose schemas; a lakehouse does only
///   when schema-enabled (`properties.defaultSchema` is present).
async fn resolve_source_sql(
    client: &FabricClient,
    workspace: &str,
    lakehouse: Option<&str>,
    warehouse: Option<&str>,
) -> Result<ResolvedSource> {
    match (lakehouse, warehouse) {
        (Some(_), Some(_)) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Provide exactly one source: --lakehouse OR --warehouse.".to_string(),
            "e.g. --lakehouse <LH_ID>  or  --warehouse <WH_ID>".to_string(),
        )
        .into()),
        (Some(lh), None) => {
            let data = client
                .get(&format!("/workspaces/{workspace}/lakehouses/{lh}"))
                .await
                .map_err(|e| enrich_forbidden(e, "lakehouse", "Viewer"))?;
            let props = data.get("properties");
            let sep = props.and_then(|p| p.get("sqlEndpointProperties"));
            let conn = sep
                .and_then(|s| s.get("connectionString"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    FabioError::with_hint(
                        ErrorCode::NotFound,
                        "Lakehouse SQL endpoint not available.",
                        "Wait for provisioning to complete, then retry.",
                    )
                })?;
            let (server, _parsed_db) = parse_connection_string(conn);
            let database = data
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            // The SQL analytics endpoint item id is the portal's Sql.Database catalog.
            let catalog = sep
                .and_then(|s| s.get("id"))
                .and_then(Value::as_str)
                .unwrap_or(lh)
                .to_string();
            // A schema-enabled lakehouse exposes `properties.defaultSchema`.
            let schema_enabled = props
                .and_then(|p| p.get("defaultSchema"))
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty());
            Ok(ResolvedSource {
                server,
                database,
                catalog,
                item_id: lh.to_string(),
                schema_enabled,
                source: serde_json::json!({ "lakehouse": lh }),
            })
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
            // A warehouse is itself the SQL endpoint, so its item id is the catalog;
            // warehouses always expose SQL schemas (dbo, etc.).
            Ok(ResolvedSource {
                server,
                database,
                catalog: wh.to_string(),
                item_id: wh.to_string(),
                schema_enabled: true,
                source: serde_json::json!({ "warehouse": wh }),
            })
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

/// The `definition.pbism` for a TMDL semantic model — byte-identical to what the
/// Fabric portal emits (`version: "4.2"`, empty `settings`).
fn pbism() -> Value {
    serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/semanticModel/definitionProperties/1.0.0/schema.json",
        "version": "4.2",
        "settings": {}
    })
}

/// Build the full TMDL definition parts — the exact multi-file shape the Fabric
/// portal's "New semantic model" produces for a Direct Lake model:
///
/// * `definition.pbism` — version 4.2
/// * `definition/model.tmdl` — model settings + `ref table` lines
/// * `definition/database.tmdl` — `compatibilityLevel: 1604`
/// * `definition/expressions.tmdl` — the `DatabaseQuery` M expression (either
///   `Sql.Database(...)` for SQL mode or `AzureStorage.DataLake(...)` for `OneLake`)
/// * `definition/tables/<name>.tmdl` — one per table (columns + directLake entity partition)
///
/// Returns `(path, content)` pairs; the caller base64-encodes them into
/// `definition.parts`.
fn build_tmdl_parts(
    src: &ResolvedSource,
    workspace: &str,
    mode: StorageMode,
    schema: &str,
    emit_schema: bool,
    tables: &[GenTable],
) -> Vec<(String, String)> {
    let mut parts = vec![
        ("definition.pbism".to_string(), pbism().to_string()),
        ("definition/model.tmdl".to_string(), tmdl_model(tables)),
        ("definition/database.tmdl".to_string(), tmdl_database()),
        (
            "definition/expressions.tmdl".to_string(),
            tmdl_expression(src, workspace, mode),
        ),
    ];
    for t in tables {
        parts.push((
            format!("definition/tables/{}.tmdl", t.name),
            tmdl_table(t, schema, emit_schema),
        ));
    }
    parts
}

/// `definition/model.tmdl` — Direct Lake model settings + one `ref table` per table.
fn tmdl_model(tables: &[GenTable]) -> String {
    let mut s = String::from(
        "model Model\n\tdefaultMode: directLake\n\tculture: en-US\n\tdefaultPowerBIDataSourceVersion: powerBI_V3\n\n",
    );
    for t in tables {
        let _ = writeln!(s, "ref table {}", t.name);
    }
    s
}

/// `definition/database.tmdl` — the compatibility level lives here (not in model.tmdl).
fn tmdl_database() -> String {
    "database\n\tcompatibilityLevel: 1604\n".to_string()
}

/// `definition/expressions.tmdl` — the shared `DatabaseQuery` M expression that
/// binds the Direct Lake partitions to storage.
///
/// * [`StorageMode::Sql`] → `Sql.Database(server, sqlEndpointId)`. `catalog` is
///   the SQL endpoint item id (the portal's exact convention), NOT the display name.
/// * [`StorageMode::Onelake`] → `AzureStorage.DataLake(onelakeDfsPath)`, where the
///   path is `https://onelake.dfs.fabric.microsoft.com/{workspace}/{item}` — the
///   lakehouse/warehouse item root (Fabric's Direct Lake on `OneLake` convention).
fn tmdl_expression(src: &ResolvedSource, workspace: &str, mode: StorageMode) -> String {
    match mode {
        StorageMode::Sql => format!(
            "expression DatabaseQuery =\n\t\tlet\n\t\t    database = Sql.Database(\"{server}\", \"{catalog}\")\n\t\tin\n\t\t    database\n",
            server = src.server,
            catalog = src.catalog,
        ),
        StorageMode::Onelake => {
            let path = format!(
                "https://onelake.dfs.fabric.microsoft.com/{workspace}/{item}",
                item = src.item_id,
            );
            format!(
                "expression DatabaseQuery =\n\t\tlet\n\t\t    Source = AzureStorage.DataLake(\"{path}\")\n\t\tin\n\t\t    Source\n"
            )
        }
    }
}

/// `definition/tables/<name>.tmdl` — columns (`dataType` + `sourceColumn`, exactly
/// like the portal — no `summarizeBy`) and a `directLake` entity partition. The
/// `entityName` is the physical (SQL/Delta) table name. `schemaName` is emitted
/// unless `emit_schema` is false (a schema-less lakehouse in `OneLake` mode).
fn tmdl_table(t: &GenTable, schema: &str, emit_schema: bool) -> String {
    let mut s = format!("table {}\n", t.name);
    for c in &t.columns {
        let _ = write!(
            s,
            "\n\tcolumn {name}\n\t\tdataType: {dt}\n\t\tsourceColumn: {name}\n",
            name = c.name,
            dt = c.data_type
        );
    }
    let schema_line = if emit_schema {
        format!("\n\t\t\tschemaName: {schema}")
    } else {
        String::new()
    };
    let _ = write!(
        s,
        "\n\tpartition {name} = entity\n\t\tmode: directLake\n\t\tsource\n\t\t\tentityName: {name}{schema_line}\n\t\t\texpressionSource: DatabaseQuery\n",
        name = t.name
    );
    s
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

    #[test]
    fn framing_notes_do_not_equate_an_unknown_submission_with_rejection() {
        let error = anyhow::Error::from(FabioError::new(ErrorCode::NetworkError, "private"));
        let receipt = Submission::failed(&error);
        assert_eq!(receipt.state, "unknown");
        let note = framing_note(&receipt);
        assert!(note.contains("Do not retry automatically"));
        assert!(note.contains("false does not prove"));
        assert!(!note.contains("private"));
        let note = framing_note(&Submission::skipped());
        assert!(note.contains("deliberately skipped"));
    }
    /// A minimal SQL-mode [`ResolvedSource`] for TMDL-shape tests.
    fn sql_source(server: &str, catalog: &str) -> ResolvedSource {
        ResolvedSource {
            server: server.to_string(),
            database: "SourceDb".to_string(),
            catalog: catalog.to_string(),
            item_id: catalog.to_string(),
            schema_enabled: true,
            source: json!({ "lakehouse": "lh" }),
        }
    }

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
        assert_eq!(tables.len(), 0);
        assert_eq!(dropped.len(), 1);
    }

    #[test]
    fn tmdl_parts_match_portal_shape() {
        let (tables, _) = plan_tables(&info_schema_rows(), None);
        let parts: std::collections::HashMap<String, String> = build_tmdl_parts(
            &sql_source("srv.datawarehouse.fabric.microsoft.com", "sqlendpointid"),
            "wsid",
            StorageMode::Sql,
            "dbo",
            true,
            &tables,
        )
        .into_iter()
        .collect();

        // definition.pbism — portal emits version 4.2 + empty settings.
        let pbism: Value = serde_json::from_str(&parts["definition.pbism"]).unwrap();
        assert_eq!(pbism["version"], "4.2");
        assert!(pbism["settings"].is_object());

        // model.tmdl — Direct Lake settings + a ref table per table.
        let model = &parts["definition/model.tmdl"];
        assert!(model.contains("model Model"));
        assert!(model.contains("\tdefaultMode: directLake"));
        assert!(model.contains("\tdefaultPowerBIDataSourceVersion: powerBI_V3"));
        assert!(model.contains("ref table dimstore"));
        assert!(model.contains("ref table factsales"));

        // database.tmdl — compatibilityLevel lives here.
        assert_eq!(
            parts["definition/database.tmdl"],
            "database\n\tcompatibilityLevel: 1604\n"
        );

        // expressions.tmdl — Sql.Database with the SQL endpoint id (not a name).
        let expr = &parts["definition/expressions.tmdl"];
        assert!(expr.contains(
            "Sql.Database(\"srv.datawarehouse.fabric.microsoft.com\", \"sqlendpointid\")"
        ));

        // A per-table file with lean columns (dataType + sourceColumn, NO summarizeBy)
        // and a directLake entity partition.
        let ds = &parts["definition/tables/dimstore.tmdl"];
        assert!(ds.starts_with("table dimstore\n"));
        assert!(ds.contains("\tcolumn StoreId\n\t\tdataType: int64\n\t\tsourceColumn: StoreId\n"));
        assert!(!ds.contains("summarizeBy"), "portal omits summarizeBy");
        assert!(ds.contains("\tpartition dimstore = entity\n\t\tmode: directLake\n\t\tsource\n\t\t\tentityName: dimstore\n\t\t\tschemaName: dbo\n\t\t\texpressionSource: DatabaseQuery\n"));
    }

    #[test]
    fn tmdl_parts_one_table_file_per_table() {
        let (tables, _) = plan_tables(&info_schema_rows(), None);
        let paths: Vec<String> = build_tmdl_parts(
            &sql_source("s", "c"),
            "ws",
            StorageMode::Sql,
            "dbo",
            true,
            &tables,
        )
        .into_iter()
        .map(|(p, _)| p)
        .collect();
        assert!(paths.contains(&"definition/tables/dimstore.tmdl".to_string()));
        assert!(paths.contains(&"definition/tables/factsales.tmdl".to_string()));
        assert!(paths.contains(&"definition/model.tmdl".to_string()));
        assert!(paths.contains(&"definition/database.tmdl".to_string()));
        assert!(paths.contains(&"definition/expressions.tmdl".to_string()));
        assert!(paths.contains(&"definition.pbism".to_string()));
    }

    /// Direct Lake on `OneLake` emits an `AzureStorage.DataLake` expression pointing
    /// at the `OneLake` `DFS` item path, and — for a schema-less lakehouse — drops
    /// `schemaName` from every partition (while keeping `entityName` +
    /// `expressionSource`). This matches Fabric's DL-on-SQL → DL-on-`OneLake`
    /// migration guidance.
    #[test]
    fn onelake_schemaless_uses_azurestorage_and_drops_schema() {
        let (tables, _) = plan_tables(&info_schema_rows(), None);
        let mut src = sql_source("srv.fabric.com", "sqlendpointid");
        src.item_id = "lakehouseid".to_string();
        src.schema_enabled = false;
        let parts: std::collections::HashMap<String, String> =
            build_tmdl_parts(&src, "wsid", StorageMode::Onelake, "dbo", false, &tables)
                .into_iter()
                .collect();

        // expressions.tmdl — AzureStorage.DataLake at the OneLake item root, NOT Sql.Database.
        let expr = &parts["definition/expressions.tmdl"];
        assert!(
            expr.contains(
                "AzureStorage.DataLake(\"https://onelake.dfs.fabric.microsoft.com/wsid/lakehouseid\")"
            ),
            "expr: {expr}"
        );
        assert!(!expr.contains("Sql.Database"));

        // Per-table partition keeps entityName + expressionSource but drops schemaName.
        let ds = &parts["definition/tables/dimstore.tmdl"];
        assert!(ds.contains("\t\t\tentityName: dimstore\n"));
        assert!(ds.contains("\t\t\texpressionSource: DatabaseQuery\n"));
        assert!(!ds.contains("schemaName"), "schema-less lakehouse: {ds}");
    }

    /// Direct Lake on `OneLake` against a schema-enabled source (warehouse or
    /// schema-enabled lakehouse) keeps `schemaName` in every partition.
    #[test]
    fn onelake_schema_enabled_keeps_schema() {
        let (tables, _) = plan_tables(&info_schema_rows(), None);
        let mut src = sql_source("srv.fabric.com", "sqlendpointid");
        src.item_id = "whid".to_string();
        src.schema_enabled = true;
        let parts: std::collections::HashMap<String, String> =
            build_tmdl_parts(&src, "wsid", StorageMode::Onelake, "sales", true, &tables)
                .into_iter()
                .collect();
        let expr = &parts["definition/expressions.tmdl"];
        assert!(expr.contains(
            "AzureStorage.DataLake(\"https://onelake.dfs.fabric.microsoft.com/wsid/whid\")"
        ));
        let ds = &parts["definition/tables/dimstore.tmdl"];
        assert!(ds.contains("\t\t\tschemaName: sales\n"), "ds: {ds}");
        assert!(ds.contains("\t\t\texpressionSource: DatabaseQuery\n"));
    }

    #[test]
    fn storage_mode_labels_match_fabric_terminology() {
        assert_eq!(storage_mode_label(StorageMode::Sql), "directLakeOnSql");
        assert_eq!(
            storage_mode_label(StorageMode::Onelake),
            "directLakeOnOneLake"
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
