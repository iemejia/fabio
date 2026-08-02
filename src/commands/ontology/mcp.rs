//! Ontology MCP (Model Context Protocol) consumption endpoint.
//!
//! A Fabric ontology (preview) item can be consumed as an MCP server, exposing
//! its schema/query surface to external AI systems (VS Code agent mode, Claude,
//! Copilot Studio, ...) over the MCP protocol. The consumption endpoint is a
//! deterministic URL that agents cannot guess, so fabio constructs it. This is
//! the ontology analog of `data-agent mcp-url`.
//!
//! See: <https://learn.microsoft.com/fabric/iq/ontology/how-to-use-ontology-mcp-server>

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{self, FabricClient};
use crate::output;

/// Build the canonical ontology MCP server URL.
///
/// Format (per Microsoft docs):
/// `{base}/mcp/dataPlane/workspaces/{workspace}/items/{id}/ontologyEndpoint`.
/// External MCP clients connect to this URL over HTTP transport, signing in with
/// Fabric credentials. Note this differs from the data-agent MCP URL shape
/// (`/mcp/workspaces/{ws}/dataagents/{id}/agent`): ontology uses the generic
/// `dataPlane/.../items/...` path with an `ontologyEndpoint` suffix.
pub(super) fn build_mcp_url(base: &str, workspace: &str, id: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/mcp/dataPlane/workspaces/{workspace}/items/{id}/ontologyEndpoint")
}

/// Print the ontology MCP server URL, plus a lightweight existence check and the
/// consumption prerequisites.
pub async fn mcp_url(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let url = build_mcp_url(client::fabric_base_url(), workspace, id);
    // The URL itself is deterministic; a light existence check just improves the
    // hint (a 404 means the id/workspace is wrong, not that the URL is malformed).
    let exists = client
        .get(&format!("/workspaces/{workspace}/ontologies/{id}"))
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
            "Consume this URL as an MCP server (HTTP transport) from VS Code agent mode, \
             Claude, Copilot Studio, or any MCP client, signing in with your Fabric \
             credentials. Prerequisites: an F2+/P1 capacity and the 'Ontology item (preview)' \
             tenant setting enabled.",
        );
    } else {
        result["hint"] = Value::from(format!(
            "Ontology '{id}' was not found in workspace '{workspace}'. \
             List ontologies with: fabio ontology list --workspace {workspace}"
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
        let url = build_mcp_url("https://api.fabric.microsoft.com/v1", "ws-123", "ont-456");
        assert_eq!(
            url,
            "https://api.fabric.microsoft.com/v1/mcp/dataPlane/workspaces/ws-123/items/ont-456/ontologyEndpoint"
        );
    }

    #[test]
    fn build_mcp_url_trims_trailing_slash_on_base() {
        let url = build_mcp_url("https://api.fabric.microsoft.com/v1/", "w", "o");
        assert_eq!(
            url,
            "https://api.fabric.microsoft.com/v1/mcp/dataPlane/workspaces/w/items/o/ontologyEndpoint"
        );
    }

    #[test]
    fn build_mcp_url_honors_custom_base() {
        let url = build_mcp_url("https://example.test/v1", "w", "o");
        assert_eq!(
            url,
            "https://example.test/v1/mcp/dataPlane/workspaces/w/items/o/ontologyEndpoint"
        );
    }

    #[test]
    fn build_mcp_url_is_https_and_trusted() {
        let url = build_mcp_url(client::fabric_base_url(), "w", "o");
        assert!(url.starts_with("https://"));
        assert!(url.contains("api.fabric.microsoft.com"));
        assert!(url.ends_with("/ontologyEndpoint"));
    }
}
