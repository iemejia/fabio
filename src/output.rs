use std::borrow::Cow;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use comfy_table::{Cell, Table};
use serde::Serialize;
use serde_json::Value;

use crate::agent;
use crate::cli::{Cli, OutputFormat};
use crate::errors::{ErrorCode, ErrorDetail, FabioError, HintType, RelatedResource};

/// Fields that contain user-authored content and should be wrapped with untrusted markers.
const UNTRUSTED_FIELDS: &[&str] = &["displayName", "description", "name", "message"];

/// Wrap user-authored string fields in a JSON value with untrusted content markers.
/// Recursively walks the JSON tree and wraps values of keys matching `UNTRUSTED_FIELDS`.
fn wrap_untrusted_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if UNTRUSTED_FIELDS.contains(&key.as_str()) {
                    if let Value::String(s) = val {
                        *s = format!("<<<UNTRUSTED>>>{s}<<<END_UNTRUSTED>>>");
                    }
                } else {
                    wrap_untrusted_fields(val);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                wrap_untrusted_fields(item);
            }
        }
        _ => {}
    }
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
    #[serde(skip_serializing_if = "Option::is_none", rename = "hintType")]
    hint_type: Option<HintType>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "verifyAfter")]
    verify_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retriable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "requestId")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "moreDetails")]
    more_details: Option<Vec<ErrorDetail>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "relatedResource")]
    related_resource: Option<RelatedResource>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "agentNotice")]
    agent_notice: Option<String>,
}

/// Attach the passive `updateAvailable` notice to a JSON success envelope when
/// one is pending (a newer fabio is known and an AI agent is detected). Consumed
/// once per process, so only the first JSON envelope carries it. No-op for
/// non-object envelopes.
fn attach_version_notice(envelope: &mut Value) {
    if let Value::Object(map) = envelope
        && let Some(notice) = crate::version_check::take_notice()
    {
        map.insert("updateAvailable".to_string(), notice);
    }
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

/// Check if any item in the list has a non-empty `tags` array.
pub fn has_tags(items: &[Value]) -> bool {
    items.iter().any(|item| {
        item.get("tags")
            .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty()))
    })
}

/// Enrich items with a flat `_tagsDisplay` field containing comma-separated tag names.
/// This enables rendering tags in table output using the standard column mechanism.
/// Returns a new Vec with the enriched items (does not modify originals).
pub fn enrich_with_tags_display(items: &[Value]) -> Vec<Value> {
    items
        .iter()
        .map(|item| {
            let mut enriched = item.clone();
            if let Some(tags) = item.get("tags").and_then(Value::as_array) {
                let names: Vec<&str> = tags
                    .iter()
                    .filter_map(|t| t.get("displayName").and_then(Value::as_str))
                    .collect();
                enriched["_tagsDisplay"] = Value::from(names.join(", "));
            }
            enriched
        })
        .collect()
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
            // `--query` runs against the raw payload (the list), matching `az`
            // (JMESPath over the response body). A query that yields an array keeps
            // the list envelope (`data` + `count`); a query that yields a scalar or
            // object is emitted as a bare `{"data": <value>}` — no array-wrapping,
            // no `count` (so `[0].id` gives a string, not a 1-element list).
            let data = Value::Array(limited_items.to_vec());
            let output_data = match cli.query {
                Some(ref q) => apply_query(&data, q),
                None => data,
            };
            let mut envelope = if let Value::Array(arr) = output_data {
                let count = arr.len();
                let mut env = serde_json::json!({ "data": arr, "count": count });
                if truncated {
                    env["truncated"] = Value::Bool(true);
                    env["total_available"] = serde_json::json!(items.len());
                }
                if let Some(token) = continuation_token {
                    env["continuationToken"] = Value::from(token);
                }
                env
            } else {
                // Scalar/object/null query result: emit as a single-value envelope.
                serde_json::json!({ "data": output_data })
            };
            if cli.wrap_untrusted {
                wrap_untrusted_fields(&mut envelope);
            }
            attach_version_notice(&mut envelope);
            println!(
                "{}",
                serde_json::to_string(&envelope).unwrap_or_else(|_| r#"{"error":{"code":"SERIALIZATION_ERROR","message":"Failed to serialize output"}}"#.to_string())
            );
        }
        OutputFormat::Table => {
            // When a query is present, render its result: an array as a table, a
            // scalar/object directly (do NOT fall back to the un-projected list).
            if let Some(ref q) = cli.query {
                let data = Value::Array(limited_items.to_vec());
                let output_data = apply_query(&data, q);
                match output_data {
                    Value::Array(ref arr) => render_table(arr, columns, headers),
                    Value::Null => {}
                    ref other => println!("{}", format_value(other)),
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
                render_plain_value(&output_data, plain_key);
            } else {
                for item in limited_items {
                    render_plain_item(item, plain_key);
                }
            }
        }
        OutputFormat::Csv | OutputFormat::Tsv => {
            let sep = if matches!(cli.effective_output(), OutputFormat::Tsv) {
                '\t'
            } else {
                ','
            };
            // Respect --query: emit the projected rows, not the raw list.
            if let Some(ref q) = cli.query {
                let data = Value::Array(limited_items.to_vec());
                match apply_query(&data, q) {
                    Value::Array(ref arr) => print!("{}", format_delimited_list(arr, columns, sep)),
                    Value::Null => {}
                    Value::Object(ref o) => {
                        print!(
                            "{}",
                            format_delimited_object(&Value::Object(o.clone()), sep)
                        );
                    }
                    ref other => println!("{}", format_csv_value(other, sep)),
                }
            } else {
                print!("{}", format_delimited_list(limited_items, columns, sep));
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
            let mut envelope_data = output_data.into_owned();
            if cli.wrap_untrusted {
                wrap_untrusted_fields(&mut envelope_data);
            }
            let mut envelope = serde_json::json!({ "data": envelope_data });
            attach_version_notice(&mut envelope);
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
            print!("{}", format_delimited_object(output_data.as_ref(), sep));
        }
    }
}

/// Render an error to stderr as structured JSON.
///
/// When an AI agent is detected and the error hint suggests a dangerous
/// (safety-bypass) flag, an `agentNotice` field is appended to warn the
/// agent not to retry with that flag without explicit user approval.
pub fn render_error(err: &FabioError) {
    // Resolve the effective hint type: use explicit if set, otherwise infer.
    let effective_hint_type = err.hint.as_ref().map(|hint| {
        err.hint_type
            .unwrap_or_else(|| infer_hint_type(err.code, hint))
    });

    // Determine whether to include agent safety notice:
    // Fires when hint_type is SafetyBypass (either explicit or inferred) AND
    // an AI agent is detected as the caller.
    let agent_notice = if effective_hint_type == Some(HintType::SafetyBypass) {
        agent::agent_notice()
    } else {
        None
    };

    let envelope = ErrorEnvelope {
        error: ErrorBody {
            code: err.code.to_string(),
            message: err.message.clone(),
            hint: err.hint.clone(),
            hint_type: effective_hint_type,
            verify_after: err.verify_after.clone(),
            retriable: err.retriable,
            request_id: err.request_id.clone(),
            more_details: err.more_details.clone(),
            related_resource: err.related_resource.clone(),
            agent_notice,
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

/// Infer the hint type from the error code and hint text content.
///
/// This provides a conservative classification for existing `with_hint()` call sites
/// that don't specify an explicit `HintType`. The logic uses a priority chain:
///
/// 1. Contains a dangerous flag → `SafetyBypass`
/// 2. Error code is `AuthRequired` → `AuthFix`
/// 3. Error code is `RateLimited` / `NetworkError` → `RetrySafe`
/// 4. Hint contains "must be one of" / "Valid " / "(got: " → `SyntaxFix`
///    (enum/casing corrections that preserve intent)
/// 5. Everything else → `SemanticCorrection` (conservative default — the hint
///    may change the operation's meaning; agent should verify)
fn infer_hint_type(code: ErrorCode, hint: &str) -> HintType {
    // 1. Dangerous flag suggestion always takes priority
    if agent::hint_suggests_dangerous_flag(hint) {
        return HintType::SafetyBypass;
    }

    // 2. Auth/login fixes are always safe to auto-apply
    if code == ErrorCode::AuthRequired {
        return HintType::AuthFix;
    }

    // 3. Transient failures — retry is safe
    if code == ErrorCode::RateLimited || code == ErrorCode::NetworkError {
        return HintType::RetrySafe;
    }

    // 4. Enum/casing corrections that preserve the user's original intent.
    // These patterns indicate the hint is fixing syntax (wrong case, invalid enum value)
    // not changing what the operation does.
    if hint.contains("must be one of")
        || hint.contains("Valid values:")
        || hint.contains("Valid roles:")
        || hint.contains("Valid SKUs:")
        || hint.contains("(got: ")
    {
        return HintType::SyntaxFix;
    }

    // 5. Conservative default: if we can't classify, assume the hint may change
    // the operation's semantics. Agents should verify or ask the user.
    HintType::SemanticCorrection
}

/// Check if dry-run is active and render a preview response.
/// Returns `true` if dry-run is active (caller should skip the real operation).
#[inline]
pub fn dry_run_guard(cli: &Cli, operation: &str, details: &Value) -> bool {
    if !cli.dry_run {
        return false;
    }
    let obj = build_dry_run_object(operation, details);
    render_object(cli, &obj, "would_execute");
    true
}

/// Build the `--dry-run` preview envelope for an operation.
///
/// For a destructive operation (per the agent command schema), the envelope
/// gains `"destructive": true` and — when an AI agent is detected — an
/// `agentNotice` telling the agent to confirm the irreversible action with the
/// user before executing it for real.
fn build_dry_run_object(operation: &str, details: &Value) -> Value {
    let mut obj = serde_json::json!({
        "dry_run": true,
        "would_execute": operation,
        "details": details,
        "hint": "Remove --dry-run to execute this operation."
    });
    if agent::is_destructive_operation(operation) {
        obj["destructive"] = Value::Bool(true);
        if let Some(notice) = agent::destructive_notice() {
            obj["agentNotice"] = Value::String(notice);
        }
    }
    obj
}

/// Print one item in plain mode: the `plain_key` field if the item is an object
/// that has it, otherwise the item value itself.
fn render_plain_item(item: &Value, plain_key: &str) {
    if let Some(val) = item.get(plain_key) {
        println!("{}", format_value(val));
    } else {
        println!("{}", format_value(item));
    }
}

/// Render a (possibly query-projected) value in plain mode. Arrays print one
/// element per line; a scalar prints directly; an object prints its `plain_key`
/// (or itself); null prints nothing.
fn render_plain_value(value: &Value, plain_key: &str) {
    match value {
        Value::Array(arr) => {
            for item in arr {
                render_plain_item(item, plain_key);
            }
        }
        Value::Null => {}
        _ => render_plain_item(value, plain_key),
    }
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

/// Render a list of items as delimited text (CSV or TSV).
/// Returns the formatted string with header row + data rows.
/// Each row is terminated with a newline.
fn format_delimited_list(items: &[Value], columns: &[&str], separator: char) -> String {
    let sep_str = separator.to_string();
    let mut output = String::new();
    // Header row
    output.push_str(&columns.join(&sep_str));
    output.push('\n');
    // Data rows
    for item in items {
        let row: Vec<String> = columns
            .iter()
            .map(|col| {
                let val = resolve_nested(item, col);
                format_csv_value(val, separator)
            })
            .collect();
        output.push_str(&row.join(&sep_str));
        output.push('\n');
    }
    output
}

/// Render a single object as delimited text (CSV or TSV).
/// Returns header row (keys) + single data row (values).
/// Falls back to plain `format_value` for non-object values.
fn format_delimited_object(obj: &Value, separator: char) -> String {
    let sep_str = separator.to_string();
    if let Value::Object(map) = obj {
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        let vals: Vec<String> = map
            .values()
            .map(|v| format_csv_value(v, separator))
            .collect();
        format!("{}\n{}\n", keys.join(&sep_str), vals.join(&sep_str))
    } else {
        format!("{}\n", format_value(obj))
    }
}

/// Apply a `JMESPath` query expression to extract/transform data.
///
/// Uses full `JMESPath` specification (see <https://jmespath.org/>).
/// Returns `Value::Null` if the expression is invalid or the result is null.
pub fn apply_query(value: &Value, query: &str) -> Value {
    use std::convert::TryFrom;

    let Ok(var) = jmespath::Variable::try_from(value) else {
        return Value::Null;
    };

    let Ok(expr) = jmespath::compile(query) else {
        return Value::Null;
    };

    let Ok(result) = expr.search(&var) else {
        return Value::Null;
    };

    // Convert the JMESPath Variable back to serde_json::Value
    serde_json::to_value(result.as_ref()).unwrap_or(Value::Null)
}

/// A detected problem with a `--query` expression plus a teaching hint.
struct QueryAdvice {
    message: String,
    hint: String,
}

/// Convert a simple `jq` path (`.a.b`, `.data[].id`, `.[]`) into the equivalent
/// `JMESPath` suggestion: strip the leading `.`, and strip a leading `data`
/// segment (fabio's `--query` already runs on the value UNDER `data`).
fn jq_to_jmespath_suggestion(q: &str) -> String {
    let s = q.trim_start_matches('.');
    // Strip a leading `data` segment (the payload IS the value under `data`).
    let mapped = match s.strip_prefix("data") {
        Some(rest) if rest.starts_with('[') => rest.to_string(), // `.data[]...` → `[]...`
        Some(rest) if rest.starts_with('.') => rest[1..].to_string(), // `.data.x` → `x`
        Some("") => "@".to_string(),                             // `.data` → `@`
        _ => s.to_string(),
    };
    if mapped.is_empty() {
        "@".to_string()
    } else {
        mapped
    }
}

/// `jq`-only tokens that never appear in a valid `JMESPath` projection — their
/// presence signals the caller wrote `jq` instead of `JMESPath`.
const JQ_TOKENS: &[&str] = &[
    ".[]",
    "select(",
    "| .",
    "|.",
    "to_entries",
    "from_entries",
    "map_values(",
    "ascii_downcase",
    "ascii_upcase",
];

/// Detect the two most common `--query` mistakes coding agents make:
/// (1) **`jq` syntax** — fabio's `--query` is `JMESPath` (like Azure CLI's
/// `--query`), NOT `jq`; and (2) **envelope confusion** — querying `data.*`/`count`
/// even though `--query` already runs on the value UNDER `data` (the payload).
/// Returns a teaching message + hint (with a corrected expression when it can be
/// derived). Pure and data-independent. Returns `None` for a plausibly-valid
/// `JMESPath` expression (a compile check catches the rest).
fn analyze_query(query: &str) -> Option<QueryAdvice> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }

    // (1) jq path syntax: a JMESPath expression never starts with '.'.
    if q.starts_with('.') {
        // Complex jq (pipes / select) → generic guidance; a simple path gets a
        // concrete corrected expression.
        if q.contains('|') || q.contains("select(") {
            return Some(QueryAdvice {
                message: format!(
                    "`--query` looks like jq, but fabio's --query is JMESPath: `{query}`"
                ),
                hint: JMESPATH_NOT_JQ_HINT.to_string(),
            });
        }
        let corrected = jq_to_jmespath_suggestion(q);
        return Some(QueryAdvice {
            message: format!(
                "`--query` looks like jq (starts with '.'), but fabio's --query is JMESPath: `{query}`"
            ),
            hint: format!(
                "fabio's --query is JMESPath (https://jmespath.org), like Azure CLI's --query — NOT jq. \
                 JMESPath expressions do not start with '.', and the query runs on the value UNDER `data` \
                 (so never write `.data`). Try: --query '{corrected}'. Count a list with --query 'length([])'."
            ),
        });
    }

    // jq iterate / pipe-to-dot / jq-only builtins appearing mid-expression.
    if let Some(tok) = JQ_TOKENS.iter().find(|t| q.contains(**t)) {
        return Some(QueryAdvice {
            message: format!(
                "`--query` uses jq syntax (`{tok}`), but fabio's --query is JMESPath: `{query}`"
            ),
            hint: JMESPATH_NOT_JQ_HINT.to_string(),
        });
    }

    // (2) Envelope confusion: the payload IS the value under `data`.
    if q == "data" || q.starts_with("data.") || q.starts_with("data[") {
        let inner = q.strip_prefix("data").unwrap_or(q).trim_start_matches('.');
        let corrected = if inner.is_empty() {
            "@".to_string()
        } else {
            inner.to_string()
        };
        return Some(QueryAdvice {
            message: format!(
                "`--query` targets the envelope key `data`, but --query already runs on the payload under `data`: `{query}`"
            ),
            hint: format!(
                "fabio wraps results as {{\"data\": …}}, but --query operates on the value INSIDE `data` \
                 (like Azure CLI). Drop the `data` prefix — use --query '{corrected}' \
                 (e.g. `data[].name` → `[].name`, `data.id` → `id`)."
            ),
        });
    }
    if q == "count" {
        return Some(QueryAdvice {
            message: "`--query count` targets the envelope's `count` field, which is metadata and not part of the queryable payload".to_string(),
            hint: "The envelope's `count` is not visible to --query (which sees the payload under `data`). \
                   Count a list with the JMESPath idiom --query 'length([])'.".to_string(),
        });
    }
    None
}

const JMESPATH_NOT_JQ_HINT: &str = "fabio's --query is JMESPath (https://jmespath.org), like Azure CLI's --query — NOT jq. \
     Filter a list with `[?field=='value']` (not `select(...)`), project with `[].field`, \
     index with `[0]`, count with `length([])`. The query runs on the value under `data`, so never \
     prefix with `data`. Run `fabio context agent --full` for the full output/query contract.";

/// Validate a `--query` expression up front (data-independent) so a malformed or
/// `jq`-shaped query fails fast with a teaching error BEFORE any API call — instead
/// of silently returning `{"data":null}` with exit 0 (which misleads agents that
/// then act on the empty result). Catches `jq` syntax, envelope confusion, and any
/// expression that does not compile as `JMESPath`. Returns the teaching error to
/// surface, or `None` when the query is acceptable.
pub fn validate_query(query: &str) -> Option<FabioError> {
    if let Some(advice) = analyze_query(query) {
        return Some(FabioError::with_typed_hint(
            ErrorCode::InvalidInput,
            advice.message,
            advice.hint,
            HintType::SyntaxFix,
        ));
    }
    if jmespath::compile(query).is_err() {
        return Some(FabioError::with_typed_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --query expression (not valid JMESPath): `{query}`"),
            JMESPATH_NOT_JQ_HINT.to_string(),
            HintType::SyntaxFix,
        ));
    }
    None
}

/// Decode base64-encoded definition parts inline.
/// Adds a `decodedPayload` field alongside the original `payload` for each part.
/// Handles both JSON payloads (parsed into objects) and plain text (kept as strings).
/// Accepts owned `Value` to avoid cloning the entire response.
pub fn decode_definition_parts(mut data: Value) -> Value {
    let base64_engine = BASE64;
    if let Some(parts) = data
        .get_mut("definition")
        .and_then(|d| d.get_mut("parts"))
        .and_then(|p| p.as_array_mut())
    {
        for part in parts {
            if let Some(payload) = part.get("payload").and_then(|p| p.as_str())
                && let Ok(decoded_bytes) = base64_engine.decode(payload)
                && let Ok(decoded_str) = String::from_utf8(decoded_bytes)
            {
                if let Ok(json_val) = serde_json::from_str::<Value>(&decoded_str) {
                    part["decodedPayload"] = json_val;
                } else {
                    part["decodedPayload"] = Value::String(decoded_str);
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
        assert_eq!(apply_query(&obj, "name"), Value::from("test"));
    }

    #[test]
    fn apply_query_extracts_nested_field() {
        let obj = serde_json::json!({"a": {"b": {"c": 42}}});
        assert_eq!(apply_query(&obj, "a.b.c"), serde_json::json!(42));
    }

    #[test]
    fn apply_query_extracts_array_field() {
        let obj = serde_json::json!({"items": [
            {"name": "alpha", "id": "1"},
            {"name": "beta", "id": "2"},
        ]});
        let result = apply_query(&obj, "items[*].name");
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
    fn apply_query_array_index() {
        let obj = serde_json::json!({"items": ["a", "b", "c"]});
        assert_eq!(apply_query(&obj, "items[0]"), serde_json::json!("a"));
        assert_eq!(apply_query(&obj, "items[2]"), serde_json::json!("c"));
    }

    #[test]
    fn apply_query_array_slice() {
        let obj = serde_json::json!({"items": [0, 1, 2, 3, 4]});
        assert_eq!(apply_query(&obj, "items[1:3]"), serde_json::json!([1, 2]));
    }

    #[test]
    fn apply_query_multiselect_list() {
        let obj = serde_json::json!({"a": 1, "b": 2, "c": 3});
        assert_eq!(apply_query(&obj, "[a, c]"), serde_json::json!([1, 3]));
    }

    #[test]
    fn apply_query_multiselect_hash() {
        let obj = serde_json::json!({"name": "fabio", "version": "1.0"});
        assert_eq!(
            apply_query(&obj, "{tool: name, ver: version}"),
            serde_json::json!({"tool": "fabio", "ver": "1.0"})
        );
    }

    #[test]
    fn apply_query_filter_expression() {
        let obj = serde_json::json!({"items": [
            {"name": "a", "size": 10},
            {"name": "b", "size": 50},
            {"name": "c", "size": 30},
        ]});
        let result = apply_query(&obj, "items[?size > `20`].name");
        assert_eq!(result, serde_json::json!(["b", "c"]));
    }

    #[test]
    fn apply_query_pipe_expression() {
        let obj = serde_json::json!({"items": [
            {"name": "alpha"},
            {"name": "beta"},
            {"name": "gamma"},
        ]});
        let result = apply_query(&obj, "items[*].name | [0]");
        assert_eq!(result, serde_json::json!("alpha"));
    }

    #[test]
    fn apply_query_length_function() {
        let obj = serde_json::json!({"items": [1, 2, 3, 4, 5]});
        assert_eq!(apply_query(&obj, "length(items)"), serde_json::json!(5));
    }

    #[test]
    fn apply_query_invalid_expression_returns_null() {
        let obj = serde_json::json!({"name": "test"});
        // Invalid JMESPath syntax
        assert_eq!(apply_query(&obj, "[[[invalid"), Value::Null);
    }

    #[test]
    fn validate_query_accepts_valid_jmespath() {
        // Common valid JMESPath forms an agent would use must pass unchanged.
        for q in [
            "id",
            "[].displayName",
            "[0].id",
            "[?type=='Lakehouse'].id",
            "length([])",
            "{name: displayName, id: id}",
            "sort_by([], &name)",
            "[?size > `10`].name",
            "people | [0]",
            "@",
        ] {
            assert!(
                validate_query(q).is_none(),
                "should accept valid JMESPath: {q}"
            );
        }
    }

    #[test]
    fn validate_query_rejects_jq_leading_dot_with_suggestion() {
        let err = validate_query(".data[].name").unwrap();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        let hint = err.hint.unwrap();
        // Suggests the corrected JMESPath (leading dot + data prefix stripped).
        assert!(
            hint.contains("'[].name'"),
            "hint should suggest '[].name': {hint}"
        );
        assert!(hint.contains("JMESPath"));
    }

    #[test]
    fn validate_query_rejects_jq_select_pipe() {
        let err = validate_query("[] | select(.type=='X')").unwrap();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(err.message.contains("jq"));
    }

    #[test]
    fn validate_query_rejects_envelope_data_prefix() {
        let err = validate_query("data[].displayName").unwrap();
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("'[].displayName'"),
            "should suggest dropping the data prefix: {hint}"
        );
    }

    #[test]
    fn validate_query_rejects_bare_count_suggests_length() {
        let err = validate_query("count").unwrap();
        assert!(err.hint.unwrap().contains("length([])"));
    }

    #[test]
    fn validate_query_rejects_invalid_jmespath_syntax() {
        // Not jq, not envelope confusion, just malformed — still a teaching error
        // (not a silent null).
        let err = validate_query("[[[invalid").unwrap();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(err.hint.unwrap().contains("JMESPath"));
    }

    #[test]
    fn jq_to_jmespath_suggestion_strips_dot_and_data() {
        assert_eq!(jq_to_jmespath_suggestion(".data[].id"), "[].id");
        assert_eq!(jq_to_jmespath_suggestion(".data.id"), "id");
        assert_eq!(jq_to_jmespath_suggestion(".id"), "id");
        assert_eq!(jq_to_jmespath_suggestion(".[]"), "[]");
        assert_eq!(jq_to_jmespath_suggestion(".data"), "@");
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
    fn dry_run_object_marks_destructive_operations() {
        let details = serde_json::json!({"id": "x"});
        // A destructive command (per commands.json) gets the destructive marker.
        let obj = build_dry_run_object("item delete", &details);
        assert_eq!(obj["destructive"], Value::Bool(true));
        assert_eq!(obj["dry_run"], Value::Bool(true));
        assert_eq!(obj["would_execute"], Value::from("item delete"));
    }

    #[test]
    fn dry_run_object_omits_destructive_for_read_only_operations() {
        let details = serde_json::json!({});
        let obj = build_dry_run_object("workspace list", &details);
        assert!(obj.get("destructive").is_none());
        // A non-destructive op never carries the agent confirm notice.
        assert!(obj.get("agentNotice").is_none());
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

    /// Helper to construct a Cli for testing (parses args after "fabio context agent").
    fn make_test_cli(extra_args: &[&str]) -> Cli {
        const VALID_OUTPUT_VALUES: &str = "json, table, plain, csv, tsv";

        let mut cli = Cli {
            output: OutputFormat::Json,
            json: false,
            query: None,
            quiet: false,
            force: false,
            dry_run: false,
            verbose: false,
            readonly: false,
            wrap_untrusted: false,
            enable_commands: None,
            disable_commands: None,
            limit: None,
            all: false,
            continuation_token: None,
            profile: None,
            lro_timeout: None,
            command: Command::Context {
                command: crate::commands::context::ContextCommand::Agent {
                    group: None,
                    full: false,
                    format: crate::commands::context::AgentFormat::Native,
                    budget: None,
                },
            },
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
            hint_type: None,
            verify_after: None,
            retriable: Some(true),
            request_id: None,
            more_details: None,
            related_resource: None,
            agent_notice: None,
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
            hint_type: None,
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
            agent_notice: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(!json.contains("retriable"));
    }

    #[test]
    fn error_body_serializes_request_id_when_set() {
        let body = ErrorBody {
            code: "API_ERROR".to_string(),
            message: "server error".to_string(),
            hint: None,
            hint_type: None,
            verify_after: None,
            retriable: None,
            request_id: Some("cfafbeb1-8037-4d0c-896e-a46fb27ff227".to_string()),
            more_details: None,
            related_resource: None,
            agent_notice: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains(r#""requestId":"cfafbeb1-8037-4d0c-896e-a46fb27ff227""#));
    }

    #[test]
    fn error_body_omits_request_id_when_none() {
        let body = ErrorBody {
            code: "NOT_FOUND".to_string(),
            message: "not found".to_string(),
            hint: None,
            hint_type: None,
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
            agent_notice: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(!json.contains("requestId"));
    }

    #[test]
    fn error_body_serializes_more_details_when_set() {
        let body = ErrorBody {
            code: "API_ERROR".to_string(),
            message: "validation failed".to_string(),
            hint: None,
            hint_type: None,
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: Some(vec![
                ErrorDetail {
                    error_code: "InvalidField".to_string(),
                    message: "name is required".to_string(),
                },
                ErrorDetail {
                    error_code: "InvalidField".to_string(),
                    message: "type is invalid".to_string(),
                },
            ]),
            related_resource: None,
            agent_notice: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains(r#""moreDetails""#));
        assert!(json.contains(r#""errorCode":"InvalidField""#));
        assert!(json.contains(r#""name is required""#));
    }

    #[test]
    fn error_body_serializes_related_resource_when_set() {
        let body = ErrorBody {
            code: "NOT_FOUND".to_string(),
            message: "item not found".to_string(),
            hint: None,
            hint_type: None,
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: Some(RelatedResource {
                resource_id: "abc-123".to_string(),
                resource_type: "Notebook".to_string(),
            }),
            agent_notice: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains(r#""relatedResource""#));
        assert!(json.contains(r#""resourceId":"abc-123""#));
        assert!(json.contains(r#""resourceType":"Notebook""#));
    }

    #[test]
    fn error_body_omits_all_optional_fields_when_none() {
        let body = ErrorBody {
            code: "UNKNOWN".to_string(),
            message: "something".to_string(),
            hint: None,
            hint_type: None,
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
            agent_notice: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        // Should only have code and message
        assert!(!json.contains("hint"));
        assert!(!json.contains("hintType"));
        assert!(!json.contains("verifyAfter"));
        assert!(!json.contains("retriable"));
        assert!(!json.contains("requestId"));
        assert!(!json.contains("moreDetails"));
        assert!(!json.contains("relatedResource"));
        assert!(!json.contains("agentNotice"));
    }

    #[test]
    fn error_body_serializes_agent_notice_when_set() {
        let body = ErrorBody {
            code: "INVALID_INPUT".to_string(),
            message: "plan is stale".to_string(),
            hint: Some("Use --force to apply anyway.".to_string()),
            hint_type: Some(HintType::SafetyBypass),
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
            agent_notice: Some(
                "Note for AI agents (Claude Code): do not retry with the safety-bypass flag"
                    .to_string(),
            ),
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains(r#""agentNotice""#));
        assert!(json.contains("do not retry"));
        assert!(json.contains(r#""hintType":"safety_bypass""#));
    }

    #[test]
    fn error_body_omits_agent_notice_when_none() {
        let body = ErrorBody {
            code: "NOT_FOUND".to_string(),
            message: "not found".to_string(),
            hint: Some("Check with fabio list.".to_string()),
            hint_type: Some(HintType::SemanticCorrection),
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
            agent_notice: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(!json.contains("agentNotice"));
        assert!(json.contains(r#""hintType":"semantic_correction""#));
    }

    #[test]
    fn error_body_serializes_hint_type_and_verify_after() {
        let body = ErrorBody {
            code: "INVALID_INPUT".to_string(),
            message: "Invalid load mode".to_string(),
            hint: Some("--mode must be one of: Overwrite, Append".to_string()),
            hint_type: Some(HintType::SyntaxFix),
            verify_after: Some(
                "fabio lakehouse show-table --workspace $WS --id $LH --name $TABLE".to_string(),
            ),
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
            agent_notice: None,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains(r#""hintType":"syntax_fix""#));
        assert!(json.contains(r#""verifyAfter""#));
        assert!(json.contains("show-table"));
    }

    // ─── infer_hint_type tests ───────────────────────────────────────────────

    #[test]
    fn infer_hint_type_dangerous_flag_is_safety_bypass() {
        assert_eq!(
            infer_hint_type(ErrorCode::InvalidInput, "Use --force to apply anyway."),
            HintType::SafetyBypass
        );
        assert_eq!(
            infer_hint_type(
                ErrorCode::InvalidInput,
                "Use --overwrite to replace existing content."
            ),
            HintType::SafetyBypass
        );
        assert_eq!(
            infer_hint_type(
                ErrorCode::InvalidInput,
                "Use --hard-delete to permanently remove."
            ),
            HintType::SafetyBypass
        );
    }

    #[test]
    fn infer_hint_type_auth_required_is_auth_fix() {
        assert_eq!(
            infer_hint_type(
                ErrorCode::AuthRequired,
                "Run 'fabio auth login' to authenticate."
            ),
            HintType::AuthFix
        );
    }

    #[test]
    fn infer_hint_type_rate_limited_is_retry_safe() {
        assert_eq!(
            infer_hint_type(
                ErrorCode::RateLimited,
                "Too many requests. Retry after a short backoff."
            ),
            HintType::RetrySafe
        );
    }

    #[test]
    fn infer_hint_type_network_error_is_retry_safe() {
        assert_eq!(
            infer_hint_type(
                ErrorCode::NetworkError,
                "Connection timed out. Retry in a few seconds."
            ),
            HintType::RetrySafe
        );
    }

    #[test]
    fn infer_hint_type_enum_correction_is_syntax_fix() {
        assert_eq!(
            infer_hint_type(
                ErrorCode::InvalidInput,
                "--mode must be one of: Overwrite, Append (got: 'overwrite')"
            ),
            HintType::SyntaxFix
        );
        assert_eq!(
            infer_hint_type(
                ErrorCode::InvalidInput,
                "Valid roles: Admin, Member, Contributor, Viewer."
            ),
            HintType::SyntaxFix
        );
    }

    #[test]
    fn infer_hint_type_default_is_semantic_correction() {
        assert_eq!(
            infer_hint_type(
                ErrorCode::InvalidInput,
                "Remove --readonly flag or set FABIO_READONLY=0 to allow mutations."
            ),
            HintType::SemanticCorrection
        );
        assert_eq!(
            infer_hint_type(
                ErrorCode::NotFound,
                "Run 'fabio lakehouse list' to see available items."
            ),
            HintType::SemanticCorrection
        );
    }

    #[test]
    fn infer_hint_type_dangerous_flag_overrides_auth_code() {
        // If hint contains a dangerous flag, it's SafetyBypass even if error code is AuthRequired
        assert_eq!(
            infer_hint_type(ErrorCode::AuthRequired, "Use --force to bypass."),
            HintType::SafetyBypass
        );
    }

    // ─── resolve_nested tests ────────────────────────────────────────────────

    #[test]
    fn resolve_nested_simple_key() {
        let obj = serde_json::json!({"name": "Alice", "id": "123"});
        assert_eq!(resolve_nested(&obj, "name"), &Value::String("Alice".into()));
    }

    #[test]
    fn resolve_nested_dot_path() {
        let obj = serde_json::json!({"properties": {"queryServiceUri": "https://example.com"}});
        assert_eq!(
            resolve_nested(&obj, "properties.queryServiceUri"),
            &Value::String("https://example.com".into())
        );
    }

    #[test]
    fn resolve_nested_deep_path() {
        let obj = serde_json::json!({"a": {"b": {"c": {"d": 42}}}});
        assert_eq!(resolve_nested(&obj, "a.b.c.d"), &serde_json::json!(42));
    }

    #[test]
    fn resolve_nested_missing_key_returns_null() {
        let obj = serde_json::json!({"name": "test"});
        assert_eq!(resolve_nested(&obj, "missing"), &Value::Null);
    }

    #[test]
    fn resolve_nested_partial_path_returns_null() {
        let obj = serde_json::json!({"a": {"b": 1}});
        assert_eq!(resolve_nested(&obj, "a.x.y"), &Value::Null);
    }

    #[test]
    fn resolve_nested_on_non_object_returns_null() {
        let obj = serde_json::json!("just a string");
        assert_eq!(resolve_nested(&obj, "key"), &Value::Null);
    }

    // ─── format_csv_value edge cases for query results ───────────────────────

    #[test]
    fn format_csv_value_float() {
        let val = serde_json::json!(99.95);
        assert_eq!(format_csv_value(&val, ','), "99.95");
    }

    #[test]
    fn format_csv_value_large_integer() {
        let val = serde_json::json!(9_007_199_254_740_991_i64);
        assert_eq!(format_csv_value(&val, ','), "9007199254740991");
    }

    #[test]
    fn format_csv_value_boolean_true() {
        let val = serde_json::json!(true);
        assert_eq!(format_csv_value(&val, ','), "true");
    }

    #[test]
    fn format_csv_value_boolean_false() {
        let val = serde_json::json!(false);
        assert_eq!(format_csv_value(&val, ','), "false");
    }

    #[test]
    fn format_csv_value_date_string() {
        let val = Value::String("2024-01-15T10:30:00Z".into());
        assert_eq!(format_csv_value(&val, ','), "2024-01-15T10:30:00Z");
    }

    #[test]
    fn format_csv_value_empty_string() {
        let val = Value::String(String::new());
        assert_eq!(format_csv_value(&val, ','), "");
    }

    #[test]
    fn format_csv_value_nested_object() {
        let val = serde_json::json!({"key": "value", "num": 42});
        let result = format_csv_value(&val, ',');
        // Contains comma from JSON serialization, so must be quoted
        assert!(result.starts_with('"'));
        assert!(result.ends_with('"'));
        assert!(result.contains("key"));
    }

    #[test]
    fn format_csv_value_array_value() {
        let val = serde_json::json!([1, 2, 3]);
        let result = format_csv_value(&val, ',');
        // Array serialization contains commas, so must be quoted
        assert!(result.starts_with('"'));
        assert!(result.ends_with('"'));
        assert!(result.contains("[1"));
    }

    #[test]
    fn format_csv_value_carriage_return_quotes() {
        let val = Value::String("line1\r\nline2".into());
        assert_eq!(format_csv_value(&val, ','), "\"line1\r\nline2\"");
    }

    #[test]
    fn format_csv_value_tsv_no_quote_for_comma() {
        // In TSV mode, commas should NOT trigger quoting
        let val = Value::String("foo,bar".into());
        assert_eq!(format_csv_value(&val, '\t'), "foo,bar");
    }

    #[test]
    fn format_csv_value_tsv_quotes_tab() {
        let val = Value::String("has\ttab".into());
        assert_eq!(format_csv_value(&val, '\t'), "\"has\ttab\"");
    }

    // ─── format_delimited_list tests ─────────────────────────────────────────

    #[test]
    fn delimited_list_basic_tabular_csv() {
        let items = vec![
            serde_json::json!({"name": "Alice", "age": 30, "city": "Paris"}),
            serde_json::json!({"name": "Bob", "age": 25, "city": "London"}),
            serde_json::json!({"name": "Carol", "age": 35, "city": "Berlin"}),
        ];
        let columns = &["name", "age", "city"];
        let result = format_delimited_list(&items, columns, ',');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 4); // header + 3 rows
        assert_eq!(lines[0], "name,age,city");
        assert_eq!(lines[1], "Alice,30,Paris");
        assert_eq!(lines[2], "Bob,25,London");
        assert_eq!(lines[3], "Carol,35,Berlin");
    }

    #[test]
    fn delimited_list_basic_tabular_tsv() {
        let items = vec![
            serde_json::json!({"col1": 1, "col2": "hello"}),
            serde_json::json!({"col1": 2, "col2": "world"}),
        ];
        let columns = &["col1", "col2"];
        let result = format_delimited_list(&items, columns, '\t');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "col1\tcol2");
        assert_eq!(lines[1], "1\thello");
        assert_eq!(lines[2], "2\tworld");
    }

    #[test]
    fn delimited_list_null_values_empty_cells() {
        let items = vec![
            serde_json::json!({"id": 1, "name": "test", "value": null}),
            serde_json::json!({"id": 2, "name": null, "value": 42}),
        ];
        let columns = &["id", "name", "value"];
        let result = format_delimited_list(&items, columns, ',');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[1], "1,test,");
        assert_eq!(lines[2], "2,,42");
    }

    #[test]
    fn delimited_list_nested_json_in_cells() {
        let items = vec![serde_json::json!({
            "id": 1,
            "metadata": {"key": "val"}
        })];
        let columns = &["id", "metadata"];
        let result = format_delimited_list(&items, columns, ',');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[0], "id,metadata");
        // metadata is a JSON object, should be quoted since it contains commas
        assert!(lines[1].starts_with("1,\""));
        assert!(lines[1].contains("key"));
    }

    #[test]
    fn delimited_list_comma_in_string_value() {
        let items = vec![serde_json::json!({"name": "Doe, John", "id": 1})];
        let columns = &["name", "id"];
        let result = format_delimited_list(&items, columns, ',');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[1], "\"Doe, John\",1");
    }

    #[test]
    fn delimited_list_newline_in_value() {
        let items = vec![serde_json::json!({"msg": "line1\nline2", "id": 1})];
        let columns = &["id", "msg"];
        let result = format_delimited_list(&items, columns, ',');
        // Should contain quoted multiline value
        assert!(result.contains("\"line1\nline2\""));
    }

    #[test]
    fn delimited_list_empty_result_set() {
        let items: Vec<Value> = vec![];
        let columns = &["col1", "col2", "col3"];
        let result = format_delimited_list(&items, columns, ',');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 1); // header only
        assert_eq!(lines[0], "col1,col2,col3");
    }

    #[test]
    fn delimited_list_single_column() {
        let items = vec![
            serde_json::json!({"count": 42}),
            serde_json::json!({"count": 99}),
        ];
        let columns = &["count"];
        let result = format_delimited_list(&items, columns, ',');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "count");
        assert_eq!(lines[1], "42");
        assert_eq!(lines[2], "99");
    }

    #[test]
    fn delimited_list_dynamic_query_columns() {
        // Simulates typical SQL query result with mixed column types
        let items = vec![
            serde_json::json!({
                "Name": "Widget A",
                "Total Revenue": 1234.56,
                "Created Date": "2024-03-15",
                "Active": true
            }),
            serde_json::json!({
                "Name": "Widget B",
                "Total Revenue": 789.01,
                "Created Date": "2024-06-20",
                "Active": false
            }),
        ];
        let columns = &["Name", "Total Revenue", "Created Date", "Active"];
        let result = format_delimited_list(&items, columns, ',');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "Name,Total Revenue,Created Date,Active");
        assert_eq!(lines[1], "Widget A,1234.56,2024-03-15,true");
        assert_eq!(lines[2], "Widget B,789.01,2024-06-20,false");
    }

    #[test]
    fn delimited_list_missing_columns_render_empty() {
        // When a row doesn't have a column, it should render as empty
        let items = vec![
            serde_json::json!({"a": 1, "b": 2}),
            serde_json::json!({"a": 3}), // missing "b"
        ];
        let columns = &["a", "b"];
        let result = format_delimited_list(&items, columns, ',');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[1], "1,2");
        assert_eq!(lines[2], "3,"); // missing column renders empty
    }

    #[test]
    fn delimited_list_quotes_in_values() {
        let items = vec![serde_json::json!({"text": "say \"hello\"", "id": 1})];
        let columns = &["id", "text"];
        let result = format_delimited_list(&items, columns, ',');
        let lines: Vec<&str> = result.lines().collect();
        // Quotes in value should be doubled per RFC 4180
        assert_eq!(lines[1], "1,\"say \"\"hello\"\"\"");
    }

    #[test]
    fn delimited_list_tsv_comma_not_quoted() {
        // In TSV mode, commas in values should NOT be quoted
        let items = vec![serde_json::json!({"name": "Doe, John", "id": 1})];
        let columns = &["name", "id"];
        let result = format_delimited_list(&items, columns, '\t');
        let lines: Vec<&str> = result.lines().collect();
        // Tab separator, comma in value is fine without quoting
        assert_eq!(lines[1], "Doe, John\t1");
    }

    // ─── format_delimited_object tests ───────────────────────────────────────

    #[test]
    fn delimited_object_basic_csv() {
        let obj = serde_json::json!({"status": "ok", "rows_affected": 5});
        let result = format_delimited_object(&obj, ',');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        // Keys as header, values as data row
        assert!(lines[0].contains("status"));
        assert!(lines[0].contains("rows_affected"));
        assert!(lines[1].contains("ok"));
        assert!(lines[1].contains('5'));
    }

    #[test]
    fn delimited_object_basic_tsv() {
        let obj = serde_json::json!({"col1": "value1", "col2": 42});
        let result = format_delimited_object(&obj, '\t');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains('\t'));
        assert!(lines[1].contains('\t'));
    }

    #[test]
    fn delimited_object_non_object_scalar() {
        let val = serde_json::json!("just a string");
        let result = format_delimited_object(&val, ',');
        assert_eq!(result, "just a string\n");
    }

    #[test]
    fn delimited_object_query_empty_result() {
        // Typical empty-result object from query commands
        let obj = serde_json::json!({
            "rows_returned": 0,
            "message": "Query executed successfully (no results returned)."
        });
        let result = format_delimited_object(&obj, ',');
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("rows_returned"));
        assert!(lines[0].contains("message"));
        assert!(lines[1].contains('0'));
        assert!(lines[1].contains("Query executed successfully"));
    }

    // ─── has_tags tests ──────────────────────────────────────────────────────

    #[test]
    fn has_tags_returns_false_for_empty_list() {
        assert!(!super::has_tags(&[]));
    }

    #[test]
    fn has_tags_returns_false_when_no_items_have_tags() {
        let items = vec![
            serde_json::json!({"displayName": "A", "id": "1"}),
            serde_json::json!({"displayName": "B", "id": "2", "tags": []}),
        ];
        assert!(!super::has_tags(&items));
    }

    #[test]
    fn has_tags_returns_true_when_any_item_has_tags() {
        let items = vec![
            serde_json::json!({"displayName": "A", "id": "1"}),
            serde_json::json!({"displayName": "B", "id": "2", "tags": [{"id": "t1", "displayName": "Prod"}]}),
        ];
        assert!(super::has_tags(&items));
    }

    // ─── enrich_with_tags_display tests ──────────────────────────────────────

    #[test]
    fn enrich_with_tags_display_adds_comma_separated_names() {
        let items = vec![
            serde_json::json!({"displayName": "A", "tags": [{"id": "t1", "displayName": "Prod"}, {"id": "t2", "displayName": "Finance"}]}),
            serde_json::json!({"displayName": "B"}),
        ];
        let enriched = super::enrich_with_tags_display(&items);
        assert_eq!(enriched[0]["_tagsDisplay"], "Prod, Finance");
        assert!(enriched[1].get("_tagsDisplay").is_none());
    }

    #[test]
    fn enrich_with_tags_display_handles_missing_display_name() {
        let items = vec![
            serde_json::json!({"displayName": "A", "tags": [{"id": "t1"}, {"id": "t2", "displayName": "OK"}]}),
        ];
        let enriched = super::enrich_with_tags_display(&items);
        assert_eq!(enriched[0]["_tagsDisplay"], "OK");
    }
}
