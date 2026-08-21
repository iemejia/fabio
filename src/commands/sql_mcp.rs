//! Execute T-SQL via the remote Fabric Data Warehouse MCP server (`--via-mcp`).
//!
//! An alternative execution backend for `warehouse query` / `sql-endpoint query`
//! that routes the T-SQL through the hosted remote MCP server's `execute_query`
//! tool instead of a direct native-TDS (`database.windows.net`) connection.
//!
//! Why: the remote MCP path authenticates with the **Fabric** token
//! (`api.fabric.microsoft.com`) — the same token every other fabio command uses —
//! so it eliminates fabio's biggest SQL friction, the separate
//! `database.windows.net`-audience token (`FABIO_SQL_ACCESS_TOKEN`). It also needs
//! no outbound TCP 1433, which helps in locked-down environments. It is opt-in:
//! native TDS remains the default (it is richer — typed values, execution plans,
//! DMV insights, statistics).
//!
//! The server returns results as an embedded `text/csv` resource plus a text
//! summary; this module parses the CSV into the SAME list-of-objects envelope the
//! TDS path produces, so `--via-mcp` is a drop-in for `query`. (CSV is untyped, so
//! all values render as strings — the one documented behavioral difference.)

use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{self, FabricClient};
use crate::commands::tds_utils::item_sql_mcp_url;
use crate::errors::{ErrorCode, FabioError};
use crate::mcp_client::McpClient;
use crate::output;

/// Candidate names for the server's T-SQL execution tool. The live server exposes
/// `execute_query`; the Microsoft docs call it `executeSQL`. Both are accepted.
const EXECUTE_TOOL_NAMES: [&str; 2] = ["execute_query", "executeSQL"];

/// Resolve the execution tool name from the server's advertised tool list.
///
/// Prefers a known name (`execute_query`/`executeSQL`); if the server exposes a
/// single tool under a different name, use it (forward-compatible). Returns `None`
/// when the server advertises no usable tool. Pure for testing.
#[must_use]
pub fn resolve_execute_tool(tools: &[Value]) -> Option<String> {
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect();
    if let Some(known) = names.iter().find(|n| EXECUTE_TOOL_NAMES.contains(n)) {
        return Some((*known).to_string());
    }
    // Forward-compatible: a single-tool server whose tool was renamed.
    if let [only] = names.as_slice() {
        return Some((*only).to_string());
    }
    None
}

/// Extract the first `text/csv` resource text from a tool result's content blocks.
#[must_use]
pub fn extract_csv_resource(content: &[Value]) -> Option<String> {
    content.iter().find_map(|block| {
        let resource = block.get("resource")?;
        let mime = resource.get("mimeType").and_then(Value::as_str)?;
        if mime.eq_ignore_ascii_case("text/csv") {
            resource
                .get("text")
                .and_then(Value::as_str)
                .map(String::from)
        } else {
            None
        }
    })
}

/// Concatenate the `text`-type content blocks (the server's human-readable summary,
/// e.g. "Query returned 2 rows.").
#[must_use]
pub fn extract_text_summary(content: &[Value]) -> String {
    content
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse a CSV document into `(column_names, rows)`, each row a JSON object keyed
/// by column name (all values strings — CSV is untyped). Mirrors the shape of the
/// TDS path's `execute_sql_rows` so rendering is identical.
pub fn parse_csv_to_rows(csv_text: &str) -> Result<(Vec<String>, Vec<Value>)> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(csv_text.as_bytes());
    let columns: Vec<String> = reader
        .headers()
        .map_err(|e| FabioError::new(ErrorCode::ApiError, format!("Invalid CSV from MCP: {e}")))?
        .iter()
        .map(str::to_string)
        .collect();

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record
            .map_err(|e| FabioError::new(ErrorCode::ApiError, format!("Invalid CSV row: {e}")))?;
        let mut obj = serde_json::Map::new();
        for (i, col) in columns.iter().enumerate() {
            obj.insert(col.clone(), Value::from(record.get(i).unwrap_or("")));
        }
        rows.push(Value::Object(obj));
    }
    Ok((columns, rows))
}

/// Execute `sql` against a Warehouse / SQL analytics endpoint item via the remote
/// MCP server and render the result (list envelope for a result set, scalar
/// summary otherwise).
pub async fn execute_via_mcp(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    sql: &str,
) -> Result<()> {
    let url = item_sql_mcp_url(client::fabric_base_url(), workspace, item_id);
    // HTTPS + trusted-Microsoft-host check before sending the Fabric bearer token.
    client::validate_trusted_url(&url, "query --via-mcp endpoint")?;
    let auth = client.require_auth().await?;

    let mcp = McpClient::connect(&url, Some(auth)).await?;
    let tools = mcp.list_tools().await?;
    let tool = resolve_execute_tool(&tools).ok_or_else(|| {
        let available: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect();
        FabioError::with_hint(
            ErrorCode::ApiError,
            format!(
                "The Fabric Data Warehouse MCP server exposes no T-SQL execution tool (available: {available:?})."
            ),
            "Drop --via-mcp to run the query over native TDS instead.",
        )
    })?;

    let result = mcp
        .call_tool(
            &tool,
            json!({ "workspaceId": workspace, "itemId": item_id, "query": sql }),
        )
        .await?;

    if result.is_error {
        let msg = result.text();
        return Err(FabioError::with_hint(
            ErrorCode::ApiError,
            format!("MCP execute_query failed: {msg}"),
            "Review the T-SQL, or drop --via-mcp to run over native TDS.",
        )
        .into());
    }

    if let Some(csv_text) = extract_csv_resource(&result.content) {
        let (columns, rows) = parse_csv_to_rows(&csv_text)?;
        let col_refs: Vec<&str> = columns.iter().map(String::as_str).collect();
        let plain_key = columns.first().map_or("", String::as_str);
        output::render_list(cli, &rows, &col_refs, &col_refs, plain_key);
    } else {
        // No result set (DDL/DML) — render the server's summary as a scalar.
        let summary = extract_text_summary(&result.content);
        let obj = json!({
            "status": "executed",
            "message": if summary.is_empty() {
                "Query executed successfully (no result set returned).".to_string()
            } else {
                summary
            },
        });
        output::render_object(cli, &obj, "message");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_execute_tool_prefers_known_name() {
        let tools = vec![json!({"name": "execute_query"}), json!({"name": "other"})];
        assert_eq!(
            resolve_execute_tool(&tools).as_deref(),
            Some("execute_query")
        );
        let docs = vec![json!({"name": "executeSQL"})];
        assert_eq!(resolve_execute_tool(&docs).as_deref(), Some("executeSQL"));
    }

    #[test]
    fn resolve_execute_tool_falls_back_to_single_tool() {
        let tools = vec![json!({"name": "run_sql_v2"})];
        assert_eq!(resolve_execute_tool(&tools).as_deref(), Some("run_sql_v2"));
        // Ambiguous (multiple unknown) → None.
        let ambiguous = vec![json!({"name": "a"}), json!({"name": "b"})];
        assert_eq!(resolve_execute_tool(&ambiguous), None);
        assert_eq!(resolve_execute_tool(&[]), None);
    }

    #[test]
    fn extract_csv_resource_finds_text_csv_block() {
        let content = vec![
            json!({"type": "resource", "resource": {"mimeType": "text/csv", "text": "a,b\n1,2\n"}}),
            json!({"type": "text", "text": "Query returned 1 rows."}),
        ];
        assert_eq!(
            extract_csv_resource(&content).as_deref(),
            Some("a,b\n1,2\n")
        );
        assert_eq!(extract_text_summary(&content), "Query returned 1 rows.");
    }

    #[test]
    fn extract_csv_resource_none_when_no_resource() {
        let content = vec![json!({"type": "text", "text": "Query returned 0 rows."})];
        assert!(extract_csv_resource(&content).is_none());
    }

    #[test]
    fn parse_csv_to_rows_builds_keyed_objects() {
        let (cols, rows) =
            parse_csv_to_rows("Region,Amount\r\nFrance,100\r\nGermany,350\r\n").unwrap();
        assert_eq!(cols, vec!["Region", "Amount"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["Region"], "France");
        assert_eq!(rows[0]["Amount"], "100");
        assert_eq!(rows[1]["Region"], "Germany");
    }

    #[test]
    fn parse_csv_to_rows_handles_quoted_commas() {
        let (cols, rows) = parse_csv_to_rows("name,note\n\"a,b\",\"line1\nline2\"\n").unwrap();
        assert_eq!(cols, vec!["name", "note"]);
        assert_eq!(rows[0]["name"], "a,b");
        assert_eq!(rows[0]["note"], "line1\nline2");
    }

    #[test]
    fn parse_csv_to_rows_empty_result_set() {
        let (cols, rows) = parse_csv_to_rows("Region,Amount\n").unwrap();
        assert_eq!(cols, vec!["Region", "Amount"]);
        assert!(rows.is_empty());
    }
}
