use serde::Serialize;

use crate::cli::Cli;
use crate::output;

use super::AgentFormat;

/// Schema version for the agent-context output. Bump on breaking changes.
const SCHEMA_VERSION: &str = "2";

#[derive(Serialize)]
struct PortalOnlyOp {
    operation: &'static str,
    item_type: &'static str,
    reason: &'static str,
}

#[derive(Serialize)]
struct Flag {
    name: &'static str,
    #[serde(rename = "type")]
    kind: &'static str,
    description: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<&'static str>,
}

#[derive(Serialize)]
struct EnvVar {
    name: &'static str,
    description: &'static str,
    default: &'static str,
}

#[derive(Serialize)]
struct ErrorCodeInfo {
    code: &'static str,
    description: &'static str,
    exit_code: u8,
}

pub(super) fn execute(
    cli: &Cli,
    group_filter: Option<&str>,
    full: bool,
    format: AgentFormat,
    budget: Option<usize>,
) {
    // MCP/OpenAI formats always emit full tool definitions (filtered by --group if provided).
    if !matches!(format, AgentFormat::Native) {
        execute_standard_format(cli, group_filter, format);
        return;
    }

    // --group: return full details for a single group.
    if let Some(group) = group_filter {
        execute_group(cli, group);
        return;
    }

    // --budget: return progressively more detail until budget is exhausted.
    if let Some(token_budget) = budget {
        execute_budget(cli, token_budget);
        return;
    }

    // --full: return the complete 14K-line schema dump.
    if full {
        execute_full(cli);
        return;
    }

    // Default (no flags): compact index — group names + subcommand lists.
    execute_compact(cli, None);
}

/// Describe a single subcommand: all metadata + cross-referenced example.
pub(super) fn execute_describe(cli: &Cli, group: &str, command: &str) {
    let commands = commands_schema();
    let group_normalized = group.to_lowercase().replace(['-', '_'], "");

    // Find the group.
    let Some((group_key, group_obj)) = find_group(&commands, &group_normalized) else {
        let available: Vec<&str> = commands
            .as_object()
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let result = serde_json::json!({
            "error": format!("No command group found for '{group}'"),
            "available_groups": available,
            "hint": "Use 'fabio context agent' to see all groups"
        });
        output::render_object(cli, &result, "error");
        return;
    };

    // Find the subcommand within the group.
    let cmd_normalized = command.to_lowercase().replace('_', "-");
    let subcommands = group_obj
        .get("subcommands")
        .and_then(serde_json::Value::as_object);

    let Some(subcmds) = subcommands else {
        let result = serde_json::json!({
            "error": format!("Group '{group_key}' has no subcommands"),
            "hint": format!("Run 'fabio context agent --group {group_key}' for full details")
        });
        output::render_object(cli, &result, "error");
        return;
    };

    let Some((cmd_key, cmd_obj)) = subcmds
        .iter()
        .find(|(k, _)| k.to_lowercase().replace('_', "-") == cmd_normalized)
    else {
        let available: Vec<&str> = subcmds.keys().map(String::as_str).collect();
        let result = serde_json::json!({
            "error": format!("No subcommand '{command}' in group '{group_key}'"),
            "available_subcommands": available,
            "hint": format!("Run 'fabio context agent --group {group_key}' for full details")
        });
        output::render_object(cli, &result, "error");
        return;
    };

    // Build the describe output — merge command metadata with cross-referenced example.
    let mut result = serde_json::Map::new();
    result.insert(
        "command".to_owned(),
        serde_json::json!(format!("fabio {group_key} {cmd_key}")),
    );

    // Copy all fields from the command schema.
    if let Some(obj) = cmd_obj.as_object() {
        for (k, v) in obj {
            result.insert(k.clone(), v.clone());
        }
    }

    // Add group-level auth_scope if not already present.
    if !result.contains_key("auth_scope")
        && let Some(scope) = group_obj.get("auth_scope")
    {
        result.insert("auth_scope".to_owned(), scope.clone());
    }

    // Cross-reference: look for a matching output example.
    let example_key = format!("{group_key}/{cmd_key}");
    let example_normalized = example_key.to_lowercase().replace(['-', '_'], "");
    if let Some(content) =
        super::find_entry(super::examples::example_entries(), &example_normalized)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(content)
    {
        result.insert("output_example".to_owned(), val);
    }

    let obj = serde_json::Value::Object(result);
    output::render_object(cli, &obj, "command");
}

/// Search commands by keyword, returning ranked results.
pub(super) fn execute_find(cli: &Cli, query: &str) {
    let commands = commands_schema();
    let Some(commands_map) = commands.as_object() else {
        return;
    };

    // Tokenize query into lowercase words.
    let query_tokens: Vec<&str> = query.split_whitespace().collect();
    let query_lower = query.to_lowercase();

    let mut results: Vec<(f64, serde_json::Value)> = Vec::new();

    for (group_name, group_val) in commands_map {
        let Some(subcommands) = group_val
            .get("subcommands")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };

        for (cmd_name, cmd_val) in subcommands {
            let score =
                compute_relevance(group_name, cmd_name, cmd_val, &query_tokens, &query_lower);
            if score > 0.0 {
                results.push((score, serde_json::json!({
                    "command": format!("fabio {group_name} {cmd_name}"),
                    "score": (score * 100.0).round() / 100.0,
                    "description": cmd_val.get("description").and_then(serde_json::Value::as_str).unwrap_or(""),
                    "mutates": cmd_val.get("mutates").and_then(serde_json::Value::as_bool).unwrap_or(false),
                })));
            }
        }
    }

    // Search best-practices and workflows for matching topics.
    search_knowledge_entries(
        super::best_practices::entries(),
        "best-practice",
        "fabio context best-practices",
        &query_tokens,
        &query_lower,
        &mut results,
    );
    search_knowledge_entries(
        super::workflows::entries(),
        "workflow",
        "fabio context workflow",
        &query_tokens,
        &query_lower,
        &mut results,
    );

    // Sort by score descending, take top 10.
    results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let top_results: Vec<serde_json::Value> =
        results.into_iter().take(10).map(|(_, v)| v).collect();

    if top_results.is_empty() {
        let result = serde_json::json!({
            "results": [],
            "query": query,
            "hint": "Try broader keywords, or use 'fabio context agent' to browse all groups"
        });
        output::render_object(cli, &result, "query");
    } else {
        let result = serde_json::json!({
            "results": top_results,
            "query": query,
            "hint": "Use 'fabio context describe <GROUP> <CMD>' for full details on any result"
        });
        output::render_object(cli, &result, "query");
    }
}

/// Search knowledge base entries (best-practices, workflows) and add matches to results.
fn search_knowledge_entries(
    entries: &[(&str, &str)],
    entry_type: &str,
    command_prefix: &str,
    tokens: &[&str],
    query_lower: &str,
    results: &mut Vec<(f64, serde_json::Value)>,
) {
    for &(name, content) in entries {
        let content_lower = content.to_lowercase();
        let name_lower = name.to_lowercase();
        let mut score = 0.0;

        // Exact name match (highest).
        if name_lower.contains(query_lower) {
            score += 5.0;
        }

        // Token-based matching in name and content.
        for token in tokens {
            let token_lower = token.to_lowercase();
            if name_lower.contains(&token_lower) {
                score += 3.0;
            }
            if content_lower.contains(&token_lower) {
                score += 1.5;
            }
        }

        if score > 0.0 {
            // Extract summary from JSON content if available.
            let description = serde_json::from_str::<serde_json::Value>(content)
                .ok()
                .and_then(|v| {
                    v.get("summary")
                        .or_else(|| v.get("description"))
                        .or_else(|| v.get("title"))
                        .and_then(serde_json::Value::as_str)
                        .map(String::from)
                })
                .unwrap_or_default();

            results.push((
                score,
                serde_json::json!({
                    "command": format!("{command_prefix} {name}"),
                    "score": (score * 100.0).round() / 100.0,
                    "description": description,
                    "type": entry_type,
                }),
            ));
        }
    }
}

/// Compute relevance score for a command against the query tokens.
fn compute_relevance(
    group: &str,
    cmd: &str,
    cmd_val: &serde_json::Value,
    tokens: &[&str],
    query_lower: &str,
) -> f64 {
    let mut score = 0.0;

    // Build searchable text from command metadata.
    let description = cmd_val
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let notes = cmd_val
        .get("notes")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let hint = cmd_val
        .get("hint")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let cmd_full = format!("{group} {cmd}");
    let desc_lower = description.to_lowercase();
    let notes_lower = notes.to_lowercase();
    let hint_lower = hint.to_lowercase();
    let cmd_lower = cmd_full.to_lowercase();

    // Exact substring match in command name (highest weight).
    if cmd_lower.contains(query_lower) {
        score += 5.0;
    }

    // Token-based matching.
    for token in tokens {
        let token_lower = token.to_lowercase();

        // Match in command/group name.
        if cmd_lower.contains(&token_lower) {
            score += 3.0;
        }
        // Match in description.
        if desc_lower.contains(&token_lower) {
            score += 2.0;
        }
        // Match in notes/hint.
        if notes_lower.contains(&token_lower) || hint_lower.contains(&token_lower) {
            score += 1.0;
        }
        // Match in flag names.
        if let Some(flags) = cmd_val.get("flags").and_then(serde_json::Value::as_object) {
            for flag_name in flags.keys() {
                if flag_name.to_lowercase().contains(&token_lower) {
                    score += 1.5;
                    break; // Only count once per token
                }
            }
        }
    }

    score
}

// ─── Implementation details ──────────────────────────────────────────────────

/// Full schema dump (the original `fabio context agent` behavior).
fn execute_full(cli: &Cli) {
    // Build the JSON object field-by-field to avoid deep serde recursion on the stack.
    // On Windows the default stack is ~1 MB; serde_json::to_value() on a deeply nested
    // 146 KB JSON tree overflows it. By constructing the envelope manually and inserting
    // the pre-parsed serde_json::Value blobs directly we keep stack depth bounded.
    let mut value = serde_json::Map::new();
    value.insert(
        "schema_version".to_owned(),
        serde_json::json!(SCHEMA_VERSION),
    );
    value.insert("name".to_owned(), serde_json::json!("fabio"));
    value.insert(
        "version".to_owned(),
        serde_json::json!(env!("CARGO_PKG_VERSION")),
    );
    value.insert(
        "description".to_owned(),
        serde_json::json!("Agent-native CLI for managing Microsoft Fabric artifacts and data"),
    );
    value.insert(
        "global_flags".to_owned(),
        serde_json::to_value(global_flags()).expect("serialize global_flags"),
    );
    value.insert(
        "environment_variables".to_owned(),
        serde_json::to_value(environment_variables()).expect("serialize env_vars"),
    );
    // Large pre-parsed blobs inserted directly — no recursive to_value traversal.
    value.insert("commands".to_owned(), commands_schema());
    value.insert(
        "error_codes".to_owned(),
        serde_json::to_value(error_codes()).expect("serialize error_codes"),
    );
    value.insert("job_types".to_owned(), job_types());
    value.insert("definition_paths".to_owned(), definition_paths());
    value.insert(
        "portal_only_operations".to_owned(),
        serde_json::to_value(portal_only_operations()).expect("serialize portal_ops"),
    );
    value.insert("workflows".to_owned(), workflows());
    value.insert("output_conventions".to_owned(), output_conventions());

    let obj = serde_json::Value::Object(value);
    output::render_object(cli, &obj, "name");
}

/// Build the effective safety state for the current CLI invocation.
fn build_safety_state(cli: &Cli) -> serde_json::Value {
    serde_json::json!({
        "readonly": cli.readonly,
        "wrap_untrusted": cli.wrap_untrusted,
        "enable_commands": cli.enable_commands,
        "disable_commands": cli.disable_commands,
    })
}

/// Compact mode: group names + descriptions + subcommand name lists only.
fn execute_compact(cli: &Cli, group_filter: Option<&str>) {
    let commands = commands_schema();
    let Some(commands_map) = commands.as_object() else {
        output::render_object(cli, &commands, "commands");
        return;
    };

    let mut compact = serde_json::Map::new();

    for (group_name, group_val) in commands_map {
        // If --group was also specified, filter to that single group.
        if let Some(filter) = group_filter {
            let filter_normalized = filter.to_lowercase().replace(['-', '_'], "");
            if group_name.to_lowercase().replace(['-', '_'], "") != filter_normalized {
                continue;
            }
        }

        let description = group_val
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        let subcommand_names: Vec<&str> = group_val
            .get("subcommands")
            .and_then(serde_json::Value::as_object)
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default();

        compact.insert(
            group_name.clone(),
            serde_json::json!({
                "description": description,
                "subcommands": subcommand_names,
            }),
        );
    }

    if compact.is_empty()
        && let Some(filter) = group_filter
    {
        let available: Vec<&str> = commands_map.keys().map(String::as_str).collect();
        let result = serde_json::json!({
            "error": format!("No command group found for '{filter}'"),
            "available_groups": available,
            "hint": "Use 'fabio context agent' to see all groups"
        });
        output::render_object(cli, &result, "error");
        return;
    }

    let mut result = serde_json::Map::new();
    result.insert(
        "schema_version".to_owned(),
        serde_json::json!(SCHEMA_VERSION),
    );
    result.insert(
        "version".to_owned(),
        serde_json::json!(env!("CARGO_PKG_VERSION")),
    );
    result.insert("commands".to_owned(), serde_json::Value::Object(compact));

    // Include effective safety state so agents know what's restricted.
    result.insert("safety".to_owned(), build_safety_state(cli));

    result.insert(
        "hint".to_owned(),
        serde_json::json!(
            "Use 'fabio context agent --group <GROUP>' for full details on a specific group"
        ),
    );

    let obj = serde_json::Value::Object(result);
    output::render_object(cli, &obj, "schema_version");
}

/// Budget mode: progressively includes groups with full detail until the token budget
/// is exhausted. Starts with the most commonly-used groups and adds more until the
/// serialized output exceeds the budget. Uses a 4 chars/token approximation.
#[allow(clippy::too_many_lines)]
fn execute_budget(cli: &Cli, token_budget: usize) {
    const PRIORITY_GROUPS: &[&str] = &[
        "workspace",
        "lakehouse",
        "item",
        "notebook",
        "warehouse",
        "deploy",
        "semantic-model",
        "report",
        "kql-database",
        "data-pipeline",
        "auth",
        "data-agent",
        "eventstream",
        "eventhouse",
        "sql-database",
        "connection",
        "git",
        "capacity",
        "environment",
        "job-scheduler",
        "context",
        "catalog",
    ];

    let commands = commands_schema();
    let Some(commands_map) = commands.as_object() else {
        output::render_object(cli, &commands, "commands");
        return;
    };

    let char_budget = token_budget * 4; // ~4 chars per token approximation

    let mut result = serde_json::Map::new();
    result.insert(
        "schema_version".to_owned(),
        serde_json::json!(SCHEMA_VERSION),
    );
    result.insert(
        "version".to_owned(),
        serde_json::json!(env!("CARGO_PKG_VERSION")),
    );
    result.insert("budget_tokens".to_owned(), serde_json::json!(token_budget));

    // Phase 1: Add groups with full detail in priority order.
    let mut included_groups = serde_json::Map::new();
    let mut remaining_groups: Vec<&str> = Vec::new();

    // Build ordered list: priority groups first, then the rest alphabetically.
    let mut ordered_groups: Vec<&str> = Vec::new();
    for &g in PRIORITY_GROUPS {
        if commands_map.contains_key(g) {
            ordered_groups.push(g);
        }
    }
    for key in commands_map.keys() {
        if !PRIORITY_GROUPS.contains(&key.as_str()) {
            ordered_groups.push(key.as_str());
        }
    }

    for group_name in &ordered_groups {
        let Some(group_val) = commands_map.get(*group_name) else {
            continue;
        };

        // Try adding this group with full detail.
        included_groups.insert((*group_name).to_owned(), group_val.clone());

        // Check size.
        result.insert(
            "commands".to_owned(),
            serde_json::Value::Object(included_groups.clone()),
        );
        let serialized =
            serde_json::to_string(&serde_json::Value::Object(result.clone())).unwrap_or_default();

        if serialized.len() > char_budget {
            // Over budget — remove this group and stop adding full detail.
            included_groups.remove(*group_name);
            remaining_groups.push(group_name);
        }
    }

    // Phase 2: Add remaining groups as compact entries (name + description only).
    if !remaining_groups.is_empty() {
        let mut compact_remainder = serde_json::Map::new();
        for group_name in &remaining_groups {
            if let Some(group_val) = commands_map.get(*group_name) {
                let desc = group_val
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let subcmd_names: Vec<&str> = group_val
                    .get("subcommands")
                    .and_then(serde_json::Value::as_object)
                    .map(|m| m.keys().map(String::as_str).collect())
                    .unwrap_or_default();
                compact_remainder.insert(
                    (*group_name).to_owned(),
                    serde_json::json!({"description": desc, "subcommands": subcmd_names}),
                );
            }
        }
        result.insert(
            "commands_compact".to_owned(),
            serde_json::Value::Object(compact_remainder),
        );
    }

    result.insert(
        "commands".to_owned(),
        serde_json::Value::Object(included_groups),
    );
    result.insert(
        "hint".to_owned(),
        serde_json::json!(
            "Use 'fabio context agent --group <GROUP>' for full details on groups shown in commands_compact"
        ),
    );

    let obj = serde_json::Value::Object(result);
    output::render_object(cli, &obj, "schema_version");
}

/// Single group mode: returns full command details for one group only.
fn execute_group(cli: &Cli, group: &str) {
    let commands = commands_schema();
    let group_normalized = group.to_lowercase().replace(['-', '_'], "");

    let Some((group_key, group_val)) = find_group(&commands, &group_normalized) else {
        let available: Vec<&str> = commands
            .as_object()
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let result = serde_json::json!({
            "error": format!("No command group found for '{group}'"),
            "available_groups": available,
            "hint": "Use 'fabio context agent' to see all groups"
        });
        output::render_object(cli, &result, "error");
        return;
    };

    let mut result = serde_json::Map::new();
    result.insert(
        "schema_version".to_owned(),
        serde_json::json!(SCHEMA_VERSION),
    );
    result.insert(
        "version".to_owned(),
        serde_json::json!(env!("CARGO_PKG_VERSION")),
    );
    result.insert(
        "global_flags".to_owned(),
        serde_json::to_value(global_flags()).expect("serialize global_flags"),
    );
    result.insert(
        "error_codes".to_owned(),
        serde_json::to_value(error_codes()).expect("serialize error_codes"),
    );
    result.insert("group".to_owned(), serde_json::json!(group_key));
    result.insert("group_details".to_owned(), group_val.clone());

    let obj = serde_json::Value::Object(result);
    output::render_object(cli, &obj, "group");
}

/// Find a group in the commands schema by normalized key.
fn find_group<'a>(
    commands: &'a serde_json::Value,
    normalized_key: &str,
) -> Option<(&'a str, &'a serde_json::Value)> {
    commands.as_object().and_then(|m| {
        m.iter()
            .find(|(k, _)| k.to_lowercase().replace(['-', '_'], "") == *normalized_key)
            .map(|(k, v)| (k.as_str(), v))
    })
}

// ─── Standard format emission (MCP / OpenAI) ────────────────────────────────

/// Emit the schema in MCP or `OpenAI` tool-definition format.
fn execute_standard_format(cli: &Cli, group_filter: Option<&str>, format: AgentFormat) {
    let commands = commands_schema();
    let Some(commands_map) = commands.as_object() else {
        output::render_object(cli, &commands, "commands");
        return;
    };

    let mut tools: Vec<serde_json::Value> = Vec::new();

    for (group_name, group_val) in commands_map {
        // Apply --group filter if provided.
        if let Some(filter) = group_filter {
            let filter_normalized = filter.to_lowercase().replace(['-', '_'], "");
            if group_name.to_lowercase().replace(['-', '_'], "") != filter_normalized {
                continue;
            }
        }

        let auth_scope = group_val
            .get("auth_scope")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("fabric");

        let Some(subcommands) = group_val
            .get("subcommands")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };

        for (cmd_name, cmd_val) in subcommands {
            let tool = match format {
                AgentFormat::Mcp => build_mcp_tool(group_name, cmd_name, cmd_val, auth_scope),
                AgentFormat::Openai => build_openai_tool(group_name, cmd_name, cmd_val, auth_scope),
                AgentFormat::Native => unreachable!(),
            };
            tools.push(tool);
        }
    }

    if tools.is_empty()
        && let Some(filter) = group_filter
    {
        let available: Vec<&str> = commands_map.keys().map(String::as_str).collect();
        let result = serde_json::json!({
            "error": format!("No command group found for '{filter}'"),
            "available_groups": available,
            "hint": "Use 'fabio context agent' to see all groups"
        });
        output::render_object(cli, &result, "error");
        return;
    }

    let key = match format {
        AgentFormat::Mcp => "tools",
        AgentFormat::Openai => "functions",
        AgentFormat::Native => unreachable!(),
    };

    let mut result = serde_json::Map::new();
    result.insert(
        "schema_version".to_owned(),
        serde_json::json!(SCHEMA_VERSION),
    );
    result.insert(
        "version".to_owned(),
        serde_json::json!(env!("CARGO_PKG_VERSION")),
    );
    result.insert(key.to_owned(), serde_json::Value::Array(tools));
    let obj = serde_json::Value::Object(result);
    output::render_object(cli, &obj, key);
}

/// Build a single MCP tool definition from a fabio subcommand.
fn build_mcp_tool(
    group: &str,
    cmd: &str,
    cmd_val: &serde_json::Value,
    auth_scope: &str,
) -> serde_json::Value {
    let tool_name = format!("fabio_{group}_{cmd}").replace('-', "_");
    let description = cmd_val
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let (properties, required) = build_json_schema_params(cmd_val);

    let mut annotations = serde_json::Map::new();
    annotations.insert("auth_scope".to_owned(), serde_json::json!(auth_scope));
    if cmd_val.get("mutates").and_then(serde_json::Value::as_bool) == Some(true) {
        annotations.insert("readOnlyHint".to_owned(), serde_json::json!(false));
    } else {
        annotations.insert("readOnlyHint".to_owned(), serde_json::json!(true));
    }
    if cmd_val
        .get("destructive")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        annotations.insert("destructiveHint".to_owned(), serde_json::json!(true));
    }
    if cmd_val.get("async").and_then(serde_json::Value::as_bool) == Some(true) {
        annotations.insert("async".to_owned(), serde_json::json!(true));
    }

    // Build invocation template.
    let invocation = format!("fabio {group} {cmd}");

    let mut input_schema = serde_json::Map::new();
    input_schema.insert("type".to_owned(), serde_json::json!("object"));
    input_schema.insert(
        "properties".to_owned(),
        serde_json::Value::Object(properties),
    );
    if !required.is_empty() {
        input_schema.insert("required".to_owned(), serde_json::json!(required));
    }

    serde_json::json!({
        "name": tool_name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": annotations,
        "invocation": invocation,
    })
}

/// Build a single `OpenAI` function-calling definition from a fabio subcommand.
fn build_openai_tool(
    group: &str,
    cmd: &str,
    cmd_val: &serde_json::Value,
    auth_scope: &str,
) -> serde_json::Value {
    let tool_name = format!("fabio_{group}_{cmd}").replace('-', "_");
    let description = cmd_val
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    // Add auth_scope and mutation hints to the description for OpenAI (no annotations field).
    let mutates = cmd_val.get("mutates").and_then(serde_json::Value::as_bool) == Some(true);
    let full_description = if mutates {
        format!("{description} [mutates, scope={auth_scope}]")
    } else {
        format!("{description} [read-only, scope={auth_scope}]")
    };

    let (properties, required) = build_json_schema_params(cmd_val);

    let mut parameters = serde_json::Map::new();
    parameters.insert("type".to_owned(), serde_json::json!("object"));
    parameters.insert(
        "properties".to_owned(),
        serde_json::Value::Object(properties),
    );
    if !required.is_empty() {
        parameters.insert("required".to_owned(), serde_json::json!(required));
    }
    parameters.insert("additionalProperties".to_owned(), serde_json::json!(false));

    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool_name,
            "description": full_description,
            "parameters": parameters,
        }
    })
}

/// Convert fabio flag definitions to JSON Schema properties + required array.
fn build_json_schema_params(
    cmd_val: &serde_json::Value,
) -> (serde_json::Map<String, serde_json::Value>, Vec<String>) {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    let Some(flags) = cmd_val.get("flags").and_then(serde_json::Value::as_object) else {
        return (properties, required);
    };

    for (flag_name, flag_val) in flags {
        // Strip leading -- and convert hyphens to underscores for JSON Schema.
        let param_name = flag_name.trim_start_matches('-').replace('-', "_");

        let mut prop = serde_json::Map::new();

        // Determine if flag_val is a structured object or shorthand string.
        if let Some(obj) = flag_val.as_object() {
            // Map fabio types to JSON Schema types.
            let fabio_type = obj
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("string");
            match fabio_type {
                "bool" => {
                    prop.insert("type".to_owned(), serde_json::json!("boolean"));
                }
                "integer" | "u64" => {
                    prop.insert("type".to_owned(), serde_json::json!("integer"));
                }
                "enum" => {
                    prop.insert("type".to_owned(), serde_json::json!("string"));
                    if let Some(values) = obj.get("values") {
                        prop.insert("enum".to_owned(), values.clone());
                    }
                }
                _ => {
                    prop.insert("type".to_owned(), serde_json::json!("string"));
                }
            }

            // Add description if present.
            if let Some(desc) = obj.get("description") {
                prop.insert("description".to_owned(), desc.clone());
            }

            // Add default if present.
            if let Some(default) = obj.get("default") {
                prop.insert("default".to_owned(), default.clone());
            }

            // Track required flags.
            if obj.get("required").and_then(serde_json::Value::as_bool) == Some(true) {
                required.push(param_name.clone());
            }
        } else {
            // Shorthand: value is just a type string.
            prop.insert("type".to_owned(), serde_json::json!("string"));
        }

        properties.insert(param_name, serde_json::Value::Object(prop));
    }

    (properties, required)
}

fn global_flags() -> Vec<Flag> {
    vec![
        Flag {
            name: "--output",
            kind: "enum",
            description: "Output format",
            default: Some("json"),
        },
        Flag {
            name: "--json",
            kind: "bool",
            description: "Shorthand for --output json",
            default: Some("false"),
        },
        Flag {
            name: "--query",
            kind: "string",
            description: "JMESPath query expression (e.g., 'id', '[*].name', '[?size>`10`].id'). See https://jmespath.org/",
            default: None,
        },
        Flag {
            name: "--quiet",
            kind: "bool",
            description: "Suppress all stdout output",
            default: Some("false"),
        },
        Flag {
            name: "--force",
            kind: "bool",
            description: "Skip confirmation prompts for destructive operations",
            default: Some("false"),
        },
        Flag {
            name: "--dry-run",
            kind: "bool",
            description: "Preview what would happen without making changes",
            default: Some("false"),
        },
        Flag {
            name: "--limit",
            kind: "integer",
            description: "Maximum number of items to return in list commands",
            default: None,
        },
        Flag {
            name: "--all",
            kind: "bool",
            description: "Fetch all pages (auto-paginate). Without this, only the first page is returned with a continuationToken for manual pagination.",
            default: Some("false"),
        },
        Flag {
            name: "--continuation-token",
            kind: "string",
            description: "Resume pagination from a specific continuation token (returned by a previous list call)",
            default: None,
        },
        Flag {
            name: "--profile",
            kind: "string",
            description: "Use a named profile for default settings",
            default: None,
        },
        Flag {
            name: "--lro-timeout",
            kind: "integer",
            description: "Maximum seconds to wait for long-running operations (default: 120)",
            default: Some("120"),
        },
        Flag {
            name: "--verbose",
            kind: "bool",
            description: "Enable HTTP/LRO/auth diagnostic tracing on stderr. For debugging only — do not use in normal operation. Suppressed by --quiet.",
            default: Some("false"),
        },
    ]
}

fn environment_variables() -> Vec<EnvVar> {
    vec![
        EnvVar {
            name: "FABIO_FABRIC_API_ENDPOINT",
            description: "Override the Fabric REST API base URL (for sovereign clouds or private link)",
            default: "https://api.fabric.microsoft.com/v1",
        },
        EnvVar {
            name: "FABIO_ONELAKE_DFS_ENDPOINT",
            description: "Override the OneLake DFS base URL",
            default: "https://onelake.dfs.fabric.microsoft.com",
        },
        EnvVar {
            name: "FABIO_ONELAKE_BLOB_ENDPOINT",
            description: "Override the OneLake Blob base URL",
            default: "https://onelake.blob.fabric.microsoft.com",
        },
        EnvVar {
            name: "FABIO_ARM_ENDPOINT",
            description: "Override the Azure Resource Manager base URL",
            default: "https://management.azure.com",
        },
        EnvVar {
            name: "FABIO_FABRIC_SCOPE",
            description: "Override the Fabric API token scope",
            default: "https://api.fabric.microsoft.com/.default",
        },
        EnvVar {
            name: "FABIO_STORAGE_SCOPE",
            description: "Override the Azure Storage token scope",
            default: "https://storage.azure.com/.default",
        },
        EnvVar {
            name: "FABIO_SQL_SCOPE",
            description: "Override the SQL/TDS token scope",
            default: "https://database.windows.net/.default",
        },
        EnvVar {
            name: "FABIO_ARM_SCOPE",
            description: "Override the Azure Resource Manager token scope",
            default: "https://management.azure.com/.default",
        },
        EnvVar {
            name: "FABIO_POWERBI_ENDPOINT",
            description: "Override the Power BI REST API base URL (used by --api powerbi)",
            default: "https://api.powerbi.com/v1.0/myorg",
        },
    ]
}

fn error_codes() -> Vec<ErrorCodeInfo> {
    vec![
        ErrorCodeInfo {
            code: "AUTH_REQUIRED",
            description: "No valid credentials found. Run 'fabio auth login'.",
            exit_code: 1,
        },
        ErrorCodeInfo {
            code: "FORBIDDEN",
            description: "Insufficient permissions. Check workspace role (Admin/Member/Contributor/Viewer) and API scopes.",
            exit_code: 1,
        },
        ErrorCodeInfo {
            code: "NOT_FOUND",
            description: "Requested resource does not exist.",
            exit_code: 1,
        },
        ErrorCodeInfo {
            code: "CONFLICT",
            description: "Resource already exists or state conflict.",
            exit_code: 1,
        },
        ErrorCodeInfo {
            code: "RATE_LIMITED",
            description: "Too many requests. Retry after backoff.",
            exit_code: 1,
        },
        ErrorCodeInfo {
            code: "CAPACITY_INACTIVE",
            description: "Fabric capacity is paused or inactive.",
            exit_code: 1,
        },
        ErrorCodeInfo {
            code: "INVALID_INPUT",
            description: "Invalid argument value or missing required field.",
            exit_code: 1,
        },
        ErrorCodeInfo {
            code: "API_ERROR",
            description: "Upstream Fabric API returned an error.",
            exit_code: 1,
        },
        ErrorCodeInfo {
            code: "TIMEOUT",
            description: "Operation exceeded maximum wait time.",
            exit_code: 1,
        },
        ErrorCodeInfo {
            code: "NETWORK_ERROR",
            description: "Network connectivity issue.",
            exit_code: 1,
        },
    ]
}

fn job_types() -> serde_json::Value {
    serde_json::from_str(include_str!("data/agent/job_types.json"))
        .expect("job_types.json must contain valid JSON")
}

fn definition_paths() -> serde_json::Value {
    serde_json::from_str(include_str!("data/agent/definition_paths.json"))
        .expect("definition_paths.json must contain valid JSON")
}

fn portal_only_operations() -> Vec<PortalOnlyOp> {
    vec![
        PortalOnlyOp {
            operation: "initialize",
            item_type: "GraphModel",
            reason: "New 4-part definition format (graphType/graphDefinition/dataSources/styling) is documented for CI/CD but data loading does not complete via REST API alone (Jun 2026). Refresh triggers without VersionConfig error but stays NotStarted. Portal initialization still needed for the graph to become queryable.",
        },
        PortalOnlyOp {
            operation: "configure-kql-source",
            item_type: "Reflex",
            reason: "kqlSource-v1 is officially documented (Mar 2026) but updateDefinition still returns 'Invalid definition' (previously 'importArtifactRequest field is required'). Configure KQL sources through the portal first.",
        },
        PortalOnlyOp {
            operation: "configure-credentials",
            item_type: "SemanticModel (DirectQuery)",
            reason: "OAuth2 credentials cannot be created via REST API (only 1-hour raw access tokens via Gateways API). Fabric Connections API excludes OAuth2Credentials from create/update schemas. Use ServicePrincipal, WorkspaceIdentity, or Direct Lake instead.",
        },
    ]
}

fn commands_schema() -> serde_json::Value {
    serde_json::from_str(include_str!("data/agent/commands.json"))
        .expect("commands.json must contain valid JSON")
}

fn workflows() -> serde_json::Value {
    serde_json::from_str(include_str!("data/agent/workflows.json"))
        .expect("workflows.json must contain valid JSON")
}

fn output_conventions() -> serde_json::Value {
    serde_json::from_str(include_str!("data/agent/output_conventions.json"))
        .expect("output_conventions.json must contain valid JSON")
}

// ─── Schema drift detection & auto-generation ────────────────────────────────

/// Extract the actual CLI surface from clap's `Command` metadata.
/// Returns a map of `group_name` -> vec of subcommand names.
#[cfg(test)]
fn extract_clap_surface() -> std::collections::BTreeMap<String, Vec<String>> {
    use clap::CommandFactory;

    // Commands that are internal utilities, not agent-facing.
    const EXCLUDED: &[&str] = &["completions", "help"];

    let cmd = crate::cli::Cli::command();
    let mut surface = std::collections::BTreeMap::new();

    for group in cmd.get_subcommands() {
        let group_name = group.get_name().to_owned();

        if EXCLUDED.contains(&group_name.as_str()) {
            continue;
        }

        let subcommands: Vec<String> = group
            .get_subcommands()
            .filter(|sc| sc.get_name() != "help")
            .map(|sc| sc.get_name().to_owned())
            .collect();

        surface.insert(group_name, subcommands);
    }

    surface
}

/// Generate a complete `commands.json` from clap metadata, merging with existing
/// annotations (mutates, returns, async, destructive, `auth_scope`) from the
/// current `commands.json`.
#[cfg(test)]
fn generate_schema_from_clap() -> serde_json::Value {
    use clap::CommandFactory;
    let cmd = crate::cli::Cli::command();
    let existing = commands_schema();
    let existing_map = existing.as_object();

    let mut result = serde_json::Map::new();

    for group in cmd.get_subcommands() {
        let group_name = group.get_name().to_owned();
        if group_name == "help" {
            continue;
        }

        let existing_group = existing_map.and_then(|m| m.get(&group_name));
        let auth_scope = existing_group
            .and_then(|g| g.get("auth_scope"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("fabric");

        let group_desc = group
            .get_about()
            .map(ToString::to_string)
            .or_else(|| {
                existing_group
                    .and_then(|g| g.get("description"))
                    .and_then(serde_json::Value::as_str)
                    .map(String::from)
            })
            .unwrap_or_default();

        let subcommands = generate_subcommands(group, existing_group);

        let mut group_obj = serde_json::Map::new();
        group_obj.insert("auth_scope".to_owned(), serde_json::json!(auth_scope));
        group_obj.insert("description".to_owned(), serde_json::json!(group_desc));
        group_obj.insert(
            "subcommands".to_owned(),
            serde_json::Value::Object(subcommands),
        );

        // Carry over group-level annotations preserved in the existing schema.
        // This is critical for leaf commands (e.g. `upgrade`) that have no
        // subcommands: their `examples`, `mutates`, `returns`, etc. live at the
        // group level and would otherwise be silently dropped on every
        // regeneration.
        if let Some(existing) = existing_group {
            for key in [
                "examples",
                "mutates",
                "returns",
                "async",
                "destructive",
                "flags",
                "hint",
                "notes",
                "output_fields",
            ] {
                if let Some(val) = existing.get(key) {
                    group_obj.insert(key.to_owned(), val.clone());
                }
            }
        }

        result.insert(group_name, serde_json::Value::Object(group_obj));
    }

    serde_json::Value::Object(result)
}

/// Generate subcommand entries for a single group.
#[cfg(test)]
#[allow(dead_code)]
fn generate_subcommands(
    group: &clap::Command,
    existing_group: Option<&serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut subcommands = serde_json::Map::new();

    for sc in group.get_subcommands() {
        let sc_name = sc.get_name().to_owned();
        if sc_name == "help" {
            continue;
        }

        let existing_sc = existing_group
            .and_then(|g| g.get("subcommands"))
            .and_then(serde_json::Value::as_object)
            .and_then(|s| s.get(&sc_name));

        let sc_desc = sc
            .get_about()
            .map(ToString::to_string)
            .or_else(|| {
                existing_sc
                    .and_then(|s| s.get("description"))
                    .and_then(serde_json::Value::as_str)
                    .map(String::from)
            })
            .unwrap_or_default();

        let flags = generate_flags(sc, existing_sc);

        let mut sc_obj = serde_json::Map::new();
        sc_obj.insert("description".to_owned(), serde_json::json!(sc_desc));
        if !flags.is_empty() {
            sc_obj.insert("flags".to_owned(), serde_json::Value::Object(flags));
        }

        // Preserve annotations from existing schema (takes priority over inference).
        if let Some(existing) = existing_sc {
            for key in [
                "mutates",
                "returns",
                "async",
                "destructive",
                "examples",
                "hint",
                "notes",
                "output_fields",
                "aliases",
            ] {
                if let Some(val) = existing.get(key) {
                    sc_obj.insert(key.to_owned(), val.clone());
                }
            }
        }

        // Auto-infer missing semantic annotations from command name conventions.
        if !sc_obj.contains_key("mutates") {
            sc_obj.insert(
                "mutates".to_owned(),
                serde_json::json!(infer_mutates(&sc_name)),
            );
        }
        if !sc_obj.contains_key("returns") {
            sc_obj.insert(
                "returns".to_owned(),
                serde_json::json!(infer_returns(&sc_name)),
            );
        }
        if !sc_obj.contains_key("destructive") && infer_destructive(&sc_name) {
            sc_obj.insert("destructive".to_owned(), serde_json::json!(true));
        }

        subcommands.insert(sc_name, serde_json::Value::Object(sc_obj));
    }

    subcommands
}

/// Generate flag entries for a single subcommand from clap arg metadata.
#[cfg(test)]
#[allow(dead_code)]
fn generate_flags(
    sc: &clap::Command,
    existing_sc: Option<&serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut flags = serde_json::Map::new();

    for arg in sc.get_arguments() {
        let Some(long) = arg.get_long() else {
            continue;
        };
        if arg.is_global_set() {
            continue;
        }

        let flag_name = format!("--{long}");
        let mut flag_obj = serde_json::Map::new();

        // Determine type: check action first (bool/count), then possible values (enum).
        let action = arg.get_action();
        match action {
            clap::ArgAction::SetTrue | clap::ArgAction::SetFalse => {
                flag_obj.insert("type".to_owned(), serde_json::json!("bool"));
            }
            clap::ArgAction::Count => {
                flag_obj.insert("type".to_owned(), serde_json::json!("integer"));
            }
            _ => {
                let possible_values: Vec<String> = arg
                    .get_possible_values()
                    .iter()
                    .filter_map(|pv| pv.get_name_and_aliases().next().map(String::from))
                    .collect();

                if possible_values.is_empty() {
                    let type_str = existing_sc
                        .and_then(|s| s.get("flags"))
                        .and_then(serde_json::Value::as_object)
                        .and_then(|f| f.get(&flag_name))
                        .and_then(|fv| fv.get("type"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("string");
                    flag_obj.insert("type".to_owned(), serde_json::json!(type_str));
                } else {
                    flag_obj.insert("type".to_owned(), serde_json::json!("enum"));
                    flag_obj.insert("values".to_owned(), serde_json::json!(possible_values));
                }
            }
        }

        if arg.is_required_set() {
            flag_obj.insert("required".to_owned(), serde_json::json!(true));
        }

        if let Some(help) = arg.get_help() {
            let help_str = help.to_string();
            if !help_str.is_empty() {
                flag_obj.insert("description".to_owned(), serde_json::json!(help_str));
            }
        }

        // Preserve existing default annotations.
        if let Some(default) = existing_sc
            .and_then(|s| s.get("flags"))
            .and_then(serde_json::Value::as_object)
            .and_then(|f| f.get(&flag_name))
            .and_then(serde_json::Value::as_object)
            .and_then(|ef| ef.get("default"))
        {
            flag_obj.insert("default".to_owned(), default.clone());
        }

        flags.insert(flag_name, serde_json::Value::Object(flag_obj));
    }

    flags
}

/// Infer whether a command mutates state from its name.
/// Read-only patterns: list, show, get-*, query, describe, status, check-*, search, diagnostics.
#[cfg(test)]
#[allow(dead_code)]
fn infer_mutates(cmd_name: &str) -> bool {
    const READ_ONLY_PREFIXES: &[&str] = &[
        "list",
        "show",
        "get-",
        "query",
        "describe",
        "status",
        "check-",
        "search",
        "diagnostics",
        "deeplink",
        "connection-string",
        "exists",
        "sample",
        "show-queryplan",
        "discover-",
        "iceberg-",
        "table-schema",
    ];

    for prefix in READ_ONLY_PREFIXES {
        if cmd_name.starts_with(prefix) || cmd_name == *prefix {
            return false;
        }
    }
    true
}

/// Infer the return type from a command name.
#[cfg(test)]
#[allow(dead_code)]
fn infer_returns(cmd_name: &str) -> &'static str {
    if cmd_name.starts_with("list") {
        return "list";
    }
    if cmd_name.starts_with("show")
        || cmd_name.starts_with("get-")
        || cmd_name.starts_with("create")
        || cmd_name.starts_with("add-")
        || cmd_name == "query"
        || cmd_name == "search"
        || cmd_name == "status"
        || cmd_name.starts_with("describe")
        || cmd_name.starts_with("discover-")
        || cmd_name.starts_with("iceberg-")
        || cmd_name == "table-schema"
        || cmd_name == "connection-string"
        || cmd_name == "exists"
        || cmd_name == "sample"
        || cmd_name == "diagnostics"
        || cmd_name == "deeplink"
        || cmd_name == "plan"
        || cmd_name == "export"
        || cmd_name == "import"
        || cmd_name == "url"
        || cmd_name == "inspect"
    {
        return "object";
    }
    if cmd_name.starts_with("delete")
        || cmd_name.starts_with("remove")
        || cmd_name.starts_with("update")
        || cmd_name.starts_with("set-")
        || cmd_name.starts_with("assign")
        || cmd_name.starts_with("unassign")
        || cmd_name == "run"
        || cmd_name == "deploy"
        || cmd_name == "publish"
        || cmd_name == "reset"
        || cmd_name == "stop"
        || cmd_name == "start"
        || cmd_name == "cancel"
        || cmd_name == "restart"
        || cmd_name == "shutdown"
        || cmd_name.starts_with("clear")
        || cmd_name == "commit"
        || cmd_name == "pull"
        || cmd_name == "connect"
        || cmd_name == "disconnect"
        || cmd_name == "initialize"
    {
        return "void";
    }
    "object"
}

/// Infer whether a command is destructive (deletes data permanently).
#[cfg(test)]
#[allow(dead_code)]
fn infer_destructive(cmd_name: &str) -> bool {
    cmd_name.starts_with("delete") || cmd_name.starts_with("remove") || cmd_name == "vacuum-table"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run a closure on a thread with 8 MB stack (clap `Command` tree is deeply nested).
    fn with_large_stack<F: FnOnce() + Send + 'static>(f: F) {
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(f)
            .expect("spawn thread")
            .join()
            .expect("thread panicked");
    }

    /// Drift detection: ensures `commands.json` covers all CLI groups.
    #[test]
    fn agent_schema_covers_all_groups() {
        with_large_stack(|| {
            let clap_surface = extract_clap_surface();
            let schema = commands_schema();
            let schema_map = schema.as_object().expect("commands.json should be object");

            let mut missing_groups = Vec::new();
            for group_name in clap_surface.keys() {
                let normalized = group_name.to_lowercase().replace('_', "-");
                if !schema_map
                    .keys()
                    .any(|k| k == &normalized || k == group_name)
                {
                    missing_groups.push(group_name.clone());
                }
            }

            assert!(
                missing_groups.is_empty(),
                "commands.json is missing these command groups: {missing_groups:?}\n\
                 Run `cargo test generate_agent_schema -- --ignored` to regenerate."
            );
        });
    }

    /// Regenerate `commands.json` from clap metadata.
    ///
    /// Run with:
    /// ```bash
    /// cargo test generate_agent_schema -- --ignored
    /// ```
    ///
    /// Merges structural data (groups, subcommands, flags) from clap with
    /// existing semantic annotations (`mutates`, `returns`, `async`,
    /// `destructive`, `auth_scope`, `examples`) preserved from the current
    /// committed `commands.json`.  After regenerating, add any missing semantic
    /// annotations to the new entries and run the drift detection tests to
    /// verify.
    #[test]
    #[ignore = "writes commands.json to disk — run manually after adding/changing commands"]
    fn generate_agent_schema() {
        with_large_stack(|| {
            let schema = generate_schema_from_clap();
            let json =
                serde_json::to_string_pretty(&schema).expect("serialize commands schema to JSON");
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/commands/context/data/agent/commands.json");
            std::fs::write(&path, format!("{json}\n")).expect("write commands.json");
            println!("Wrote {}", path.display());
        });
    }

    /// Drift detection: ensures `commands.json` covers all subcommands within each group.
    #[test]
    fn agent_schema_covers_all_subcommands() {
        with_large_stack(|| {
            let clap_surface = extract_clap_surface();
            let schema = commands_schema();
            let schema_map = schema.as_object().expect("commands.json should be object");

            let mut missing = Vec::new();

            for (group_name, subcommands) in &clap_surface {
                let normalized_group = group_name.to_lowercase().replace('_', "-");
                let Some(group_val) = schema_map
                    .get(&normalized_group)
                    .or_else(|| schema_map.get(group_name))
                else {
                    continue;
                };

                let Some(schema_subcmds) = group_val
                    .get("subcommands")
                    .and_then(serde_json::Value::as_object)
                else {
                    if !subcommands.is_empty() {
                        missing.push(format!(
                            "{normalized_group}: all ({} subcommands)",
                            subcommands.len()
                        ));
                    }
                    continue;
                };

                for sc_name in subcommands {
                    if !schema_subcmds.contains_key(sc_name) {
                        missing.push(format!("{normalized_group} {sc_name}"));
                    }
                }
            }

            assert!(
                missing.is_empty(),
                "commands.json is missing these subcommands:\n  {}\n\
                 Run `cargo test generate_agent_schema -- --ignored` to regenerate.",
                missing.join("\n  ")
            );
        });
    }
}
