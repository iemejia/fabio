use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{
    execute_and_render_sql, parse_connection_string, pool_insights_sql,
};
use crate::output;

use super::get_connection_string;

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
    super::execute_insights_query(cli, client, workspace, id, sql).await
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
    super::execute_insights_query(cli, client, workspace, id, &sql).await
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
    super::execute_insights_query(cli, client, workspace, id, &sql).await
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
    super::execute_insights_query(cli, client, workspace, id, &sql).await
}

pub(super) async fn pool_insights(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    top: u32,
) -> Result<()> {
    let sql = pool_insights_sql(top);
    super::execute_insights_query(cli, client, workspace, id, &sql).await
}

pub(super) async fn queries_kill(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    session_id: i32,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "warehouse queries-kill",
        &serde_json::json!({ "session_id": session_id }),
    ) {
        return Ok(());
    }

    let sql = format!("KILL {session_id}");
    let (connection_string, item_name) = get_connection_string(client, workspace, id).await?;
    let (server, parsed_db) = parse_connection_string(&connection_string);
    let database = if item_name.is_empty() {
        parsed_db
    } else {
        item_name
    };
    execute_and_render_sql(cli, client, &server, &database, &sql).await?;

    let obj = serde_json::json!({ "session_id": session_id, "status": "killed" });
    output::render_object(cli, &obj, "status");
    Ok(())
}
