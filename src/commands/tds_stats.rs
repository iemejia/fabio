//! Shared T-SQL **statistics** operations for the `warehouse` and `sql-database`
//! command groups.
//!
//! Both groups expose an identical `statistics-list/show/create/update/delete`
//! surface; the only differences are (1) how the TDS connection is resolved and
//! (2) the op-name prefix used in the dry-run guard. These helpers hold the
//! shared SQL + dry-run/render logic and take:
//!   * `backend` — the op-name prefix (`"warehouse"` / `"sql-database"`), and
//!   * `resolve` — a **lazy** async closure yielding `(server, database)`, invoked
//!     only AFTER the dry-run guard so `--dry-run` never opens a connection.

use std::future::Future;

use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{
    build_drop_statistics_sql, build_update_statistics_sql, execute_and_render_sql,
};
use crate::output;

/// The `sys.stats` projection shared by `statistics list`.
const STATS_LIST_QUERY: &str = "SELECT s.name AS statistic_name, \
     SCHEMA_NAME(t.schema_id) AS schema_name, \
     t.name AS table_name, \
     c.name AS column_name, \
     s.auto_created, s.user_created \
     FROM sys.stats s \
     JOIN sys.stats_columns sc ON s.object_id = sc.object_id AND s.stats_id = sc.stats_id \
     JOIN sys.columns c ON sc.object_id = c.object_id AND sc.column_id = c.column_id \
     JOIN sys.tables t ON s.object_id = t.object_id";

/// List statistics objects, optionally filtered by `table` (`schema.table`).
pub async fn list<F, Fut>(
    cli: &Cli,
    client: &FabricClient,
    table: Option<&str>,
    resolve: F,
) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(String, String)>>,
{
    let sql = table.map_or_else(
        || format!("{STATS_LIST_QUERY} ORDER BY t.name, s.name"),
        |tbl| {
            let (schema, table_name) = tbl.split_once('.').unwrap_or(("dbo", tbl));
            format!(
                "{STATS_LIST_QUERY} \
                 WHERE SCHEMA_NAME(t.schema_id) = '{schema}' AND t.name = '{table_name}' \
                 ORDER BY s.name"
            )
        },
    );
    let (server, database) = resolve().await?;
    execute_and_render_sql(cli, client, &server, &database, &sql).await
}

/// Show statistic details via `DBCC SHOW_STATISTICS` (owning table resolved from
/// `sys.stats`).
pub async fn show<F, Fut>(cli: &Cli, client: &FabricClient, name: &str, resolve: F) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(String, String)>>,
{
    let sql = format!(
        "DECLARE @tbl NVARCHAR(500); \
         SELECT @tbl = QUOTENAME(SCHEMA_NAME(t.schema_id)) + '.' + QUOTENAME(t.name) \
         FROM sys.stats s JOIN sys.tables t ON s.object_id = t.object_id \
         WHERE s.name = N'{name}'; \
         IF @tbl IS NULL RAISERROR('Statistic not found: {name}', 16, 1); \
         DBCC SHOW_STATISTICS (@tbl, N'{name}')"
    );
    let (server, database) = resolve().await?;
    execute_and_render_sql(cli, client, &server, &database, &sql).await
}

/// Create a user-defined statistic on a column. Dry-run guarded.
pub async fn create<F, Fut>(
    cli: &Cli,
    client: &FabricClient,
    backend: &str,
    table: &str,
    column: &str,
    name: &str,
    resolve: F,
) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(String, String)>>,
{
    if output::dry_run_guard(
        cli,
        &format!("{backend} statistics-create"),
        &serde_json::json!({ "table": table, "column": column, "name": name }),
    ) {
        return Ok(());
    }
    let sql = format!("CREATE STATISTICS [{name}] ON {table} ([{column}])");
    let (server, database) = resolve().await?;
    execute_and_render_sql(cli, client, &server, &database, &sql).await?;
    let obj = serde_json::json!({
        "name": name,
        "table": table,
        "column": column,
        "status": "created"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Update (refresh) an existing statistic. Dry-run guarded.
pub async fn update<F, Fut>(
    cli: &Cli,
    client: &FabricClient,
    backend: &str,
    name: &str,
    resolve: F,
) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(String, String)>>,
{
    if output::dry_run_guard(
        cli,
        &format!("{backend} statistics-update"),
        &serde_json::json!({ "name": name }),
    ) {
        return Ok(());
    }
    let sql = build_update_statistics_sql(name);
    let (server, database) = resolve().await?;
    execute_and_render_sql(cli, client, &server, &database, &sql).await?;
    let obj = serde_json::json!({ "name": name, "status": "updated" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Delete a user-defined statistic. Dry-run guarded.
pub async fn delete<F, Fut>(
    cli: &Cli,
    client: &FabricClient,
    backend: &str,
    name: &str,
    resolve: F,
) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(String, String)>>,
{
    if output::dry_run_guard(
        cli,
        &format!("{backend} statistics-delete"),
        &serde_json::json!({ "name": name }),
    ) {
        return Ok(());
    }
    let sql = build_drop_statistics_sql(name);
    let (server, database) = resolve().await?;
    execute_and_render_sql(cli, client, &server, &database, &sql).await?;
    let obj = serde_json::json!({ "name": name, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}
