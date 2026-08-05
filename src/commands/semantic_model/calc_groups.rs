//! `semantic-model` calculation-group authoring — `add-calculation-group`,
//! `delete-calculation-group`, `add-calculation-item`, `delete-calculation-item`,
//! `list-calculation-groups`.
//!
//! A calculation group is a special table carrying a `calculationGroup` block of
//! `calculationItem <name> = <DAX>` entries, a single string column, and a
//! `partition <name> = calculationGroup`. The model must also set
//! `discourageImplicitMeasures`. fabio builds/edits these via the shared
//! definition read-modify-write (no XMLA/TOM).

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::analyze::tab_indent;
use super::tmdl::{
    add_model_ref, decl_name, fetch_parts, find_table_file, is_table_tmdl,
    join_preserving_trailing_newline, part_content, push_parts, quote_tmdl_name, remove_model_ref,
    remove_part, replace_part, tmdl_table_name, upsert_part,
};

const MODEL_TMDL: &str = "definition/model.tmdl";

fn table_path(name: &str) -> String {
    format!("definition/tables/{name}.tmdl")
}

fn is_calc_group_table(content: &str) -> bool {
    content
        .lines()
        .any(|l| tab_indent(l) == 1 && l.trim() == "calculationGroup")
}

// ── add-calculation-group ─────────────────────────────────────────────────────

pub(super) async fn add_calculation_group(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    column_name: &str,
) -> Result<()> {
    let op = "semantic-model add-calculation-group";
    let parts = fetch_parts(client, workspace, id, op).await?;

    if group_exists(&parts, name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("A table/calculation group named '{name}' already exists."),
            "Pick a different name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_calc_group_bim(bim, name, column_name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let content = render_calc_group_table(name, column_name);
        let with_table = upsert_part(&parts, &table_path(name), &content);
        let model = part_content(&with_table, MODEL_TMDL).unwrap_or("");
        let model = ensure_model_flag(model, "discourageImplicitMeasures");
        let model = add_model_ref(&model, "table", name);
        replace_part(&with_table, MODEL_TMDL, &model)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "calculationGroup": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "calculation_group_added", "id": id, "calculationGroup": name }),
        "status",
    );
    Ok(())
}

fn render_calc_group_table(name: &str, column_name: &str) -> String {
    let q = quote_tmdl_name(name);
    format!(
        "table {q}\n\n\tcalculationGroup\n\n\tcolumn {}\n\t\tdataType: string\n\t\tsourceColumn: Name\n\n\tpartition {q} = calculationGroup\n",
        quote_tmdl_name(column_name)
    )
}

/// Ensure a model-level boolean flag line (e.g. `discourageImplicitMeasures`) is
/// present among `model.tmdl`'s scalar properties.
fn ensure_model_flag(model_tmdl: &str, flag: &str) -> String {
    if model_tmdl.lines().any(|l| l.trim() == flag) {
        return model_tmdl.to_string();
    }
    let lines: Vec<&str> = model_tmdl.lines().collect();
    // Insert after the contiguous run of indent-1 model properties following the
    // `model <name>` declaration.
    let mut insert_at = 0;
    for (i, l) in lines.iter().enumerate() {
        if tab_indent(l) == 0 && l.trim_start().starts_with("model ") {
            insert_at = i + 1;
            let mut j = i + 1;
            while j < lines.len() && tab_indent(lines[j]) == 1 {
                insert_at = j + 1;
                j += 1;
            }
            break;
        }
    }
    let mut out: Vec<String> = lines[..insert_at]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    out.push(format!("\t{flag}"));
    out.extend(lines[insert_at..].iter().map(|s| (*s).to_string()));
    let mut result = out.join("\n");
    if model_tmdl.ends_with('\n') {
        result.push('\n');
    }
    result
}

// ── delete-calculation-group ──────────────────────────────────────────────────

pub(super) async fn delete_calculation_group(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model delete-calculation-group";
    let parts = fetch_parts(client, workspace, id, op).await?;

    if !group_exists(&parts, name) {
        return Err(group_not_found(name));
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_calc_group_bim(bim, name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let without = remove_part(&parts, &table_path(name));
        let model = part_content(&without, MODEL_TMDL).unwrap_or("");
        let new_model = remove_model_ref(model, "table", name);
        replace_part(&without, MODEL_TMDL, &new_model)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "calculationGroup": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "calculation_group_deleted", "id": id, "calculationGroup": name }),
        "status",
    );
    Ok(())
}

// ── add-calculation-item / delete-calculation-item ────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn add_calculation_item(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    group: &str,
    name: &str,
    expression: &str,
    ordinal: Option<i64>,
) -> Result<()> {
    let op = "semantic-model add-calculation-item";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_calc_item_bim(bim, group, name, expression, ordinal)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, group)?;
        if !is_calc_group_table(&parts[idx].1) {
            return Err(FabioError::invalid_input(format!(
                "Table '{group}' is not a calculation group."
            ))
            .into());
        }
        if calc_item_names(&parts[idx].1).iter().any(|n| n == name) {
            return Err(FabioError::with_hint(
                ErrorCode::Conflict,
                format!("Calculation item '{group}.{name}' already exists."),
                "Pick a different name.".to_string(),
            )
            .into());
        }
        let new_content = add_calc_item_tmdl(&parts[idx].1, name, expression, ordinal);
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "calculationGroup": group, "item": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "calculation_item_added", "id": id, "calculationGroup": group, "item": name }),
        "status",
    );
    Ok(())
}

pub(super) async fn delete_calculation_item(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    group: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model delete-calculation-item";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_calc_item_bim(bim, group, name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, group)?;
        let (new_content, removed) = delete_calc_item_tmdl(&parts[idx].1, name);
        if !removed {
            return Err(FabioError::not_found(format!(
                "Calculation item '{group}.{name}' not found"
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
        &serde_json::json!({ "id": id, "calculationGroup": group, "item": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "calculation_item_deleted", "id": id, "calculationGroup": group, "item": name }),
        "status",
    );
    Ok(())
}

// ── list-calculation-groups ───────────────────────────────────────────────────

pub(super) async fn list_calculation_groups(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let op = "semantic-model list-calculation-groups";
    let parts = fetch_parts(client, workspace, id, op).await?;
    let groups = collect_calc_groups(&parts);
    output::render_list(
        cli,
        &groups,
        &["name", "itemCount"],
        &["NAME", "ITEMS"],
        "name",
    );
    Ok(())
}

// ── pure TMDL editors ─────────────────────────────────────────────────────────

/// The line span `[start, end)` of the `calculationGroup` block within a table
/// file (from the `calculationGroup` line through its indent≥2 body).
fn calc_group_span(lines: &[&str]) -> Option<(usize, usize)> {
    let decl = lines
        .iter()
        .position(|l| tab_indent(l) == 1 && l.trim() == "calculationGroup")?;
    let mut end = decl + 1;
    while end < lines.len() && (lines[end].trim().is_empty() || tab_indent(lines[end]) >= 2) {
        end += 1;
    }
    while end > decl + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    Some((decl, end))
}

fn calc_item_names(content: &str) -> Vec<String> {
    content
        .lines()
        .filter(|l| tab_indent(l) == 2 && l.trim_start().starts_with("calculationItem "))
        .filter_map(|l| decl_name(l.trim_start_matches('\t'), "calculationItem"))
        .collect()
}

fn build_calc_item_lines(name: &str, expression: &str, ordinal: Option<i64>) -> Vec<String> {
    let mut block: Vec<String> = Vec::new();
    let expr = expression.trim();
    if expr.contains('\n') {
        block.push(format!("\t\tcalculationItem {} =", quote_tmdl_name(name)));
        for l in expr.lines() {
            block.push(format!("\t\t\t{}", l.trim_end()));
        }
    } else {
        block.push(format!(
            "\t\tcalculationItem {} = {expr}",
            quote_tmdl_name(name)
        ));
    }
    if let Some(o) = ordinal {
        block.push(format!("\t\t\tordinal: {o}"));
    }
    block
}

fn add_calc_item_tmdl(content: &str, name: &str, expression: &str, ordinal: Option<i64>) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let Some((_, end)) = calc_group_span(&lines) else {
        return content.to_string();
    };
    let mut item = vec![String::new()];
    item.extend(build_calc_item_lines(name, expression, ordinal));
    let mut out: Vec<String> = Vec::new();
    out.extend(lines[..end].iter().map(|s| (*s).to_string()));
    out.extend(item);
    out.extend(lines[end..].iter().map(|s| (*s).to_string()));
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn delete_calc_item_tmdl(content: &str, name: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    // Find the calculationItem decl at indent 2.
    let Some(decl) = lines.iter().position(|l| {
        tab_indent(l) == 2
            && decl_name(l.trim_start_matches('\t'), "calculationItem").as_deref() == Some(name)
    }) else {
        return (content.to_string(), false);
    };
    let mut end = decl + 1;
    while end < lines.len() && (lines[end].trim().is_empty() || tab_indent(lines[end]) >= 3) {
        end += 1;
    }
    while end > decl + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let mut out: Vec<String> = Vec::new();
    out.extend(lines[..decl].iter().map(|s| (*s).to_string()));
    out.extend(lines[end..].iter().map(|s| (*s).to_string()));
    (
        join_preserving_trailing_newline(&out, content.ends_with('\n')),
        true,
    )
}

fn collect_calc_groups(parts: &[(String, String)]) -> Vec<Value> {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_calc_groups_bim(bim);
    }
    parts
        .iter()
        .filter(|(p, c)| is_table_tmdl(p) && is_calc_group_table(c))
        .map(|(_, c)| {
            let items = calc_item_names(c);
            serde_json::json!({
                "name": tmdl_table_name(c).unwrap_or_default(),
                "itemCount": items.len(),
                "items": items,
            })
        })
        .collect()
}

fn group_exists(parts: &[(String, String)], name: &str) -> bool {
    if let Some(bim) = part_content(parts, "model.bim") {
        return serde_json::from_str::<Value>(bim).ok().is_some_and(|j| {
            j.get("model")
                .and_then(|m| m.get("tables"))
                .and_then(Value::as_array)
                .is_some_and(|ts| {
                    ts.iter()
                        .any(|t| t.get("name").and_then(Value::as_str) == Some(name))
                })
        });
    }
    parts.iter().any(|(p, _)| p == &table_path(name))
}

fn group_not_found(name: &str) -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::NotFound,
        format!("Calculation group '{name}' not found in the model definition."),
        "List calculation groups with `fabio semantic-model list-calculation-groups`.".to_string(),
    )
    .into()
}

// ── model.bim editors ─────────────────────────────────────────────────────────

fn bim_tables_mut(j: &mut Value) -> Result<&mut Vec<Value>> {
    j.get_mut("model")
        .and_then(|m| m.get_mut("tables"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no tables").into())
}

fn add_calc_group_bim(bim: &str, name: &str, column_name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    if let Some(m) = j.get_mut("model").and_then(Value::as_object_mut) {
        m.insert("discourageImplicitMeasures".to_string(), Value::Bool(true));
    }
    bim_tables_mut(&mut j)?.push(serde_json::json!({
        "name": name,
        "calculationGroup": { "calculationItems": [] },
        "columns": [{ "name": column_name, "dataType": "string", "sourceColumn": "Name" }],
        "partitions": [{ "name": name, "source": { "type": "calculationGroup" } }]
    }));
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_calc_group_bim(bim: &str, name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let tables = bim_tables_mut(&mut j)?;
    let before = tables.len();
    tables.retain(|t| t.get("name").and_then(Value::as_str) != Some(name));
    if tables.len() == before {
        return Err(group_not_found(name));
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn calc_items_bim<'a>(j: &'a mut Value, group: &str) -> Result<&'a mut Vec<Value>> {
    bim_tables_mut(j)?
        .iter_mut()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(group))
        .ok_or_else(|| group_not_found(group))?
        .get_mut("calculationGroup")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            FabioError::invalid_input(format!("Table '{group}' is not a calculation group"))
        })?
        .entry("calculationItems")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| FabioError::invalid_input("calculationItems is not an array").into())
}

fn add_calc_item_bim(
    bim: &str,
    group: &str,
    name: &str,
    expression: &str,
    ordinal: Option<i64>,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let items = calc_items_bim(&mut j, group)?;
    if items
        .iter()
        .any(|i| i.get("name").and_then(Value::as_str) == Some(name))
    {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("Calculation item '{group}.{name}' already exists."),
            "Pick a different name.".to_string(),
        )
        .into());
    }
    let mut item = serde_json::json!({ "name": name, "expression": expression });
    if let Some(o) = ordinal {
        item["ordinal"] = Value::from(o);
    }
    items.push(item);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_calc_item_bim(bim: &str, group: &str, name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let items = calc_items_bim(&mut j, group)?;
    let before = items.len();
    items.retain(|i| i.get("name").and_then(Value::as_str) != Some(name));
    if items.len() == before {
        return Err(
            FabioError::not_found(format!("Calculation item '{group}.{name}' not found")).into(),
        );
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn collect_calc_groups_bim(bim: &str) -> Vec<Value> {
    let Ok(j) = serde_json::from_str::<Value>(bim) else {
        return Vec::new();
    };
    j.get("model")
        .and_then(|m| m.get("tables"))
        .and_then(Value::as_array)
        .map(|tables| {
            tables
                .iter()
                .filter(|t| t.get("calculationGroup").is_some())
                .map(|t| {
                    let items: Vec<String> = t
                        .pointer("/calculationGroup/calculationItems")
                        .and_then(Value::as_array)
                        .map(|is| {
                            is.iter()
                                .filter_map(|i| i.get("name").and_then(Value::as_str))
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default();
                    serde_json::json!({
                        "name": t.get("name").and_then(Value::as_str).unwrap_or(""),
                        "itemCount": items.len(),
                        "items": items,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cg_table() -> String {
        "table TimeCalc\n\n\tcalculationGroup\n\n\t\tcalculationItem Current = SELECTEDMEASURE()\n\n\tcolumn 'Time Calculation'\n\t\tdataType: string\n\t\tsourceColumn: Name\n\n\tpartition TimeCalc = calculationGroup\n".to_string()
    }

    #[test]
    fn render_group_table_shape() {
        let out = render_calc_group_table("Time", "Time Col");
        assert!(out.contains("table Time"));
        assert!(out.contains("\tcalculationGroup"));
        assert!(out.contains("\tcolumn 'Time Col'"));
        assert!(out.contains("\tpartition Time = calculationGroup"));
    }

    #[test]
    fn ensure_model_flag_inserts_once() {
        let m = "model Model\n\tculture: en-US\n\tdefaultPowerBIDataSourceVersion: powerBI_V3\n\nref table X\n";
        let out = ensure_model_flag(m, "discourageImplicitMeasures");
        assert!(out.contains("\tdiscourageImplicitMeasures"));
        // inserted before the ref block
        assert!(out.find("discourageImplicitMeasures").unwrap() < out.find("ref table X").unwrap());
        let again = ensure_model_flag(&out, "discourageImplicitMeasures");
        assert_eq!(out, again);
    }

    #[test]
    fn detect_and_list_items() {
        assert!(is_calc_group_table(&cg_table()));
        assert_eq!(calc_item_names(&cg_table()), vec!["Current".to_string()]);
    }

    #[test]
    fn add_and_delete_calc_item() {
        let out = add_calc_item_tmdl(&cg_table(), "YTD", "CALCULATE(SELECTEDMEASURE())", Some(1));
        assert!(out.contains("\t\tcalculationItem YTD = CALCULATE(SELECTEDMEASURE())"));
        assert!(out.contains("\t\t\tordinal: 1"));
        // item lands inside the calculationGroup block, before the column
        assert!(
            out.find("calculationItem YTD").unwrap()
                < out.find("column 'Time Calculation'").unwrap()
        );
        let (del, removed) = delete_calc_item_tmdl(&out, "YTD");
        assert!(removed);
        assert!(!del.contains("calculationItem YTD"));
        assert!(del.contains("calculationItem Current"));
    }

    #[test]
    fn bim_calc_group_lifecycle() {
        let bim = r#"{"model":{"name":"Model","tables":[]}}"#;
        let g = add_calc_group_bim(bim, "TimeCalc", "Time Calculation").unwrap();
        let j: Value = serde_json::from_str(&g).unwrap();
        assert_eq!(j["model"]["discourageImplicitMeasures"], true);
        let i = add_calc_item_bim(&g, "TimeCalc", "Current", "SELECTEDMEASURE()", None).unwrap();
        let ji: Value = serde_json::from_str(&i).unwrap();
        assert_eq!(
            ji["model"]["tables"][0]["calculationGroup"]["calculationItems"][0]["name"],
            "Current"
        );
        let groups = collect_calc_groups_bim(&i);
        assert_eq!(groups[0]["itemCount"], 1);
        let di = delete_calc_item_bim(&i, "TimeCalc", "Current").unwrap();
        let jdi: Value = serde_json::from_str(&di).unwrap();
        assert_eq!(
            jdi["model"]["tables"][0]["calculationGroup"]["calculationItems"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        let dg = delete_calc_group_bim(&g, "TimeCalc").unwrap();
        let jdg: Value = serde_json::from_str(&dg).unwrap();
        assert_eq!(jdg["model"]["tables"].as_array().unwrap().len(), 0);
    }
}
