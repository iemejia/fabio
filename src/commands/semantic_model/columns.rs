//! `semantic-model` column authoring — `add-calculated-column`, `delete-column`,
//! `rename-column`, `update-column`.
//!
//! Columns live inside a table's `definition/tables/<T>.tmdl` file as
//! `column <name>` blocks (a data column has `dataType:`/`sourceColumn:`; a
//! calculated column carries `= <DAX>` on its declaration line). fabio edits
//! them via the shared definition read-modify-write (no XMLA/TOM). Renaming a
//! column does NOT rewrite DAX/relationship references (documented).

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
};

/// Properties `add`/`update` can set on a column.
#[derive(Default)]
pub(super) struct ColumnProps<'a> {
    pub data_type: Option<&'a str>,
    pub format_string: Option<&'a str>,
    pub summarize_by: Option<&'a str>,
    pub display_folder: Option<&'a str>,
    pub description: Option<&'a str>,
    pub hidden: Option<bool>,
}

fn normalize_data_type(v: &str) -> Result<&'static str> {
    match v.to_ascii_lowercase().as_str() {
        "string" | "text" => Ok("string"),
        "int64" | "integer" | "int" | "long" | "bigint" => Ok("int64"),
        "double" | "float" | "number" => Ok("double"),
        "decimal" | "currency" => Ok("decimal"),
        "datetime" | "date" => Ok("dateTime"),
        "boolean" | "bool" => Ok("boolean"),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid data type '{v}'."),
            "Valid values: string, int64, double, decimal, dateTime, boolean.".to_string(),
        )
        .into()),
    }
}

fn normalize_summarize_by(v: &str) -> Result<&'static str> {
    match v.to_ascii_lowercase().as_str() {
        "none" => Ok("none"),
        "sum" => Ok("sum"),
        "count" => Ok("count"),
        "min" => Ok("min"),
        "max" => Ok("max"),
        "average" | "avg" => Ok("average"),
        "distinctcount" => Ok("distinctCount"),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --summarize-by '{v}'."),
            "Valid values: none, sum, count, min, max, average, distinctCount.".to_string(),
        )
        .into()),
    }
}

// ── add-calculated-column ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn add_calculated_column(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    name: &str,
    expression: &str,
    props: &ColumnProps<'_>,
) -> Result<()> {
    let op = "semantic-model add-calculated-column";
    let data_type = props.data_type.map(normalize_data_type).transpose()?;
    let summarize_by = props.summarize_by.map(normalize_summarize_by).transpose()?;
    let parts = fetch_parts(client, workspace, id, op).await?;

    if column_exists(&parts, table, name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("Column '{table}[{name}]' already exists."),
            "Use `update-column`, or pick a different name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_calculated_column_bim(bim, table, name, expression, data_type, props)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, table)?;
        let block = build_calculated_column_lines(name, expression, data_type, summarize_by, props);
        let new_content = insert_table_child_lines(&parts[idx].1, &block);
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": table, "column": name, "expression": expression }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "column_added", "id": id, "table": table, "column": name }),
        "status",
    );
    Ok(())
}

fn build_calculated_column_lines(
    name: &str,
    expression: &str,
    data_type: Option<&str>,
    summarize_by: Option<&str>,
    props: &ColumnProps<'_>,
) -> Vec<String> {
    let mut block: Vec<String> = Vec::new();
    if let Some(d) = props.description.filter(|x| !x.is_empty()) {
        for dl in d.split('\n') {
            block.push(format!("\t/// {}", dl.trim_end()));
        }
    }
    block.push(format!(
        "\tcolumn {} = {}",
        quote_tmdl_name(name),
        expression.trim()
    ));
    // dataType is recommended for a calculated column.
    block.push(format!("\t\tdataType: {}", data_type.unwrap_or("string")));
    if let Some(fs) = props.format_string.filter(|x| !x.is_empty()) {
        block.push(format!("\t\tformatString: {fs}"));
    }
    if let Some(sb) = summarize_by {
        block.push(format!("\t\tsummarizeBy: {sb}"));
    }
    if let Some(df) = props.display_folder.filter(|x| !x.is_empty()) {
        block.push(format!("\t\tdisplayFolder: {df}"));
    }
    if props.hidden == Some(true) {
        block.push("\t\tisHidden".to_string());
    }
    block
}

// ── delete-column ─────────────────────────────────────────────────────────────

pub(super) async fn delete_column(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model delete-column";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_column_bim(bim, table, name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, table)?;
        let (new_content, removed) = delete_column_tmdl(&parts[idx].1, name);
        if !removed {
            return Err(FabioError::not_found(format!(
                "Column '{table}[{name}]' not found in the model definition."
            ))
            .into());
        }
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": table, "column": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "column_deleted", "id": id, "table": table, "column": name }),
        "status",
    );
    Ok(())
}

// ── rename-column ─────────────────────────────────────────────────────────────

pub(super) async fn rename_column(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    name: &str,
    new_name: &str,
) -> Result<()> {
    let op = "semantic-model rename-column";
    if name == new_name {
        return Err(
            FabioError::invalid_input("--new-name must differ from --name".to_string()).into(),
        );
    }
    let parts = fetch_parts(client, workspace, id, op).await?;
    if column_exists(&parts, table, new_name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("Column '{table}[{new_name}]' already exists."),
            "Pick a different --new-name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = rename_column_bim(bim, table, name, new_name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, table)?;
        let (new_content, renamed) = rename_column_tmdl(&parts[idx].1, name, new_name);
        if !renamed {
            return Err(FabioError::not_found(format!(
                "Column '{table}[{name}]' not found in the model definition."
            ))
            .into());
        }
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": table, "column": name, "newName": new_name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "column_renamed", "id": id, "table": table, "column": name, "newName": new_name }),
        "status",
    );
    Ok(())
}

// ── update-column ─────────────────────────────────────────────────────────────

pub(super) async fn update_column(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    name: &str,
    props: &ColumnProps<'_>,
) -> Result<()> {
    let op = "semantic-model update-column";
    if props.data_type.is_none()
        && props.format_string.is_none()
        && props.summarize_by.is_none()
        && props.display_folder.is_none()
        && props.description.is_none()
        && props.hidden.is_none()
    {
        return Err(FabioError::invalid_input(
            "Provide at least one of --data-type / --format-string / --summarize-by / --display-folder / --description / --hidden"
                .to_string(),
        )
        .into());
    }
    let data_type = props.data_type.map(normalize_data_type).transpose()?;
    let summarize_by = props.summarize_by.map(normalize_summarize_by).transpose()?;
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = update_column_bim(bim, table, name, data_type, summarize_by, props)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, table)?;
        let (new_content, updated) =
            update_column_tmdl(&parts[idx].1, name, data_type, summarize_by, props);
        if !updated {
            return Err(FabioError::not_found(format!(
                "Column '{table}[{name}]' not found in the model definition."
            ))
            .into());
        }
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": table, "column": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "column_updated", "id": id, "table": table, "column": name }),
        "status",
    );
    Ok(())
}

// ── pure TMDL editors ─────────────────────────────────────────────────────────

fn table_has_column(content: &str, name: &str) -> bool {
    content.lines().any(|l| {
        tab_indent(l) == 1
            && decl_name(l.trim_start_matches('\t'), "column").as_deref() == Some(name)
    })
}

fn column_exists(parts: &[(String, String)], table: &str, name: &str) -> bool {
    if let Some(bim) = part_content(parts, "model.bim") {
        if let Ok(j) = serde_json::from_str::<Value>(bim) {
            return j
                .get("model")
                .and_then(|m| m.get("tables"))
                .and_then(Value::as_array)
                .is_some_and(|ts| {
                    ts.iter()
                        .filter(|t| t.get("name").and_then(Value::as_str) == Some(table))
                        .any(|t| {
                            t.get("columns")
                                .and_then(Value::as_array)
                                .is_some_and(|cols| {
                                    cols.iter().any(|c| {
                                        c.get("name").and_then(Value::as_str) == Some(name)
                                    })
                                })
                        })
                });
        }
        return false;
    }
    parts.iter().any(|(p, c)| {
        is_table_tmdl(p)
            && super::tmdl::tmdl_table_name(c).as_deref() == Some(table)
            && table_has_column(c, name)
    })
}

fn delete_column_tmdl(content: &str, name: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = child_span(&lines, "column", name) else {
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

fn rename_column_tmdl(content: &str, old: &str, new: &str) -> (String, bool) {
    let mut renamed = false;
    let mut out: Vec<String> = Vec::new();
    for line in content.lines() {
        if !renamed
            && tab_indent(line) == 1
            && decl_name(line.trim_start_matches('\t'), "column").as_deref() == Some(old)
        {
            let after = line.trim_start().strip_prefix("column ").unwrap_or("");
            // Preserve a calculated column's ` = expr` remainder if present.
            let rest = after
                .find('=')
                .map_or(String::new(), |i| format!(" {}", &after[i..]));
            out.push(format!("\tcolumn {}{}", quote_tmdl_name(new), rest));
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

fn is_column_managed_prop(line: &str, props: &ColumnProps<'_>, data_type: Option<&str>) -> bool {
    let t = line.trim_start();
    (data_type.is_some() && t.starts_with("dataType:"))
        || (props.format_string.is_some() && t.starts_with("formatString:"))
        || (props.summarize_by.is_some() && t.starts_with("summarizeBy:"))
        || (props.display_folder.is_some() && t.starts_with("displayFolder:"))
        || (props.hidden.is_some() && (t.starts_with("isHidden")))
}

fn update_column_tmdl(
    content: &str,
    name: &str,
    data_type: Option<&str>,
    summarize_by: Option<&str>,
    props: &ColumnProps<'_>,
) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = child_span(&lines, "column", name) else {
        return (content.to_string(), false);
    };
    // The decl line is the first non-comment line at indent 1 in the span.
    let decl_idx = (start..end)
        .find(|&i| tab_indent(lines[i]) == 1 && !lines[i].trim_start().starts_with("///"))
        .unwrap_or(start);

    let mut new_block: Vec<String> = Vec::new();
    // Description (replace leading `///` comments if a new one is given).
    if let Some(d) = props.description {
        for dl in d.split('\n') {
            new_block.push(format!("\t/// {}", dl.trim_end()));
        }
    } else {
        for l in &lines[start..decl_idx] {
            new_block.push((*l).to_string());
        }
    }
    new_block.push(lines[decl_idx].to_string());

    // Re-emit existing property lines, applying overrides; drop managed props we
    // will re-add so a value change replaces cleanly.
    let mut saw = ManagedSeen::default();
    for l in &lines[decl_idx + 1..end] {
        let t = l.trim_start();
        if is_column_managed_prop(l, props, data_type) {
            if let Some(dt) = data_type.filter(|_| t.starts_with("dataType:")) {
                new_block.push(format!("\t\tdataType: {dt}"));
                saw.data_type = true;
            } else if let Some(fs) = props
                .format_string
                .filter(|_| t.starts_with("formatString:"))
            {
                new_block.push(format!("\t\tformatString: {fs}"));
                saw.format_string = true;
            } else if let Some(sb) = summarize_by.filter(|_| t.starts_with("summarizeBy:")) {
                new_block.push(format!("\t\tsummarizeBy: {sb}"));
                saw.summarize_by = true;
            } else if let Some(df) = props
                .display_folder
                .filter(|_| t.starts_with("displayFolder:"))
            {
                new_block.push(format!("\t\tdisplayFolder: {df}"));
                saw.display_folder = true;
            } else if props.hidden.is_some() && t.starts_with("isHidden") {
                if props.hidden == Some(true) {
                    new_block.push("\t\tisHidden".to_string());
                }
                saw.hidden = true;
            }
        } else {
            new_block.push((*l).to_string());
        }
    }
    // Append any managed props that weren't already present.
    if let Some(dt) = data_type.filter(|_| !saw.data_type) {
        new_block.push(format!("\t\tdataType: {dt}"));
    }
    if let Some(fs) = props.format_string.filter(|_| !saw.format_string) {
        new_block.push(format!("\t\tformatString: {fs}"));
    }
    if let Some(sb) = summarize_by.filter(|_| !saw.summarize_by) {
        new_block.push(format!("\t\tsummarizeBy: {sb}"));
    }
    if let Some(df) = props.display_folder.filter(|_| !saw.display_folder) {
        new_block.push(format!("\t\tdisplayFolder: {df}"));
    }
    if props.hidden == Some(true) && !saw.hidden {
        new_block.push("\t\tisHidden".to_string());
    }

    let mut out: Vec<String> = Vec::new();
    out.extend(lines[..start].iter().map(|s| (*s).to_string()));
    out.extend(new_block);
    out.extend(lines[end..].iter().map(|s| (*s).to_string()));
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    (result, true)
}

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
struct ManagedSeen {
    data_type: bool,
    format_string: bool,
    summarize_by: bool,
    display_folder: bool,
    hidden: bool,
}
// ── model.bim editors ─────────────────────────────────────────────────────────

fn bim_column<'a>(j: &'a mut Value, table: &str, name: &str) -> Option<&'a mut Value> {
    j.get_mut("model")
        .and_then(|m| m.get_mut("tables"))
        .and_then(Value::as_array_mut)?
        .iter_mut()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(table))?
        .get_mut("columns")
        .and_then(Value::as_array_mut)?
        .iter_mut()
        .find(|c| c.get("name").and_then(Value::as_str) == Some(name))
}

fn bim_table<'a>(j: &'a mut Value, table: &str) -> Result<&'a mut Value> {
    j.get_mut("model")
        .and_then(|m| m.get_mut("tables"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no tables"))?
        .iter_mut()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(table))
        .ok_or_else(|| FabioError::not_found(format!("Table '{table}' not found")).into())
}

fn add_calculated_column_bim(
    bim: &str,
    table: &str,
    name: &str,
    expression: &str,
    data_type: Option<&str>,
    props: &ColumnProps<'_>,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let mut col = serde_json::json!({
        "name": name,
        "type": "calculated",
        "expression": expression,
        "dataType": data_type.unwrap_or("string"),
    });
    if let Some(fs) = props.format_string.filter(|x| !x.is_empty()) {
        col["formatString"] = Value::from(fs);
    }
    if let Some(df) = props.display_folder.filter(|x| !x.is_empty()) {
        col["displayFolder"] = Value::from(df);
    }
    if let Some(d) = props.description.filter(|x| !x.is_empty()) {
        col["description"] = Value::from(d);
    }
    if props.hidden == Some(true) {
        col["isHidden"] = Value::Bool(true);
    }
    bim_table(&mut j, table)?
        .as_object_mut()
        .unwrap()
        .entry("columns")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .unwrap()
        .push(col);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_column_bim(bim: &str, table: &str, name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let cols = bim_table(&mut j, table)?
        .get_mut("columns")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::not_found(format!("Column '{table}[{name}]' not found")))?;
    let before = cols.len();
    cols.retain(|c| c.get("name").and_then(Value::as_str) != Some(name));
    if cols.len() == before {
        return Err(FabioError::not_found(format!("Column '{table}[{name}]' not found")).into());
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn rename_column_bim(bim: &str, table: &str, name: &str, new_name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let col = bim_column(&mut j, table, name)
        .ok_or_else(|| FabioError::not_found(format!("Column '{table}[{name}]' not found")))?;
    col["name"] = Value::from(new_name);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn update_column_bim(
    bim: &str,
    table: &str,
    name: &str,
    data_type: Option<&str>,
    summarize_by: Option<&str>,
    props: &ColumnProps<'_>,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let col = bim_column(&mut j, table, name)
        .ok_or_else(|| FabioError::not_found(format!("Column '{table}[{name}]' not found")))?;
    if let Some(dt) = data_type {
        col["dataType"] = Value::from(dt);
    }
    if let Some(fs) = props.format_string {
        col["formatString"] = Value::from(fs);
    }
    if let Some(sb) = summarize_by {
        col["summarizeBy"] = Value::from(sb);
    }
    if let Some(df) = props.display_folder {
        col["displayFolder"] = Value::from(df);
    }
    if let Some(d) = props.description {
        col["description"] = Value::from(d);
    }
    if let Some(h) = props.hidden {
        col["isHidden"] = Value::Bool(h);
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_tmdl() -> String {
        "table Sales\n\tlineageTag: t1\n\n\tcolumn Amount\n\t\tdataType: double\n\t\tsourceColumn: Amount\n\n\tcolumn Region\n\t\tdataType: string\n\t\tsourceColumn: Region\n\n\tpartition p = m\n\t\tsource = let x = 1 in x\n".to_string()
    }

    #[test]
    fn add_calculated_column_after_scalar_props() {
        let props = ColumnProps {
            data_type: Some("string"),
            display_folder: Some("Calc"),
            ..Default::default()
        };
        let block = build_calculated_column_lines(
            "Region Upper",
            "UPPER('Sales'[Region])",
            Some("string"),
            None,
            &props,
        );
        let out = insert_table_child_lines(&table_tmdl(), &block);
        assert!(out.contains("\tcolumn 'Region Upper' = UPPER('Sales'[Region])"));
        assert!(out.contains("\t\tdataType: string"));
        assert!(out.contains("\t\tdisplayFolder: Calc"));
        let lt = out.find("lineageTag: t1").unwrap();
        let col = out.find("column 'Region Upper'").unwrap();
        assert!(
            lt < col,
            "new column must land after the table's scalar props"
        );
    }

    #[test]
    fn delete_column_removes_block() {
        let (out, removed) = delete_column_tmdl(&table_tmdl(), "Amount");
        assert!(removed);
        assert!(!out.contains("column Amount"));
        assert!(out.contains("column Region"));
        assert!(out.contains("partition p"));
    }

    #[test]
    fn rename_column_data_and_calculated() {
        let (out, renamed) = rename_column_tmdl(&table_tmdl(), "Region", "Territory");
        assert!(renamed);
        assert!(out.contains("\tcolumn Territory"));
        assert!(!out.contains("column Region\n"));
        // calculated column keeps its expression
        let calc = "table T\n\tcolumn 'Full' = [A] & [B]\n\t\tdataType: string\n";
        let (out2, r2) = rename_column_tmdl(calc, "Full", "Full Name");
        assert!(r2);
        assert!(out2.contains("\tcolumn 'Full Name' = [A] & [B]"));
    }

    #[test]
    fn update_column_replaces_and_adds_props() {
        let props = ColumnProps {
            format_string: Some("0.00"),
            summarize_by: Some("sum"),
            ..Default::default()
        };
        let (out, updated) = update_column_tmdl(&table_tmdl(), "Amount", None, Some("sum"), &props);
        assert!(updated);
        let seg = out.split("column Amount").nth(1).unwrap();
        assert!(seg.contains("formatString: 0.00"));
        assert!(seg.contains("summarizeBy: sum"));
        assert!(seg.contains("dataType: double")); // untouched prop preserved
    }

    #[test]
    fn update_column_replaces_existing_datatype() {
        let props = ColumnProps::default();
        let (out, _u) = update_column_tmdl(&table_tmdl(), "Amount", Some("decimal"), None, &props);
        let seg = out.split("column Amount").nth(1).unwrap();
        assert!(seg.contains("dataType: decimal"));
        assert!(!seg.contains("dataType: double"));
    }

    #[test]
    fn bim_column_lifecycle() {
        let bim = r#"{"model":{"tables":[{"name":"Sales","columns":[{"name":"Amount","dataType":"double","sourceColumn":"Amount"}]}]}}"#;
        let props = ColumnProps::default();
        let added = add_calculated_column_bim(
            bim,
            "Sales",
            "Double Amt",
            "[Amount]*2",
            Some("double"),
            &props,
        )
        .unwrap();
        let j: Value = serde_json::from_str(&added).unwrap();
        assert_eq!(j["model"]["tables"][0]["columns"][1]["type"], "calculated");
        let renamed = rename_column_bim(&added, "Sales", "Double Amt", "Doubled").unwrap();
        assert!(renamed.contains("Doubled"));
        let deleted = delete_column_bim(&added, "Sales", "Amount").unwrap();
        let jd: Value = serde_json::from_str(&deleted).unwrap();
        assert_eq!(
            jd["model"]["tables"][0]["columns"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
