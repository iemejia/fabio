//! Minimal Model Context Protocol (MCP) Streamable-HTTP client for consuming a
//! published Fabric data agent at runtime.
//!
//! This replaces the retired `OpenAI` Assistants API path. Per the official
//! Fabric data agent SDK, a published data agent exposes an MCP server with a
//! **single tool**: the client `initialize`s, discovers the tool via
//! `tools/list`, then `tools/call`s it with the natural-language question and
//! reads the text answer from the result content blocks.
//!
//! Transport: MCP Streamable HTTP (a single POST endpoint that answers with
//! either `application/json` or a `text/event-stream` SSE body). Authentication
//! reuses the caller's Fabric bearer token.

use std::time::Duration;

use anyhow::Result;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{Value, json};

use crate::client;
use crate::errors::{ErrorCode, FabioError};

/// MCP protocol version fabio advertises on `initialize` (a recent stable spec
/// revision). The server negotiates down if it speaks an older revision.
const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Result of a single MCP `tools/call` against a data agent.
pub(super) struct McpAnswer {
    /// The concatenated text content of the answer.
    pub answer: String,
    /// The raw MCP tool result (all content blocks + any `structuredContent`),
    /// surfaced under `--raw` for debugging / structured consumption.
    pub raw: Value,
    /// The discovered tool name (for transparency in the output envelope).
    pub tool: String,
}

/// Ask a published data agent a single question over its MCP endpoint.
///
/// Mirrors the official SDK flow: initialize -> tools/list -> tools/call ->
/// extract text. `max_wait` bounds the whole exchange (the tool call itself can
/// take minutes while the agent runs SQL/DAX).
pub(super) async fn run_mcp_query(
    mcp_url: &str,
    token: &str,
    question: &str,
    max_wait: Duration,
) -> Result<McpAnswer> {
    let http = client::http_client_builder()
        .timeout(max_wait)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| {
            FabioError::with_hint(
                ErrorCode::NetworkError,
                e.to_string(),
                "Failed to build the HTTP client for the MCP request.",
            )
        })?;

    let mut session = McpSession::new(&http, mcp_url, token);

    session.initialize().await?;
    session.notify_initialized().await?;
    let tool = session.list_first_tool().await?;

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
    let question_arg = first_input_property(&tool).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::ApiError,
            format!("MCP tool '{tool_name}' exposes no input properties to carry the question."),
            "The data agent's MCP tool schema is unexpected. Verify the agent is published and \
             reachable, and retry.",
        )
    })?;

    let result = session
        .call_tool(&tool_name, &question_arg, question)
        .await?;

    let answer = extract_text(&result);
    Ok(McpAnswer {
        answer,
        raw: result,
        tool: tool_name,
    })
}

/// A single MCP Streamable-HTTP session (initialize + subsequent calls share the
/// negotiated `Mcp-Session-Id`).
struct McpSession<'a> {
    http: &'a reqwest::Client,
    url: &'a str,
    token: &'a str,
    session_id: Option<String>,
    next_id: i64,
}

impl<'a> McpSession<'a> {
    const fn new(http: &'a reqwest::Client, url: &'a str, token: &'a str) -> Self {
        Self {
            http,
            url,
            token,
            session_id: None,
            next_id: 1,
        }
    }

    /// Perform the `initialize` handshake and capture the session id.
    async fn initialize(&mut self) -> Result<()> {
        let params = json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "fabio", "version": env!("CARGO_PKG_VERSION") },
        });
        self.request("initialize", params).await?;
        Ok(())
    }

    /// Send the `notifications/initialized` notification (no response expected).
    async fn notify_initialized(&mut self) -> Result<()> {
        self.notify("notifications/initialized").await
    }

    /// Discover the data agent's single tool via `tools/list`.
    async fn list_first_tool(&mut self) -> Result<Value> {
        let result = self.request("tools/list", Value::Null).await?;
        result
            .get("tools")
            .and_then(Value::as_array)
            .and_then(|t| t.first())
            .cloned()
            .ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::ApiError,
                    "The data agent's MCP server exposed no tools.",
                    "A published data agent exposes exactly one query tool. Verify the agent is \
                     published (fabio data-agent publish) and that you have access to it.",
                )
                .into()
            })
    }

    /// Call the discovered tool with the question bound to `question_arg`.
    async fn call_tool(
        &mut self,
        tool_name: &str,
        question_arg: &str,
        question: &str,
    ) -> Result<Value> {
        let params = json!({
            "name": tool_name,
            "arguments": { question_arg: question },
        });
        let result = self.request("tools/call", params).await?;
        // A tool that failed sets isError=true and puts the message in content.
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            let msg = extract_text(&result);
            return Err(FabioError::with_hint(
                ErrorCode::ApiError,
                format!("The data agent returned an error: {msg}"),
                "Check the agent's data sources and instructions: fabio data-agent get-config \
                 --workspace <WS> --id <ID>. Verify the capacity is active.",
            )
            .into());
        }
        Ok(result)
    }

    /// Send a JSON-RPC request and return its `result` (errors are mapped).
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut body = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if !params.is_null() {
            body["params"] = params;
        }

        let resp = self.post(&body).await?;
        let status = resp.status();
        self.capture_session_id(&resp);

        if !status.is_success() {
            let code = status.as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(enrich_mcp_error(code, &text, self.url, method).into());
        }

        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = resp.text().await.map_err(|e| {
            FabioError::new(
                ErrorCode::NetworkError,
                format!("Failed to read MCP {method} response body: {e}"),
            )
        })?;

        let message = if content_type.contains("text/event-stream") {
            parse_sse_for_id(&text, id)?
        } else {
            serde_json::from_str::<Value>(&text).map_err(|e| {
                FabioError::new(
                    ErrorCode::ApiError,
                    format!("Failed to parse MCP {method} response: {e}"),
                )
            })?
        };

        if let Some(err) = message.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown MCP error");
            return Err(FabioError::with_hint(
                ErrorCode::ApiError,
                format!("MCP {method} failed: {msg}"),
                "The data agent MCP server rejected the request. Verify the agent is published \
                 and you have access to its data sources.",
            )
            .into());
        }

        message.get("result").cloned().ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                format!("MCP {method} response contained no result."),
            )
            .into()
        })
    }

    /// Send a JSON-RPC notification (no id, no response body expected).
    async fn notify(&mut self, method: &str) -> Result<()> {
        let body = json!({ "jsonrpc": "2.0", "method": method });
        let resp = self.post(&body).await?;
        self.capture_session_id(&resp);
        // Notifications are answered with 202 Accepted (or 200) and no JSON-RPC
        // body; a non-success status is still worth surfacing.
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(enrich_mcp_error(code, &text, self.url, method).into());
        }
        Ok(())
    }

    /// POST a JSON-RPC body with the standard MCP headers + session id.
    async fn post(&self, body: &Value) -> Result<reqwest::Response> {
        let mut req = self
            .http
            .post(self.url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION);
        if let Some(sid) = &self.session_id {
            req = req.header("Mcp-Session-Id", sid);
        }
        req.json(body).send().await.map_err(|e| {
            FabioError::with_hint(
                ErrorCode::NetworkError,
                format!("MCP request failed: {e}"),
                "Verify the data agent is published and reachable: fabio data-agent mcp-url \
                 --workspace <WS> --id <ID>.",
            )
            .into()
        })
    }

    /// Capture the `Mcp-Session-Id` header (set by the server on initialize) so
    /// it is echoed on subsequent requests.
    fn capture_session_id(&mut self, resp: &reqwest::Response) {
        if self.session_id.is_none()
            && let Some(sid) = resp
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
        {
            self.session_id = Some(sid.to_string());
        }
    }
}

/// Choose the tool input property that carries the question.
///
/// Prefers a conventionally-named property (question/query/prompt/input/text),
/// then the first `required` property, then the first declared property. With
/// `serde_json`'s `preserve_order` this matches the SDK's `next(iter(props))`
/// for the common single-property tool while staying robust to multi-property
/// schemas and key ordering. Pure for unit testing.
fn first_input_property(tool: &Value) -> Option<String> {
    const PREFERRED: [&str; 5] = ["question", "query", "prompt", "input", "text"];

    let schema = tool.get("inputSchema")?;
    let props = schema.get("properties").and_then(Value::as_object)?;
    if props.is_empty() {
        return None;
    }

    for want in PREFERRED {
        if let Some(key) = props.keys().find(|k| k.eq_ignore_ascii_case(want)) {
            return Some(key.clone());
        }
    }
    if let Some(first_required) = schema
        .get("required")
        .and_then(Value::as_array)
        .and_then(|r| r.first())
        .and_then(Value::as_str)
    {
        return Some(first_required.to_string());
    }
    props.keys().next().cloned()
}

/// Extract and concatenate the text content blocks from an MCP tool result.
///
/// The MCP `tools/call` result has a `content` array of typed blocks; the data
/// agent's answer is carried in `text` blocks. Pure for unit testing.
fn extract_text(result: &Value) -> String {
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if text.is_empty() {
        "(No response from data agent)".to_string()
    } else {
        text
    }
}

/// Parse a Streamable-HTTP SSE body and return the JSON-RPC message whose `id`
/// matches the request. Server-initiated notifications/requests (no id or a
/// different id) are skipped. Pure for unit testing.
fn parse_sse_for_id(body: &str, id: i64) -> Result<Value> {
    let mut data_lines: Vec<&str> = Vec::new();
    let mut candidate: Option<Value> = None;

    let flush = |lines: &mut Vec<&str>, candidate: &mut Option<Value>| {
        if lines.is_empty() {
            return;
        }
        let payload = lines.join("\n");
        lines.clear();
        if let Ok(v) = serde_json::from_str::<Value>(&payload)
            && v.get("id").and_then(Value::as_i64) == Some(id)
        {
            *candidate = Some(v);
        }
    };

    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        } else if line.is_empty() {
            flush(&mut data_lines, &mut candidate);
            if candidate.is_some() {
                break;
            }
        }
    }
    flush(&mut data_lines, &mut candidate);

    candidate.ok_or_else(|| {
        FabioError::new(
            ErrorCode::ApiError,
            "No matching MCP response found in the SSE stream.",
        )
        .into()
    })
}

/// Map an HTTP failure from the MCP endpoint to an actionable fabio error.
fn enrich_mcp_error(status: u16, body: &str, url: &str, method: &str) -> FabioError {
    match status {
        404 => FabioError::with_hint(
            ErrorCode::NotFound,
            format!("MCP endpoint returned 404 for {method}: {body}"),
            format!(
                "The data agent MCP endpoint was not found. The agent must be PUBLISHED for its \
                 MCP server to exist. Publish it (fabio data-agent publish), then retry. URL: {url}"
            ),
        ),
        401 => FabioError::with_hint(
            ErrorCode::AuthRequired,
            format!("MCP endpoint returned 401 for {method}: {body}"),
            "Authentication failed. Re-run 'fabio auth login' and ensure you have access to the \
             data agent and its data sources.",
        ),
        403 => FabioError::with_hint(
            ErrorCode::Forbidden,
            format!("MCP endpoint returned 403 for {method}: {body}"),
            "You lack permission for this data agent. You need at least Viewer on the workspace \
             and read access to the agent's underlying data sources.",
        ),
        429 => FabioError::with_hint(
            ErrorCode::RateLimited,
            format!("MCP endpoint rate-limited {method}: {body}"),
            "Wait before retrying. If this persists, the Fabric capacity may be under heavy load.",
        ),
        _ => FabioError::from_status(status, format!("MCP {method} failed: {body}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_input_property_prefers_conventional_name() {
        let tool = json!({
            "inputSchema": { "properties": { "foo": {}, "question": {}, "bar": {} } }
        });
        assert_eq!(first_input_property(&tool).as_deref(), Some("question"));
    }

    #[test]
    fn first_input_property_is_case_insensitive() {
        let tool = json!({ "inputSchema": { "properties": { "Query": {} } } });
        assert_eq!(first_input_property(&tool).as_deref(), Some("Query"));
    }

    #[test]
    fn first_input_property_falls_back_to_required_then_first() {
        // No conventional name: prefer the first required property.
        let tool = json!({
            "inputSchema": {
                "properties": { "alpha": {}, "beta": {} },
                "required": ["beta"]
            }
        });
        assert_eq!(first_input_property(&tool).as_deref(), Some("beta"));

        // No conventional name and no required: first declared property
        // (preserve_order keeps insertion order).
        let tool = json!({ "inputSchema": { "properties": { "alpha": {}, "beta": {} } } });
        assert_eq!(first_input_property(&tool).as_deref(), Some("alpha"));
    }

    #[test]
    fn first_input_property_none_when_empty() {
        let tool = json!({ "inputSchema": { "properties": {} } });
        assert!(first_input_property(&tool).is_none());
        let tool = json!({ "inputSchema": {} });
        assert!(first_input_property(&tool).is_none());
    }

    #[test]
    fn extract_text_joins_text_blocks_only() {
        let result = json!({
            "content": [
                { "type": "text", "text": "The total is 42." },
                { "type": "image", "data": "..." },
                { "type": "text", "text": "Across all regions." }
            ]
        });
        assert_eq!(
            extract_text(&result),
            "The total is 42.\nAcross all regions."
        );
    }

    #[test]
    fn extract_text_defaults_when_no_text() {
        assert_eq!(
            extract_text(&json!({ "content": [] })),
            "(No response from data agent)"
        );
        assert_eq!(extract_text(&json!({})), "(No response from data agent)");
    }

    #[test]
    fn parse_sse_returns_matching_id() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n";
        let v = parse_sse_for_id(body, 7).unwrap();
        assert_eq!(v["result"]["ok"], true);
    }

    #[test]
    fn parse_sse_skips_non_matching_and_notifications() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"answer\":\"hi\"}}\n\n";
        let v = parse_sse_for_id(body, 2).unwrap();
        assert_eq!(v["result"]["answer"], "hi");
    }

    #[test]
    fn parse_sse_handles_multiline_data() {
        // Per SSE, consecutive data: lines are joined with newlines.
        let body = "data: {\"jsonrpc\":\"2.0\",\"id\":1,\ndata: \"result\":{\"v\":5}}\n\n";
        let v = parse_sse_for_id(body, 1).unwrap();
        assert_eq!(v["result"]["v"], 5);
    }

    #[test]
    fn parse_sse_errors_without_match() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"id\":99,\"result\":{}}\n\n";
        assert!(parse_sse_for_id(body, 1).is_err());
    }
}
