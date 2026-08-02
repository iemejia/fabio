//! `ontology search` — natural-language query over an ontology's data estate.
//!
//! This is fabio's first MCP-CLIENT feature: it consumes the Fabric ontology MCP
//! server's `search_ontology` tool via the generic [`crate::mcp_client`]. The
//! ontology MCP server exposes two tools — `list_ontology_entity_types` (already
//! covered offline by `ontology list-entity-types`) and `search_ontology`, which
//! performs server-side Fabric IQ reasoning over the ontology's bound data and
//! has no pure-fabio equivalent. `ontology search` drives that tool end-to-end.

use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{self, FabricClient};
use crate::mcp_client::McpClient;
use crate::output;

use super::mcp::build_mcp_url;

/// The MCP tool this command drives.
const TOOL: &str = "search_ontology";

/// Ask a natural-language question over the ontology's data estate.
///
/// `natural_language_response` toggles the tool's derived NL answer (in addition
/// to the raw JSON results it always returns).
pub(super) async fn search(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    prompt: &str,
    natural_language_response: bool,
) -> Result<()> {
    let url = build_mcp_url(client::fabric_base_url(), workspace, id);
    // HTTPS + trusted-Microsoft-host check before sending the Fabric bearer token.
    client::validate_trusted_url(&url, "ontology search endpoint")?;

    if output::dry_run_guard(
        cli,
        "ontology search",
        &json!({
            "workspace": workspace,
            "id": id,
            "endpoint": url,
            "query": prompt,
            "tool": "search_ontology",
        }),
    ) {
        return Ok(());
    }

    let auth = client.require_auth().await?;
    let mcp = McpClient::connect(&url, Some(auth)).await?;

    // Confirm the server exposes the tool (guards against a renamed/removed tool
    // and surfaces the available tools if not).
    let tools = mcp.list_tools().await?;
    if !tools
        .iter()
        .any(|t| t.get("name").and_then(Value::as_str) == Some(TOOL))
    {
        let available: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect();
        anyhow::bail!(
            "The ontology MCP server does not expose a '{TOOL}' tool (available: {available:?}). \
             Verify the ontology is published and the Ontology-item preview is enabled."
        );
    }

    let result = mcp
        .call_tool(
            TOOL,
            json!({
                "naturalLanguageQuery": prompt,
                "naturalLanguageResponse": natural_language_response,
            }),
        )
        .await?;

    // The tool returns its answer as text content; it is usually a JSON document.
    let text = result.text();
    let answer: Value = serde_json::from_str(&text).unwrap_or_else(|_| Value::from(text.clone()));

    let out = json!({
        "query": prompt,
        "answer": answer,
        "isError": result.is_error,
    });
    output::render_object(cli, &out, "answer");

    if result.is_error {
        anyhow::bail!("search_ontology returned an error result");
    }
    Ok(())
}
