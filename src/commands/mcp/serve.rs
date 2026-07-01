//! MCP JSON-RPC 2.0 server over stdio.
//!
//! Handles `initialize`, `notifications/initialized`, `tools/list`, and `tools/call`.
//! Read-only by default: mutation tools are hidden unless `allow_write` is true.

use std::io::Write;

use anyhow::Result;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::cli::Cli;

/// Safety policy for the MCP server session.
struct McpPolicy {
    allow_write: bool,
    allow_tool_patterns: Option<Vec<String>>,
}

impl McpPolicy {
    /// Check if a tool should be visible (listed in tools/list).
    fn is_tool_visible(&self, tool_name: &str, mutates: bool) -> bool {
        // If mutation and write not allowed, hide it.
        if mutates && !self.allow_write {
            return false;
        }
        // If allow_tool patterns set, check match via glob pattern system.
        if let Some(patterns) = &self.allow_tool_patterns {
            return patterns
                .iter()
                .any(|p| mcp_tool_matches_pattern(tool_name, p));
        }
        true
    }
}

/// Match an MCP tool name against a user-provided pattern.
///
/// Tool names are `fabio_{group}_{subcommand}` (all underscores).
/// Patterns use dot notation with glob support: `workspace.list`, `*.delete`, `kql-*`.
/// Hyphens in patterns are normalized to underscores for matching.
fn mcp_tool_matches_pattern(tool_name: &str, pattern: &str) -> bool {
    // Strip the "fabio_" prefix from the tool name.
    let Some(rest) = tool_name.strip_prefix("fabio_") else {
        return false;
    };

    // Normalize the pattern: hyphens → underscores (tool names use underscores).
    let normalized_pattern = pattern.replace('-', "_");

    // Convert the dot-separated pattern to underscore-separated for direct matching.
    // Pattern "workspace.list" → "workspace_list" for prefix/exact matching.
    // But we need glob support, so we use command_pattern_matches with dots.
    //
    // Strategy: convert the TOOL NAME from underscore to dot notation by finding
    // the group boundary. We know all group names from the schema, but to keep it
    // simple we match patterns directly against the underscore-form using the
    // same glob logic but with '_' in the pattern treated as literal separators
    // within group/subcommand names, and '.' as the group-subcommand boundary.

    // Simple approach: if pattern has a dot separator, split there.
    // Otherwise it's a group-only match (prefix).
    if let Some((pat_group, pat_sub)) = normalized_pattern.split_once('.') {
        // Pattern has group.subcommand — need to find the boundary in tool name.
        // Match: tool must start with group pattern + "_" + subcommand pattern.
        let pat_sub_underscore = pat_sub.replace('.', "_");
        if pat_group == "*" {
            // *.delete pattern: subcommand is at the end after any group prefix
            // Match if the tool name ENDS with _<subcommand>
            let suffix = format!("_{pat_sub_underscore}");
            return segment_matches_str(rest, &format!("*{suffix}"));
        }
        // Specific group pattern: find tool names that start with group_
        let group_prefix = format!("{pat_group}_");
        if let Some(sub_part) = rest.strip_prefix(&group_prefix) {
            return crate::commands::segment_matches(sub_part, &pat_sub_underscore);
        }
        // Also try wildcard in group: kql_* → prefix match
        if pat_group.contains('*') {
            // Try matching the group part
            // Find where the group ends: try each possible split point
            for i in 1..rest.len() {
                if rest.as_bytes().get(i) == Some(&b'_') {
                    let candidate_group = &rest[..i];
                    let candidate_sub = &rest[i + 1..];
                    if crate::commands::segment_matches(candidate_group, pat_group)
                        && crate::commands::segment_matches(candidate_sub, &pat_sub_underscore)
                    {
                        return true;
                    }
                }
            }
        }
        false
    } else {
        // No dot — group-level match (all subcommands in group).
        // Pattern "workspace" matches "workspace_list", "workspace_create", etc.
        if normalized_pattern == "*" {
            return true;
        }
        if normalized_pattern.contains('*') {
            // Glob in group name: "kql_*" matches "kql_database_query"
            return crate::commands::segment_matches(rest, &format!("{normalized_pattern}_*"))
                || crate::commands::segment_matches(rest, &normalized_pattern);
        }
        // Exact group prefix: "workspace" matches "workspace_*"
        rest.starts_with(&format!("{normalized_pattern}_")) || rest == normalized_pattern
    }
}

/// Helper for pattern matching on full string with `*` wildcard.
fn segment_matches_str(value: &str, pattern: &str) -> bool {
    crate::commands::segment_matches(value, pattern)
}

/// Print the list of tools that would be exposed with the given policy, then exit.
pub(super) fn list_tools(cli: &Cli, allow_write: bool, allow_tool: Option<&[String]>) {
    let policy = McpPolicy {
        allow_write,
        allow_tool_patterns: allow_tool.map(<[String]>::to_vec),
    };

    let all_tools = build_mcp_tools();
    let filtered: Vec<Value> = all_tools
        .into_iter()
        .filter(|tool| {
            let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
            let mutates = tool
                .get("annotations")
                .and_then(|a| a.get("readOnlyHint"))
                .and_then(Value::as_bool)
                == Some(false);
            policy.is_tool_visible(name, mutates)
        })
        .collect();

    let output = json!({
        "tools": filtered,
        "count": filtered.len(),
        "policy": {
            "allow_write": allow_write,
            "allow_tool": allow_tool,
        }
    });
    crate::output::render_object(cli, &output, "count");
}

/// Run the MCP server, reading JSON-RPC messages from stdin and writing responses to stdout.
pub(super) async fn run(
    _cli: &Cli,
    allow_write: bool,
    allow_tool: Option<&[String]>,
) -> Result<()> {
    let policy = McpPolicy {
        allow_write,
        allow_tool_patterns: allow_tool.map(<[String]>::to_vec),
    };

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_response(&json!({
                    "jsonrpc": "2.0",
                    "error": {"code": -32700, "message": format!("Parse error: {e}")},
                    "id": null
                }))?;
                continue;
            }
        };

        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");

        // Notifications have no id — don't send a response.
        let is_notification = id.is_none();

        let response = match method {
            "initialize" => Some(handle_initialize(id.as_ref())),
            "notifications/initialized" | "initialized" => None,
            "tools/list" => Some(handle_tools_list(id.as_ref(), &policy)),
            "tools/call" => Some(handle_tools_call(&request, id.as_ref(), &policy).await),
            "ping" => Some(json!({"jsonrpc": "2.0", "result": {}, "id": id})),
            _ => {
                if is_notification {
                    None
                } else {
                    Some(json!({
                        "jsonrpc": "2.0",
                        "error": {"code": -32601, "message": format!("Method not found: {method}")},
                        "id": id
                    }))
                }
            }
        };

        if let Some(resp) = response {
            write_response(&resp)?;
        }
    }

    Ok(())
}

/// Write a JSON-RPC response to stdout (synchronous, avoids holding locks across await).
fn write_response(value: &Value) -> Result<()> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    writeln!(lock, "{value}")?;
    lock.flush()?;
    Ok(())
}

fn handle_initialize(id: Option<&Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "fabio",
                "version": env!("CARGO_PKG_VERSION")
            }
        },
        "id": id
    })
}

fn handle_tools_list(id: Option<&Value>, policy: &McpPolicy) -> Value {
    let all_tools = build_mcp_tools();
    let filtered: Vec<Value> = all_tools
        .into_iter()
        .filter(|tool| {
            let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
            let mutates = tool
                .get("annotations")
                .and_then(|a| a.get("readOnlyHint"))
                .and_then(Value::as_bool)
                == Some(false);
            policy.is_tool_visible(name, mutates)
        })
        .collect();
    json!({
        "jsonrpc": "2.0",
        "result": {"tools": filtered},
        "id": id
    })
}

async fn handle_tools_call(request: &Value, id: Option<&Value>, policy: &McpPolicy) -> Value {
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let tool_name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // Parse tool name back to group + subcommand.
    let Some((group, cmd)) = parse_tool_name(tool_name) else {
        return json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [{"type": "text", "text": json!({"error": {"code": "INVALID_INPUT", "message": format!("Unknown tool: {tool_name}")}}).to_string()}],
                "isError": true
            },
            "id": id
        });
    };

    // Check if tool is allowed by policy (catches calls to tools not in tools/list).
    let mutates = is_tool_mutating(tool_name);
    if !policy.is_tool_visible(tool_name, mutates) {
        let reason = if mutates && !policy.allow_write {
            "Tool is a mutation and --allow-write was not set"
        } else {
            "Tool is not in the --allow-tool filter"
        };
        return json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [{"type": "text", "text": json!({"error": {"code": "FORBIDDEN", "message": format!("Tool '{tool_name}' is blocked: {reason}")}}).to_string()}],
                "isError": true
            },
            "id": id
        });
    }

    // Build CLI arguments from JSON.
    let args = build_cli_args(&group, &cmd, &arguments);

    // Execute the command as a subprocess (avoids global state issues, clean isolation).
    let output =
        tokio::process::Command::new(std::env::current_exe().unwrap_or_else(|_| "fabio".into()))
            .args(&args)
            .env("FABIO_OUTPUT", "json")
            .output()
            .await;

    match output {
        Ok(result) => {
            let stdout_text = String::from_utf8_lossy(&result.stdout).to_string();
            let stderr_text = String::from_utf8_lossy(&result.stderr).to_string();

            if result.status.success() {
                json!({
                    "jsonrpc": "2.0",
                    "result": {
                        "content": [{"type": "text", "text": stdout_text}]
                    },
                    "id": id
                })
            } else {
                let error_text = if stderr_text.is_empty() {
                    stdout_text
                } else {
                    stderr_text
                };
                json!({
                    "jsonrpc": "2.0",
                    "result": {
                        "content": [{"type": "text", "text": error_text}],
                        "isError": true
                    },
                    "id": id
                })
            }
        }
        Err(e) => json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [{"type": "text", "text": json!({"error": {"code": "NETWORK_ERROR", "message": format!("Failed to execute: {e}")}}).to_string()}],
                "isError": true
            },
            "id": id
        }),
    }
}

/// Parse an MCP tool name back to (group, subcommand).
/// `fabio_lakehouse_sync` -> `("lakehouse", "sync")`
/// `fabio_kql_database_query` -> `("kql-database", "query")`
/// Check if a tool is a mutating operation by looking up its schema annotation.
fn is_tool_mutating(tool_name: &str) -> bool {
    let Some((group, cmd)) = parse_tool_name(tool_name) else {
        return false;
    };
    let commands = crate::commands::context::agent_commands_schema();
    let group_normalized = group.replace('-', "_");
    commands
        .as_object()
        .and_then(|m| m.get(&group).or_else(|| m.get(&group_normalized)))
        .and_then(|g| g.get("subcommands"))
        .and_then(Value::as_object)
        .and_then(|s| {
            let cmd_normalized = cmd.replace('_', "-");
            s.get(&cmd).or_else(|| s.get(&cmd_normalized))
        })
        .and_then(|c| c.get("mutates"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn parse_tool_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("fabio_")?;

    // Load the commands schema to match against known groups.
    let commands = crate::commands::context::agent_commands_schema();
    let commands_map = commands.as_object()?;

    // Try progressively longer group prefixes (handles multi-word groups like kql_database).
    let parts: Vec<&str> = rest.splitn(10, '_').collect();
    for split_point in (1..parts.len()).rev() {
        let group_candidate = parts[..split_point].join("-");
        let cmd_candidate = parts[split_point..].join("-");

        let group_normalized = group_candidate.to_lowercase().replace(['-', '_'], "");
        if commands_map
            .keys()
            .any(|k| k.to_lowercase().replace(['-', '_'], "") == group_normalized)
        {
            return Some((group_candidate, cmd_candidate));
        }
    }

    None
}

/// Convert JSON arguments to CLI flag arguments.
fn build_cli_args(group: &str, cmd: &str, arguments: &Value) -> Vec<String> {
    let mut args = vec![group.to_owned(), cmd.to_owned()];

    if let Some(obj) = arguments.as_object() {
        for (key, value) in obj {
            let flag = format!("--{}", key.replace('_', "-"));

            match value {
                Value::Bool(true) => args.push(flag),
                Value::Bool(false) => {} // Omit false booleans
                Value::String(s) => {
                    args.push(flag);
                    args.push(s.clone());
                }
                Value::Number(n) => {
                    args.push(flag);
                    args.push(n.to_string());
                }
                Value::Array(arr) => {
                    for item in arr {
                        args.push(flag.clone());
                        args.push(item.as_str().unwrap_or(&item.to_string()).to_owned());
                    }
                }
                _ => {
                    args.push(flag);
                    args.push(value.to_string());
                }
            }
        }
    }

    args
}

/// Build the MCP tools array from the commands schema.
fn build_mcp_tools() -> Vec<Value> {
    let commands = crate::commands::context::agent_commands_schema();
    let Some(commands_map) = commands.as_object() else {
        return Vec::new();
    };

    let mut tools = Vec::new();
    for (group_name, group_val) in commands_map {
        let auth_scope = group_val
            .get("auth_scope")
            .and_then(Value::as_str)
            .unwrap_or("fabric");

        let Some(subcommands) = group_val.get("subcommands").and_then(Value::as_object) else {
            continue;
        };

        for (cmd_name, cmd_val) in subcommands {
            let tool = build_single_mcp_tool(group_name, cmd_name, cmd_val, auth_scope);
            tools.push(tool);
        }
    }
    tools
}

/// Build a single MCP tool definition.
fn build_single_mcp_tool(group: &str, cmd: &str, cmd_val: &Value, auth_scope: &str) -> Value {
    let tool_name = format!("fabio_{group}_{cmd}").replace('-', "_");
    let description = cmd_val
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");

    let (properties, required) = build_json_schema_params(cmd_val);

    let mut annotations = Map::new();
    annotations.insert("auth_scope".to_owned(), json!(auth_scope));
    if cmd_val.get("mutates").and_then(Value::as_bool) == Some(true) {
        annotations.insert("readOnlyHint".to_owned(), json!(false));
    } else {
        annotations.insert("readOnlyHint".to_owned(), json!(true));
    }
    if cmd_val.get("destructive").and_then(Value::as_bool) == Some(true) {
        annotations.insert("destructiveHint".to_owned(), json!(true));
    }

    let mut input_schema = Map::new();
    input_schema.insert("type".to_owned(), json!("object"));
    input_schema.insert("properties".to_owned(), Value::Object(properties));
    if !required.is_empty() {
        input_schema.insert("required".to_owned(), json!(required));
    }

    json!({
        "name": tool_name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": annotations,
    })
}

/// Convert fabio flag definitions to JSON Schema properties + required array.
fn build_json_schema_params(cmd_val: &Value) -> (Map<String, Value>, Vec<String>) {
    let mut properties = Map::new();
    let mut required = Vec::new();

    let Some(flags) = cmd_val.get("flags").and_then(Value::as_object) else {
        return (properties, required);
    };

    for (flag_name, flag_val) in flags {
        let param_name = flag_name.trim_start_matches('-').replace('-', "_");
        let mut prop = Map::new();

        if let Some(obj) = flag_val.as_object() {
            let fabio_type = obj.get("type").and_then(Value::as_str).unwrap_or("string");
            match fabio_type {
                "bool" => {
                    prop.insert("type".to_owned(), json!("boolean"));
                }
                "integer" | "u64" => {
                    prop.insert("type".to_owned(), json!("integer"));
                }
                "enum" => {
                    prop.insert("type".to_owned(), json!("string"));
                    if let Some(values) = obj.get("values") {
                        prop.insert("enum".to_owned(), values.clone());
                    }
                }
                _ => {
                    prop.insert("type".to_owned(), json!("string"));
                }
            }
            if let Some(desc) = obj.get("description") {
                prop.insert("description".to_owned(), desc.clone());
            }
            if let Some(default) = obj.get("default") {
                prop.insert("default".to_owned(), default.clone());
            }
            if obj.get("required").and_then(Value::as_bool) == Some(true) {
                required.push(param_name.clone());
            }
        } else {
            prop.insert("type".to_owned(), json!("string"));
        }

        properties.insert(param_name, Value::Object(prop));
    }

    (properties, required)
}
