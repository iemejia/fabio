//! Warehouse schema discovery over `INFORMATION_SCHEMA` (tables + columns).
//!
//! Fabric's remote Data Warehouse MCP server exposes only an `execute_query` tool
//! (no schema/metadata tools), so agents otherwise hand-write `INFORMATION_SCHEMA`
//! queries. These subcommands make table/column discovery first-class, bounded, and
//! typed, reusing the same TDS execution path as `warehouse query`.

use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{describe_table_sql, list_tables_sql, split_schema_qualified};
use crate::errors::enrich_forbidden;

use super::execute_insights_query;

/// List tables and views in a warehouse (optionally scoped to one schema).
pub(super) async fn list_tables(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    schema: Option<&str>,
) -> Result<()> {
    let sql = list_tables_sql(schema);
    execute_insights_query(cli, client, workspace, id, &sql)
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse list-tables", "Viewer"))
}

/// Describe the columns of a single table (`--table [schema.]table`).
pub(super) async fn describe_table(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
) -> Result<()> {
    let (schema, tbl) = split_schema_qualified(table);
    let sql = describe_table_sql(schema.as_deref(), &tbl);
    execute_insights_query(cli, client, workspace, id, &sql)
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse describe-table", "Viewer"))
}
