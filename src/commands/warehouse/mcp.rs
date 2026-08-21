//! Fabric Data Warehouse remote MCP (Model Context Protocol) consumption endpoint.
//!
//! A Fabric Warehouse (or SQL analytics endpoint) can be consumed by the hosted
//! remote Fabric Data Warehouse MCP server (preview), which exposes a single
//! `executeSQL` tool over streamable HTTP. The endpoint URL is deterministic, so
//! fabio constructs it. This is the Warehouse analog of `kql-database mcp-url` and
//! `ontology mcp-url`.
//!
//! See: <https://learn.microsoft.com/fabric/data-warehouse/data-warehouse-mcp-server>

use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::render_sql_mcp_url;

/// Print the Fabric Data Warehouse remote MCP server URLs (item-scoped + global)
/// for a warehouse, plus a lightweight existence check and consumption note.
pub(super) async fn mcp_url(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    // The URL itself is deterministic; the existence check just improves the hint
    // (a 404 means the id/workspace is wrong, not that the URL is malformed).
    let exists = client
        .get(&format!("/workspaces/{workspace}/warehouses/{id}"))
        .await
        .is_ok();

    let hint = format!(
        "Warehouse '{id}' was not found in workspace '{workspace}'. \
         List warehouses with: fabio warehouse list --workspace {workspace}"
    );
    render_sql_mcp_url(cli, workspace, id, exists, &hint);
    Ok(())
}
