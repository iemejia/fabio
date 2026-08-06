use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::execute_and_render_sql;
use crate::output;

/// Helper: resolve SQL database connection and execute a TDS query, rendering results.
async fn execute_sql_db_query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql_text: &str,
) -> Result<()> {
    let (server, _port, database) =
        super::query::resolve_sql_connection(client, workspace, id).await?;
    execute_and_render_sql(cli, client, &server, &database, sql_text).await
}

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
    execute_sql_db_query(cli, client, workspace, id, sql).await
}

pub(super) async fn queries_history(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    top: u32,
    label: Option<&str>,
) -> Result<()> {
    let sql = crate::commands::tds_utils::queries_history_sql(top, label);
    execute_sql_db_query(cli, client, workspace, id, &sql).await
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
        "sql-database queries-kill",
        &serde_json::json!({ "session_id": session_id }),
    ) {
        return Ok(());
    }

    let sql = format!("KILL {session_id}");
    execute_sql_db_query(cli, client, workspace, id, &sql).await?;

    let obj = serde_json::json!({ "session_id": session_id, "status": "killed" });
    output::render_object(cli, &obj, "status");
    Ok(())
}
