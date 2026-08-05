//! Shared TMDL / `model.bim` definition round-trip plumbing for the
//! `semantic-model` granular authoring commands (`authoring`, `relationships`,
//! `roles`, `columns`, `tables`, `translations`).
//!
//! fabio is a REST CLI with no XMLA/TOM, so every granular model edit is a
//! definition read-modify-write: `getDefinition` → edit the TMDL parts (or
//! `model.bim`) in place → `updateDefinition`. This module holds the fetch/push
//! plumbing, the part-list helpers, and the small TMDL parsing/quoting utilities
//! the authoring modules share.

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;

use crate::client::FabricClient;
use crate::errors::enrich_forbidden;

use super::analyze::{decode_parts, strip_tmdl_name, tab_indent};

/// Fetch a semantic model's definition and decode its parts into `(path, text)`.
pub(super) async fn fetch_parts(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    op: &str,
) -> Result<Vec<(String, String)>> {
    let def = client
        .post(
            &format!("/workspaces/{workspace}/semanticModels/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, op, "Contributor"))?;
    Ok(decode_parts(&def))
}

/// Push a new set of definition parts via `updateDefinition` (LRO).
pub(super) async fn push_parts(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    parts: &[(String, String)],
    op: &str,
) -> Result<()> {
    let definition_parts: Vec<Value> = parts
        .iter()
        .map(|(path, content)| {
            serde_json::json!({
                "path": path,
                "payload": BASE64.encode(content.as_bytes()),
                "payloadType": "InlineBase64"
            })
        })
        .collect();
    client
        .post(
            &format!("/workspaces/{workspace}/semanticModels/{id}/updateDefinition"),
            &serde_json::json!({ "definition": { "parts": definition_parts } }),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, op, "Contributor"))?;
    Ok(())
}

/// Return a copy of `parts` with the content of `path` replaced (no-op if absent).
pub(super) fn replace_part(
    parts: &[(String, String)],
    path: &str,
    content: &str,
) -> Vec<(String, String)> {
    parts
        .iter()
        .map(|(p, c)| {
            if p == path {
                (p.clone(), content.to_string())
            } else {
                (p.clone(), c.clone())
            }
        })
        .collect()
}

/// Return a copy of `parts` with `path` set to `content` — replacing it if it
/// exists, otherwise appending it (used for `definition/relationships.tmdl`,
/// which may not exist until the first relationship is added).
pub(super) fn upsert_part(
    parts: &[(String, String)],
    path: &str,
    content: &str,
) -> Vec<(String, String)> {
    if parts.iter().any(|(p, _)| p == path) {
        replace_part(parts, path, content)
    } else {
        let mut out = parts.to_vec();
        out.push((path.to_string(), content.to_string()));
        out
    }
}

/// Return a copy of `parts` with `path` removed.
pub(super) fn remove_part(parts: &[(String, String)], path: &str) -> Vec<(String, String)> {
    parts.iter().filter(|(p, _)| p != path).cloned().collect()
}

/// The content of `path` in `parts`, if present.
pub(super) fn part_content<'a>(parts: &'a [(String, String)], path: &str) -> Option<&'a str> {
    parts
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, c)| c.as_str())
}

/// The model name declared by `model <name>` in `model.tmdl` (defaults `Model`).
pub(super) fn model_name(model_tmdl: &str) -> String {
    model_tmdl
        .lines()
        .find(|l| tab_indent(l) == 0 && l.trim_start().starts_with("model "))
        .map_or_else(
            || "Model".to_string(),
            |l| strip_tmdl_name(&l.trim_start()[6..]),
        )
}

pub(super) fn is_table_tmdl(path: &str) -> bool {
    path.starts_with("definition/tables/")
        && std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tmdl"))
}

/// The logical table name declared in a `definition/tables/<T>.tmdl` file.
pub(super) fn tmdl_table_name(content: &str) -> Option<String> {
    content
        .lines()
        .find(|l| tab_indent(l) == 0 && l.trim_start().starts_with("table "))
        .map(|l| strip_tmdl_name(&l.trim_start()[6..]))
}

/// The object name declared by a `<keyword> <name>[ = …]` line (quotes stripped).
pub(super) fn decl_name(trimmed: &str, keyword: &str) -> Option<String> {
    let rest = trimmed.strip_prefix(keyword)?.strip_prefix(' ')?;
    // Objects whose declaration can carry an inline `= expression`.
    let name_part = if matches!(
        keyword,
        "measure"
            | "column"
            | "partition"
            | "table"
            | "calculationItem"
            | "expression"
            | "function"
    ) {
        rest.split('=').next().unwrap_or(rest)
    } else {
        rest
    };
    let n = strip_tmdl_name(name_part);
    (!n.is_empty()).then_some(n)
}

/// Index of the `definition/tables/<table>.tmdl` part for `table`.
pub(super) fn find_table_file(parts: &[(String, String)], table: &str) -> Result<usize> {
    parts
        .iter()
        .position(|(p, c)| is_table_tmdl(p) && tmdl_table_name(c).as_deref() == Some(table))
        .ok_or_else(|| {
            crate::errors::FabioError::with_hint(
                crate::errors::ErrorCode::NotFound,
                format!("Table '{table}' not found in the model definition."),
                "List tables with `fabio semantic-model list-tables`.".to_string(),
            )
            .into()
        })
}

/// Quote a TMDL identifier if it needs quoting (contains a space or other
/// characters that break a bare identifier). A bare alphanumeric/underscore name
/// is returned as-is; everything else is single-quoted.
pub(super) fn quote_tmdl_name(name: &str) -> String {
    let bare = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.chars().next().is_some_and(|c| c.is_ascii_digit());
    if bare {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// A `Table.Column` TMDL reference, quoting each part as needed.
pub(super) fn column_ref(table: &str, column: &str) -> String {
    format!("{}.{}", quote_tmdl_name(table), quote_tmdl_name(column))
}

/// A table-level child-object declaration at indent 1 (`column`/`measure`/
/// `hierarchy`/`partition`/`calculationGroup`). Child objects MUST come AFTER
/// the table's own scalar properties (e.g. `lineageTag:`).
pub(super) fn is_child_object_decl(line: &str) -> bool {
    let t = line.trim_start();
    [
        "column ",
        "measure ",
        "hierarchy ",
        "partition ",
        "calculationGroup",
    ]
    .iter()
    .any(|k| t.starts_with(k))
}

/// Insert a block of child-object lines into a table file before the first
/// table-level child object (or its leading `///` comment) — i.e. after the
/// table's scalar properties. Yields the canonical layout and never separates
/// the `table` declaration from its own properties.
pub(super) fn insert_table_child_lines(content: &str, block: &[String]) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut inserted = false;
    let mut seen_table = false;
    for line in &lines {
        if !inserted
            && seen_table
            && tab_indent(line) == 1
            && (line.trim_start().starts_with("///") || is_child_object_decl(line))
        {
            out.extend(block.iter().cloned());
            out.push(String::new());
            inserted = true;
        }
        out.push((*line).to_string());
        if tab_indent(line) == 0 && line.trim_start().starts_with("table ") {
            seen_table = true;
        }
    }
    if !inserted {
        out.push(String::new());
        out.extend(block.iter().cloned());
    }
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// The line span `[start, end)` of a `<keyword> <name>` child-object block in a
/// table file, INCLUDING its leading contiguous `///` description comments and
/// its trailing indent≥2 body.
pub(super) fn child_span(lines: &[&str], keyword: &str, name: &str) -> Option<(usize, usize)> {
    let decl = lines.iter().position(|l| {
        tab_indent(l) == 1
            && decl_name(l.trim_start_matches('\t'), keyword).as_deref() == Some(name)
    })?;
    let mut start = decl;
    while start > 0 {
        let prev = lines[start - 1];
        if tab_indent(prev) == 1 && prev.trim_start().starts_with("///") {
            start -= 1;
        } else {
            break;
        }
    }
    let mut end = decl + 1;
    while end < lines.len() && (lines[end].trim().is_empty() || tab_indent(lines[end]) >= 2) {
        end += 1;
    }
    while end > decl + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    Some((start, end))
}

/// Join lines, collapsing runs of ≥3 newlines to 2 and preserving a single
/// trailing newline when the original had one.
pub(super) fn join_preserving_trailing_newline(out: &[String], had_trailing: bool) -> String {
    let mut joined = out.join("\n");
    while joined.contains("\n\n\n") {
        joined = joined.replace("\n\n\n", "\n\n");
    }
    let joined = joined.trim_end().to_string();
    if had_trailing && !joined.is_empty() {
        format!("{joined}\n")
    } else {
        joined
    }
}

/// Add a `ref <kind> <name>` line to `model.tmdl` if not already present. Tables
/// and roles require a ref line in `model.tmdl` (relationships do not — they are
/// auto-discovered). Returns the (possibly-updated) content.
pub(super) fn add_model_ref(model_tmdl: &str, kind: &str, name: &str) -> String {
    let ref_line = format!("ref {kind} {}", quote_tmdl_name(name));
    if model_tmdl.lines().any(|l| l.trim() == ref_line) {
        return model_tmdl.to_string();
    }
    let base = model_tmdl.trim_end();
    let last = base.lines().last().unwrap_or("");
    // Group with an existing same-kind ref block (no blank line); otherwise start
    // a new group with a blank-line separator.
    let sep = if last.trim_start().starts_with(&format!("ref {kind} ")) {
        "\n"
    } else {
        "\n\n"
    };
    let mut result = format!("{base}{sep}{ref_line}");
    if model_tmdl.ends_with('\n') || model_tmdl.is_empty() {
        result.push('\n');
    }
    result
}

/// Remove a `ref <kind> <name>` line from `model.tmdl`, collapsing the blank line
/// that the removal may leave behind.
pub(super) fn remove_model_ref(model_tmdl: &str, kind: &str, name: &str) -> String {
    let target = format!("ref {kind} {}", quote_tmdl_name(name));
    let mut out: Vec<&str> = model_tmdl
        .lines()
        .filter(|l| l.trim() != target.as_str())
        .collect();
    while out.len() >= 2
        && out[out.len() - 1].trim().is_empty()
        && out[out.len() - 2].trim().is_empty()
    {
        out.pop();
    }
    let mut result = out.join("\n");
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }
    if model_tmdl.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_tmdl_name_bare_vs_quoted() {
        assert_eq!(quote_tmdl_name("Sales"), "Sales");
        assert_eq!(quote_tmdl_name("Sales_2"), "Sales_2");
        assert_eq!(quote_tmdl_name("Sales Amount"), "'Sales Amount'");
        assert_eq!(quote_tmdl_name("2024"), "'2024'");
        assert_eq!(quote_tmdl_name("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn column_ref_quotes_each_part() {
        assert_eq!(column_ref("Sales", "Amount"), "Sales.Amount");
        assert_eq!(
            column_ref("Sales Fact", "Net Amount"),
            "'Sales Fact'.'Net Amount'"
        );
    }

    #[test]
    fn add_model_ref_appends_and_is_idempotent() {
        let m = "model Model\n\tculture: en-US\n\nref table Sales\n";
        let out = add_model_ref(m, "role", "WestOnly");
        assert!(out.contains("ref role WestOnly"));
        let again = add_model_ref(&out, "role", "WestOnly");
        assert_eq!(out, again);
    }

    #[test]
    fn remove_model_ref_drops_line() {
        let m = "model Model\n\nref table Sales\nref role WestOnly\n";
        let out = remove_model_ref(m, "role", "WestOnly");
        assert!(!out.contains("WestOnly"));
        assert!(out.contains("ref table Sales"));
    }

    #[test]
    fn upsert_and_remove_part() {
        let parts = vec![("model.tmdl".to_string(), "x".to_string())];
        let up = upsert_part(&parts, "definition/relationships.tmdl", "r");
        assert_eq!(up.len(), 2);
        let up2 = upsert_part(&up, "definition/relationships.tmdl", "r2");
        assert_eq!(up2.len(), 2);
        assert_eq!(
            part_content(&up2, "definition/relationships.tmdl"),
            Some("r2")
        );
        let rm = remove_part(&up2, "definition/relationships.tmdl");
        assert_eq!(rm.len(), 1);
    }
}
