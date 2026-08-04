//! Activator (Reflex) remote MCP server: URL exposure + rule-management client.
//!
//! The Fabric Activator MCP server (`{base}/mcp/workspaces/{ws}/reflexes/{id}`)
//! exposes rule-management tools that have NO Fabric REST API equivalent:
//! `create_rule`, `list_rules`, `start_rule`, `stop_rule`, `delete_rule`, and
//! `get_activations_for_rule`. `reflex mcp-url` prints the URL for external MCP
//! clients (VS Code agent mode, GitHub Copilot, Claude) to author rules via
//! natural language; the other handlers here drive the deterministic
//! management tools directly through fabio's generic MCP client
//! ([`crate::mcp_client`]), mirroring `ontology search`.
//!
//! See: <https://learn.microsoft.com/fabric/real-time-intelligence/mcp-remote-activator>

use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{self, FabricClient};
use crate::errors::{ErrorCode, FabioError};
use crate::mcp_client::McpClient;
use crate::output;

/// Build the canonical Activator (Reflex) MCP server URL.
///
/// Format (per Microsoft docs):
/// `{base}/mcp/workspaces/{workspace}/reflexes/{id}`. This follows the
/// data-agent MCP shape (`/mcp/workspaces/{ws}/dataagents/{id}/...`) rather than
/// the ontology/kql `dataPlane/.../items/...` shape.
pub(super) fn build_mcp_url(base: &str, workspace: &str, id: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/mcp/workspaces/{workspace}/reflexes/{id}")
}

/// Print the Activator MCP server URL, plus a lightweight existence check.
pub(super) async fn mcp_url(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let url = build_mcp_url(client::fabric_base_url(), workspace, id);
    let exists = client
        .get(&format!("/workspaces/{workspace}/reflexes/{id}"))
        .await
        .is_ok();

    let mut result = json!({
        "id": id,
        "mcpUrl": url,
        "transport": "http",
        "exists": exists,
    });
    if exists {
        result["note"] = Value::from(
            "Consume this URL as an MCP server (HTTP transport) from VS Code agent mode, \
             GitHub Copilot, Claude, or any MCP client, signing in with your Fabric \
             credentials. Tools: create_rule, list_rules, start_rule, stop_rule, delete_rule, \
             get_activations_for_rule. Rules monitor a KQL source (Azure Data Explorer cluster \
             or Fabric eventhouse) and act via email/Teams. fabio drives the management tools \
             natively: reflex list-rules / start-rule / stop-rule / delete-rule / rule-activations.",
        );
    } else {
        result["hint"] = Value::from(format!(
            "Reflex '{id}' was not found in workspace '{workspace}'. \
             List reflexes with: fabio reflex list --workspace {workspace}"
        ));
    }
    output::render_object(cli, &result, "mcpUrl");
    Ok(())
}

/// Reject a mutation up front when `--readonly` is active. The MCP mutating
/// tools bypass the `FabricClient` request helpers (which enforce readonly for
/// POST/PUT/PATCH/DELETE), so we replicate that guard here.
fn guard_readonly(cli: &Cli, tool: &str) -> Result<()> {
    if cli.readonly {
        return Err(FabioError::with_hint(
            ErrorCode::ReadonlyMode,
            format!("Blocked Activator '{tool}' — readonly mode is active"),
            "Remove --readonly (or set FABIO_READONLY=0) to allow mutations.",
        )
        .into());
    }
    Ok(())
}

/// Connect to the Activator MCP server, confirm `tool` exists, call it, and
/// return its parsed JSON result (the tool encodes its payload as a JSON text
/// content block). Surfaces tool-level errors as a fabio `API_ERROR`.
async fn call_reflex_tool(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    tool: &str,
    arguments: Value,
) -> Result<Value> {
    let url = build_mcp_url(client::fabric_base_url(), workspace, id);
    // HTTPS + trusted-Microsoft-host check before sending the Fabric bearer token.
    client::validate_trusted_url(&url, "reflex MCP endpoint")?;

    let auth = client.require_auth().await?;
    let mcp = McpClient::connect(&url, Some(auth)).await?;

    let tools = mcp.list_tools().await?;
    if !tools
        .iter()
        .any(|t| t.get("name").and_then(Value::as_str) == Some(tool))
    {
        let available: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect();
        anyhow::bail!(
            "The Activator MCP server does not expose a '{tool}' tool (available: {available:?}). \
             Verify the reflex exists and the Activator MCP preview is enabled for your tenant."
        );
    }

    let result = mcp.call_tool(tool, arguments).await?;
    let text = result.text();
    let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| Value::from(text.clone()));
    if result.is_error {
        return Err(FabioError::api_error(format!(
            "Activator tool '{tool}' returned an error: {text}"
        ))
        .into());
    }
    Ok(parsed)
}

/// List all monitoring rules defined in a reflex (Activator artifact).
pub(super) async fn list_rules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let args = json!({ "listRulesParams": { "artifactId": id, "workspaceId": workspace } });
    let result = call_reflex_tool(client, workspace, id, "list_rules", args).await?;
    output::render_object(cli, &result, "rules");
    Ok(())
}

/// Start (enable) or stop (disable) a rule via the `start_rule`/`stop_rule` tool.
pub(super) async fn set_rule_state(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    rule_id: &str,
    start: bool,
) -> Result<()> {
    let (tool, params_key, group_verb) = if start {
        ("start_rule", "startRuleParams", "reflex start-rule")
    } else {
        ("stop_rule", "stopRuleParams", "reflex stop-rule")
    };

    if output::dry_run_guard(
        cli,
        group_verb,
        &json!({ "workspace": workspace, "id": id, "ruleId": rule_id, "tool": tool }),
    ) {
        return Ok(());
    }
    guard_readonly(cli, tool)?;

    let args =
        json!({ params_key: { "artifactId": id, "workspaceId": workspace, "ruleId": rule_id } });
    let result = call_reflex_tool(client, workspace, id, tool, args).await?;
    output::render_object(cli, &result, "result");
    Ok(())
}

/// Delete a rule via the `delete_rule` tool. Irreversible.
pub(super) async fn delete_rule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    rule_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "reflex delete-rule",
        &json!({ "workspace": workspace, "id": id, "ruleId": rule_id, "tool": "delete_rule" }),
    ) {
        return Ok(());
    }
    guard_readonly(cli, "delete_rule")?;

    let args = json!({ "deleteRuleParams": { "artifactId": id, "workspaceId": workspace, "ruleId": rule_id } });
    let result = call_reflex_tool(client, workspace, id, "delete_rule", args).await?;
    output::render_object(cli, &result, "result");
    Ok(())
}

/// Get the activation (fired-alert) history for a rule via
/// `get_activations_for_rule`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn rule_activations(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    rule_id: &str,
    start_time: Option<&str>,
    end_time: Option<&str>,
    max_results: Option<u32>,
) -> Result<()> {
    let mut params = json!({
        "artifactId": id,
        "workspaceId": workspace,
        "ruleId": rule_id,
    });
    if let Some(s) = start_time {
        params["startTime"] = Value::from(s);
    }
    if let Some(e) = end_time {
        params["endTime"] = Value::from(e);
    }
    if let Some(m) = max_results {
        params["maxResults"] = json!(m);
    }

    let args = json!({ "getActivationsParams": params });
    let result = call_reflex_tool(client, workspace, id, "get_activations_for_rule", args).await?;
    output::render_object(cli, &result, "activations");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::build_mcp_url;
    use crate::client;

    #[test]
    fn build_mcp_url_matches_documented_format() {
        let url = build_mcp_url("https://api.fabric.microsoft.com/v1", "ws-123", "rx-456");
        assert_eq!(
            url,
            "https://api.fabric.microsoft.com/v1/mcp/workspaces/ws-123/reflexes/rx-456"
        );
    }

    #[test]
    fn build_mcp_url_trims_trailing_slash_on_base() {
        let url = build_mcp_url("https://api.fabric.microsoft.com/v1/", "w", "r");
        assert_eq!(
            url,
            "https://api.fabric.microsoft.com/v1/mcp/workspaces/w/reflexes/r"
        );
    }

    #[test]
    fn build_mcp_url_is_https_and_trusted() {
        let url = build_mcp_url(client::fabric_base_url(), "w", "r");
        assert!(url.starts_with("https://"));
        assert!(url.contains("api.fabric.microsoft.com"));
        // Must pass the SSRF/trusted-host guard used before sending the token.
        assert!(client::validate_trusted_url(&url, "test").is_ok());
    }
}
