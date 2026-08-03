//! KQL query execution, schema discovery, ingestion, diagnostics, and deeplink commands.

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::kql_utils;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

// ─── Query ───────────────────────────────────────────────────────────────────

pub(super) async fn query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    kql: Option<&str>,
    query_uri_override: Option<&str>,
) -> Result<()> {
    // Resolve KQL text: --kql flag, @file prefix, or stdin
    let kql_text = kql_utils::resolve_kql_input(kql)?;

    // Resolve Query URI and database name
    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;

    // Execute KQL query
    let (rows, columns) = kql_utils::execute_kql(client, &kusto_uri, &db_name, &kql_text).await?;

    // Render output
    kql_utils::render_kql_results(cli, &rows, &columns);

    Ok(())
}

// ─── Schema Discovery ────────────────────────────────────────────────────────

pub(super) async fn list_entities(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    entity_type: Option<&str>,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;

    // Use .show database schema to get all entities at once
    let kql = format!(".show database ['{db_name}'] schema as json");
    let (rows, _columns) = kql_utils::execute_kql(client, &kusto_uri, &db_name, &kql).await?;

    // The schema-as-json command returns a single row with a "DatabaseSchema" column
    let schema_json = rows
        .first()
        .and_then(|r| r.get("DatabaseSchema").or_else(|| r.get("Schema")))
        .and_then(Value::as_str)
        .unwrap_or("{}");

    let raw_schema: Value = serde_json::from_str(schema_json).unwrap_or_default();

    // The schema is nested: {"Databases": {"<db-id>": {"Tables": {...}, ...}}}
    // Extract the first (and typically only) database entry
    let schema = raw_schema
        .get("Databases")
        .and_then(Value::as_object)
        .and_then(|dbs| dbs.values().next())
        .unwrap_or(&raw_schema);

    let mut entities: Vec<Value> = Vec::new();

    // Extract tables
    if entity_type.is_none_or(|t| t == "table")
        && let Some(tables) = schema.get("Tables").and_then(Value::as_object)
    {
        for (name, _info) in tables {
            entities.push(serde_json::json!({
                "name": name,
                "type": "table",
            }));
        }
    }

    // Extract materialized views
    if entity_type.is_none_or(|t| t == "materialized-view")
        && let Some(views) = schema.get("MaterializedViews").and_then(Value::as_object)
    {
        for (name, _info) in views {
            entities.push(serde_json::json!({
                "name": name,
                "type": "materialized-view",
            }));
        }
    }

    // Extract external tables
    if entity_type.is_none_or(|t| t == "external-table")
        && let Some(ext) = schema.get("ExternalTables").and_then(Value::as_object)
    {
        for (name, _info) in ext {
            entities.push(serde_json::json!({
                "name": name,
                "type": "external-table",
            }));
        }
    }

    // Extract functions
    if entity_type.is_none_or(|t| t == "function")
        && let Some(funcs) = schema.get("Functions").and_then(Value::as_object)
    {
        for (name, _info) in funcs {
            entities.push(serde_json::json!({
                "name": name,
                "type": "function",
            }));
        }
    }

    output::render_list(cli, &entities, &["name", "type"], &["name", "type"], "name");
    Ok(())
}

pub(super) async fn describe(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;

    // .show database schema returns all columns in all tables
    let kql = format!(
        ".show database ['{db_name}'] schema \
         | project TableName, ColumnName, ColumnType, Folder"
    );
    let (rows, columns) = kql_utils::execute_kql(client, &kusto_uri, &db_name, &kql).await?;

    kql_utils::render_kql_results(cli, &rows, &columns);
    Ok(())
}

pub(super) async fn describe_entity(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    entity_name: &str,
    entity_type: &str,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;

    let kql = match entity_type {
        "table" => format!(".show table ['{entity_name}'] schema as json"),
        "materialized-view" => {
            format!(".show materialized-view ['{entity_name}'] schema as json")
        }
        "external-table" => {
            format!(".show external table ['{entity_name}'] schema as json")
        }
        "function" => format!(".show function ['{entity_name}']"),
        other => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Unknown entity type: '{other}'"),
                "Valid entity types: table, materialized-view, external-table, function"
                    .to_string(),
            )
            .into());
        }
    };

    let (rows, columns) = kql_utils::execute_kql(client, &kusto_uri, &db_name, &kql).await?;

    // For schema-as-json commands, try to parse and render the schema nicely
    if entity_type != "function"
        && let Some(schema_str) = rows
            .first()
            .and_then(|r| r.get("Schema").or_else(|| r.get("DatabaseSchema")))
            .and_then(Value::as_str)
        && let Ok(schema) = serde_json::from_str::<Value>(schema_str)
    {
        output::render_object(cli, &schema, "name");
        return Ok(());
    }

    // Fallback: render raw rows
    kql_utils::render_kql_results(cli, &rows, &columns);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn sample(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    entity_name: &str,
    count: u32,
    entity_type: &str,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;

    let kql = match entity_type {
        "table" | "function" => format!("['{entity_name}'] | take {count}"),
        "materialized-view" => {
            format!("materialized_view('{entity_name}') | take {count}")
        }
        "external-table" => {
            format!("external_table('{entity_name}') | take {count}")
        }
        other => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Unknown entity type: '{other}'"),
                "Valid entity types: table, materialized-view, external-table, function"
                    .to_string(),
            )
            .into());
        }
    };

    let (rows, columns) = kql_utils::execute_kql(client, &kusto_uri, &db_name, &kql).await?;

    kql_utils::render_kql_results(cli, &rows, &columns);
    Ok(())
}

// ─── Ingestion & Diagnostics ─────────────────────────────────────────────────

/// Options describing a storage-backed ingestion source (`OneLake` / ADLS Gen2).
///
/// When `source_path` is `None`, ingestion falls back to inline mode using the
/// `data` argument of [`ingest`].
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct IngestSource<'a> {
    pub source_path: Option<&'a str>,
    pub source_lakehouse: Option<&'a str>,
    pub source_workspace: Option<&'a str>,
    pub format: Option<&'a str>,
    pub ignore_first_record: bool,
}

/// Normalize a user-supplied format string to the Kusto ingestion format token.
fn normalize_kusto_format(fmt: &str) -> Result<&'static str> {
    match fmt.to_lowercase().as_str() {
        "csv" => Ok("csv"),
        "tsv" => Ok("tsv"),
        "scsv" => Ok("scsv"),
        "psv" => Ok("psv"),
        "json" | "singlejson" => Ok("singlejson"),
        "multijson" | "multi-json" => Ok("multijson"),
        "parquet" => Ok("parquet"),
        "orc" => Ok("orc"),
        "avro" => Ok("avro"),
        "raw" | "txt" => Ok("txt"),
        other => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Unsupported ingestion format: {other}"),
            "Supported formats: Csv, Tsv, Json, MultiJson, Parquet, Avro, Orc.".to_string(),
        )
        .into()),
    }
}

/// Build the storage connection string that Kusto reads from.
///
/// A `OneLake` lakehouse path is resolved to a `onelake.blob.fabric.microsoft.com`
/// URL; a full HTTPS URL is validated against the trusted-domain allowlist. Both
/// are suffixed with `;impersonate` so the engine reads storage using the caller's
/// identity (no SAS token needed).
fn build_storage_connection(
    client: &FabricClient,
    workspace: &str,
    src: &IngestSource,
) -> Result<String> {
    let raw = src
        .source_path
        .expect("build_storage_connection requires source_path");

    let url = if raw.starts_with("https://") || raw.starts_with("http://") {
        crate::client::validate_trusted_url(raw, "--source-path")?;
        raw.to_string()
    } else if let Some(lakehouse) = src.source_lakehouse {
        let ws = src.source_workspace.unwrap_or(workspace);
        let path = raw.trim_start_matches('/');
        client.onelake_blob_item_url(ws, &format!("{lakehouse}/{path}"))
    } else {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--source-path is a relative path but --source-lakehouse was not provided.".to_string(),
            "Either pass a full https:// OneLake/ADLS URL, or add --source-lakehouse <id> so the \
             lakehouse-relative path can be resolved."
                .to_string(),
        )
        .into());
    };

    // `h'...'` obfuscates the connection string in Kusto logs; `;impersonate`
    // reads storage with the submitting principal's identity.
    Ok(format!("h'{url};impersonate'"))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn ingest(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    data: Option<&str>,
    src: IngestSource<'_>,
    query_uri_override: Option<&str>,
) -> Result<()> {
    // Storage-backed ingestion (OneLake / ADLS) for large files.
    if src.source_path.is_some() {
        return ingest_from_storage(cli, client, workspace, id, table, &src, query_uri_override)
            .await;
    }

    // Inline ingestion (small payloads, ~4 MB cap).
    let csv_data = kql_utils::resolve_kql_input(data)?;

    // Dry-run guard: ingestion is a mutation
    if output::dry_run_guard(
        cli,
        "kql-database ingest",
        &serde_json::json!({
            "table": table,
            "data_size_bytes": csv_data.len(),
        }),
    ) {
        return Ok(());
    }

    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;

    // Construct the inline ingestion command
    let kql = format!(".ingest inline into table ['{table}'] <|\n{csv_data}");

    let (rows, columns) = kql_utils::execute_kql(client, &kusto_uri, &db_name, &kql).await?;

    if rows.is_empty() {
        let obj = serde_json::json!({
            "status": "ingested",
            "table": table,
            "data_size_bytes": csv_data.len(),
        });
        output::render_object(cli, &obj, "status");
    } else {
        kql_utils::render_kql_results(cli, &rows, &columns);
    }
    Ok(())
}

/// Build the `.ingest into` management command for a storage-backed source.
///
/// Pure/string-only so it is unit-testable without a live client.
fn build_ingest_command(
    table: &str,
    connection: &str,
    format: &str,
    ignore_first_record: bool,
) -> String {
    let mut props: Vec<String> = vec![format!("format='{format}'")];
    if ignore_first_record && matches!(format, "csv" | "tsv" | "scsv" | "psv") {
        props.push("ignoreFirstRecord=true".to_string());
    }
    format!(
        ".ingest into table ['{table}'] ({connection}) with ({})",
        props.join(", ")
    )
}

/// Ingest from a storage source (`OneLake` blob / ADLS Gen2) via `.ingest into`.
async fn ingest_from_storage(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    src: &IngestSource<'_>,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let connection = build_storage_connection(client, workspace, src)?;
    let format = normalize_kusto_format(src.format.unwrap_or("csv"))?;

    if output::dry_run_guard(
        cli,
        "kql-database ingest",
        &serde_json::json!({
            "table": table,
            "source_path": src.source_path,
            "format": format,
            "mode": "storage",
        }),
    ) {
        return Ok(());
    }

    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;

    // `.ingest into` pulls the referenced blob synchronously and returns the
    // resulting extent(s). Suitable for one-shot loads of moderately large files.
    let kql = build_ingest_command(table, &connection, format, src.ignore_first_record);

    let (rows, columns) = kql_utils::execute_kql(client, &kusto_uri, &db_name, &kql).await?;

    if rows.is_empty() {
        let obj = serde_json::json!({
            "status": "ingested",
            "table": table,
            "format": format,
            "mode": "storage",
        });
        output::render_object(cli, &obj, "status");
    } else {
        kql_utils::render_kql_results(cli, &rows, &columns);
    }
    Ok(())
}

pub(super) async fn show_queryplan(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    kql: Option<&str>,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let kql_text = kql_utils::resolve_kql_input(kql)?;

    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;

    // Use .show queryplan to get the execution plan
    let plan_kql = format!(".show queryplan <| {kql_text}");

    let (rows, columns) = kql_utils::execute_kql(client, &kusto_uri, &db_name, &plan_kql).await?;

    kql_utils::render_kql_results(cli, &rows, &columns);
    Ok(())
}

pub(super) async fn diagnostics(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;

    // Run multiple diagnostic commands; each section is independent
    let sections = [
        ("capacity", ".show capacity"),
        ("cluster", ".show cluster"),
        (
            "principal_roles",
            ".show database principals | project Role, PrincipalType, PrincipalDisplayName",
        ),
        ("diagnostics", ".show diagnostics"),
        (
            "ingestion_failures",
            ".show ingestion failures \
             | where IngestionSourcePath != '' \
             | where Timestamp > ago(24h) \
             | summarize Count=count() by Table, FailureKind \
             | order by Count desc",
        ),
    ];

    let mut result = serde_json::Map::with_capacity(sections.len());

    // Execute all diagnostic queries concurrently (independent operations)
    let mut join_set = tokio::task::JoinSet::new();
    for (name, kql) in &sections {
        let kusto_uri = kusto_uri.clone();
        let db_name = db_name.clone();
        let kql = (*kql).to_string();
        let name = *name;
        let client = client.clone();
        join_set.spawn(async move {
            let res = kql_utils::execute_kql(&client, &kusto_uri, &db_name, &kql).await;
            (name, res)
        });
    }

    while let Some(outcome) = join_set.join_next().await {
        if let Ok((name, res)) = outcome {
            match res {
                Ok((rows, _columns)) => {
                    result.insert(name.to_string(), Value::Array(rows));
                }
                Err(e) => {
                    result.insert(
                        name.to_string(),
                        serde_json::json!({"error": e.to_string()}),
                    );
                }
            }
        }
    }

    let obj = Value::Object(result);
    output::render_object(cli, &obj, "capacity");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn deeplink(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    kql: &str,
    style: &str,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;

    // Auto-detect cluster type from URI pattern
    let detected_style = if style != "auto" {
        style.to_string()
    } else if kusto_uri.contains(".kusto.fabric.microsoft.com") {
        "fabric".to_string()
    } else {
        "adx".to_string()
    };

    // URL-encode the query
    let encoded_query = urlencoding::encode(kql);

    let url = match detected_style.as_str() {
        "fabric" => {
            // Fabric portal query workbench
            format!(
                "https://app.fabric.microsoft.com/groups/{workspace}/kqlDatabases/{id}?\
                 query={encoded_query}&database={db_name}"
            )
        }
        "adx" => {
            // Azure Data Explorer Web Explorer
            format!(
                "https://dataexplorer.azure.com/clusters/{kusto_uri}/databases/{db_name}\
                 ?query={encoded_query}"
            )
        }
        other => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Unknown deeplink style: '{other}'"),
                "Valid styles: auto, fabric, adx".to_string(),
            )
            .into());
        }
    };

    let obj = serde_json::json!({
        "url": url,
        "style": detected_style,
        "database": db_name,
    });
    output::render_object(cli, &obj, "url");
    Ok(())
}

// ─── Query Monitoring ────────────────────────────────────────────────────────

/// Show currently running queries on the KQL database.
pub(super) async fn queries_running(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;
    let (rows, columns) =
        kql_utils::execute_kql(client, &kusto_uri, &db_name, ".show running queries").await?;
    kql_utils::render_kql_results(cli, &rows, &columns);
    Ok(())
}

/// Show the operations journal (completed operations history).
pub(super) async fn journal(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;
    let (rows, columns) =
        kql_utils::execute_kql(client, &kusto_uri, &db_name, ".show journal").await?;
    kql_utils::render_kql_results(cli, &rows, &columns);
    Ok(())
}

/// Show recently completed queries.
pub(super) async fn queries_completed(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    query_uri_override: Option<&str>,
) -> Result<()> {
    let (kusto_uri, db_name) =
        kql_utils::resolve_query_uri(client, workspace, id, query_uri_override).await?;
    let (rows, columns) =
        kql_utils::execute_kql(client, &kusto_uri, &db_name, ".show queries").await?;
    kql_utils::render_kql_results(cli, &rows, &columns);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_kusto_format_maps_friendly_names() {
        assert_eq!(normalize_kusto_format("Csv").unwrap(), "csv");
        assert_eq!(normalize_kusto_format("MultiJson").unwrap(), "multijson");
        assert_eq!(normalize_kusto_format("json").unwrap(), "singlejson");
        assert_eq!(normalize_kusto_format("Parquet").unwrap(), "parquet");
        assert_eq!(normalize_kusto_format("TSV").unwrap(), "tsv");
        assert!(normalize_kusto_format("xlsx").is_err());
    }

    #[test]
    fn build_ingest_command_multijson_no_header() {
        let cmd = build_ingest_command(
            "payments",
            "h'https://onelake.blob.fabric.microsoft.com/ws/lh/Files/raw/p.json;impersonate'",
            "multijson",
            true, // ignoreFirstRecord is CSV/TSV-only, must be dropped for multijson
        );
        assert_eq!(
            cmd,
            ".ingest into table ['payments'] (h'https://onelake.blob.fabric.microsoft.com/ws/lh/Files/raw/p.json;impersonate') with (format='multijson')"
        );
    }

    #[test]
    fn build_ingest_command_csv_with_header_skip() {
        let cmd = build_ingest_command("orders", "h'https://x;impersonate'", "csv", true);
        assert_eq!(
            cmd,
            ".ingest into table ['orders'] (h'https://x;impersonate') with (format='csv', ignoreFirstRecord=true)"
        );
    }
}
