use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{
    capture_query_plan, execute_and_render_sql, pool_insights_sql, resolve_lakehouse_sql,
    resolve_sql_input,
};
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// Helper: resolve lakehouse SQL connection and execute a TDS query.
async fn execute_lakehouse_query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql_text: &str,
) -> Result<()> {
    let (server, database) = resolve_lakehouse_sql(client, workspace, id).await?;
    execute_and_render_sql(cli, client, &server, &database, sql_text).await
}

// ─── Plan ────────────────────────────────────────────────────────────────────

pub(super) async fn plan(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql: Option<&str>,
) -> Result<()> {
    let sql_text = resolve_sql_input(sql)?;
    let (server, database) = resolve_lakehouse_sql(client, workspace, id).await?;

    let plans = capture_query_plan(client, &server, &database, &sql_text).await?;

    let plan_objects: Vec<Value> = plans
        .iter()
        .enumerate()
        .map(|(i, xml)| {
            serde_json::json!({
                "statementIndex": i,
                "planXml": xml
            })
        })
        .collect();
    let obj = serde_json::json!({
        "statementCount": plans.len(),
        "plans": plan_objects
    });
    output::render_object(cli, &obj, "statementCount");

    Ok(())
}

// ─── Query Insights ──────────────────────────────────────────────────────────

pub(super) async fn queries_running(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let sql = "SELECT r.session_id, r.status, \
               r.command, r.start_time, r.total_elapsed_time \
               FROM sys.dm_exec_requests r \
               WHERE r.status != 'background' \
               ORDER BY r.total_elapsed_time DESC";
    execute_lakehouse_query(cli, client, workspace, id, sql).await
}

pub(super) async fn queries_frequent(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    top: u32,
) -> Result<()> {
    let sql = format!(
        "SELECT TOP ({top}) \
         last_run_command, number_of_runs, \
         avg_total_elapsed_time_ms, \
         min_run_total_elapsed_time_ms, \
         max_run_total_elapsed_time_ms, \
         number_of_successful_runs, \
         query_hash \
         FROM queryinsights.frequently_run_queries \
         ORDER BY number_of_runs DESC"
    );
    execute_lakehouse_query(cli, client, workspace, id, &sql).await
}

pub(super) async fn queries_long_running(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    top: u32,
) -> Result<()> {
    let sql = format!(
        "SELECT TOP ({top}) \
         last_run_command, number_of_runs, \
         median_total_elapsed_time_ms, \
         last_run_total_elapsed_time_ms, \
         last_run_start_time, \
         query_hash \
         FROM queryinsights.long_running_queries \
         ORDER BY median_total_elapsed_time_ms DESC"
    );
    execute_lakehouse_query(cli, client, workspace, id, &sql).await
}

pub(super) async fn queries_history(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    top: u32,
) -> Result<()> {
    let sql = format!(
        "SELECT TOP ({top}) \
         command, status, \
         total_elapsed_time_ms, \
         login_name, \
         start_time, end_time, \
         row_count, \
         query_hash \
         FROM queryinsights.exec_requests_history \
         ORDER BY start_time DESC"
    );
    execute_lakehouse_query(cli, client, workspace, id, &sql).await
}

pub(super) async fn pool_insights(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    top: u32,
) -> Result<()> {
    let sql = pool_insights_sql(top);
    execute_lakehouse_query(cli, client, workspace, id, &sql).await
}

// ─── Table Health ──────────────────────────────────────────────────────────────

/// Build the `sp_get_table_health_metrics` call for a table name, escaping the
/// single-quoted literal to keep the T-SQL well-formed. Pure function for testing.
fn build_table_health_sql(table: &str) -> String {
    // Escape single quotes per T-SQL string-literal rules ('' == literal ').
    let escaped = table.replace('\'', "''");
    format!("EXEC sys.sp_get_table_health_metrics @table_name = N'{escaped}';")
}

/// Validate a caller-supplied table name before it is embedded into T-SQL.
fn validate_table_name(table: &str) -> Result<()> {
    let trimmed = table.trim();
    if trimmed.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Table name is empty".to_string(),
            "Provide a fully-qualified table name, e.g. --table dbo.FactSales",
        )
        .into());
    }
    Ok(())
}

pub(super) async fn table_health(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
) -> Result<()> {
    validate_table_name(table)?;
    let sql = build_table_health_sql(table.trim());
    execute_lakehouse_query(cli, client, workspace, id, &sql).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_health_sql_wraps_name() {
        assert_eq!(
            build_table_health_sql("dbo.FactSales"),
            "EXEC sys.sp_get_table_health_metrics @table_name = N'dbo.FactSales';"
        );
    }

    #[test]
    fn table_health_sql_escapes_single_quotes() {
        // A stray single quote must be doubled so the literal stays well-formed.
        assert_eq!(
            build_table_health_sql("dbo.O'Brien"),
            "EXEC sys.sp_get_table_health_metrics @table_name = N'dbo.O''Brien';"
        );
    }

    #[test]
    fn table_health_sql_neutralizes_injection_attempt() {
        // A classic injection payload becomes an inert (doubled-quote) literal.
        let sql = build_table_health_sql("x'; DROP TABLE t; --");
        assert!(sql.contains("x''; DROP TABLE t; --"));
        // Exactly one statement terminator we added; the injected ';' is inside the literal.
        assert!(sql.ends_with("--';"));
    }

    #[test]
    fn validate_table_name_rejects_empty() {
        assert!(validate_table_name("").is_err());
        assert!(validate_table_name("   ").is_err());
    }

    #[test]
    fn validate_table_name_accepts_qualified() {
        assert!(validate_table_name("dbo.FactSales").is_ok());
        assert!(validate_table_name("FactSales").is_ok());
    }
}
