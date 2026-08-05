//! `semantic-model` partition authoring — `add-partition`, `update-partition`,
//! `delete-partition`, `list-partitions`.
//!
//! Partitions define a table's data source and live inside the table's
//! `definition/tables/<T>.tmdl` as `partition <name> = <mode>` blocks with a
//! `source = <expr>` body (M for `= m`, DAX for `= calculated`). A table can
//! carry several partitions (e.g. incremental-refresh ranges). fabio edits them
//! via the shared definition read-modify-write (no XMLA/TOM). A table must keep
//! at least one partition, so `delete-partition` refuses to remove the last one.

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::analyze::tab_indent;
use super::tmdl::{
    child_span, decl_name, fetch_parts, find_table_file, insert_table_child_lines, is_table_tmdl,
    join_preserving_trailing_newline, part_content, push_parts, quote_tmdl_name, replace_part,
    tmdl_table_name,
};

/// Partition source kind.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceKind {
    M,
    Calculated,
}

impl SourceKind {
    const fn mode(self) -> &'static str {
        match self {
            Self::M => "m",
            Self::Calculated => "calculated",
        }
    }
}

// ── add-partition ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn add_partition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    name: &str,
    kind: SourceKind,
    expression: &str,
) -> Result<()> {
    let op = "semantic-model add-partition";
    let parts = fetch_parts(client, workspace, id, op).await?;

    if partition_exists(&parts, table, name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("Partition '{table}.{name}' already exists."),
            "Use `update-partition`, or pick a different name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_partition_bim(bim, table, name, kind, expression)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, table)?;
        let block = build_partition_lines(name, kind, expression);
        let new_content = insert_table_child_lines(&parts[idx].1, &block);
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": table, "partition": name, "mode": kind.mode() }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "partition_added", "id": id, "table": table, "partition": name }),
        "status",
    );
    Ok(())
}

/// Render a partition's TMDL lines. A single-line source is inline; a multi-line
/// source becomes an indented block.
fn build_partition_lines(name: &str, kind: SourceKind, expression: &str) -> Vec<String> {
    let mut block: Vec<String> = Vec::new();
    block.push(format!(
        "\tpartition {} = {}",
        quote_tmdl_name(name),
        kind.mode()
    ));
    if kind == SourceKind::Calculated {
        block.push("\t\tmode: import".to_string());
    }
    let expr = expression.trim();
    if expr.contains('\n') {
        block.push("\t\tsource =".to_string());
        for l in expr.lines() {
            block.push(format!("\t\t\t{}", l.trim_end()));
        }
    } else {
        block.push(format!("\t\tsource = {expr}"));
    }
    block
}

// ── update-partition ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn update_partition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    name: &str,
    kind: SourceKind,
    expression: &str,
) -> Result<()> {
    let op = "semantic-model update-partition";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = update_partition_bim(bim, table, name, kind, expression)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, table)?;
        let (new_content, updated) = update_partition_tmdl(&parts[idx].1, name, kind, expression);
        if !updated {
            return Err(
                FabioError::not_found(format!("Partition '{table}.{name}' not found")).into(),
            );
        }
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": table, "partition": name, "mode": kind.mode() }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "partition_updated", "id": id, "table": table, "partition": name }),
        "status",
    );
    Ok(())
}

/// Replace a partition's declaration mode + source, preserving other property
/// lines. Returns `(new_content, updated)`.
fn update_partition_tmdl(
    content: &str,
    name: &str,
    kind: SourceKind,
    expression: &str,
) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = child_span(&lines, "partition", name) else {
        return (content.to_string(), false);
    };
    // Locate the decl line (first indent-1 non-comment line in the span).
    let decl_idx = (start..end)
        .find(|&i| tab_indent(lines[i]) == 1 && !lines[i].trim_start().starts_with("///"))
        .unwrap_or(start);

    let mut block: Vec<String> = Vec::new();
    // Leading `///` comments (if any) are preserved.
    block.extend(lines[start..decl_idx].iter().map(|s| (*s).to_string()));
    block.push(format!(
        "\tpartition {} = {}",
        quote_tmdl_name(name),
        kind.mode()
    ));
    // Preserve non-source property lines (mode:, queryGroup:, etc.), skip the old
    // source (its `= ` line + any indent≥3 continuation).
    let mut i = decl_idx + 1;
    let mut skipping_source = false;
    while i < end {
        let l = lines[i];
        let t = l.trim_start();
        if t.starts_with("source") {
            skipping_source = true;
            i += 1;
            continue;
        }
        if skipping_source && tab_indent(l) >= 3 {
            i += 1;
            continue;
        }
        skipping_source = false;
        // Drop an existing `mode:` (we re-add it for calculated) to avoid dupes.
        if t.starts_with("mode:") {
            i += 1;
            continue;
        }
        block.push(l.to_string());
        i += 1;
    }
    if kind == SourceKind::Calculated {
        block.push("\t\tmode: import".to_string());
    }
    let expr = expression.trim();
    if expr.contains('\n') {
        block.push("\t\tsource =".to_string());
        for l in expr.lines() {
            block.push(format!("\t\t\t{}", l.trim_end()));
        }
    } else {
        block.push(format!("\t\tsource = {expr}"));
    }

    let mut out: Vec<String> = Vec::new();
    out.extend(lines[..start].iter().map(|s| (*s).to_string()));
    out.extend(block);
    out.extend(lines[end..].iter().map(|s| (*s).to_string()));
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    (result, true)
}

// ── delete-partition ──────────────────────────────────────────────────────────

pub(super) async fn delete_partition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model delete-partition";
    let parts = fetch_parts(client, workspace, id, op).await?;

    // A table must keep at least one partition.
    let count = partition_count(&parts, table);
    if count <= 1 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Cannot delete the only partition of table '{table}'."),
            "A table must keep at least one partition. Delete the table instead, or add another partition first."
                .to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_partition_bim(bim, table, name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, table)?;
        let (new_content, removed) = delete_partition_tmdl(&parts[idx].1, name);
        if !removed {
            return Err(
                FabioError::not_found(format!("Partition '{table}.{name}' not found")).into(),
            );
        }
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": table, "partition": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "partition_deleted", "id": id, "table": table, "partition": name }),
        "status",
    );
    Ok(())
}

fn delete_partition_tmdl(content: &str, name: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = child_span(&lines, "partition", name) else {
        return (content.to_string(), false);
    };
    let mut out: Vec<String> = Vec::new();
    out.extend(lines[..start].iter().map(|s| (*s).to_string()));
    out.extend(lines[end..].iter().map(|s| (*s).to_string()));
    (
        join_preserving_trailing_newline(&out, content.ends_with('\n')),
        true,
    )
}

// ── list-partitions ───────────────────────────────────────────────────────────

pub(super) async fn list_partitions(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: Option<&str>,
) -> Result<()> {
    let op = "semantic-model list-partitions";
    let parts = fetch_parts(client, workspace, id, op).await?;
    let partitions = collect_partitions(&parts, table);
    output::render_list(
        cli,
        &partitions,
        &["table", "name", "mode"],
        &["TABLE", "NAME", "MODE"],
        "name",
    );
    Ok(())
}

/// Parse `(name, mode)` partitions from a table file.
fn parse_partitions_tmdl(content: &str) -> Vec<(String, String)> {
    content
        .lines()
        .filter(|l| tab_indent(l) == 1 && l.trim_start().starts_with("partition "))
        .filter_map(|l| {
            let t = l.trim_start();
            let name = decl_name(t, "partition")?;
            let mode = t
                .split('=')
                .nth(1)
                .map_or_else(String::new, |m| m.trim().to_string());
            Some((name, mode))
        })
        .collect()
}

fn collect_partitions(parts: &[(String, String)], table: Option<&str>) -> Vec<Value> {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_partitions_bim(bim, table);
    }
    let mut out = Vec::new();
    for (p, c) in parts {
        if !is_table_tmdl(p) {
            continue;
        }
        let tname = tmdl_table_name(c).unwrap_or_default();
        if table.is_some_and(|t| t != tname) {
            continue;
        }
        for (name, mode) in parse_partitions_tmdl(c) {
            out.push(serde_json::json!({ "table": tname, "name": name, "mode": mode }));
        }
    }
    out
}

fn partition_count(parts: &[(String, String)], table: &str) -> usize {
    collect_partitions(parts, Some(table)).len()
}

fn partition_exists(parts: &[(String, String)], table: &str, name: &str) -> bool {
    collect_partitions(parts, Some(table))
        .iter()
        .any(|p| p.get("name").and_then(Value::as_str) == Some(name))
}

// ── model.bim editors ─────────────────────────────────────────────────────────

fn bim_table<'a>(j: &'a mut Value, table: &str) -> Result<&'a mut Value> {
    j.get_mut("model")
        .and_then(|m| m.get_mut("tables"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no tables"))?
        .iter_mut()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(table))
        .ok_or_else(|| FabioError::not_found(format!("Table '{table}' not found")).into())
}

fn partition_source_json(kind: SourceKind, expression: &str) -> Value {
    match kind {
        SourceKind::M => serde_json::json!({ "type": "m", "expression": expression }),
        SourceKind::Calculated => {
            serde_json::json!({ "type": "calculated", "expression": expression })
        }
    }
}

fn add_partition_bim(
    bim: &str,
    table: &str,
    name: &str,
    kind: SourceKind,
    expression: &str,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    bim_table(&mut j, table)?
        .as_object_mut()
        .unwrap()
        .entry("partitions")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .unwrap()
        .push(
            serde_json::json!({ "name": name, "source": partition_source_json(kind, expression) }),
        );
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn update_partition_bim(
    bim: &str,
    table: &str,
    name: &str,
    kind: SourceKind,
    expression: &str,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let p = bim_table(&mut j, table)?
        .get_mut("partitions")
        .and_then(Value::as_array_mut)
        .and_then(|ps| {
            ps.iter_mut()
                .find(|p| p.get("name").and_then(Value::as_str) == Some(name))
        })
        .ok_or_else(|| FabioError::not_found(format!("Partition '{table}.{name}' not found")))?;
    p["source"] = partition_source_json(kind, expression);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_partition_bim(bim: &str, table: &str, name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let ps = bim_table(&mut j, table)?
        .get_mut("partitions")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::not_found(format!("Partition '{table}.{name}' not found")))?;
    let before = ps.len();
    ps.retain(|p| p.get("name").and_then(Value::as_str) != Some(name));
    if ps.len() == before {
        return Err(FabioError::not_found(format!("Partition '{table}.{name}' not found")).into());
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn collect_partitions_bim(bim: &str, table: Option<&str>) -> Vec<Value> {
    let Ok(j) = serde_json::from_str::<Value>(bim) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(tables) = j
        .get("model")
        .and_then(|m| m.get("tables"))
        .and_then(Value::as_array)
    {
        for t in tables {
            let tname = t.get("name").and_then(Value::as_str).unwrap_or("");
            if table.is_some_and(|x| x != tname) {
                continue;
            }
            if let Some(ps) = t.get("partitions").and_then(Value::as_array) {
                for p in ps {
                    let mode = p
                        .pointer("/source/type")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    out.push(serde_json::json!({
                        "table": tname,
                        "name": p.get("name").and_then(Value::as_str).unwrap_or(""),
                        "mode": mode,
                    }));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_tmdl() -> String {
        "table Sales\n\n\tcolumn Amount\n\t\tdataType: double\n\t\tsourceColumn: Amount\n\n\tpartition Sales = m\n\t\tsource = let Source = 1 in Source\n".to_string()
    }

    #[test]
    fn build_m_and_calculated_partitions() {
        let m = build_partition_lines("P", SourceKind::M, "let x = 1 in x");
        assert_eq!(m[0], "\tpartition P = m");
        assert_eq!(m[1], "\t\tsource = let x = 1 in x");
        let c = build_partition_lines("C", SourceKind::Calculated, "ROW(\"a\",1)");
        assert!(c.contains(&"\tpartition C = calculated".to_string()));
        assert!(c.contains(&"\t\tmode: import".to_string()));
    }

    #[test]
    fn build_multiline_source() {
        let m = build_partition_lines("P", SourceKind::M, "let\n  a = 1\nin a");
        assert_eq!(m[1], "\t\tsource =");
        assert_eq!(m[2], "\t\t\tlet");
        assert!(m.iter().any(|l| l == "\t\t\t  a = 1"));
    }

    #[test]
    fn parse_partitions_reads_mode() {
        let parsed = parse_partitions_tmdl(&table_tmdl());
        assert_eq!(parsed, vec![("Sales".to_string(), "m".to_string())]);
    }

    #[test]
    fn update_partition_replaces_source() {
        let (out, updated) =
            update_partition_tmdl(&table_tmdl(), "Sales", SourceKind::M, "let y = 2 in y");
        assert!(updated);
        assert!(out.contains("\t\tsource = let y = 2 in y"));
        assert!(!out.contains("let Source = 1"));
        assert!(out.contains("\tpartition Sales = m"));
    }

    #[test]
    fn delete_partition_removes_block() {
        let two =
            "table T\n\n\tpartition A = m\n\t\tsource = 1\n\n\tpartition B = m\n\t\tsource = 2\n";
        let (out, removed) = delete_partition_tmdl(two, "A");
        assert!(removed);
        assert!(!out.contains("partition A"));
        assert!(out.contains("partition B"));
    }

    #[test]
    fn bim_partition_lifecycle() {
        let bim = r#"{"model":{"tables":[{"name":"Sales","partitions":[{"name":"P1","source":{"type":"m","expression":"1"}}]}]}}"#;
        let added = add_partition_bim(bim, "Sales", "P2", SourceKind::M, "2").unwrap();
        let j: Value = serde_json::from_str(&added).unwrap();
        assert_eq!(
            j["model"]["tables"][0]["partitions"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let updated = update_partition_bim(&added, "Sales", "P2", SourceKind::M, "22").unwrap();
        assert!(updated.contains("\"22\""));
        let listed = collect_partitions_bim(&added, Some("Sales"));
        assert_eq!(listed.len(), 2);
        let deleted = delete_partition_bim(&added, "Sales", "P2").unwrap();
        let jd: Value = serde_json::from_str(&deleted).unwrap();
        assert_eq!(
            jd["model"]["tables"][0]["partitions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
