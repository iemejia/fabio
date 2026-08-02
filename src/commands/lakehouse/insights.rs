use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{
    capture_query_plan, execute_and_render_sql, resolve_lakehouse_sql, resolve_sql_input,
};
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
