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
use crate::mcp_client::McpClient;
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

// ─── KQL example retrieval (MCP client) ──────────────────────────────────────

/// The eventhouse MCP server's two example-retrieval tools, driven by `examples`.
const TOOL_GENERAL: &str = "getGeneralKQLExamples";
const TOOL_SPECIFIC: &str = "getSpecificKQLExamples";
/// The eventhouse MCP server's schema-context tool, driven by `schema-context`.
const TOOL_SCHEMA: &str = "getSchema";

/// Connect to the eventhouse remote MCP server and confirm it exposes every tool
/// in `required` (guards against a renamed/removed tool, surfacing what's available).
async fn connect_require_tools(
    client: &FabricClient,
    url: &str,
    required: &[&str],
) -> Result<McpClient> {
    let auth = client.require_auth().await?;
    let mcp = McpClient::connect(url, Some(auth)).await?;
    let tools = mcp.list_tools().await?;
    let available: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect();
    for name in required {
        if !available.contains(name) {
            anyhow::bail!(
                "The eventhouse MCP server does not expose a '{name}' tool (available: {available:?})."
            );
        }
    }
    Ok(mcp)
}

/// Which example tools to call, given the `--general-only` / `--specific-only`
/// scope flags. Pure function for testing. Returns `(want_general, want_specific)`.
const fn select_example_scope(general_only: bool, specific_only: bool) -> (bool, bool) {
    match (general_only, specific_only) {
        (true, false) => (true, false),
        (false, true) => (false, true),
        // Neither (default) or both → return both.
        _ => (true, true),
    }
}

/// Retrieve KQL example pairs relevant to a natural-language prompt from the
/// eventhouse remote MCP server. This is the KQL analog of `ontology search`:
/// it drives the server's `getGeneralKQLExamples` (curated public NL→KQL pairs)
/// and `getSpecificKQLExamples` (examples curated/learned from THIS database) tools
/// — grounding for authoring KQL that fabio has no offline equivalent for
/// (schema is available via `describe`, execution via `query`, generation via
/// `rti nl-to-kql`).
pub(super) async fn examples(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    prompt: &str,
    general_only: bool,
    specific_only: bool,
) -> Result<()> {
    let (want_general, want_specific) = select_example_scope(general_only, specific_only);
    let url = build_mcp_url(client::fabric_base_url(), workspace, id);
    // HTTPS + trusted-Microsoft-host check before sending the Fabric bearer token.
    client::validate_trusted_url(&url, "kql-database examples endpoint")?;

    if output::dry_run_guard(
        cli,
        "kql-database examples",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "endpoint": url,
            "prompt": prompt,
            "tools": {
                "getGeneralKQLExamples": want_general,
                "getSpecificKQLExamples": want_specific,
            },
        }),
    ) {
        return Ok(());
    }

    let mut required = Vec::new();
    if want_general {
        required.push(TOOL_GENERAL);
    }
    if want_specific {
        required.push(TOOL_SPECIFIC);
    }
    let mcp = connect_require_tools(client, &url, &required).await?;

    let mut out = serde_json::json!({ "prompt": prompt, "endpoint": url });
    let mut any_error = false;
    for (want, name, key) in [
        (want_general, TOOL_GENERAL, "generalExamples"),
        (want_specific, TOOL_SPECIFIC, "specificExamples"),
    ] {
        if !want {
            continue;
        }
        let result = mcp
            .call_tool(name, serde_json::json!({ "referenceText": prompt }))
            .await?;
        any_error |= result.is_error;
        out[key] = serde_json::json!({
            "text": result.text(),
            "isError": result.is_error,
        });
    }

    output::render_object(cli, &out, "prompt");
    if any_error {
        // A common cause is an empty database (the tools ground on schema/data).
        anyhow::bail!(
            "One or more example tools returned an error (an empty database is a common cause)."
        );
    }
    Ok(())
}

/// Retrieve the relevant schema CONTEXT for a natural-language prompt from the
/// eventhouse remote MCP server's `getSchema` tool. Unlike the offline `describe`
/// (raw schema), this returns a semantically-scoped bundle: relevant tables /
/// materialized-views / external-tables + functions, plus COLUMN VALUE SAMPLES,
/// cardinality/distinct-value STATS, and KQL-authoring guidance — grounding for
/// writing KQL that fabio has no offline equivalent for (the samples come from
/// real data).
pub(super) async fn schema_context(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    prompt: &str,
) -> Result<()> {
    let url = build_mcp_url(client::fabric_base_url(), workspace, id);
    // HTTPS + trusted-Microsoft-host check before sending the Fabric bearer token.
    client::validate_trusted_url(&url, "kql-database schema-context endpoint")?;

    if output::dry_run_guard(
        cli,
        "kql-database schema-context",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "endpoint": url,
            "prompt": prompt,
            "tool": TOOL_SCHEMA,
        }),
    ) {
        return Ok(());
    }

    let mcp = connect_require_tools(client, &url, &[TOOL_SCHEMA]).await?;
    let result = mcp
        .call_tool(TOOL_SCHEMA, serde_json::json!({ "referenceText": prompt }))
        .await?;

    let out = serde_json::json!({
        "prompt": prompt,
        "endpoint": url,
        "schema": {
            "text": result.text(),
            "isError": result.is_error,
        },
    });
    output::render_object(cli, &out, "prompt");
    if result.is_error {
        // A common cause is an empty database (the tool grounds on schema/data).
        anyhow::bail!("getSchema returned an error result (an empty database is a common cause).");
    }
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

    #[test]
    fn select_example_scope_defaults_to_both() {
        assert_eq!(select_example_scope(false, false), (true, true));
    }

    #[test]
    fn select_example_scope_honors_general_only() {
        assert_eq!(select_example_scope(true, false), (true, false));
    }

    #[test]
    fn select_example_scope_honors_specific_only() {
        assert_eq!(select_example_scope(false, true), (false, true));
    }
}
