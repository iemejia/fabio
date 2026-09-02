//! Shared utilities for KQL/Kusto query execution and response parsing.
//!
//! Used by `kql_database`, `kql_queryset`, and other commands that need to
//! execute KQL queries against Kusto endpoints.

use std::io;

use anyhow::Result;
use reqwest::header::AUTHORIZATION;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{self, FabricClient};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};

// ─── Input Resolution ────────────────────────────────────────────────────────

/// Resolve KQL text from multiple input sources:
/// - `Some("text")` — use inline text directly
/// - `Some("@path")` — read from file at path
/// - `None` — read from stdin (fails if empty)
pub fn resolve_kql_input(kql: Option<&str>) -> Result<String> {
    match kql {
        Some(s) if s.starts_with('@') => {
            let file_path = &s[1..];
            std::fs::read_to_string(file_path).map_err(|e| {
                FabioError::not_found(format!("KQL file not found: {file_path}: {e}")).into()
            })
        }
        Some(s) => Ok(s.to_string()),
        None => {
            let buf = io::read_to_string(io::stdin()).map_err(|e| {
                FabioError::new(
                    ErrorCode::ApiError,
                    format!("Failed to read KQL from stdin: {e}"),
                )
            })?;
            if buf.trim().is_empty() {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "No KQL provided. Use --kql, @file, or pipe KQL via stdin.".to_string(),
                    "Example: fabio kql-database query --workspace <WS> --id <ID> --kql \"MyTable | take 10\"".to_string(),
                )
                .into());
            }
            Ok(buf)
        }
    }
}

// ─── Query URI Resolution ────────────────────────────────────────────────────

/// Resolve the Kusto query URI and database name for a KQL database.
/// Tries the item properties first; falls back to user-provided override.
pub async fn resolve_query_uri(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    override_uri: Option<&str>,
) -> Result<(String, String)> {
    // Get the KQL database metadata
    let data = client
        .get(&format!("/workspaces/{workspace}/kqlDatabases/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "kql-database query", "Viewer"))?;

    let db_name = data
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // If user provided a query URI override, validate and use it
    if let Some(uri) = override_uri {
        client::validate_trusted_url(uri, "--query-uri")?;
        let uri = uri.trim_end_matches('/').to_string();
        return Ok((uri, db_name));
    }

    // Try to extract query URI from properties
    let properties = data.get("properties");

    // Try known property paths
    let query_uri = properties
        .and_then(|p| p.get("queryServiceUri"))
        .and_then(Value::as_str)
        .or_else(|| {
            properties
                .and_then(|p| p.get("queryUri"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            properties
                .and_then(|p| p.get("databaseUrl"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            // Try parentEventhouseItemId-based URI construction
            properties
                .and_then(|p| p.get("parentEventhouseItemId"))
                .and_then(Value::as_str)
                .map(|_| {
                    // Cannot construct URI without region; fall through to error
                    ""
                })
                .filter(|s| !s.is_empty())
        });

    if let Some(uri) = query_uri {
        let uri = uri.trim_end_matches('/').to_string();
        if !uri.is_empty() {
            // Validate URI from API properties against trusted domains
            client::validate_trusted_url(&uri, "queryServiceUri (from database properties)")?;
            return Ok((uri, db_name));
        }
    }

    Err(FabioError::with_hint(
        ErrorCode::NotFound,
        "Could not determine Kusto query URI from database properties.".to_string(),
        "Provide the query URI manually with --query-uri. Find it in Fabric portal: \
         KQL Database → Database details → Query URI. \
         Example: fabio kql-database query --workspace <WS> --id <ID> --query-uri https://<id>.<region>.kusto.fabric.microsoft.com --kql \"T | take 10\""
            .to_string(),
    )
    .into())
}

// ─── Query Execution ─────────────────────────────────────────────────────────

/// Execute a KQL query against a Kusto endpoint. Returns parsed rows and column names.
///
/// Routes management commands (starting with `.`) to `/v1/rest/mgmt`, T-SQL
/// queries (starting with `SELECT`) to `/v1/rest/query` with the Kusto SQL
/// dialect option, and KQL data queries to `/v2/rest/query`.
pub async fn execute_kql(
    client: &FabricClient,
    kusto_uri: &str,
    db_name: &str,
    kql_text: &str,
) -> Result<(Vec<Value>, Vec<String>)> {
    execute_kql_with_timeout(client, kusto_uri, db_name, kql_text, None).await
}

/// Format a whole-second duration as a Kusto `servertimeout` timespan (`hh:mm:ss`).
/// Kusto caps the server-side query timeout at 1 hour, so the value is clamped.
fn format_servertimeout(secs: u64) -> String {
    let secs = secs.clamp(1, 3600);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}")
}

/// Execute a KQL/T-SQL/mgmt query, optionally setting the Kusto server-side query
/// timeout (`servertimeout` request option, in whole seconds, clamped to 1h). A
/// timeout bounds a long-running query so it can never hang an agent/CI caller.
pub async fn execute_kql_with_timeout(
    client: &FabricClient,
    kusto_uri: &str,
    db_name: &str,
    kql_text: &str,
    timeout_secs: Option<u64>,
) -> Result<(Vec<Value>, Vec<String>)> {
    // Acquire token scoped to the Kusto query URI
    let scope = format!("{kusto_uri}/.default");
    let token = client.require_token_for_scope(&scope).await?;

    // Management commands (starting with '.') use /v1/rest/mgmt; T-SQL queries
    // (leading SELECT) use /v1/rest/query with the SQL dialect option; KQL
    // queries use /v2/rest/query.
    let is_mgmt = kql_text.trim_start().starts_with('.');
    let is_tsql = !is_mgmt && is_tsql_query(kql_text);
    let url = if is_mgmt {
        format!("{kusto_uri}/v1/rest/mgmt")
    } else if is_tsql {
        format!("{kusto_uri}/v1/rest/query")
    } else {
        format!("{kusto_uri}/v2/rest/query")
    };
    let mut body = serde_json::json!({
        "db": db_name,
        "csl": kql_text,
    });
    // Assemble request options: SQL dialect for T-SQL, server timeout if requested.
    let mut options = serde_json::Map::new();
    if is_tsql {
        // Kusto executes T-SQL only when told the query language is SQL.
        options.insert("query_language".to_string(), Value::from("Sql"));
    }
    if let Some(secs) = timeout_secs {
        options.insert(
            "servertimeout".to_string(),
            Value::from(format_servertimeout(secs)),
        );
    }
    if !options.is_empty() {
        body["properties"] = serde_json::json!({ "Options": Value::Object(options) });
    }

    let resp = client
        .http()
        .post(&url)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header("Content-Type", "application/json; charset=utf-8")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            FabioError::new(
                ErrorCode::NetworkError,
                format!("Kusto request failed: {e}"),
            )
        })?;

    let status = resp.status();
    let resp_text = resp.text().await.map_err(|e| {
        FabioError::new(
            ErrorCode::ApiError,
            format!("Failed to read Kusto response: {e}"),
        )
    })?;

    if !status.is_success() {
        return Err(FabioError::with_hint(
            ErrorCode::ApiError,
            format!("Kusto query failed (HTTP {status}): {resp_text}"),
            "Verify the KQL database is accessible and the query syntax is valid.".to_string(),
        )
        .into());
    }

    // Parse response: v1 (mgmt) returns {"Tables":[...]}, v2 (query) returns array of frames
    let parsed: Value = serde_json::from_str(&resp_text).map_err(|e| {
        FabioError::new(
            ErrorCode::ApiError,
            format!("Failed to parse Kusto response: {e}"),
        )
    })?;

    if is_mgmt {
        parse_kusto_v1_response(&parsed)
    } else if is_tsql {
        // T-SQL over Kusto returns the v1 `{"Tables":[...]}` shape.
        parse_kusto_v1_response(&parsed)
    } else {
        parse_kusto_v2_response(&parsed)
    }
}

/// True when `text` is a T-SQL query (a leading `SELECT`). Kusto has no `SELECT`
/// operator, so a leading `SELECT` word unambiguously marks T-SQL. The leading
/// alphabetic run must equal `select` (so an identifier like `SelectedRows` in a
/// KQL query is not misdetected).
fn is_tsql_query(text: &str) -> bool {
    let t = text.trim_start();
    let first: String = t.chars().take_while(char::is_ascii_alphabetic).collect();
    first.eq_ignore_ascii_case("select")
}

// ─── Response Parsing ────────────────────────────────────────────────────────

/// Parse Kusto v1 response format (used by management commands via `/v1/rest/mgmt`).
///
/// The v1 format is: `{"Tables": [{"TableName": "...", "Columns": [...], "Rows": [[...], ...]}]}`
/// We take the first table as the primary result.
pub fn parse_kusto_v1_response(resp: &Value) -> Result<(Vec<Value>, Vec<String>)> {
    let tables = resp
        .get("Tables")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "Unexpected Kusto v1 response: missing 'Tables' array.".to_string(),
            )
        })?;

    // Use the first table as primary result
    let Some(table) = tables.first() else {
        return Ok((Vec::new(), Vec::new()));
    };

    let columns: Vec<String> =
        table
            .get("Columns")
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |cols| {
                cols.iter()
                    .filter_map(|c| {
                        c.get("ColumnName")
                            .and_then(Value::as_str)
                            .map(String::from)
                    })
                    .collect()
            });

    if columns.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let rows: Vec<Value> =
        table
            .get("Rows")
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |rows| {
                rows.iter()
                    .map(|row| {
                        let mut obj = serde_json::Map::with_capacity(columns.len());
                        if let Some(row_arr) = row.as_array() {
                            for (i, val) in row_arr.iter().enumerate() {
                                let col_name = columns
                                    .get(i)
                                    .cloned()
                                    .unwrap_or_else(|| format!("column{i}"));
                                obj.insert(col_name, val.clone());
                            }
                        }
                        Value::Object(obj)
                    })
                    .collect()
            });

    Ok((rows, columns))
}

/// Parse Kusto v2 response format into rows and column names.
///
/// The v2 format is a JSON array of frames:
/// - `DataSetHeader` — dataset metadata
/// - `DataTable` — result table(s) (look for `TableKind: "PrimaryResult"`)
/// - `DataSetCompletion` — final status
pub fn parse_kusto_v2_response(frames: &Value) -> Result<(Vec<Value>, Vec<String>)> {
    let frame_array = frames.as_array().ok_or_else(|| {
        FabioError::new(
            ErrorCode::ApiError,
            "Unexpected Kusto response format: expected JSON array of frames.".to_string(),
        )
    })?;

    // Find the PrimaryResult frame
    let primary_frame = frame_array
        .iter()
        .find(|f| {
            f.get("FrameType").and_then(Value::as_str) == Some("DataTable")
                && f.get("TableKind").and_then(Value::as_str) == Some("PrimaryResult")
        })
        .or_else(|| {
            // Fallback: first DataTable frame
            frame_array
                .iter()
                .find(|f| f.get("FrameType").and_then(Value::as_str) == Some("DataTable"))
        });

    let Some(frame) = primary_frame else {
        // Check if there's an error in the completion frame
        if let Some(completion) = frame_array
            .iter()
            .find(|f| f.get("FrameType").and_then(Value::as_str) == Some("DataSetCompletion"))
            && completion.get("HasErrors").and_then(Value::as_bool) == Some(true)
        {
            let error_msg = completion
                .get("OneApiErrors")
                .map_or("Unknown Kusto error", |e| {
                    e.as_str().unwrap_or("Unknown Kusto error")
                });
            return Err(FabioError::new(
                ErrorCode::ApiError,
                format!("Kusto query error: {error_msg}"),
            )
            .into());
        }
        return Ok((Vec::new(), Vec::new()));
    };

    // Extract column names
    let columns: Vec<String> =
        frame
            .get("Columns")
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |cols| {
                cols.iter()
                    .filter_map(|c| {
                        c.get("ColumnName")
                            .and_then(Value::as_str)
                            .map(String::from)
                    })
                    .collect()
            });

    if columns.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // Extract rows and convert to JSON objects
    let rows: Vec<Value> =
        frame
            .get("Rows")
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |rows| {
                rows.iter()
                    .map(|row| {
                        let mut obj = serde_json::Map::with_capacity(columns.len());
                        if let Some(row_arr) = row.as_array() {
                            for (i, val) in row_arr.iter().enumerate() {
                                let col_name = columns
                                    .get(i)
                                    .cloned()
                                    .unwrap_or_else(|| format!("column{i}"));
                                obj.insert(col_name, val.clone());
                            }
                        }
                        Value::Object(obj)
                    })
                    .collect()
            });

    Ok((rows, columns))
}

// ─── Output Helpers ──────────────────────────────────────────────────────────

/// Render KQL query results using the standard output system.
/// Shows an empty-result message when no rows are returned.
pub fn render_kql_results(cli: &crate::cli::Cli, rows: &[Value], columns: &[String]) {
    if rows.is_empty() {
        let obj = serde_json::json!({
            "rows_returned": 0,
            "message": "Query executed successfully (no results returned)."
        });
        crate::output::render_object(cli, &obj, "message");
    } else {
        let col_refs: Vec<&str> = columns.iter().map(String::as_str).collect();
        crate::output::render_list(cli, rows, &col_refs, &col_refs, &columns[0]);
    }
}

// ─── One-shot vs. continuous (--follow) query runner ─────────────────────────

/// Options controlling how a KQL query is executed: one-shot (bounded by an
/// optional server timeout) or continuous `--follow` (client-side polling that
/// streams NDJSON and always terminates).
///
/// Kusto queries are request/response and always terminate; there is no
/// server-push streaming query. `--follow` therefore re-runs the query every
/// `interval` and is ALWAYS bounded by `max_duration`, the global `--limit`, or
/// Ctrl-C — safe for an agent/CI caller. Shared by `eventhouse query` and
/// `kql-database query`.
#[derive(Debug, Default, Clone)]
pub struct QueryRunOptions {
    /// Server-side query timeout in seconds (Kusto `servertimeout`, max 3600).
    pub timeout: Option<u64>,
    /// Continuously re-run the query, streaming NDJSON until bounded out.
    pub follow: bool,
    /// Seconds between polls in follow mode (default 5).
    pub interval: Option<u64>,
    /// Total seconds to follow before stopping (default 60) — the safety bound.
    pub max_duration: Option<u64>,
    /// Emit only rows whose value in this column exceeds the max seen (incremental tail).
    pub dedup_column: Option<String>,
}

impl QueryRunOptions {
    /// Reject follow-only flags when `--follow` is not set.
    pub fn validate(&self) -> Result<()> {
        if !self.follow
            && (self.interval.is_some()
                || self.max_duration.is_some()
                || self.dedup_column.is_some())
        {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--interval, --max-duration, and --dedup-column require --follow".to_string(),
                "Add --follow to stream the query on an interval, or drop those flags for a one-shot query.".to_string(),
            )
            .into());
        }
        Ok(())
    }
}

/// Run a KQL/T-SQL/management query one-shot, or continuously with `--follow`.
pub async fn run_query(
    cli: &Cli,
    client: &FabricClient,
    kusto_uri: &str,
    db_name: &str,
    kql_text: &str,
    opts: &QueryRunOptions,
) -> Result<()> {
    if opts.follow {
        let fo = crate::commands::follow::FollowOptions {
            interval: opts.interval,
            max_duration: opts.max_duration,
            dedup_column: opts.dedup_column.clone(),
        };
        return crate::commands::follow::follow_stream(
            cli,
            &fo,
            async || {
                execute_kql_with_timeout(client, kusto_uri, db_name, kql_text, opts.timeout).await
            },
            |_| false,
        )
        .await;
    }
    let (rows, columns) =
        execute_kql_with_timeout(client, kusto_uri, db_name, kql_text, opts.timeout).await?;
    render_kql_results(cli, &rows, &columns);
    Ok(())
}
// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tsql_detection_matches_leading_select_only() {
        // T-SQL: a leading SELECT (any case, with leading whitespace).
        assert!(is_tsql_query("SELECT TOP 3 * FROM T"));
        assert!(is_tsql_query("  select a, b from T order by a"));
        assert!(is_tsql_query("\n\tSELECT 1"));
        // KQL: no leading SELECT statement.
        assert!(!is_tsql_query("RawData | count"));
        assert!(!is_tsql_query("TransformedData | where x > 1 | project a"));
        assert!(!is_tsql_query(".show tables"));
        // An identifier that merely starts with the letters "select" is NOT T-SQL.
        assert!(!is_tsql_query("SelectedRows | count"));
    }

    #[test]
    fn servertimeout_formats_and_clamps() {
        assert_eq!(format_servertimeout(90), "00:01:30");
        assert_eq!(format_servertimeout(3599), "00:59:59");
        assert_eq!(format_servertimeout(3600), "01:00:00");
        assert_eq!(format_servertimeout(0), "00:00:01"); // clamped up to 1s
        assert_eq!(format_servertimeout(99_999), "01:00:00"); // clamped to 1h max
    }

    #[test]
    fn query_run_options_validate_requires_follow() {
        let mut o = QueryRunOptions {
            interval: Some(2),
            ..Default::default()
        };
        assert!(o.validate().is_err());
        o.follow = true;
        assert!(o.validate().is_ok());
        assert!(QueryRunOptions::default().validate().is_ok());
    }

    #[test]
    fn test_resolve_kql_input_inline() {
        let result = resolve_kql_input(Some("print x=42")).unwrap();
        assert_eq!(result, "print x=42");
    }

    #[test]
    fn test_resolve_kql_input_file() {
        let tmp = std::env::temp_dir().join("fabio_test_kql_input.kql");
        std::fs::write(&tmp, ".show tables").unwrap();
        let arg = format!("@{}", tmp.display());
        let result = resolve_kql_input(Some(&arg)).unwrap();
        assert_eq!(result, ".show tables");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_resolve_kql_input_file_not_found() {
        let result = resolve_kql_input(Some("@/nonexistent/path.kql"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[test]
    fn test_parse_v1_basic() {
        let resp = json!({
            "Tables": [{
                "TableName": "Table_0",
                "Columns": [
                    {"ColumnName": "Name", "DataType": "String"},
                    {"ColumnName": "Count", "DataType": "Int64"}
                ],
                "Rows": [["Alice", 10], ["Bob", 20]]
            }]
        });
        let (rows, columns) = parse_kusto_v1_response(&resp).unwrap();
        assert_eq!(columns, vec!["Name", "Count"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["Name"], "Alice");
        assert_eq!(rows[0]["Count"], 10);
        assert_eq!(rows[1]["Name"], "Bob");
    }

    #[test]
    fn test_parse_v1_empty_tables() {
        let resp = json!({"Tables": []});
        let (rows, columns) = parse_kusto_v1_response(&resp).unwrap();
        assert!(rows.is_empty());
        assert!(columns.is_empty());
    }

    #[test]
    fn test_parse_v1_missing_tables() {
        let resp = json!({"error": "bad"});
        let result = parse_kusto_v1_response(&resp);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_v2_primary_result() {
        let frames = json!([
            {"FrameType": "DataSetHeader", "IsProgressive": false},
            {
                "FrameType": "DataTable",
                "TableKind": "PrimaryResult",
                "Columns": [{"ColumnName": "x", "ColumnType": "int"}],
                "Rows": [[42]]
            },
            {"FrameType": "DataSetCompletion", "HasErrors": false}
        ]);
        let (rows, columns) = parse_kusto_v2_response(&frames).unwrap();
        assert_eq!(columns, vec!["x"]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["x"], 42);
    }

    #[test]
    fn test_parse_v2_error_completion() {
        let frames = json!([
            {"FrameType": "DataSetHeader"},
            {"FrameType": "DataSetCompletion", "HasErrors": true, "OneApiErrors": "Bad query"}
        ]);
        let result = parse_kusto_v2_response(&frames);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Bad query"));
    }

    #[test]
    fn test_parse_v2_not_array() {
        let resp = json!({"error": "not frames"});
        let result = parse_kusto_v2_response(&resp);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("expected JSON array")
        );
    }

    #[test]
    fn test_parse_v2_no_datatable_no_error() {
        let frames = json!([
            {"FrameType": "DataSetHeader"},
            {"FrameType": "DataSetCompletion", "HasErrors": false}
        ]);
        let (rows, columns) = parse_kusto_v2_response(&frames).unwrap();
        assert!(rows.is_empty());
        assert!(columns.is_empty());
    }
}
