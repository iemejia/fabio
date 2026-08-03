//! Eventhouse / KQL database remote MCP (Model Context Protocol) consumption endpoint.
//!
//! A Fabric KQL database (in an eventhouse) can be consumed as a hosted remote MCP
//! server, exposing its schema/query surface to external AI systems (VS Code agent
//! mode, GitHub Copilot, Copilot Studio, Azure AI Foundry, ...) over HTTP transport.
//! The consumption endpoint is a deterministic URL that agents cannot guess, so fabio
//! constructs it. This is the KQL-database analog of `data-agent mcp-url` and
//! `ontology mcp-url`.
//!
//! See: <https://learn.microsoft.com/fabric/real-time-intelligence/mcp-eventhouse>

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{self, FabricClient};
use crate::output;

/// Build the canonical eventhouse/KQL-database remote MCP server URL.
///
/// Format (per Microsoft docs):
/// `{base}/mcp/dataPlane/workspaces/{workspace}/items/{id}/kqlEndpoint`.
/// External MCP clients connect to this URL over HTTP transport, signing in with
/// Fabric credentials. Same generic `dataPlane/.../items/...` shape as the ontology
/// MCP URL, but with a `kqlEndpoint` suffix (ontology uses `ontologyEndpoint`).
pub(super) fn build_mcp_url(base: &str, workspace: &str, id: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/mcp/dataPlane/workspaces/{workspace}/items/{id}/kqlEndpoint")
}

/// Print the KQL-database remote MCP server URL, plus a lightweight existence check
/// and the consumption note.
pub(super) async fn mcp_url(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let url = build_mcp_url(client::fabric_base_url(), workspace, id);
    // The URL itself is deterministic; a light existence check just improves the
    // hint (a 404 means the id/workspace is wrong, not that the URL is malformed).
    let exists = client
        .get(&format!("/workspaces/{workspace}/kqlDatabases/{id}"))
        .await
        .is_ok();

    let mut result = serde_json::json!({
        "id": id,
        "mcpUrl": url,
        "transport": "http",
        "exists": exists,
    });
    if exists {
        result["note"] = Value::from(
            "Consume this URL as a remote MCP server (HTTP transport) from VS Code agent \
             mode, GitHub Copilot, Copilot Studio, Azure AI Foundry, or any MCP client, \
             signing in with a Fabric credential that has access to the eventhouse. The \
             server exposes tools to discover KQL schemas, generate KQL from natural \
             language, execute queries, and sample data.",
        );
    } else {
        result["hint"] = Value::from(format!(
            "KQL database '{id}' was not found in workspace '{workspace}'. \
             List KQL databases with: fabio kql-database list --workspace {workspace}"
        ));
    }
    output::render_object(cli, &result, "mcpUrl");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_mcp_url_matches_documented_format() {
        let url = build_mcp_url("https://api.fabric.microsoft.com/v1", "ws-123", "kql-456");
        assert_eq!(
            url,
            "https://api.fabric.microsoft.com/v1/mcp/dataPlane/workspaces/ws-123/items/kql-456/kqlEndpoint"
        );
    }

    #[test]
    fn build_mcp_url_trims_trailing_slash_on_base() {
        let url = build_mcp_url("https://api.fabric.microsoft.com/v1/", "w", "k");
        assert_eq!(
            url,
            "https://api.fabric.microsoft.com/v1/mcp/dataPlane/workspaces/w/items/k/kqlEndpoint"
        );
    }

    #[test]
    fn build_mcp_url_honors_custom_base() {
        let url = build_mcp_url("https://example.test/v1", "w", "k");
        assert_eq!(
            url,
            "https://example.test/v1/mcp/dataPlane/workspaces/w/items/k/kqlEndpoint"
        );
    }
}
