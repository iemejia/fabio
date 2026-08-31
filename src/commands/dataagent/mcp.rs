//! Data-agent runtime consumption over MCP.
//!
//! A *published* Fabric data agent exposes an MCP server with a **single tool**
//! (the `OpenAI` Assistants API that previously backed this path was retired by
//! `OpenAI` on 2026-08-26). This module is a thin consumer of the generic
//! [`crate::mcp_client`]: it connects, discovers the single tool, binds the
//! question to that tool's primary argument, and returns the text answer. All
//! transport (JSON-RPC framing, initialize handshake, JSON/SSE parsing,
//! session handling) lives in the shared client.

use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};

use crate::errors::{ErrorCode, FabioError};
use crate::mcp_client::{McpClient, primary_tool_argument};

/// Result of a single MCP `tools/call` against a data agent.
pub(super) struct McpAnswer {
    /// The concatenated text content of the answer.
    pub answer: String,
    /// The raw MCP tool result (content blocks + any `structuredContent`),
    /// surfaced under `--raw`.
    pub raw: Value,
    /// The discovered tool name (for transparency in the output envelope).
    pub tool: String,
}

/// Ask a published data agent a single question over its MCP endpoint.
///
/// `max_wait` bounds the whole exchange (the tool call itself can take minutes
/// while the agent runs SQL/DAX).
pub(super) async fn run_mcp_query(
    mcp_url: &str,
    token: &str,
    question: &str,
    max_wait: Duration,
) -> Result<McpAnswer> {
    // `token` is already a full Authorization header value ("Bearer <jwt>")
    // from `require_auth()`; pass it through unchanged (do NOT re-prefix).
    let client =
        McpClient::connect_with_timeout(mcp_url, Some(token.to_string()), max_wait).await?;

    // A published data agent exposes exactly one tool; discover it dynamically.
    let tools = client.list_tools().await?;
    let tool = tools.first().ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::ApiError,
            "The data agent's MCP server exposed no tools.",
            "A published data agent exposes exactly one query tool. Verify the agent is published \
             (fabio data-agent publish) and that you have access to it.",
        )
    })?;
    let tool_name = tool
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "MCP tools/list returned a tool without a name.",
            )
        })?
        .to_string();
    let question_arg = primary_tool_argument(tool).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::ApiError,
            format!("MCP tool '{tool_name}' exposes no input properties to carry the question."),
            "The data agent's MCP tool schema is unexpected. Verify the agent is published and \
             reachable, and retry.",
        )
    })?;

    let result = client
        .call_tool(&tool_name, json!({ question_arg: question }))
        .await?;
    if result.is_error {
        return Err(FabioError::with_hint(
            ErrorCode::ApiError,
            format!("The data agent returned an error: {}", result.text()),
            "Check the agent's data sources and instructions: fabio data-agent get-config \
             --workspace <WS> --id <ID>. Verify the capacity is active.",
        )
        .into());
    }

    Ok(McpAnswer {
        answer: if result.text().is_empty() {
            "(No response from data agent)".to_string()
        } else {
            result.text()
        },
        raw: result.raw,
        tool: tool_name,
    })
}
