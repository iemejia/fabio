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
        // The MCP server's "Getting cluster for kql database failed" is cryptic;
        // it almost always means the wrong item id was passed as the KQL source
        // (e.g. an eventhouse id instead of the KQL DATABASE item id).
        if text.contains("Getting cluster for kql database failed") {
            return Err(FabioError::with_hint(
                ErrorCode::ApiError,
                format!("Activator tool '{tool}' returned an error: {text}"),
                "The KQL source could not be resolved. Pass the KQL DATABASE item id \
                 to --eventhouse-id (alias --kql-database-id) — NOT the eventhouse id — \
                 or use --cluster <queryServiceUri> + --database <name>.",
            )
            .into());
        }
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

// ─── create-rule (drives the MCP `create_rule` tool) ────────────────────────

/// Typed inputs for the common-case `create-rule` (single KQL source, one
/// numeric/text threshold condition, one email/Teams action). For anything
/// richer (ranges, occurrence modifiers, filters, Ontology sources), use the
/// `--rule` JSON passthrough.
pub(super) struct TypedRuleSpec<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub kql: &'a str,
    pub interval: u32,
    /// Fabric eventhouse KQL database item id (mutually exclusive with `cluster`).
    pub eventhouse_id: Option<&'a str>,
    /// Workspace of the eventhouse (defaults to the reflex workspace).
    pub eventhouse_workspace: Option<&'a str>,
    /// ADX cluster host (mutually exclusive with `eventhouse_id`).
    pub cluster: Option<&'a str>,
    /// ADX database name (used with `cluster`).
    pub database: Option<&'a str>,
    pub split_column: Option<&'a str>,
    pub column: &'a str,
    pub condition: &'a str,
    pub value: &'a str,
    /// `email` or `teams`.
    pub action: &'a str,
    pub recipients: &'a [String],
    pub subject: Option<&'a str>,
    pub message: Option<&'a str>,
    pub headline: Option<&'a str>,
    pub locale: &'a str,
}

/// Encode a scalar as the tool's typed-value shape `{type, value}`, inferring the
/// type from the string (number → number, `true`/`false` → boolean, else string).
fn typed_value(v: &str) -> Value {
    v.parse::<f64>().map_or_else(
        |_| {
            if v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("false") {
                json!({ "type": "boolean", "value": v.eq_ignore_ascii_case("true") })
            } else {
                json!({ "type": "string", "value": v })
            }
        },
        |n| json!({ "type": "number", "value": n }),
    )
}

/// Build the KQL `source` object from the typed spec (exactly one of
/// eventhouse-id / cluster+database must be set).
fn build_source(workspace: &str, s: &TypedRuleSpec) -> Result<Value> {
    let eventhouse_item = match (s.eventhouse_id, s.cluster, s.database) {
        (Some(item_id), None, _) => json!({
            "itemId": item_id,
            "workspaceId": s.eventhouse_workspace.unwrap_or(workspace),
            "itemType": "KustoDatabase",
        }),
        (None, Some(cluster), Some(db)) => json!({
            "databaseName": db,
            "clusterHostName": cluster,
        }),
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "A KQL data source is required.".to_string(),
                "Provide either --eventhouse-id (a Fabric eventhouse KQL database) or \
                 --cluster + --database (an Azure Data Explorer cluster).",
            )
            .into());
        }
    };
    Ok(json!({
        "runSettings": { "executionIntervalInSeconds": s.interval },
        "query": { "queryString": s.kql },
        "eventhouseItem": eventhouse_item,
    }))
}

/// Build the `action` object (email or Teams) from the typed spec.
fn build_action(s: &TypedRuleSpec) -> Result<Value> {
    if s.recipients.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one recipient is required.".to_string(),
            "Pass --recipients alice@contoso.com[,bob@contoso.com].",
        )
        .into());
    }
    let arg =
        |name: &str, v: Value| json!({ "name": name, "isColumnReference": false, "value": v });
    let headline = s.headline.unwrap_or(s.name);
    let body = s
        .message
        .map_or_else(|| format!("Rule '{}' fired.", s.name), ToString::to_string);

    match s.action.to_ascii_lowercase().as_str() {
        "email" => Ok(json!({
            "actionType": "email",
            "arguments": [
                arg("messageLocale", json!({ "type": "string", "value": s.locale })),
                arg("to", json!({ "type": "array", "value": s.recipients })),
                arg("subject", json!({ "type": "string", "value": s.subject.unwrap_or(s.name) })),
                arg("body", json!({ "type": "string", "value": body })),
                arg("headline", json!({ "type": "string", "value": headline })),
            ],
        })),
        "teams" => Ok(json!({
            "actionType": "TeamsMessage",
            "arguments": [
                arg("messageLocale", json!({ "type": "string", "value": s.locale })),
                arg("recipientEmail", json!({ "type": "string", "value": s.recipients[0] })),
                arg("headline", json!({ "type": "string", "value": headline })),
                arg("message", json!({ "type": "string", "value": body })),
            ],
        })),
        other => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --action '{other}'."),
            "Valid values: email, teams.",
        )
        .into()),
    }
}

/// Build the full `createRuleParams` from the typed spec (fabio injects
/// `artifactId`/`workspaceId`).
fn build_create_rule_params(workspace: &str, id: &str, s: &TypedRuleSpec) -> Result<Value> {
    let source = build_source(workspace, s)?;
    let action = build_action(s)?;
    Ok(json!({
        "artifactId": id,
        "workspaceId": workspace,
        "name": s.name,
        "description": s.description.unwrap_or(s.name),
        "source": source,
        "model": {
            "stream": { "splitColumn": s.split_column.unwrap_or(""), "filters": [] },
            "detection": {
                "condition": {
                    "conditionType": s.condition,
                    "arguments": [
                        { "name": "Column", "isColumnReference": true, "value": { "type": "string", "value": s.column } },
                        { "name": "Value", "isColumnReference": false, "value": typed_value(s.value) },
                    ],
                },
                "occurrence": { "occurrenceType": "everyTime", "arguments": [] },
            },
            "action": action,
        },
    }))
}

/// Create a monitoring rule in a reflex via the MCP `create_rule` tool. Either a
/// full `createRuleParams` JSON spec (`rule`) or the typed convenience `spec` is
/// used; `rule` takes precedence. fabio always injects `artifactId`/`workspaceId`.
pub(super) async fn create_rule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    rule: Option<&str>,
    spec: Option<TypedRuleSpec<'_>>,
) -> Result<()> {
    // Resolve the createRuleParams: raw JSON (inline / @file / stdin) or typed.
    let mut params = if let Some(_r) = rule {
        let text = crate::commands::query_input::resolve_query_input(
            rule,
            "rule JSON",
            "--rule",
            "Example: fabio reflex create-rule --workspace <WS> --id <ID> --rule @rule.json",
        )?;
        let mut v: Value = serde_json::from_str(&text).map_err(|e| {
            FabioError::new(ErrorCode::InvalidInput, format!("Invalid --rule JSON: {e}"))
        })?;
        if !v.is_object() {
            return Err(FabioError::new(
                ErrorCode::InvalidInput,
                "--rule must be a JSON object (the createRuleParams payload).".to_string(),
            )
            .into());
        }
        // Inject/override the target ids so the payload always targets this reflex.
        v["artifactId"] = Value::from(id);
        v["workspaceId"] = Value::from(workspace);
        v
    } else if let Some(s) = spec {
        build_create_rule_params(workspace, id, &s)?
    } else {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "No rule definition provided.".to_string(),
            "Provide --rule <json|@file> for full control, or the typed flags \
             (--name --kql --column --condition --value --recipients + a source).",
        )
        .into());
    };

    let rule_name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("(unnamed)")
        .to_string();

    if output::dry_run_guard(
        cli,
        "reflex create-rule",
        &json!({ "workspace": workspace, "id": id, "name": rule_name, "tool": "create_rule", "params": params }),
    ) {
        return Ok(());
    }
    guard_readonly(cli, "create_rule")?;

    // Ensure the injected ids are present even if a passthrough payload omitted them.
    params["artifactId"] = Value::from(id);
    params["workspaceId"] = Value::from(workspace);

    let result = call_reflex_tool(
        client,
        workspace,
        id,
        "create_rule",
        json!({ "createRuleParams": params }),
    )
    .await?;
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
    use super::{TypedRuleSpec, build_create_rule_params, build_mcp_url, typed_value};
    use crate::client;
    use serde_json::json;

    fn base_spec<'a>(action: &'a str, recipients: &'a [String]) -> TypedRuleSpec<'a> {
        TypedRuleSpec {
            name: "High CPU",
            description: None,
            kql: "Metrics | project cpu",
            interval: 300,
            eventhouse_id: Some("kqldb-1"),
            eventhouse_workspace: None,
            cluster: None,
            database: None,
            split_column: None,
            column: "cpu",
            condition: "isGreaterThan",
            value: "90",
            action,
            recipients,
            subject: None,
            message: None,
            headline: None,
            locale: "en-US",
        }
    }

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

    #[test]
    fn typed_value_infers_type() {
        assert_eq!(typed_value("90"), json!({"type":"number","value":90.0}));
        assert_eq!(typed_value("true"), json!({"type":"boolean","value":true}));
        assert_eq!(
            typed_value("Error"),
            json!({"type":"string","value":"Error"})
        );
    }

    #[test]
    fn build_params_email_matches_verified_shape() {
        // Mirrors the live-verified payload: condition args are typed
        // {name,isColumnReference,value{type,value}}; email args are
        // messageLocale/to/subject/body/headline.
        let recips = vec!["alice@contoso.com".to_string()];
        let params = build_create_rule_params("ws", "rx", &base_spec("email", &recips)).unwrap();
        assert_eq!(params["artifactId"], "rx");
        assert_eq!(params["workspaceId"], "ws");
        assert_eq!(
            params["source"]["eventhouseItem"]["itemType"],
            "KustoDatabase"
        );
        assert_eq!(params["source"]["eventhouseItem"]["workspaceId"], "ws"); // defaults to reflex ws
        let cond = &params["model"]["detection"]["condition"];
        assert_eq!(cond["conditionType"], "isGreaterThan");
        assert_eq!(cond["arguments"][0]["name"], "Column");
        assert_eq!(cond["arguments"][0]["isColumnReference"], true);
        assert_eq!(cond["arguments"][0]["value"]["value"], "cpu");
        assert_eq!(
            cond["arguments"][1]["value"],
            json!({"type":"number","value":90.0})
        );
        let action = &params["model"]["action"];
        assert_eq!(action["actionType"], "email");
        let names: Vec<&str> = action["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["messageLocale", "to", "subject", "body", "headline"]
        );
        assert_eq!(
            action["arguments"][1]["value"],
            json!({"type":"array","value":["alice@contoso.com"]})
        );
    }

    #[test]
    fn build_params_teams_uses_teamsmessage_and_recipient_email() {
        let recips = vec!["bob@contoso.com".to_string()];
        let params = build_create_rule_params("ws", "rx", &base_spec("teams", &recips)).unwrap();
        let action = &params["model"]["action"];
        assert_eq!(action["actionType"], "TeamsMessage");
        let names: Vec<&str> = action["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["messageLocale", "recipientEmail", "headline", "message"]
        );
        assert_eq!(action["arguments"][1]["value"]["value"], "bob@contoso.com");
    }

    #[test]
    fn build_params_requires_a_source() {
        let recips = vec!["a@b.com".to_string()];
        let mut s = base_spec("email", &recips);
        s.eventhouse_id = None; // no source at all
        assert!(build_create_rule_params("ws", "rx", &s).is_err());
    }

    #[test]
    fn build_params_requires_recipients() {
        let empty: Vec<String> = vec![];
        assert!(build_create_rule_params("ws", "rx", &base_spec("email", &empty)).is_err());
    }
}
