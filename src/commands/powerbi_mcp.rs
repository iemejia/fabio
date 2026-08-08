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
use crate::errors::{ErrorCode, FabioError, HintType};
use crate::mcp_client::{McpClient, ToolResult};

/// Build the remote Power BI MCP server URL from the Fabric API base.
/// The base is e.g. `https://api.fabric.microsoft.com/v1`; the endpoint is a
/// single fixed global URL (not per-item).
pub fn powerbi_mcp_url(base: &str) -> String {
    format!("{}/mcp/powerbi", base.trim_end_matches('/'))
}

/// Does an MCP connect error indicate the Power BI MCP feature is DISABLED for
/// the tenant? A 403 at the `initialize` handshake (the endpoint itself rejects
/// the caller) is the tenant-setting gate — model/report permission problems
/// surface LATER at `call_tool`, not at connect. So a connect-time `Forbidden`
/// (or a `FeatureNotAvailable` / "feature is not available" / HTTP 403 body) is
/// the feature-disabled signal. Pure/testable.
fn is_feature_disabled(err: &anyhow::Error) -> bool {
    if let Some(fe) = err.downcast_ref::<FabioError>()
        && fe.code == ErrorCode::Forbidden
    {
        return true;
    }
    let s = err.to_string().to_ascii_lowercase();
    s.contains("featurenotavailable")
        || s.contains("feature is not available")
        || s.contains("http 403")
}

/// The teaching error to surface when the Power BI MCP feature is disabled: it
/// names the exact tenant setting, tells the agent NOT to retry these commands,
/// and enumerates the non-MCP fallbacks that work without the feature.
fn feature_disabled_error() -> FabioError {
    FabioError::with_typed_hint(
        ErrorCode::Forbidden,
        "The remote Power BI MCP server is not enabled for this tenant (the feature is disabled).",
        "A Fabric administrator must enable the tenant setting \"Users can use the Power BI Model \
         Context Protocol server endpoint (preview)\" (PowerBIMCP) in the Admin portal. Until it is \
         enabled, do NOT retry `semantic-model generate-dax`/`copilot-schema` or \
         `report copilot-metadata` — they will keep failing. Non-MCP fallbacks that need no Copilot: \
         run DAX with `semantic-model query --dax`; read the model schema with \
         `semantic-model list-tables`/`list-columns`/`list-measures`/`list-relationships`; read a \
         report's definition with `report get-definition`.",
        HintType::SemanticCorrection,
    )
}

/// Connect to the Power BI MCP server and invoke a single tool. Validates the
/// endpoint is an HTTPS trusted-Microsoft host before sending the Fabric bearer
/// token, confirms the tool exists, then calls it. When the tenant setting that
/// gates the feature is disabled, the connect 403 is mapped to a teaching error
/// (`feature_disabled_error`) so an agent learns to stop using these commands.
pub async fn call_powerbi_tool(
    client: &FabricClient,
    tool: &str,
    arguments: Value,
) -> Result<ToolResult> {
    let url = powerbi_mcp_url(client::fabric_base_url());
    // HTTPS + trusted-Microsoft-host check before sending the Fabric bearer token.
    client::validate_trusted_url(&url, "Power BI MCP endpoint")?;
    let auth = client.require_auth().await?;
    let mcp = McpClient::connect(&url, Some(auth)).await.map_err(|e| {
        if is_feature_disabled(&e) {
            feature_disabled_error().into()
        } else {
            e
        }
    })?;

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
        };
        assert_eq!(tool_text_as_json(&r)["daxQuery"], "EVALUATE X");
    }

    #[test]
    fn tool_text_as_json_wraps_non_json_text() {
        let r = ToolResult {
            content: vec![serde_json::json!({"type":"text","text":"plain message"})],
            is_error: false,
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
        };
        let v = tool_text_as_json(&r);
        assert!(v.get("schema").is_some());
        assert_eq!(v["artifact_citation"], "cite");
    }

    #[test]
    fn is_feature_disabled_detects_forbidden_fabio_error() {
        // A connect-time Forbidden (the PowerBIMCP tenant setting is off) → true.
        let e: anyhow::Error = FabioError::new(
            ErrorCode::Forbidden,
            "MCP initialize failed: HTTP 403 Forbidden",
        )
        .into();
        assert!(is_feature_disabled(&e));
    }

    #[test]
    fn is_feature_disabled_detects_feature_not_available_string() {
        let e = anyhow::anyhow!("MCP initialize error -32003: The feature is not available");
        assert!(is_feature_disabled(&e));
        let e2 = anyhow::anyhow!("FeatureNotAvailable");
        assert!(is_feature_disabled(&e2));
    }

    #[test]
    fn is_feature_disabled_false_for_unrelated_errors() {
        // A tool-level NotFound (bad model id) must NOT be treated as the feature
        // being disabled.
        let e: anyhow::Error =
            FabioError::new(ErrorCode::NotFound, "artifact 123 not found").into();
        assert!(!is_feature_disabled(&e));
        let e2 = anyhow::anyhow!("network timeout");
        assert!(!is_feature_disabled(&e2));
    }

    #[test]
    fn feature_disabled_error_names_the_tenant_setting_and_fallbacks() {
        let err = feature_disabled_error();
        assert_eq!(err.code, ErrorCode::Forbidden);
        let hint = err.hint.unwrap();
        assert!(hint.contains("PowerBIMCP"));
        assert!(
            hint.contains("query --dax"),
            "should suggest a non-MCP fallback"
        );
        // hintType tells agents this is a config gate (don't blindly retry).
        assert_eq!(err.hint_type, Some(HintType::SemanticCorrection));
    }
}
