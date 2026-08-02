//! Generic Model Context Protocol (MCP) **client** over the streamable-HTTP
//! transport (<https://modelcontextprotocol.io/>).
//!
//! This lets fabio CONSUME external MCP servers — connect to a server, run the
//! `initialize` handshake, and call its tools. It is the counterpart of
//! `fabio mcp serve` (which makes fabio an MCP *server*). The client is generic:
//! it takes an endpoint URL + an optional `Authorization` header value and
//! exposes `list_tools`/`call_tool`. The first consumer is `ontology search`
//! (the Fabric ontology MCP server's `search_ontology` tool), but nothing here
//! is ontology-specific.
//!
//! Transport notes (streamable HTTP): the client POSTs a JSON-RPC message to a
//! single endpoint and the server responds with EITHER `application/json` (one
//! response) OR `text/event-stream` (an SSE stream whose events carry the
//! response) — both are handled. If the server assigns a session via the
//! `Mcp-Session-Id` response header on `initialize`, it is echoed on subsequent
//! requests (stateless servers, like Fabric's, simply omit it).

use std::time::Duration;

use serde_json::{Value, json};

use crate::client::{http_client_builder, is_secure_or_loopback};
use crate::errors::{ErrorCode, FabioError};

/// The MCP protocol version this client advertises on `initialize`.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// A connected MCP client (streamable-HTTP transport).
pub struct McpClient {
    http: reqwest::Client,
    endpoint: String,
    auth_header: Option<String>,
    session_id: Option<String>,
    negotiated_version: String,
}

/// The result of a `tools/call`.
pub struct ToolResult {
    /// Raw MCP content blocks (each `{"type":"text","text":...}` etc.).
    pub content: Vec<Value>,
    /// Whether the tool reported an error result.
    pub is_error: bool,
}

impl ToolResult {
    /// Concatenate all `text`-type content blocks (newline-joined).
    #[must_use]
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter(|c| c.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|c| c.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl McpClient {
    /// Connect to an MCP server: validate the endpoint is HTTPS (loopback `http`
    /// is allowed for local servers), then perform the `initialize` handshake and
    /// send `notifications/initialized`. `auth_header` is the full `Authorization`
    /// header value (e.g. `"Bearer …"`), or `None` for unauthenticated servers.
    pub async fn connect(endpoint: &str, auth_header: Option<String>) -> anyhow::Result<Self> {
        if !is_secure_or_loopback(endpoint) {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Refusing to connect to a non-HTTPS MCP endpoint: {endpoint}"),
                "MCP endpoints must use https:// (loopback http is allowed only for local servers).",
            )
            .into());
        }
        let http = http_client_builder()
            .timeout(Duration::from_mins(5))
            .build()
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        let mut client = Self {
            http,
            endpoint: endpoint.to_string(),
            auth_header,
            session_id: None,
            negotiated_version: MCP_PROTOCOL_VERSION.to_string(),
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&mut self) -> anyhow::Result<()> {
        let params = json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "fabio", "version": env!("CARGO_PKG_VERSION")},
        });
        let (result, session) = self.send_request("initialize", &params).await?;
        if session.is_some() {
            self.session_id = session;
        }
        if let Some(v) = result.get("protocolVersion").and_then(Value::as_str) {
            self.negotiated_version = v.to_string();
        }
        // Best-effort: a stateless server may reject/ignore this notification.
        let _ = self
            .send_notification("notifications/initialized", &json!({}))
            .await;
        Ok(())
    }

    /// List the tools the server exposes (`tools/list`).
    pub async fn list_tools(&self) -> anyhow::Result<Vec<Value>> {
        let (result, _) = self.send_request("tools/list", &json!({})).await?;
        Ok(result
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// Invoke a tool by name with the given arguments (`tools/call`).
    pub async fn call_tool(&self, name: &str, arguments: Value) -> anyhow::Result<ToolResult> {
        let (result, _) = self
            .send_request("tools/call", &json!({"name": name, "arguments": arguments}))
            .await?;
        Ok(ToolResult {
            content: result
                .get("content")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            is_error: result
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    /// Send a JSON-RPC request and return `(result, session_id_from_header)`.
    async fn send_request(
        &self,
        method: &str,
        params: &Value,
    ) -> anyhow::Result<(Value, Option<String>)> {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let resp = self.post(&body).await?;
        let status = resp.status();
        let session = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = resp
            .text()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if !status.is_success() {
            return Err(FabioError::from_status_with_body(
                status.as_u16(),
                format!("MCP {method} failed: HTTP {status}"),
                &text,
            )
            .into());
        }

        let envelope = parse_rpc_response(&content_type, &text)?;
        if let Some(err) = envelope.get("error") {
            let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(FabioError::new(
                ErrorCode::ApiError,
                format!("MCP {method} error {code}: {message}"),
            )
            .into());
        }
        Ok((
            envelope.get("result").cloned().unwrap_or(Value::Null),
            session,
        ))
    }

    async fn send_notification(&self, method: &str, params: &Value) -> anyhow::Result<()> {
        let body = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.post(&body).await?;
        Ok(())
    }

    async fn post(&self, body: &Value) -> anyhow::Result<reqwest::Response> {
        let mut req = self
            .http
            .post(&self.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header("MCP-Protocol-Version", &self.negotiated_version);
        if let Some(auth) = &self.auth_header {
            req = req.header(reqwest::header::AUTHORIZATION, auth);
        }
        if let Some(sid) = &self.session_id {
            req = req.header("Mcp-Session-Id", sid);
        }
        req.json(body).send().await.map_err(|e| {
            FabioError::with_hint(
                ErrorCode::NetworkError,
                format!("Failed to reach MCP endpoint: {e}"),
                "Check the endpoint URL and network connectivity.",
            )
            .into()
        })
    }
}

/// Parse an `application/json` or `text/event-stream` body into the JSON-RPC
/// response envelope (the object carrying `result` or `error`).
fn parse_rpc_response(content_type: &str, body: &str) -> anyhow::Result<Value> {
    if content_type.contains("text/event-stream") {
        for data in sse_data_blocks(body) {
            if let Ok(v) = serde_json::from_str::<Value>(&data)
                && (v.get("result").is_some() || v.get("error").is_some())
            {
                return Ok(v);
            }
        }
        return Err(FabioError::new(
            ErrorCode::ApiError,
            "No JSON-RPC response found in MCP SSE stream".to_string(),
        )
        .into());
    }
    serde_json::from_str::<Value>(body).map_err(|e| {
        let preview: String = body.chars().take(200).collect();
        FabioError::with_hint(
            ErrorCode::ApiError,
            format!("Invalid MCP response: {e}"),
            format!("Server returned: {preview}"),
        )
        .into()
    })
}

/// Extract the `data:` payloads from an SSE body, one string per event (multiple
/// `data:` lines within an event are newline-joined per the SSE spec).
fn sse_data_blocks(body: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    for line in body.lines() {
        if line.is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else if let Some(rest) = line.strip_prefix("data:") {
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(rest);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_application_json_result() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"t"}]}}"#;
        let env = parse_rpc_response("application/json; charset=utf-8", body).unwrap();
        assert_eq!(env["result"]["tools"][0]["name"], "t");
    }

    #[test]
    fn parses_sse_stream_result() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n";
        let env = parse_rpc_response("text/event-stream", body).unwrap();
        assert_eq!(env["result"]["ok"], true);
    }

    #[test]
    fn sse_multiline_data_is_joined() {
        let body = "data: {\"jsonrpc\":\"2.0\",\ndata: \"id\":1,\"result\":{}}\n\n";
        let blocks = sse_data_blocks(body);
        assert_eq!(blocks.len(), 1);
        assert!(serde_json::from_str::<Value>(&blocks[0]).is_ok());
    }

    #[test]
    fn tool_result_text_concatenates_text_blocks() {
        let r = ToolResult {
            content: vec![
                json!({"type": "text", "text": "hello"}),
                json!({"type": "image", "data": "..."}),
                json!({"type": "text", "text": "world"}),
            ],
            is_error: false,
        };
        assert_eq!(r.text(), "hello\nworld");
    }

    #[test]
    fn invalid_json_body_is_error() {
        assert!(parse_rpc_response("application/json", "not json").is_err());
    }

    #[test]
    fn sse_without_response_is_error() {
        // A ping/keepalive event with no result/error.
        let body = "data: {\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n\n";
        assert!(parse_rpc_response("text/event-stream", body).is_err());
    }
}
