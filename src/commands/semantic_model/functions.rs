//! `semantic-model` DAX user-defined function (UDF) authoring —
//! `add-function`, `update-function`, `delete-function`, `list-functions`.
//!
//! DAX UDFs live in `definition/functions.tmdl` as top-level `function <name> =
//! <DAX>` blocks (a preview feature requiring model compatibility level ≥ 1702).
//! Like `expressions.tmdl`, the file is NOT `ref`-ed in `model.tmdl` (it is
//! auto-discovered) and is created on the first add / removed when the last
//! function is deleted. fabio edits these via the shared definition
//! read-modify-write (no XMLA/TOM); `add-function` bumps the model's
//! `compatibilityLevel` to 1702 when it is lower (UDFs require it).

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::analyze::tab_indent;
use super::tmdl::{
    decl_name, fetch_parts, part_content, push_parts, quote_tmdl_name, remove_part, replace_part,
    upsert_part,
};

const FUNCTIONS_PATH: &str = "definition/functions.tmdl";
const DATABASE_PATH: &str = "definition/database.tmdl";
const MIN_COMPAT_LEVEL: u32 = 1702;

// ── add-function ──────────────────────────────────────────────────────────────

pub(super) async fn add_function(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    expression: &str,
    description: Option<&str>,
) -> Result<()> {
    let op = "semantic-model add-function";
    let parts = fetch_parts(client, workspace, id, op).await?;

    if function_exists(&parts, name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("A function '{name}' already exists."),
            "Use `update-function`, or pick a different name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_function_bim(bim, name, expression, description)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let existing = part_content(&parts, FUNCTIONS_PATH).unwrap_or("");
        let block = render_function_block(name, expression, description);
        let updated = append_block(existing, &block);
        let with_fn = upsert_part(&parts, FUNCTIONS_PATH, &updated);
        // UDFs require compatibility level >= 1702; bump if lower.
        ensure_compat_level(&with_fn)
    };

    if output::dry_run_guard(cli, op, &serde_json::json!({ "id": id, "function": name })) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "function_added", "id": id, "function": name }),
        "status",
    );
    Ok(())
}

/// Bump `database.tmdl`'s `compatibilityLevel` to `MIN_COMPAT_LEVEL` if lower.
fn ensure_compat_level(parts: &[(String, String)]) -> Vec<(String, String)> {
    let Some(db) = part_content(parts, DATABASE_PATH) else {
        return parts.to_vec();
    };
    let current = db
        .lines()
        .find_map(|l| l.trim().strip_prefix("compatibilityLevel:"))
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if current >= MIN_COMPAT_LEVEL {
        return parts.to_vec();
    }
    let new_db: String = db
        .lines()
        .map(|l| {
            if l.trim().starts_with("compatibilityLevel:") {
                let indent = &l[..l.len() - l.trim_start().len()];
                format!("{indent}compatibilityLevel: {MIN_COMPAT_LEVEL}")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if db.ends_with('\n') { "\n" } else { "" };
    replace_part(parts, DATABASE_PATH, &new_db)
}

fn render_function_block(name: &str, expr: &str, description: Option<&str>) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    // TMDL descriptions are `///` comment lines IMMEDIATELY above the declaration
    // (no blank line between). Multi-line descriptions emit one `///` per line.
    if let Some(d) = description.filter(|x| !x.is_empty()) {
        for dl in d.split('\n') {
            let _ = writeln!(s, "/// {}", dl.trim_end());
        }
    }
    let expr = expr.trim();
    if expr.contains('\n') {
        let _ = writeln!(s, "function {} =", quote_tmdl_name(name));
        for l in expr.lines() {
            let _ = writeln!(s, "\t{}", l.trim_end());
        }
    } else {
        let _ = writeln!(s, "function {} = {expr}", quote_tmdl_name(name));
    }
    s
}

fn append_block(existing: &str, block: &str) -> String {
    let trimmed = existing.trim_end();
    if trimmed.is_empty() {
        block.to_string()
    } else {
        format!("{trimmed}\n\n{block}")
    }
}

// ── update-function ───────────────────────────────────────────────────────────

pub(super) async fn update_function(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    expression: &str,
    description: Option<&str>,
) -> Result<()> {
    let op = "semantic-model update-function";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = update_function_bim(bim, name, expression, description)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let existing = part_content(&parts, FUNCTIONS_PATH).unwrap_or("");
        let (updated, found) = replace_function_block(existing, name, expression, description);
        if !found {
            return Err(function_not_found(name));
        }
        upsert_part(&parts, FUNCTIONS_PATH, &updated)
    };

    if output::dry_run_guard(cli, op, &serde_json::json!({ "id": id, "function": name })) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "function_updated", "id": id, "function": name }),
        "status",
    );
    Ok(())
}

// ── delete-function ───────────────────────────────────────────────────────────

pub(super) async fn delete_function(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model delete-function";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_function_bim(bim, name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let existing = part_content(&parts, FUNCTIONS_PATH).unwrap_or("");
        let (updated, found) = remove_function_block(existing, name);
        if !found {
            return Err(function_not_found(name));
        }
        if updated.trim().is_empty() {
            remove_part(&parts, FUNCTIONS_PATH)
        } else {
            upsert_part(&parts, FUNCTIONS_PATH, &updated)
        }
    };

    if output::dry_run_guard(cli, op, &serde_json::json!({ "id": id, "function": name })) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "function_deleted", "id": id, "function": name }),
        "status",
    );
    Ok(())
}

// ── list-functions ────────────────────────────────────────────────────────────

pub(super) async fn list_functions(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let op = "semantic-model list-functions";
    let parts = fetch_parts(client, workspace, id, op).await?;
    let functions = collect_functions(&parts);
    output::render_list(
        cli,
        &functions,
        &["name", "description"],
        &["NAME", "DESCRIPTION"],
        "name",
    );
    Ok(())
}

// ── pure TMDL editors ─────────────────────────────────────────────────────────

fn function_span(lines: &[&str], name: &str) -> Option<(usize, usize)> {
    let decl = lines.iter().position(|l| {
        tab_indent(l) == 0
            && l.starts_with("function ")
            && decl_name(l, "function").as_deref() == Some(name)
    })?;
    // Extend the start UPWARD over contiguous `///` description lines (a TMDL
    // description belongs to the declaration immediately below it), so update /
    // remove also replace/drop the old description.
    let mut start = decl;
    while start > 0
        && tab_indent(lines[start - 1]) == 0
        && lines[start - 1].trim_start().starts_with("///")
    {
        start -= 1;
    }
    let mut end = decl + 1;
    while end < lines.len() && (lines[end].trim().is_empty() || tab_indent(lines[end]) >= 1) {
        end += 1;
    }
    while end > decl + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    Some((start, end))
}

fn remove_function_block(content: &str, name: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = function_span(&lines, name) else {
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

fn replace_function_block(
    content: &str,
    name: &str,
    expr: &str,
    description: Option<&str>,
) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = function_span(&lines, name) else {
        return (content.to_string(), false);
    };
    let block = render_function_block(name, expr, description);
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

/// Parse each top-level function with its `///` description (the contiguous
/// comment lines immediately above the declaration), for `list-functions`.
fn parse_functions_with_desc(content: &str) -> Vec<(String, Option<String>)> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if tab_indent(l) != 0 || !l.starts_with("function ") {
            continue;
        }
        let Some(name) = decl_name(l, "function") else {
            continue;
        };
        // Walk upward over contiguous `///` lines to reconstruct the description.
        let mut desc_lines: Vec<String> = Vec::new();
        let mut k = i;
        while k > 0 && tab_indent(lines[k - 1]) == 0 && lines[k - 1].trim_start().starts_with("///")
        {
            let text = lines[k - 1].trim_start().trim_start_matches("///").trim();
            desc_lines.push(text.to_string());
            k -= 1;
        }
        desc_lines.reverse();
        let desc = if desc_lines.is_empty() {
            None
        } else {
            Some(desc_lines.join("\n"))
        };
        out.push((name, desc));
    }
    out
}

fn collect_functions(parts: &[(String, String)]) -> Vec<Value> {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_functions_bim(bim);
    }
    part_content(parts, FUNCTIONS_PATH)
        .map(|c| {
            parse_functions_with_desc(c)
                .into_iter()
                .map(|(name, desc)| {
                    desc.map_or_else(
                        || serde_json::json!({ "name": name }),
                        |d| serde_json::json!({ "name": name, "description": d }),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn function_exists(parts: &[(String, String)], name: &str) -> bool {
    collect_functions(parts)
        .iter()
        .any(|f| f.get("name").and_then(Value::as_str) == Some(name))
}

fn function_not_found(name: &str) -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::NotFound,
        format!("Function '{name}' not found in the model definition."),
        "List functions with `fabio semantic-model list-functions`.".to_string(),
    )
    .into()
}

// ── model.bim editors ─────────────────────────────────────────────────────────

fn bim_functions_mut(j: &mut Value) -> Result<&mut Vec<Value>> {
    let m = j
        .get_mut("model")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no model object"))?;
    m.entry("functions")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| FabioError::invalid_input("functions is not an array").into())
}

fn add_function_bim(
    bim: &str,
    name: &str,
    expr: &str,
    description: Option<&str>,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    // Ensure compatibility level.
    if j.get("compatibilityLevel")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        < u64::from(MIN_COMPAT_LEVEL)
        && let Some(obj) = j.as_object_mut()
    {
        obj.insert(
            "compatibilityLevel".to_string(),
            Value::from(MIN_COMPAT_LEVEL),
        );
    }
    let mut fun = serde_json::json!({ "name": name, "expression": expr });
    if let Some(d) = description.filter(|x| !x.is_empty()) {
        fun["description"] = Value::from(d);
    }
    bim_functions_mut(&mut j)?.push(fun);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn update_function_bim(
    bim: &str,
    name: &str,
    expr: &str,
    description: Option<&str>,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let f = bim_functions_mut(&mut j)?
        .iter_mut()
        .find(|f| f.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| function_not_found(name))?;
    f["expression"] = Value::from(expr);
    // Only touch the description when one is provided (keep the existing one on a
    // pure expression update).
    if let Some(d) = description {
        if d.is_empty() {
            if let Some(obj) = f.as_object_mut() {
                obj.remove("description");
            }
        } else {
            f["description"] = Value::from(d);
        }
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_function_bim(bim: &str, name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let fs = bim_functions_mut(&mut j)?;
    let before = fs.len();
    fs.retain(|f| f.get("name").and_then(Value::as_str) != Some(name));
    if fs.len() == before {
        return Err(function_not_found(name));
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn collect_functions_bim(bim: &str) -> Vec<Value> {
    let Ok(j) = serde_json::from_str::<Value>(bim) else {
        return Vec::new();
    };
    j.get("model")
        .and_then(|m| m.get("functions"))
        .and_then(Value::as_array)
        .map(|fs| {
            fs.iter()
                .map(|f| {
                    let name = f.get("name").and_then(Value::as_str).unwrap_or("");
                    match f.get("description").and_then(Value::as_str) {
                        Some(d) if !d.is_empty() => {
                            serde_json::json!({ "name": name, "description": d })
                        }
                        _ => serde_json::json!({ "name": name }),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_and_parse() {
        assert_eq!(
            render_function_block("AddOne", "(x: INT64) => RETURN x + 1", None),
            "function AddOne = (x: INT64) => RETURN x + 1\n"
        );
        let content = "function AddOne = (x: INT64) => RETURN x + 1\n\nfunction Sq = (x: INT64) => RETURN x * x\n";
        let names: Vec<String> = parse_functions_with_desc(content)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, vec!["AddOne".to_string(), "Sq".to_string()]);
    }

    #[test]
    fn render_with_description_emits_triple_slash() {
        let block = render_function_block(
            "AddNum",
            "(x: NUMERIC = 1, y: NUMERIC = 2) => x + y",
            Some("Adds two numbers.\nDefaults to 1 + 2."),
        );
        // `///` lines come immediately above the declaration (no blank line).
        assert_eq!(
            block,
            "/// Adds two numbers.\n/// Defaults to 1 + 2.\nfunction AddNum = (x: NUMERIC = 1, y: NUMERIC = 2) => x + y\n"
        );
    }

    #[test]
    fn description_round_trips_through_span_editors() {
        // Add two functions, the first with a description.
        let a = append_block(
            "",
            &render_function_block("F", "(x)=>RETURN x", Some("first fn")),
        );
        let b = append_block(&a, &render_function_block("G", "(y)=>RETURN y", None));
        assert!(b.contains("/// first fn\nfunction F"));
        // list surfaces the description for F but not G.
        let parsed = parse_functions_with_desc(&b);
        assert_eq!(parsed[0], ("F".to_string(), Some("first fn".to_string())));
        assert_eq!(parsed[1], ("G".to_string(), None));
        // Updating F's description replaces the old `///` (no duplication) and
        // leaves G untouched.
        let (updated, found) =
            replace_function_block(&b, "F", "(x)=>RETURN x+1", Some("updated fn"));
        assert!(found);
        assert!(updated.contains("/// updated fn\nfunction F = (x)=>RETURN x+1"));
        assert!(!updated.contains("/// first fn"));
        assert_eq!(updated.matches("/// ").count(), 1);
        assert!(updated.contains("function G = (y)=>RETURN y"));
        // Removing F drops its description lines too.
        let (rm, found2) = remove_function_block(&updated, "F");
        assert!(found2);
        assert!(!rm.contains("updated fn"));
        assert!(!rm.contains("function F"));
        assert!(rm.contains("function G"));
    }

    #[test]
    fn append_replace_remove() {
        let a = append_block("", &render_function_block("F", "(x)=>RETURN x", None));
        let b = append_block(&a, &render_function_block("G", "(y)=>RETURN y", None));
        let (r, found) = replace_function_block(&b, "F", "(x)=>RETURN x+1", None);
        assert!(found);
        assert!(r.contains("function F = (x)=>RETURN x+1"));
        let (rm, found2) = remove_function_block(&b, "G");
        assert!(found2);
        assert!(!rm.contains("function G"));
        assert!(rm.contains("function F"));
    }

    #[test]
    fn ensure_compat_bumps_when_low() {
        let parts = vec![(
            DATABASE_PATH.to_string(),
            "database\n\tcompatibilityLevel: 1604\n".to_string(),
        )];
        let out = ensure_compat_level(&parts);
        assert!(out[0].1.contains("compatibilityLevel: 1702"));
        // no bump when already high
        let hi = vec![(
            DATABASE_PATH.to_string(),
            "database\n\tcompatibilityLevel: 1702\n".to_string(),
        )];
        let out2 = ensure_compat_level(&hi);
        assert_eq!(out2[0].1, hi[0].1);
    }

    #[test]
    fn bim_function_lifecycle() {
        let bim = r#"{"compatibilityLevel":1604,"model":{"tables":[]}}"#;
        let a = add_function_bim(
            bim,
            "AddOne",
            "(x: INT64) => RETURN x + 1",
            Some("adds one"),
        )
        .unwrap();
        let j: Value = serde_json::from_str(&a).unwrap();
        assert_eq!(j["compatibilityLevel"], 1702);
        assert_eq!(j["model"]["functions"][0]["name"], "AddOne");
        assert_eq!(j["model"]["functions"][0]["description"], "adds one");
        // Pure expression update keeps the existing description.
        let u = update_function_bim(&a, "AddOne", "(x) => RETURN x", None).unwrap();
        let ju: Value = serde_json::from_str(&u).unwrap();
        assert!(u.contains("(x) => RETURN x"));
        assert_eq!(ju["model"]["functions"][0]["description"], "adds one");
        // Empty description clears it.
        let cleared = update_function_bim(&a, "AddOne", "(x) => RETURN x", Some("")).unwrap();
        let jc: Value = serde_json::from_str(&cleared).unwrap();
        assert!(jc["model"]["functions"][0].get("description").is_none());
        let d = delete_function_bim(&a, "AddOne").unwrap();
        let jd: Value = serde_json::from_str(&d).unwrap();
        assert_eq!(jd["model"]["functions"].as_array().unwrap().len(), 0);
    }
}
