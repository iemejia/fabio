//! `semantic-model` authoring helpers — granular object editing WITHOUT the user
//! hand-authoring the whole definition.
//!
//! Microsoft's powerbi-modeling-mcp server edits objects live via XMLA/TOM. fabio
//! is a REST CLI (no XMLA/TOM), so it achieves the same TASKS through a
//! definition read-modify-write: `getDefinition` → edit the TMDL `tables/*.tmdl`
//! (or `model.bim`) in place → `updateDefinition`. This is exactly the MCP's
//! TMDL-file editing mode, and reuses the same TMDL machinery as `analyze --fix`
//! and `generate`.
//!
//! Commands: `set-description` (table/column/measure), `add-measure`,
//! `update-measure`. Each overwrites the model definition (irreversible), so it
//! is dry-run guarded. The Fabric `updateDefinition` API validates the result —
//! a malformed edit is rejected, never silently corrupts the model.

use std::fmt::Write as _;

use anyhow::{Result, bail};
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::analyze::tab_indent;
use super::tmdl::{
    decl_name, fetch_parts, find_table_file, insert_table_child_lines, is_table_tmdl,
    join_preserving_trailing_newline, push_parts, replace_part,
};

// ── set-description ────────────────────────────────────────────────────────────

/// Which object a `set-description` targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DescTarget {
    Table(String),
    Column { table: String, column: String },
    Measure(String),
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn set_description(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: Option<&str>,
    column: Option<&str>,
    measure: Option<&str>,
    description: &str,
) -> Result<()> {
    let target = resolve_desc_target(table, column, measure)?;
    let op = "semantic-model set-description";
    let parts = fetch_parts(client, workspace, id, op).await?;
    let (new_parts, label) = apply_set_description(&parts, &target, description)?;

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "target": label, "description": description }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "description_set", "id": id, "target": label }),
        "status",
    );
    Ok(())
}

fn resolve_desc_target(
    table: Option<&str>,
    column: Option<&str>,
    measure: Option<&str>,
) -> Result<DescTarget> {
    match (table, column, measure) {
        (_, _, Some(m)) => Ok(DescTarget::Measure(m.to_string())),
        (Some(t), Some(c), None) => Ok(DescTarget::Column {
            table: t.to_string(),
            column: c.to_string(),
        }),
        (Some(t), None, None) => Ok(DescTarget::Table(t.to_string())),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Specify the object to describe.".to_string(),
            "Use --table <T> (a table), --table <T> --column <C> (a column), or --measure <M>."
                .to_string(),
        )
        .into()),
    }
}

/// Apply the description edit to the definition parts. Returns the new parts and
/// a human label for the target. Errors if the object is not found.
pub(super) fn apply_set_description(
    parts: &[(String, String)],
    target: &DescTarget,
    description: &str,
) -> Result<(Vec<(String, String)>, String)> {
    if let Some((_, bim)) = parts.iter().find(|(p, _)| p == "model.bim") {
        let (new_bim, label) = set_description_bim(bim, target, description)?;
        let out = replace_part(parts, "model.bim", &new_bim);
        return Ok((out, label));
    }
    // TMDL: locate the owning table file, then edit it.
    let (idx, keyword, name, indent, label) = match target {
        DescTarget::Table(t) => (
            find_table_file(parts, t)?,
            "table",
            t.clone(),
            0usize,
            t.clone(),
        ),
        DescTarget::Column { table, column } => (
            find_table_file(parts, table)?,
            "column",
            column.clone(),
            1usize,
            format!("{table}[{column}]"),
        ),
        DescTarget::Measure(m) => (
            find_measure_file(parts, m)?,
            "measure",
            m.clone(),
            1usize,
            m.clone(),
        ),
    };
    let (new_content, changed) =
        tmdl_set_description(&parts[idx].1, keyword, &name, indent, description);
    if !changed {
        bail!("Could not find {label} in the model definition.");
    }
    let mut out = parts.to_vec();
    out[idx].1 = new_content;
    Ok((out, label))
}

/// Set/replace the `///` description comment(s) before the `<keyword> <name>`
/// declaration at `indent`. Returns `(new_content, changed)`.
fn tmdl_set_description(
    content: &str,
    keyword: &str,
    target_name: &str,
    indent: usize,
    description: &str,
) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 2);
    let mut changed = false;
    for line in lines {
        if tab_indent(line) == indent
            && let Some(name) = decl_name(line.trim_start_matches('\t'), keyword)
            && name == target_name
        {
            // Drop any already-emitted `///` comment lines at this indent.
            while out
                .last()
                .is_some_and(|l| tab_indent(l) == indent && l.trim_start().starts_with("///"))
            {
                out.pop();
            }
            let prefix = "\t".repeat(indent);
            for dl in description.split('\n') {
                out.push(format!("{prefix}/// {}", dl.trim_end()));
            }
            out.push(line.to_string());
            changed = true;
            continue;
        }
        out.push(line.to_string());
    }
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    (result, changed)
}

fn set_description_bim(
    bim: &str,
    target: &DescTarget,
    description: &str,
) -> Result<(String, String)> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let tables_mut = j
        .get_mut("model")
        .and_then(|m| m.get_mut("tables"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no tables"))?;
    let label = match target {
        DescTarget::Table(t) => {
            let tbl = tables_mut
                .iter_mut()
                .find(|x| x.get("name").and_then(Value::as_str) == Some(t.as_str()))
                .ok_or_else(|| FabioError::invalid_input(format!("Table '{t}' not found")))?;
            tbl["description"] = Value::from(description);
            t.clone()
        }
        DescTarget::Column { table, column } => {
            let tbl = tables_mut
                .iter_mut()
                .find(|x| x.get("name").and_then(Value::as_str) == Some(table.as_str()))
                .ok_or_else(|| FabioError::invalid_input(format!("Table '{table}' not found")))?;
            let col = tbl
                .get_mut("columns")
                .and_then(Value::as_array_mut)
                .and_then(|cols| {
                    cols.iter_mut()
                        .find(|c| c.get("name").and_then(Value::as_str) == Some(column.as_str()))
                })
                .ok_or_else(|| {
                    FabioError::invalid_input(format!("Column '{table}[{column}]' not found"))
                })?;
            col["description"] = Value::from(description);
            format!("{table}[{column}]")
        }
        DescTarget::Measure(m) => {
            let found = tables_mut.iter_mut().find_map(|t| {
                t.get_mut("measures")
                    .and_then(Value::as_array_mut)
                    .and_then(|ms| {
                        ms.iter_mut()
                            .find(|x| x.get("name").and_then(Value::as_str) == Some(m.as_str()))
                    })
            });
            let meas = found
                .ok_or_else(|| FabioError::invalid_input(format!("Measure '{m}' not found")))?;
            meas["description"] = Value::from(description);
            m.clone()
        }
    };
    Ok((
        serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()),
        label,
    ))
}

// ── add-measure / update-measure ──────────────────────────────────────────────

pub(super) struct MeasureFields<'a> {
    pub expression: Option<&'a str>,
    pub description: Option<&'a str>,
    pub format_string: Option<&'a str>,
    pub display_folder: Option<&'a str>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn add_measure(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    name: &str,
    fields: &MeasureFields<'_>,
) -> Result<()> {
    let op = "semantic-model add-measure";
    let expr = fields.expression.ok_or_else(|| {
        FabioError::invalid_input("--expression is required to add a measure".to_string())
    })?;
    let parts = fetch_parts(client, workspace, id, op).await?;

    // Reject a duplicate measure name (model-unique).
    if measure_exists(&parts, name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("A measure named '{name}' already exists."),
            "Use `semantic-model update-measure` to change it, or pick a different name."
                .to_string(),
        )
        .into());
    }

    let new_parts = if let Some((_, bim)) = parts.iter().find(|(p, _)| p == "model.bim") {
        let new_bim = add_measure_bim(bim, table, name, expr, fields)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, table)?;
        let new_content = add_measure_tmdl(&parts[idx].1, name, expr, fields);
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": table, "measure": name, "expression": expr }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "measure_added", "id": id, "table": table, "measure": name }),
        "status",
    );
    Ok(())
}

pub(super) async fn update_measure(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    measure: &str,
    fields: &MeasureFields<'_>,
) -> Result<()> {
    let op = "semantic-model update-measure";
    if fields.expression.is_none()
        && fields.description.is_none()
        && fields.format_string.is_none()
        && fields.display_folder.is_none()
    {
        return Err(FabioError::invalid_input(
            "Provide at least one of --expression / --description / --format-string / --display-folder"
                .to_string(),
        )
        .into());
    }
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some((_, bim)) = parts.iter().find(|(p, _)| p == "model.bim") {
        let new_bim = update_measure_bim(bim, measure, fields)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_measure_file(&parts, measure)?;
        let (new_content, changed) = update_measure_tmdl(&parts[idx].1, measure, fields);
        if !changed {
            bail!("Could not find measure '{measure}' in the model definition.");
        }
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "measure": measure }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "measure_updated", "id": id, "measure": measure }),
        "status",
    );
    Ok(())
}

// ── delete / rename / move measure ────────────────────────────────────────────

pub(super) async fn delete_measure(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    measure: &str,
) -> Result<()> {
    let op = "semantic-model delete-measure";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some((_, bim)) = parts.iter().find(|(p, _)| p == "model.bim") {
        let new_bim = delete_measure_bim(bim, measure)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_measure_file(&parts, measure)?;
        let (new_content, removed) = delete_measure_tmdl(&parts[idx].1, measure);
        if !removed {
            bail!("Could not find measure '{measure}' in the model definition.");
        }
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "measure": measure }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "measure_deleted", "id": id, "measure": measure }),
        "status",
    );
    Ok(())
}

pub(super) async fn rename_measure(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    measure: &str,
    new_name: &str,
) -> Result<()> {
    let op = "semantic-model rename-measure";
    if measure == new_name {
        return Err(
            FabioError::invalid_input("--new-name must differ from --measure".to_string()).into(),
        );
    }
    let parts = fetch_parts(client, workspace, id, op).await?;
    if measure_exists(&parts, new_name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("A measure named '{new_name}' already exists."),
            "Pick a different --new-name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some((_, bim)) = parts.iter().find(|(p, _)| p == "model.bim") {
        let new_bim = rename_measure_bim(bim, measure, new_name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_measure_file(&parts, measure)?;
        let (new_content, renamed) = rename_measure_tmdl(&parts[idx].1, measure, new_name);
        if !renamed {
            bail!("Could not find measure '{measure}' in the model definition.");
        }
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "measure": measure, "newName": new_name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "measure_renamed", "id": id, "measure": measure, "newName": new_name }),
        "status",
    );
    Ok(())
}

pub(super) async fn move_measure(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    measure: &str,
    to_table: &str,
) -> Result<()> {
    let op = "semantic-model move-measure";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some((_, bim)) = parts.iter().find(|(p, _)| p == "model.bim") {
        let new_bim = move_measure_bim(bim, measure, to_table)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let src_idx = find_measure_file(&parts, measure)?;
        let dst_idx = find_table_file(&parts, to_table)?;
        if src_idx == dst_idx {
            return Err(FabioError::invalid_input(format!(
                "Measure '{measure}' is already in table '{to_table}'."
            ))
            .into());
        }
        let (block, remaining) = extract_measure_block(&parts[src_idx].1, measure)
            .ok_or_else(|| FabioError::not_found(format!("Measure '{measure}' not found")))?;
        let dst_new = insert_table_child_lines(&parts[dst_idx].1, &block);
        let mut out = parts.clone();
        out[src_idx].1 = remaining;
        out[dst_idx].1 = dst_new;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "measure": measure, "toTable": to_table }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "measure_moved", "id": id, "measure": measure, "toTable": to_table }),
        "status",
    );
    Ok(())
}

/// Build the property lines (`formatString`, `displayFolder`) for a measure at
/// indent 2. `///` description is emitted separately (it precedes the measure).
fn measure_property_lines(fields: &MeasureFields) -> String {
    let mut s = String::new();
    if let Some(fs) = fields.format_string.filter(|x| !x.is_empty()) {
        let _ = writeln!(s, "\t\tformatString: {fs}");
    }
    if let Some(df) = fields.display_folder.filter(|x| !x.is_empty()) {
        let _ = writeln!(s, "\t\tdisplayFolder: {df}");
    }
    s
}

/// Render a measure's DAX expression onto the `measure … =` line: inline for a
/// single line, or a multi-line block (indent 2) for multi-line DAX.
fn render_measure_expr(expr: &str) -> String {
    let expr = expr.trim();
    if expr.contains('\n') {
        let mut body = String::new();
        for l in expr.lines() {
            let _ = writeln!(body, "\t\t{}", l.trim_end());
        }
        format!(" =\n{body}")
    } else {
        format!(" = {expr}")
    }
}

/// Build the measure's TMDL lines and insert them into the table file.
fn add_measure_tmdl(content: &str, name: &str, expr: &str, fields: &MeasureFields) -> String {
    let mut mlines: Vec<String> = Vec::new();
    if let Some(d) = fields.description.filter(|x| !x.is_empty()) {
        for dl in d.split('\n') {
            mlines.push(format!("\t/// {}", dl.trim_end()));
        }
    }
    let decl = format!("\tmeasure '{name}'{}", render_measure_expr(expr));
    for l in decl.trim_end().split('\n') {
        mlines.push(l.to_string());
    }
    for l in measure_property_lines(fields).lines() {
        mlines.push(l.to_string());
    }
    insert_table_child_lines(content, &mlines)
}

/// The line span of a measure block (delegates to the shared `child_span`).
fn measure_span(lines: &[&str], measure: &str) -> Option<(usize, usize)> {
    super::tmdl::child_span(lines, "measure", measure)
}

/// Remove a measure block from a table file. Returns `(new_content, removed)`.
fn delete_measure_tmdl(content: &str, measure: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = measure_span(&lines, measure) else {
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

/// Extract a measure block (its lines) and return the block plus the remaining
/// table content with the block removed.
fn extract_measure_block(content: &str, measure: &str) -> Option<(Vec<String>, String)> {
    let lines: Vec<&str> = content.lines().collect();
    let (start, end) = measure_span(&lines, measure)?;
    let block: Vec<String> = lines[start..end].iter().map(|s| (*s).to_string()).collect();
    let (remaining, _) = delete_measure_tmdl(content, measure);
    Some((block, remaining))
}

/// Rename a measure's declaration in place (references are NOT rewritten).
/// Returns `(new_content, renamed)`.
fn rename_measure_tmdl(content: &str, old: &str, new: &str) -> (String, bool) {
    let mut renamed = false;
    let mut out: Vec<String> = Vec::new();
    for line in content.lines() {
        if !renamed
            && tab_indent(line) == 1
            && decl_name(line.trim_start_matches('\t'), "measure").as_deref() == Some(old)
        {
            // Preserve everything from the `=` onward.
            let after = line.trim_start().strip_prefix("measure ").unwrap_or("");
            let rest = after.find('=').map_or("", |i| &after[i..]);
            out.push(format!(
                "\tmeasure {} {}",
                super::tmdl::quote_tmdl_name(new),
                rest
            ));
            renamed = true;
        } else {
            out.push(line.to_string());
        }
    }
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    (result, renamed)
}

/// Known TMDL measure sub-property keywords (indent 2), used to separate the
/// measure's expression body from its properties when replacing the expression.
fn is_measure_property_line(line: &str) -> bool {
    let t = line.trim_start();
    [
        "formatString:",
        "displayFolder:",
        "lineageTag:",
        "isHidden",
        "annotation ",
        "changedProperty ",
        "formatStringDefinition",
        "dataType:",
        "detailRowsDefinition",
    ]
    .iter()
    .any(|k| t.starts_with(k))
}

/// Update a measure in place: replace its expression (preserving properties)
/// and/or set description / formatString / displayFolder. Returns `(new, changed)`.
fn update_measure_tmdl(content: &str, measure: &str, fields: &MeasureFields) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut changed = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if tab_indent(line) == 1
            && let Some(name) = decl_name(line.trim_start_matches('\t'), "measure")
            && name == measure
        {
            // Collect the whole measure block: decl + following indent>=2 lines.
            let mut j = i + 1;
            let mut body: Vec<&str> = Vec::new();
            while j < lines.len() && (lines[j].trim().is_empty() || tab_indent(lines[j]) >= 2) {
                body.push(lines[j]);
                j += 1;
            }
            // Separate expression-continuation lines from property lines.
            let props: Vec<&str> = body
                .iter()
                .copied()
                .filter(|l| is_measure_property_line(l))
                .collect();

            // Description (replace preceding `///` lines).
            if let Some(d) = fields.description {
                while out
                    .last()
                    .is_some_and(|l| tab_indent(l) == 1 && l.trim_start().starts_with("///"))
                {
                    out.pop();
                }
                for dl in d.split('\n') {
                    out.push(format!("\t/// {}", dl.trim_end()));
                }
            }

            // Measure decl line: new expression, or keep the original.
            if let Some(expr) = fields.expression {
                out.push(
                    format!("\tmeasure '{measure}'{}", render_measure_expr(expr))
                        .trim_end_matches('\n')
                        .to_string(),
                );
            } else {
                out.push(line.to_string());
                // Keep original expression-continuation lines (non-property body).
                for b in &body {
                    if !is_measure_property_line(b) {
                        out.push((*b).to_string());
                    }
                }
            }

            // Re-emit properties, applying overrides for formatString/displayFolder.
            let mut saw_fs = false;
            let mut saw_df = false;
            for p in &props {
                let t = p.trim_start();
                if let Some(fs) = fields
                    .format_string
                    .filter(|_| t.starts_with("formatString:"))
                {
                    out.push(format!("\t\tformatString: {fs}"));
                    saw_fs = true;
                } else if let Some(df) = fields
                    .display_folder
                    .filter(|_| t.starts_with("displayFolder:"))
                {
                    out.push(format!("\t\tdisplayFolder: {df}"));
                    saw_df = true;
                } else {
                    out.push((*p).to_string());
                }
            }
            if let Some(fs) = fields.format_string.filter(|_| !saw_fs) {
                out.push(format!("\t\tformatString: {fs}"));
            }
            if let Some(df) = fields.display_folder.filter(|_| !saw_df) {
                out.push(format!("\t\tdisplayFolder: {df}"));
            }
            changed = true;
            i = j;
            continue;
        }
        out.push(line.to_string());
        i += 1;
    }
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    (result, changed)
}

fn add_measure_bim(
    bim: &str,
    table: &str,
    name: &str,
    expr: &str,
    fields: &MeasureFields,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let tbl = j
        .get_mut("model")
        .and_then(|m| m.get_mut("tables"))
        .and_then(Value::as_array_mut)
        .and_then(|ts| {
            ts.iter_mut()
                .find(|t| t.get("name").and_then(Value::as_str) == Some(table))
        })
        .ok_or_else(|| FabioError::invalid_input(format!("Table '{table}' not found")))?;
    let mut m = serde_json::json!({ "name": name, "expression": expr });
    if let Some(d) = fields.description.filter(|x| !x.is_empty()) {
        m["description"] = Value::from(d);
    }
    if let Some(fs) = fields.format_string.filter(|x| !x.is_empty()) {
        m["formatString"] = Value::from(fs);
    }
    if let Some(df) = fields.display_folder.filter(|x| !x.is_empty()) {
        m["displayFolder"] = Value::from(df);
    }
    tbl.as_object_mut()
        .unwrap()
        .entry("measures")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .unwrap()
        .push(m);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn update_measure_bim(bim: &str, measure: &str, fields: &MeasureFields) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let tables = j
        .get_mut("model")
        .and_then(|m| m.get_mut("tables"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no tables"))?;
    let meas = tables
        .iter_mut()
        .find_map(|t| {
            t.get_mut("measures")
                .and_then(Value::as_array_mut)
                .and_then(|ms| {
                    ms.iter_mut()
                        .find(|x| x.get("name").and_then(Value::as_str) == Some(measure))
                })
        })
        .ok_or_else(|| FabioError::invalid_input(format!("Measure '{measure}' not found")))?;
    if let Some(e) = fields.expression {
        meas["expression"] = Value::from(e);
    }
    if let Some(d) = fields.description {
        meas["description"] = Value::from(d);
    }
    if let Some(fs) = fields.format_string {
        meas["formatString"] = Value::from(fs);
    }
    if let Some(df) = fields.display_folder {
        meas["displayFolder"] = Value::from(df);
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn bim_tables_mut(j: &mut Value) -> Result<&mut Vec<Value>> {
    j.get_mut("model")
        .and_then(|m| m.get_mut("tables"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no tables").into())
}

fn delete_measure_bim(bim: &str, measure: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let mut removed = false;
    for t in bim_tables_mut(&mut j)? {
        if let Some(ms) = t.get_mut("measures").and_then(Value::as_array_mut) {
            let before = ms.len();
            ms.retain(|x| x.get("name").and_then(Value::as_str) != Some(measure));
            if ms.len() != before {
                removed = true;
            }
        }
    }
    if !removed {
        return Err(FabioError::not_found(format!("Measure '{measure}' not found")).into());
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn rename_measure_bim(bim: &str, measure: &str, new_name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let meas = bim_tables_mut(&mut j)?
        .iter_mut()
        .find_map(|t| {
            t.get_mut("measures")
                .and_then(Value::as_array_mut)
                .and_then(|ms| {
                    ms.iter_mut()
                        .find(|x| x.get("name").and_then(Value::as_str) == Some(measure))
                })
        })
        .ok_or_else(|| FabioError::not_found(format!("Measure '{measure}' not found")))?;
    meas["name"] = Value::from(new_name);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn move_measure_bim(bim: &str, measure: &str, to_table: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    // Extract the measure object from its current table.
    let mut extracted: Option<Value> = None;
    for t in bim_tables_mut(&mut j)? {
        if let Some(ms) = t.get_mut("measures").and_then(Value::as_array_mut)
            && let Some(pos) = ms
                .iter()
                .position(|x| x.get("name").and_then(Value::as_str) == Some(measure))
        {
            extracted = Some(ms.remove(pos));
            break;
        }
    }
    let m =
        extracted.ok_or_else(|| FabioError::not_found(format!("Measure '{measure}' not found")))?;
    // Insert into the destination table.
    let dst = bim_tables_mut(&mut j)?
        .iter_mut()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(to_table))
        .ok_or_else(|| FabioError::not_found(format!("Table '{to_table}' not found")))?;
    dst.as_object_mut()
        .unwrap()
        .entry("measures")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .unwrap()
        .push(m);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

// ── lookup helpers ────────────────────────────────────────────────────────────

fn find_measure_file(parts: &[(String, String)], measure: &str) -> Result<usize> {
    parts
        .iter()
        .position(|(p, c)| is_table_tmdl(p) && tmdl_has_measure(c, measure))
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                format!("Measure '{measure}' not found in the model definition."),
                "List measures with `fabio semantic-model list-measures`.".to_string(),
            )
            .into()
        })
}

fn tmdl_has_measure(content: &str, measure: &str) -> bool {
    content.lines().any(|l| {
        tab_indent(l) == 1
            && decl_name(l.trim_start_matches('\t'), "measure").as_deref() == Some(measure)
    })
}

fn measure_exists(parts: &[(String, String)], measure: &str) -> bool {
    if let Some((_, bim)) = parts.iter().find(|(p, _)| p == "model.bim") {
        if let Ok(j) = serde_json::from_str::<Value>(bim) {
            return j
                .get("model")
                .and_then(|m| m.get("tables"))
                .and_then(Value::as_array)
                .is_some_and(|ts| {
                    ts.iter().any(|t| {
                        t.get("measures")
                            .and_then(Value::as_array)
                            .is_some_and(|ms| {
                                ms.iter()
                                    .any(|x| x.get("name").and_then(Value::as_str) == Some(measure))
                            })
                    })
                });
        }
        return false;
    }
    parts
        .iter()
        .any(|(p, c)| is_table_tmdl(p) && tmdl_has_measure(c, measure))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sales_tmdl() -> String {
        "/// The sales table\ntable Sales\n\n\t/// Existing measure\n\tmeasure 'Total' = SUM('Sales'[Amount])\n\t\tformatString: 0.00\n\n\tcolumn Amount\n\t\tdataType: double\n\t\tsourceColumn: Amount\n\n\tpartition p = m\n\t\tsource = let x = 1 in x\n".to_string()
    }

    #[test]
    fn set_description_on_column_inserts_comment() {
        let (out, changed) =
            tmdl_set_description(&sales_tmdl(), "column", "Amount", 1, "The USD amount");
        assert!(changed);
        assert!(out.contains("\t/// The USD amount\n\tcolumn Amount"));
    }

    #[test]
    fn set_description_replaces_existing_measure_comment() {
        let (out, changed) =
            tmdl_set_description(&sales_tmdl(), "measure", "Total", 1, "Grand total");
        assert!(changed);
        assert!(out.contains("\t/// Grand total\n\tmeasure 'Total'"));
        assert!(!out.contains("Existing measure"));
    }

    #[test]
    fn set_description_on_table() {
        let (out, changed) =
            tmdl_set_description(&sales_tmdl(), "table", "Sales", 0, "Sales facts");
        assert!(changed);
        assert!(out.contains("/// Sales facts\ntable Sales"));
        assert!(!out.contains("The sales table"));
    }

    #[test]
    fn set_description_unknown_object_is_noop() {
        let (_out, changed) = tmdl_set_description(&sales_tmdl(), "column", "Nope", 1, "x");
        assert!(!changed);
    }

    #[test]
    fn add_measure_tmdl_inserts_block_after_table() {
        let f = MeasureFields {
            expression: Some("AVERAGE('Sales'[Amount])"),
            description: Some("Average amount"),
            format_string: Some("0.00"),
            display_folder: Some("Averages"),
        };
        let out = add_measure_tmdl(&sales_tmdl(), "Avg Amount", "AVERAGE('Sales'[Amount])", &f);
        assert!(
            out.contains("\t/// Average amount\n\tmeasure 'Avg Amount' = AVERAGE('Sales'[Amount])")
        );
        assert!(out.contains("\t\tformatString: 0.00"));
        assert!(out.contains("\t\tdisplayFolder: Averages"));
    }

    #[test]
    fn add_measure_multiline_expression() {
        let f = MeasureFields {
            expression: None,
            description: None,
            format_string: None,
            display_folder: None,
        };
        let out = add_measure_tmdl(&sales_tmdl(), "M", "VAR x = 1\nRETURN x", &f);
        assert!(out.contains("\tmeasure 'M' =\n\t\tVAR x = 1\n\t\tRETURN x"));
    }

    #[test]
    fn add_measure_after_table_scalar_properties() {
        // A table whose declaration is immediately followed by its own scalar
        // property (lineageTag) — the measure MUST go after it, not between the
        // `table` line and the property (which is invalid TMDL).
        let tmdl = "table T\n\tlineageTag: abc-123\n\n\tcolumn ID\n\t\tdataType: int64\n\t\tsourceColumn: ID\n";
        let f = MeasureFields {
            expression: Some("COUNTROWS('T')"),
            description: None,
            format_string: None,
            display_folder: None,
        };
        let out = add_measure_tmdl(tmdl, "Rows", "COUNTROWS('T')", &f);
        let lt = out.find("lineageTag: abc-123").unwrap();
        let meas = out.find("measure 'Rows'").unwrap();
        let col = out.find("column ID").unwrap();
        assert!(lt < meas, "measure must come after the table's lineageTag");
        assert!(meas < col, "measure should come before the columns");
    }

    #[test]
    fn update_measure_replaces_expression_and_keeps_properties() {
        let f = MeasureFields {
            expression: Some("SUM('Sales'[Amount]) * 2"),
            description: None,
            format_string: None,
            display_folder: None,
        };
        let (out, changed) = update_measure_tmdl(&sales_tmdl(), "Total", &f);
        assert!(changed);
        assert!(out.contains("\tmeasure 'Total' = SUM('Sales'[Amount]) * 2"));
        assert!(out.contains("\t\tformatString: 0.00")); // property preserved
        assert!(!out.contains("SUM('Sales'[Amount])\n\t\tformatString")); // old expr gone
    }

    #[test]
    fn update_measure_sets_format_string() {
        let f = MeasureFields {
            expression: None,
            description: Some("New desc"),
            format_string: Some("$#,0"),
            display_folder: None,
        };
        let (out, changed) = update_measure_tmdl(&sales_tmdl(), "Total", &f);
        assert!(changed);
        assert!(out.contains("\t/// New desc\n\tmeasure 'Total'"));
        assert!(out.contains("\t\tformatString: $#,0"));
        assert!(!out.contains("formatString: 0.00"));
    }

    #[test]
    fn measure_exists_detects_tmdl_measure() {
        let parts = vec![("definition/tables/Sales.tmdl".to_string(), sales_tmdl())];
        assert!(measure_exists(&parts, "Total"));
        assert!(!measure_exists(&parts, "Nope"));
    }

    #[test]
    fn delete_measure_removes_block_and_comment() {
        let (out, removed) = delete_measure_tmdl(&sales_tmdl(), "Total");
        assert!(removed);
        assert!(!out.contains("measure 'Total'"));
        assert!(!out.contains("Existing measure")); // its `///` comment gone too
        assert!(out.contains("column Amount")); // rest intact
        assert!(out.contains("partition p"));
    }

    #[test]
    fn delete_measure_missing_is_noop() {
        let (_out, removed) = delete_measure_tmdl(&sales_tmdl(), "Nope");
        assert!(!removed);
    }

    #[test]
    fn rename_measure_changes_decl_only() {
        let (out, renamed) = rename_measure_tmdl(&sales_tmdl(), "Total", "Grand Total");
        assert!(renamed);
        assert!(out.contains("measure 'Grand Total' = SUM('Sales'[Amount])"));
        assert!(!out.contains("measure 'Total'"));
        assert!(out.contains("formatString: 0.00")); // properties preserved
    }

    #[test]
    fn extract_measure_block_captures_and_removes() {
        let (block, remaining) = extract_measure_block(&sales_tmdl(), "Total").unwrap();
        assert!(block.iter().any(|l| l.contains("measure 'Total'")));
        assert!(block.iter().any(|l| l.contains("/// Existing measure")));
        assert!(block.iter().any(|l| l.contains("formatString: 0.00")));
        assert!(!remaining.contains("measure 'Total'"));
        // Re-insert into another table body keeps it valid.
        let dst = "table Other\n\tlineageTag: x\n\n\tcolumn C\n\t\tdataType: string\n\t\tsourceColumn: C\n";
        let moved = insert_table_child_lines(dst, &block);
        assert!(moved.contains("measure 'Total'"));
        let lt = moved.find("lineageTag: x").unwrap();
        let meas = moved.find("measure 'Total'").unwrap();
        assert!(lt < meas, "measure must land after the table's lineageTag");
    }

    #[test]
    fn bim_delete_rename_move_measure() {
        let bim = r#"{"model":{"tables":[{"name":"Sales","measures":[{"name":"Total","expression":"1"},{"name":"Avg","expression":"2"}]},{"name":"Dim","measures":[]}]}}"#;
        // delete
        let d = delete_measure_bim(bim, "Avg").unwrap();
        let jd: Value = serde_json::from_str(&d).unwrap();
        assert_eq!(
            jd["model"]["tables"][0]["measures"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // rename
        let r = rename_measure_bim(bim, "Total", "Sum Total").unwrap();
        let jr: Value = serde_json::from_str(&r).unwrap();
        assert_eq!(jr["model"]["tables"][0]["measures"][0]["name"], "Sum Total");
        // move Total from Sales to Dim
        let m = move_measure_bim(bim, "Total", "Dim").unwrap();
        let jm: Value = serde_json::from_str(&m).unwrap();
        assert_eq!(
            jm["model"]["tables"][0]["measures"]
                .as_array()
                .unwrap()
                .len(),
            1
        ); // Sales now has just Avg
        assert_eq!(jm["model"]["tables"][1]["measures"][0]["name"], "Total"); // Dim gained Total
    }

    #[test]
    fn bim_set_description_and_add_measure() {
        let bim = r#"{"model":{"tables":[{"name":"Sales","columns":[{"name":"Amount"}],"measures":[{"name":"Total","expression":"SUM('Sales'[Amount])"}]}]}}"#;
        let parts = vec![("model.bim".to_string(), bim.to_string())];
        // set description on a column
        let (np, label) = apply_set_description(
            &parts,
            &DescTarget::Column {
                table: "Sales".into(),
                column: "Amount".into(),
            },
            "USD",
        )
        .unwrap();
        assert_eq!(label, "Sales[Amount]");
        let j: Value = serde_json::from_str(&np[0].1).unwrap();
        assert_eq!(j["model"]["tables"][0]["columns"][0]["description"], "USD");
        // add a measure
        let f = MeasureFields {
            expression: Some("AVERAGE('Sales'[Amount])"),
            description: None,
            format_string: None,
            display_folder: None,
        };
        let nb = add_measure_bim(bim, "Sales", "Avg", "AVERAGE('Sales'[Amount])", &f).unwrap();
        let j2: Value = serde_json::from_str(&nb).unwrap();
        let ms = j2["model"]["tables"][0]["measures"].as_array().unwrap();
        assert_eq!(ms.len(), 2);
        assert!(ms.iter().any(|m| m["name"] == "Avg"));
    }
}
