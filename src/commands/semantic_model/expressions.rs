//! `semantic-model` named-expression / Power Query parameter authoring —
//! `add-expression`, `update-expression`, `delete-expression`, `list-expressions`.
//!
//! Named expressions (shared M queries and Power Query parameters) live in
//! `definition/expressions.tmdl` as top-level `expression <name> = <M>` blocks.
//! Like `relationships.tmdl`, the file is NOT `ref`-ed in `model.tmdl` (it is
//! auto-discovered) and is created on the first add / removed when the last
//! expression is deleted. A Power Query PARAMETER is a named expression whose M
//! carries an `… meta [IsParameterQuery=true, Type="…", …]` clause. fabio edits
//! these via the shared definition read-modify-write (no XMLA/TOM).

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::analyze::tab_indent;
use super::tmdl::{
    decl_name, fetch_parts, part_content, push_parts, quote_tmdl_name, remove_part, upsert_part,
};

const EXPRESSIONS_PATH: &str = "definition/expressions.tmdl";

/// Build the M for a Power Query parameter (`<value> meta [IsParameterQuery…]`).
fn build_parameter_m(value: &str, ptype: &str) -> String {
    format!(
        "\"{}\" meta [IsParameterQuery=true, Type=\"{}\", IsParameterQueryRequired=true]",
        value.replace('"', "\"\""),
        ptype
    )
}

fn normalize_param_type(t: &str) -> &'static str {
    match t.to_ascii_lowercase().as_str() {
        "number" | "decimal" | "double" => "Number",
        "logical" | "bool" | "boolean" => "Logical",
        "datetime" | "date" => "DateTime",
        // "text"/"string" and anything unrecognized default to Text.
        _ => "Text",
    }
}

// ── add-expression ────────────────────────────────────────────────────────────

pub(super) async fn add_expression(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    m: &str,
) -> Result<()> {
    let op = "semantic-model add-expression";
    let parts = fetch_parts(client, workspace, id, op).await?;

    if expression_exists(&parts, name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("A named expression '{name}' already exists."),
            "Use `update-expression`, or pick a different name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_expression_bim(bim, name, m)?;
        super::tmdl::replace_part(&parts, "model.bim", &new_bim)
    } else {
        let existing = part_content(&parts, EXPRESSIONS_PATH).unwrap_or("");
        let block = render_expression_block(name, m);
        let updated = append_block(existing, &block);
        upsert_part(&parts, EXPRESSIONS_PATH, &updated)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "expression": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "expression_added", "id": id, "expression": name }),
        "status",
    );
    Ok(())
}

/// The public entry used by the dispatch: resolve the raw-M vs parameter choice.
pub(super) fn resolve_m(
    m: Option<&str>,
    parameter_value: Option<&str>,
    parameter_type: Option<&str>,
) -> Result<String> {
    match (m, parameter_value) {
        (Some(expr), None) => Ok(expr.to_string()),
        (None, Some(val)) => Ok(build_parameter_m(val, normalize_param_type(parameter_type.unwrap_or("Text")))),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Specify exactly one expression source.".to_string(),
            "Pass --expression <M> (a raw Power Query expression) OR --parameter-value <val> [--parameter-type Text|Number|Logical|DateTime]."
                .to_string(),
        )
        .into()),
    }
}

fn render_expression_block(name: &str, m: &str) -> String {
    use std::fmt::Write as _;
    let expr = m.trim();
    if expr.contains('\n') {
        let mut s = format!("expression {} =\n", quote_tmdl_name(name));
        for l in expr.lines() {
            let _ = writeln!(s, "\t{}", l.trim_end());
        }
        s
    } else {
        format!("expression {} = {expr}\n", quote_tmdl_name(name))
    }
}

fn append_block(existing: &str, block: &str) -> String {
    let trimmed = existing.trim_end();
    if trimmed.is_empty() {
        block.to_string()
    } else {
        format!("{trimmed}\n\n{block}")
    }
}

// ── update-expression ─────────────────────────────────────────────────────────

pub(super) async fn update_expression(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    m: &str,
) -> Result<()> {
    let op = "semantic-model update-expression";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = update_expression_bim(bim, name, m)?;
        super::tmdl::replace_part(&parts, "model.bim", &new_bim)
    } else {
        let existing = part_content(&parts, EXPRESSIONS_PATH).unwrap_or("");
        let (updated, found) = replace_expression_block(existing, name, m);
        if !found {
            return Err(expression_not_found(name));
        }
        upsert_part(&parts, EXPRESSIONS_PATH, &updated)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "expression": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "expression_updated", "id": id, "expression": name }),
        "status",
    );
    Ok(())
}

// ── delete-expression ─────────────────────────────────────────────────────────

pub(super) async fn delete_expression(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model delete-expression";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_expression_bim(bim, name)?;
        super::tmdl::replace_part(&parts, "model.bim", &new_bim)
    } else {
        let existing = part_content(&parts, EXPRESSIONS_PATH).unwrap_or("");
        let (updated, found) = remove_expression_block(existing, name);
        if !found {
            return Err(expression_not_found(name));
        }
        if updated.trim().is_empty() {
            remove_part(&parts, EXPRESSIONS_PATH)
        } else {
            upsert_part(&parts, EXPRESSIONS_PATH, &updated)
        }
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "expression": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "expression_deleted", "id": id, "expression": name }),
        "status",
    );
    Ok(())
}

// ── list-expressions ──────────────────────────────────────────────────────────

pub(super) async fn list_expressions(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let op = "semantic-model list-expressions";
    let parts = fetch_parts(client, workspace, id, op).await?;
    let expressions = collect_expressions(&parts);
    output::render_list(
        cli,
        &expressions,
        &["name", "isParameter"],
        &["NAME", "PARAMETER"],
        "name",
    );
    Ok(())
}

// ── pure TMDL editors ─────────────────────────────────────────────────────────

/// The `[start, end)` line span of an `expression <name>` block (through its
/// indent≥1 body).
fn expression_span(lines: &[&str], name: &str) -> Option<(usize, usize)> {
    let decl = lines.iter().position(|l| {
        tab_indent(l) == 0
            && l.starts_with("expression ")
            && decl_name(l, "expression").as_deref() == Some(name)
    })?;
    let mut end = decl + 1;
    while end < lines.len() && (lines[end].trim().is_empty() || tab_indent(lines[end]) >= 1) {
        end += 1;
    }
    while end > decl + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    Some((decl, end))
}

fn remove_expression_block(content: &str, name: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = expression_span(&lines, name) else {
        return (content.to_string(), false);
    };
    let mut out: Vec<&str> = Vec::new();
    out.extend(&lines[..start]);
    out.extend(&lines[end..]);
    let mut joined = out.join("\n");
    while joined.contains("\n\n\n") {
        joined = joined.replace("\n\n\n", "\n\n");
    }
    let joined = joined.trim().to_string();
    let result = if joined.is_empty() {
        String::new()
    } else if content.ends_with('\n') {
        format!("{joined}\n")
    } else {
        joined
    };
    (result, true)
}

fn replace_expression_block(content: &str, name: &str, m: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = expression_span(&lines, name) else {
        return (content.to_string(), false);
    };
    let block = render_expression_block(name, m);
    let mut out: Vec<String> = Vec::new();
    out.extend(lines[..start].iter().map(|s| (*s).to_string()));
    out.extend(block.trim_end().lines().map(String::from));
    out.extend(lines[end..].iter().map(|s| (*s).to_string()));
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    (result, true)
}

fn parse_expressions(content: &str) -> Vec<(String, bool)> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if tab_indent(lines[i]) == 0
            && lines[i].starts_with("expression ")
            && let Some(name) = decl_name(lines[i], "expression")
        {
            // Scan the block body for the parameter marker.
            let mut is_param = lines[i].contains("IsParameterQuery=true");
            let mut j = i + 1;
            while j < lines.len() && (lines[j].trim().is_empty() || tab_indent(lines[j]) >= 1) {
                if lines[j].contains("IsParameterQuery=true") {
                    is_param = true;
                }
                j += 1;
            }
            out.push((name, is_param));
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn collect_expressions(parts: &[(String, String)]) -> Vec<Value> {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_expressions_bim(bim);
    }
    part_content(parts, EXPRESSIONS_PATH)
        .map(|c| {
            parse_expressions(c)
                .into_iter()
                .map(
                    |(name, is_param)| serde_json::json!({ "name": name, "isParameter": is_param }),
                )
                .collect()
        })
        .unwrap_or_default()
}

fn expression_exists(parts: &[(String, String)], name: &str) -> bool {
    collect_expressions(parts)
        .iter()
        .any(|e| e.get("name").and_then(Value::as_str) == Some(name))
}

fn expression_not_found(name: &str) -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::NotFound,
        format!("Named expression '{name}' not found in the model definition."),
        "List expressions with `fabio semantic-model list-expressions`.".to_string(),
    )
    .into()
}

// ── model.bim editors ─────────────────────────────────────────────────────────

fn bim_expressions_mut(j: &mut Value) -> Result<&mut Vec<Value>> {
    j.get_mut("model")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no model object"))?
        .entry("expressions")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| FabioError::invalid_input("expressions is not an array").into())
}

fn add_expression_bim(bim: &str, name: &str, m: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    bim_expressions_mut(&mut j)?
        .push(serde_json::json!({ "name": name, "kind": "m", "expression": m }));
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn update_expression_bim(bim: &str, name: &str, m: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let e = bim_expressions_mut(&mut j)?
        .iter_mut()
        .find(|e| e.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| expression_not_found(name))?;
    e["expression"] = Value::from(m);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_expression_bim(bim: &str, name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let es = bim_expressions_mut(&mut j)?;
    let before = es.len();
    es.retain(|e| e.get("name").and_then(Value::as_str) != Some(name));
    if es.len() == before {
        return Err(expression_not_found(name));
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn collect_expressions_bim(bim: &str) -> Vec<Value> {
    let Ok(j) = serde_json::from_str::<Value>(bim) else {
        return Vec::new();
    };
    j.get("model")
        .and_then(|m| m.get("expressions"))
        .and_then(Value::as_array)
        .map(|es| {
            es.iter()
                .map(|e| {
                    let is_param = e
                        .get("expression")
                        .and_then(Value::as_str)
                        .is_some_and(|x| x.contains("IsParameterQuery=true"));
                    serde_json::json!({
                        "name": e.get("name").and_then(Value::as_str).unwrap_or(""),
                        "isParameter": is_param,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_m_shape() {
        let m = build_parameter_m("myserver", "Text");
        assert_eq!(
            m,
            "\"myserver\" meta [IsParameterQuery=true, Type=\"Text\", IsParameterQueryRequired=true]"
        );
    }

    #[test]
    fn resolve_m_modes() {
        assert_eq!(
            resolve_m(Some("let x = 1 in x"), None, None).unwrap(),
            "let x = 1 in x"
        );
        let p = resolve_m(None, Some("v"), Some("Number")).unwrap();
        assert!(p.contains("Type=\"Number\""));
        assert!(resolve_m(None, None, None).is_err());
    }

    #[test]
    fn render_single_and_multiline() {
        assert_eq!(
            render_expression_block("P", "\"x\" meta [IsParameterQuery=true]"),
            "expression P = \"x\" meta [IsParameterQuery=true]\n"
        );
        let ml = render_expression_block("Q", "let\n  a = 1\nin a");
        assert!(ml.starts_with("expression Q =\n"));
        assert!(ml.contains("\tlet"));
        assert!(ml.contains("\t  a = 1"));
    }

    #[test]
    fn append_parse_replace_remove() {
        let a = append_block("", &render_expression_block("A", "1"));
        let b = append_block(
            &a,
            &render_expression_block("B", "\"v\" meta [IsParameterQuery=true, Type=\"Text\"]"),
        );
        let parsed = parse_expressions(&b);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], ("A".to_string(), false));
        assert_eq!(parsed[1], ("B".to_string(), true));
        // replace A
        let (r, found) = replace_expression_block(&b, "A", "2");
        assert!(found);
        assert!(r.contains("expression A = 2"));
        assert!(!r.contains("expression A = 1"));
        // remove B
        let (rm, found2) = remove_expression_block(&b, "B");
        assert!(found2);
        assert!(!rm.contains("expression B"));
        assert!(rm.contains("expression A"));
    }

    #[test]
    fn bim_expression_lifecycle() {
        let bim = r#"{"model":{"tables":[]}}"#;
        let a = add_expression_bim(
            bim,
            "Srv",
            "\"s\" meta [IsParameterQuery=true, Type=\"Text\"]",
        )
        .unwrap();
        let listed = collect_expressions_bim(&a);
        assert_eq!(listed[0]["name"], "Srv");
        assert_eq!(listed[0]["isParameter"], true);
        let u = update_expression_bim(&a, "Srv", "let x = 1 in x").unwrap();
        assert!(u.contains("let x = 1 in x"));
        let d = delete_expression_bim(&a, "Srv").unwrap();
        let jd: Value = serde_json::from_str(&d).unwrap();
        assert_eq!(jd["model"]["expressions"].as_array().unwrap().len(), 0);
    }
}
