use std::borrow::Cow;

use base64::Engine as _;
use comfy_table::{Cell, Table};
use serde::Serialize;
use serde_json::Value;

use crate::cli::{Cli, OutputFormat};
use crate::errors::FabioError;

/// JSON envelope for single-object responses.
#[derive(Serialize)]
pub struct ObjectEnvelope {
    pub data: Value,
}

/// JSON envelope for errors.
#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retriable: Option<bool>,
}

/// Render a list of items respecting --quiet, --query, and --limit flags.
/// Includes `continuationToken` in JSON envelope when more pages are available.
pub fn render_list(
    cli: &Cli,
    items: &[Value],
    columns: &[&str],
    headers: &[&str],
    plain_key: &str,
) {
    render_list_with_token(cli, items, columns, headers, plain_key, None);
}

/// Render a list of items with optional pagination continuation token.
#[allow(clippy::too_many_lines)]
pub fn render_list_with_token(
    cli: &Cli,
    items: &[Value],
    columns: &[&str],
    headers: &[&str],
    plain_key: &str,
    continuation_token: Option<&str>,
) {
    if cli.quiet {
        return;
    }

    // Apply --limit before rendering
    let limited_items: &[Value] = cli
        .limit
        .map_or(items, |limit| &items[..items.len().min(limit)]);
    let truncated = cli.limit.is_some_and(|l| items.len() > l);

    match cli.effective_output() {
        OutputFormat::Json => {
            // Only construct Value::Array for the JSON path
            let data = Value::Array(limited_items.to_vec());
            let output_data = match cli.query {
                Some(ref q) => apply_query(&data, q),
                None => data,
            };
            let display_items = match output_data {
                Value::Array(arr) => arr,
                other => vec![other],
            };
            let count = display_items.len();
            let mut envelope = serde_json::json!({
                "data": display_items,
                "count": count
            });
            if truncated {
                envelope["truncated"] = Value::Bool(true);
                envelope["total_available"] = serde_json::json!(items.len());
            }
            if let Some(token) = continuation_token {
                envelope["continuationToken"] = Value::String(token.to_string());
            }
            println!(
                "{}",
                serde_json::to_string(&envelope).unwrap_or_else(|_| r#"{"error":{"code":"SERIALIZATION_ERROR","message":"Failed to serialize output"}}"#.to_string())
            );
        }
        OutputFormat::Table => {
            // For table/plain with query, we need to apply query to each item
            if let Some(ref q) = cli.query {
                let data = Value::Array(limited_items.to_vec());
                let output_data = apply_query(&data, q);
                if let Value::Array(ref arr) = output_data {
                    render_table(arr, columns, headers);
                } else {
                    render_table(limited_items, columns, headers);
                }
            } else {
                render_table(limited_items, columns, headers);
            }
            if truncated {
                println!(
                    "... truncated ({} of {} items, use --limit to adjust)",
                    limited_items.len(),
                    items.len()
                );
            }
            if continuation_token.is_some() {
                println!("... more pages available (use --all to fetch all)");
            }
        }
        OutputFormat::Plain => {
            if let Some(ref q) = cli.query {
                let data = Value::Array(limited_items.to_vec());
                let output_data = apply_query(&data, q);
                let arr = if let Value::Array(ref a) = output_data {
                    a.as_slice()
                } else {
                    limited_items
                };
                for item in arr {
                    if let Some(val) = item.get(plain_key) {
                        println!("{}", format_value(val));
                    } else {
                        println!("{}", format_value(item));
                    }
                }
            } else {
                for item in limited_items {
                    if let Some(val) = item.get(plain_key) {
                        println!("{}", format_value(val));
                    } else {
                        println!("{}", format_value(item));
                    }
                }
            }
        }
        OutputFormat::Csv | OutputFormat::Tsv => {
            let sep = if matches!(cli.effective_output(), OutputFormat::Tsv) {
                '\t'
            } else {
                ','
            };
            // Header row
            println!("{}", columns.join(&sep.to_string()));
            // Data rows
            for item in limited_items {
                let row: Vec<String> = columns
                    .iter()
                    .map(|col| {
                        let val = resolve_nested(item, col);
                        format_csv_value(val, sep)
                    })
                    .collect();
                println!("{}", row.join(&sep.to_string()));
            }
        }
    }
}

/// Render a single object respecting --quiet and --query flags.
pub fn render_object(cli: &Cli, obj: &Value, plain_key: &str) {
    if cli.quiet {
        return;
    }

    // Use Cow to avoid cloning when no query is applied (Table/Plain paths)
    let output_data: Cow<'_, Value> = cli
        .query
        .as_ref()
        .map_or(Cow::Borrowed(obj), |q| Cow::Owned(apply_query(obj, q)));

    match cli.effective_output() {
        OutputFormat::Json => {
            let envelope = ObjectEnvelope {
                data: output_data.into_owned(),
            };
            println!(
                "{}",
                serde_json::to_string(&envelope).unwrap_or_else(|_| r#"{"error":{"code":"SERIALIZATION_ERROR","message":"Failed to serialize output"}}"#.to_string())
            );
        }
        OutputFormat::Table => {
            // For single objects, render as key-value pairs
            if let Value::Object(map) = output_data.as_ref() {
                let mut table = Table::new();
                table.set_header(vec!["KEY", "VALUE"]);
                for (key, val) in map {
                    table.add_row(vec![Cell::new(key), Cell::new(format_value(val))]);
                }
                println!("{table}");
            } else {
                // Scalar result from query
                println!("{}", format_value(output_data.as_ref()));
            }
        }
        OutputFormat::Plain => {
            if let Some(val) = output_data.get(plain_key) {
                println!("{}", format_value(val));
            } else {
                // If output is a scalar or the key doesn't exist, print raw
                match output_data.as_ref() {
                    Value::String(s) => println!("{s}"),
                    Value::Null => {}
                    other => println!(
                        "{}",
                        serde_json::to_string_pretty(other).unwrap_or_else(|_| "null".to_string())
                    ),
                }
            }
        }
        OutputFormat::Csv | OutputFormat::Tsv => {
            let sep = if matches!(cli.effective_output(), OutputFormat::Tsv) {
                '\t'
            } else {
                ','
            };
            // For single objects, emit header row + single data row
            if let Value::Object(map) = output_data.as_ref() {
                let keys: Vec<&str> = map.keys().map(String::as_str).collect();
                println!("{}", keys.join(&sep.to_string()));
                let vals: Vec<String> = map.values().map(|v| format_csv_value(v, sep)).collect();
                println!("{}", vals.join(&sep.to_string()));
            } else {
                println!("{}", format_value(output_data.as_ref()));
            }
        }
    }
}

/// Render an error to stderr as structured JSON.
pub fn render_error(err: &FabioError) {
    let envelope = ErrorEnvelope {
        error: ErrorBody {
            code: err.code.to_string(),
            message: err.message.clone(),
            hint: err.hint.clone(),
            retriable: err.retriable,
        },
    };
    eprintln!(
        "{}",
        serde_json::to_string(&envelope).unwrap_or_else(|_| {
            format!(
                r#"{{"error":{{"code":"{}","message":"(serialization failed)"}}}}"#,
                err.code
            )
        })
    );
}

/// Check if dry-run is active and render a preview response.
/// Returns `true` if dry-run is active (caller should skip the real operation).
pub fn dry_run_guard(cli: &Cli, operation: &str, details: &Value) -> bool {
    if !cli.dry_run {
        return false;
    }
    let obj = serde_json::json!({
        "dry_run": true,
        "would_execute": operation,
        "details": details
    });
    render_object(cli, &obj, "would_execute");
    true
}

/// Render items as an ASCII table.
fn render_table(items: &[Value], columns: &[&str], headers: &[&str]) {
    let mut table = Table::new();
    table.set_header(headers.iter().map(|h| Cell::new(*h)).collect::<Vec<_>>());

    for item in items {
        let row: Vec<Cell> = columns
            .iter()
            .map(|col| {
                let val = resolve_nested(item, col);
                Cell::new(format_value(val))
            })
            .collect();
        table.add_row(row);
    }

    println!("{table}");
}

/// Resolve a dot-notation path to a nested JSON value.
fn resolve_nested<'a>(value: &'a Value, path: &str) -> &'a Value {
    let mut current = value;
    for part in path.split('.') {
        match current.get(part) {
            Some(v) => current = v,
            None => return &Value::Null,
        }
    }
    current
}

/// Format a JSON value for display.
fn format_value(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        _ => serde_json::to_string(val).unwrap_or_default(),
    }
}

/// Format a value for CSV/TSV output. Quotes strings containing the separator,
/// quotes, or newlines per RFC 4180.
fn format_csv_value(val: &Value, sep: char) -> String {
    let raw = format_value(val);
    if raw.contains(sep) || raw.contains('"') || raw.contains('\n') || raw.contains('\r') {
        format!("\"{}\"", raw.replace('"', "\"\""))
    } else {
        raw
    }
}

/// Apply a simple dot-notation query to extract a field.
pub fn apply_query(value: &Value, query: &str) -> Value {
    let parts: Vec<&str> = query.split('.').collect();
    let mut current = value;
    for part in parts {
        match current {
            Value::Object(map) => {
                current = map.get(part).unwrap_or(&Value::Null);
            }
            Value::Array(arr) => {
                let extracted: Vec<Value> = arr
                    .iter()
                    .filter_map(|item| item.get(part).cloned())
                    .collect();
                return Value::Array(extracted);
            }
            _ => return Value::Null,
        }
    }
    current.clone()
}

/// Decode base64-encoded definition parts inline.
/// Adds a `decodedPayload` field alongside the original `payload` for each part.
/// Handles both JSON payloads (parsed into objects) and plain text (kept as strings).
/// Accepts owned `Value` to avoid cloning the entire response.
pub fn decode_definition_parts(mut data: Value) -> Value {
    let base64_engine = base64::engine::general_purpose::STANDARD;
    if let Some(parts) = data
        .get_mut("definition")
        .and_then(|d| d.get_mut("parts"))
        .and_then(|p| p.as_array_mut())
    {
        for part in parts {
            if let Some(payload) = part.get("payload").and_then(|p| p.as_str()) {
                if let Ok(decoded_bytes) = base64_engine.decode(payload) {
                    if let Ok(decoded_str) = String::from_utf8(decoded_bytes) {
                        if let Ok(json_val) = serde_json::from_str::<Value>(&decoded_str) {
                            part["decodedPayload"] = json_val;
                        } else {
                            part["decodedPayload"] = Value::String(decoded_str);
                        }
                    }
                }
            }
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use crate::cli::Command;

    use super::*;

    #[test]
    fn apply_query_extracts_object_field() {
        let obj = serde_json::json!({"name": "test", "id": "123"});
        assert_eq!(apply_query(&obj, "name"), Value::String("test".to_string()));
    }

    #[test]
    fn apply_query_extracts_nested_field() {
        let obj = serde_json::json!({"a": {"b": {"c": 42}}});
        assert_eq!(apply_query(&obj, "a.b.c"), serde_json::json!(42));
    }

    #[test]
    fn apply_query_extracts_array_field() {
        let arr = serde_json::json!([
            {"name": "alpha", "id": "1"},
            {"name": "beta", "id": "2"},
        ]);
        let result = apply_query(&arr, "name");
        assert_eq!(result, serde_json::json!(["alpha", "beta"]));
    }

    #[test]
    fn apply_query_missing_field_returns_null() {
        let obj = serde_json::json!({"name": "test"});
        assert_eq!(apply_query(&obj, "missing"), Value::Null);
    }

    #[test]
    fn apply_query_on_null_returns_null() {
        assert_eq!(apply_query(&Value::Null, "anything"), Value::Null);
    }

    #[test]
    fn format_value_handles_types() {
        assert_eq!(format_value(&Value::String("hello".into())), "hello");
        assert_eq!(format_value(&serde_json::json!(42)), "42");
        assert_eq!(format_value(&serde_json::json!(true)), "true");
        assert_eq!(format_value(&Value::Null), "");
        assert_eq!(format_value(&serde_json::json!({"a": 1})), r#"{"a":1}"#);
    }

    #[test]
    fn effective_output_defaults_to_json() {
        let cli = make_test_cli(&[]);
        assert!(matches!(cli.effective_output(), OutputFormat::Json));
    }

    #[test]
    fn effective_output_json_flag_overrides_table() {
        let cli = make_test_cli(&["--output", "table", "--json"]);
        assert!(matches!(cli.effective_output(), OutputFormat::Json));
    }

    #[test]
    fn dry_run_guard_returns_false_when_inactive() {
        let cli = make_test_cli(&[]);
        let details = serde_json::json!({"name": "test"});
        assert!(!dry_run_guard(&cli, "create", &details));
    }

    #[test]
    fn dry_run_guard_returns_true_when_active() {
        let cli = make_test_cli(&["--dry-run"]);
        let details = serde_json::json!({"name": "test"});
        assert!(dry_run_guard(&cli, "workspace.create", &details));
    }

    #[test]
    fn format_csv_value_plain_string() {
        let val = Value::String("hello".into());
        assert_eq!(format_csv_value(&val, ','), "hello");
    }

    #[test]
    fn format_csv_value_with_comma_quotes() {
        let val = Value::String("foo,bar".into());
        assert_eq!(format_csv_value(&val, ','), "\"foo,bar\"");
    }

    #[test]
    fn format_csv_value_with_quotes_escapes() {
        let val = Value::String("say \"hi\"".into());
        assert_eq!(format_csv_value(&val, ','), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn format_csv_value_with_newline_quotes() {
        let val = Value::String("line1\nline2".into());
        assert_eq!(format_csv_value(&val, ','), "\"line1\nline2\"");
    }

    #[test]
    fn format_csv_value_tsv_tab_separator() {
        let val = Value::String("has\ttab".into());
        assert_eq!(format_csv_value(&val, '\t'), "\"has\ttab\"");
    }

    #[test]
    fn format_csv_value_tsv_comma_no_quote() {
        // In TSV mode, commas don't need quoting
        let val = Value::String("foo,bar".into());
        assert_eq!(format_csv_value(&val, '\t'), "foo,bar");
    }

    #[test]
    fn format_csv_value_null_empty() {
        assert_eq!(format_csv_value(&Value::Null, ','), "");
    }

    #[test]
    fn format_csv_value_number() {
        let val = serde_json::json!(42);
        assert_eq!(format_csv_value(&val, ','), "42");
    }

    #[test]
    fn effective_output_csv_flag() {
        let cli = make_test_cli(&["--output", "csv"]);
        assert!(matches!(cli.effective_output(), OutputFormat::Csv));
    }

    #[test]
    fn effective_output_tsv_flag() {
        let cli = make_test_cli(&["--output", "tsv"]);
        assert!(matches!(cli.effective_output(), OutputFormat::Tsv));
    }

    /// Helper to construct a Cli for testing (parses args after "fabio agent-context").
    fn make_test_cli(extra_args: &[&str]) -> Cli {
        const VALID_OUTPUT_VALUES: &str = "json, table, plain, csv, tsv";

        let mut cli = Cli {
            output: OutputFormat::Json,
            json: false,
            query: None,
            quiet: false,
            force: false,
            dry_run: false,
            limit: None,
            all: false,
            continuation_token: None,
            profile: None,
            lro_timeout: None,
            command: Command::AgentContext,
        };

        let mut i = 0;
        while i < extra_args.len() {
            match extra_args[i] {
                "--json" => {
                    cli.json = true;
                    i += 1;
                }
                "--dry-run" => {
                    cli.dry_run = true;
                    i += 1;
                }
                "--output" => {
                    let next = extra_args.get(i + 1).copied().expect(
                        "missing value for --output in test helper. Valid values: json, table, plain, csv, tsv",
                    );
                    cli.output = match next {
                        "json" => OutputFormat::Json,
                        "table" => OutputFormat::Table,
                        "plain" => OutputFormat::Plain,
                        "csv" => OutputFormat::Csv,
                        "tsv" => OutputFormat::Tsv,
                        other => panic!(
                            "unexpected --output value in test helper: {other}. Valid values: {VALID_OUTPUT_VALUES}"
                        ),
                    };
                    i += 2;
                }
                other => {
                    panic!(
                        "unsupported test arg in make_test_cli: {other}. Supported: --json, --dry-run, --output"
                    )
                }
            }
        }

        cli
    }

    #[test]
    fn error_body_serializes_retriable_when_set() {
        let body = ErrorBody {
            code: "API_ERROR".to_string(),
            message: "server error".to_string(),
            hint: None,
            retriable: Some(true),
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains(r#""retriable":true"#));
    }

    #[test]
    fn error_body_omits_retriable_when_none() {
        let body = ErrorBody {
            code: "NOT_FOUND".to_string(),
            message: "item not found".to_string(),
            hint: None,
            retriable: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(!json.contains("retriable"));
    }
}
