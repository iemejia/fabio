//! Client for the **remote Power BI MCP server** (`{fabric}/mcp/powerbi`), a
//! hosted Model-Context-Protocol endpoint that lets fabio consume Copilot-powered
//! Power BI capabilities that have no direct REST equivalent:
//! - `GenerateQuery` — natural-language → DAX (Copilot's DAX engine)
//! - `GetSemanticModelSchema` — Copilot-oriented schema + author custom instructions
//! - `GetReportMetadata` — synthesized report schema (pages, visuals, bindings)
//!
//! fabio connects as an MCP CLIENT over the streamable-HTTP transport (the same
//! `mcp_client` used by `ontology search` / `kql-database examples`), signing in
//! with the Fabric bearer token. The endpoint is a single FIXED global URL (not
//! per-item); the tools resolve the artifact by its GUID. Requires the tenant
//! setting "Users can use the Power BI Model Context Protocol server endpoint".

use anyhow::Result;
use serde_json::Value;

use crate::client::{self, FabricClient};
use crate::mcp_client::{McpClient, ToolResult};

/// Build the remote Power BI MCP server URL from the Fabric API base.
/// The base is e.g. `https://api.fabric.microsoft.com/v1`; the endpoint is a
/// single fixed global URL (not per-item).
pub fn powerbi_mcp_url(base: &str) -> String {
    format!("{}/mcp/powerbi", base.trim_end_matches('/'))
}

/// Connect to the Power BI MCP server and invoke a single tool. Validates the
/// endpoint is an HTTPS trusted-Microsoft host before sending the Fabric bearer
/// token, confirms the tool exists, then calls it. If the gating `PowerBIMCP`
/// tenant setting is disabled the connect 403 propagates and is turned into a
/// teaching error generically by `commands::tenant_gate::enrich`.
pub async fn call_powerbi_tool(
    client: &FabricClient,
    tool: &str,
    arguments: Value,
) -> Result<ToolResult> {
    let url = powerbi_mcp_url(client::fabric_base_url());
    // HTTPS + trusted-Microsoft-host check before sending the Fabric bearer token.
    client::validate_trusted_url(&url, "Power BI MCP endpoint")?;
    let auth = client.require_auth().await?;
    let mcp = McpClient::connect(&url, Some(auth)).await?;

    let tools = mcp.list_tools().await?;
    let known = tools
        .iter()
        .any(|t| t.get("name").and_then(Value::as_str) == Some(tool));
    if !known {
        let available: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect();
        anyhow::bail!(
            "Power BI MCP server does not expose a '{tool}' tool. Available: {}",
            available.join(", ")
        );
    }

    mcp.call_tool(tool, arguments).await
}

/// Parse an MCP tool's result into a single JSON object. The Power BI MCP server
/// returns MULTIPLE text content blocks (e.g. `GetSemanticModelSchema` returns a
/// `{schema,…}` block plus an `{artifact_citation}` block), so each text block is
/// parsed as JSON and object blocks are merged into one object (first value wins
/// on a key collision). If no block is a JSON object, the concatenated raw text
/// is returned as `{"text": "..."}`.
pub fn tool_text_as_json(result: &ToolResult) -> Value {
    let mut merged = serde_json::Map::new();
    let mut had_object = false;
    for block in &result.content {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let Some(text) = block.get("text").and_then(Value::as_str) else {
            continue;
        };
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text.trim()) {
            had_object = true;
            for (k, v) in map {
                merged.entry(k).or_insert(v);
            }
        }
    }
    if had_object {
        return Value::Object(merged);
    }
    serde_json::json!({ "text": result.text() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powerbi_mcp_url_appends_suffix() {
        assert_eq!(
            powerbi_mcp_url("https://api.fabric.microsoft.com/v1"),
            "https://api.fabric.microsoft.com/v1/mcp/powerbi"
        );
    }

    #[test]
    fn powerbi_mcp_url_trims_trailing_slash() {
        assert_eq!(
            powerbi_mcp_url("https://api.fabric.microsoft.com/v1/"),
            "https://api.fabric.microsoft.com/v1/mcp/powerbi"
        );
    }

    #[test]
    fn powerbi_mcp_url_honors_custom_base() {
        assert_eq!(
            powerbi_mcp_url("https://example.test/v1"),
            "https://example.test/v1/mcp/powerbi"
        );
    }

    #[test]
    fn tool_text_as_json_parses_json_text() {
        let r = ToolResult {
            content: vec![
                serde_json::json!({"type":"text","text":"{\"daxQuery\":\"EVALUATE X\"}"}),
            ],
            is_error: false,
            raw: serde_json::json!({}),
        };
        assert_eq!(tool_text_as_json(&r)["daxQuery"], "EVALUATE X");
    }

    #[test]
    fn tool_text_as_json_wraps_non_json_text() {
        let r = ToolResult {
            content: vec![serde_json::json!({"type":"text","text":"plain message"})],
            is_error: false,
            raw: serde_json::json!({}),
        };
        assert_eq!(tool_text_as_json(&r)["text"], "plain message");
    }

    #[test]
    fn tool_text_as_json_merges_multiple_object_blocks() {
        // GetSemanticModelSchema returns a schema block + an artifact_citation
        // block; both object blocks must be merged (not joined then failed).
        let r = ToolResult {
            content: vec![
                serde_json::json!({"type":"text","text":"{\"schema\":{\"Tables\":[]}}"}),
                serde_json::json!({"type":"text","text":"{\"artifact_citation\":\"cite\"}"}),
            ],
            is_error: false,
            raw: serde_json::json!({}),
        };
        let v = tool_text_as_json(&r);
        assert!(v.get("schema").is_some());
        assert_eq!(v["artifact_citation"], "cite");
    }
}
