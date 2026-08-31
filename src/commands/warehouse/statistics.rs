//! Warehouse statistics — thin delegations to the shared `tds_stats` module.
//! The only warehouse-specific piece is the connection resolver.

use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_stats;

pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: Option<&str>,
) -> Result<()> {
    tds_stats::list(cli, client, table, || {
        super::resolve_connection(client, workspace, id)
    })
    .await
}

pub(super) async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    tds_stats::show(cli, client, name, || {
        super::resolve_connection(client, workspace, id)
    })
    .await
}

pub(super) async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    column: &str,
    name: &str,
) -> Result<()> {
    tds_stats::create(cli, client, "warehouse", table, column, name, || {
        super::resolve_connection(client, workspace, id)
    })
    .await
}

pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    tds_stats::update(cli, client, "warehouse", name, || {
        super::resolve_connection(client, workspace, id)
    })
    .await
}

pub(super) async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    tds_stats::delete(cli, client, "warehouse", name, || {
        super::resolve_connection(client, workspace, id)
    })
    .await
}

#[cfg(test)]
mod tests {
    use crate::commands::tds_utils::{build_drop_statistics_sql, build_update_statistics_sql};

    #[test]
    fn update_sql_resolves_table_and_uses_dynamic_sql() {
        let sql = build_update_statistics_sql("stat_id");
        assert!(sql.contains("FROM sys.stats s JOIN sys.tables t"));
        assert!(sql.contains("WHERE s.name = N'stat_id'"));
        assert!(sql.contains("UPDATE STATISTICS ' + @tbl"));
        assert!(sql.contains("EXEC sp_executesql"));
        assert!(!sql.contains("UPDATE STATISTICS [stat_id]"));
    }

    #[test]
    fn delete_sql_resolves_table_and_uses_object_dot_stat() {
        let sql = build_drop_statistics_sql("stat_id");
        assert!(sql.contains("FROM sys.stats s JOIN sys.tables t"));
        assert!(sql.contains("DROP STATISTICS ' + @tbl + N'.'"));
        assert!(sql.contains("EXEC sp_executesql"));
        assert!(!sql.contains("DROP STATISTICS [stat_id]"));
    }

    #[test]
    fn builders_escape_quotes() {
        assert!(build_update_statistics_sql("a'b").contains("N'a''b'"));
        assert!(build_drop_statistics_sql("a'b").contains("N'a''b'"));
    }
}
