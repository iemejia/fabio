//! SQL-database statistics — thin delegations to the shared `tds_stats` module.
//! The only sql-database-specific piece is the connection resolver.

use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_stats;

/// Resolve this SQL database's `(server, database)` (dropping the unused port).
async fn resolve(client: &FabricClient, workspace: &str, id: &str) -> Result<(String, String)> {
    let (server, _port, database) =
        super::query::resolve_sql_connection(client, workspace, id).await?;
    Ok((server, database))
}

pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: Option<&str>,
) -> Result<()> {
    tds_stats::list(cli, client, table, || resolve(client, workspace, id)).await
}

pub(super) async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    tds_stats::show(cli, client, name, || resolve(client, workspace, id)).await
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
    tds_stats::create(cli, client, "sql-database", table, column, name, || {
        resolve(client, workspace, id)
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
    tds_stats::update(cli, client, "sql-database", name, || {
        resolve(client, workspace, id)
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
    tds_stats::delete(cli, client, "sql-database", name, || {
        resolve(client, workspace, id)
    })
    .await
}
