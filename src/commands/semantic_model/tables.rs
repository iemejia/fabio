//! `semantic-model` table lifecycle — `add-table`, `delete-table`,
//! `rename-table`.
//!
//! Tables are `definition/tables/<name>.tmdl` files, each `ref`-ed from
//! `model.tmdl` (`ref table <name>`). fabio edits them via the shared definition
//! read-modify-write (no XMLA/TOM). `add-table` creates a CALCULATED table (a DAX
//! table expression) — self-contained and valid without a data-source partition;
//! Fabric infers its columns. `delete-table` CASCADES: it also removes any
//! relationships and role RLS filters that reference the table (else the
//! `updateDefinition` push would be rejected for a dangling reference).

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::analyze::tab_indent;
use super::relationships::remove_relationships_referencing_table;
use super::roles::cascade_remove_table_from_roles;
use super::tmdl::{
    add_model_ref, fetch_parts, find_table_file, is_table_tmdl, part_content, push_parts,
    quote_tmdl_name, remove_model_ref, remove_part, replace_part, tmdl_table_name, upsert_part,
};

const MODEL_TMDL: &str = "definition/model.tmdl";
const RELATIONSHIPS_PATH: &str = "definition/relationships.tmdl";

fn table_path(name: &str) -> String {
    format!("definition/tables/{name}.tmdl")
}

fn table_exists(parts: &[(String, String)], name: &str) -> bool {
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
    parts
        .iter()
        .any(|(p, c)| is_table_tmdl(p) && tmdl_table_name(c).as_deref() == Some(name))
}

// ── add-table (calculated) ────────────────────────────────────────────────────

pub(super) async fn add_table(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    expression: &str,
) -> Result<()> {
    let op = "semantic-model add-table";
    let parts = fetch_parts(client, workspace, id, op).await?;

    if table_exists(&parts, name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("A table named '{name}' already exists."),
            "Pick a different name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_calculated_table_bim(bim, name, expression)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let content = render_calculated_table(name, expression);
        let with_table = upsert_part(&parts, &table_path(name), &content);
        let model = part_content(&with_table, MODEL_TMDL).unwrap_or("");
        let new_model = add_model_ref(model, "table", name);
        replace_part(&with_table, MODEL_TMDL, &new_model)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": name, "expression": expression }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "table_added", "id": id, "table": name }),
        "status",
    );
    Ok(())
}

fn render_calculated_table(name: &str, expression: &str) -> String {
    let q = quote_tmdl_name(name);
    format!(
        "table {q}\n\n\tpartition {q} = calculated\n\t\tmode: import\n\t\tsource = {}\n",
        expression.trim()
    )
}

// ── delete-table (cascades relationships + role filters) ──────────────────────

pub(super) async fn delete_table(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model delete-table";
    let parts = fetch_parts(client, workspace, id, op).await?;

    if !table_exists(&parts, name) {
        return Err(table_not_found(name));
    }

    let mut removed_relationships: Vec<String> = Vec::new();
    let mut affected_roles: Vec<String> = Vec::new();

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let (new_bim, rels) = delete_table_bim(bim, name)?;
        removed_relationships = rels;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        // 1) drop the table file + its ref
        let mut next = remove_part(&parts, &table_path(name));
        let model = part_content(&next, MODEL_TMDL).unwrap_or("");
        let new_model = remove_model_ref(model, "table", name);
        next = replace_part(&next, MODEL_TMDL, &new_model);
        // 2) cascade: relationships referencing the table
        if let Some(rels) = part_content(&next, RELATIONSHIPS_PATH) {
            let (new_rels, removed) = remove_relationships_referencing_table(rels, name);
            removed_relationships = removed;
            next = if new_rels.trim().is_empty() {
                remove_part(&next, RELATIONSHIPS_PATH)
            } else {
                replace_part(&next, RELATIONSHIPS_PATH, &new_rels)
            };
        }
        // 3) cascade: role RLS filters on the table
        let (with_roles, roles) = cascade_remove_table_from_roles(&next, name);
        affected_roles = roles;
        with_roles
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({
            "id": id,
            "table": name,
            "cascadedRelationships": removed_relationships,
            "affectedRoles": affected_roles,
        }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({
            "status": "table_deleted",
            "id": id,
            "table": name,
            "cascadedRelationships": removed_relationships,
            "affectedRoles": affected_roles,
        }),
        "status",
    );
    Ok(())
}

// ── rename-table ──────────────────────────────────────────────────────────────

pub(super) async fn rename_table(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    new_name: &str,
) -> Result<()> {
    let op = "semantic-model rename-table";
    if name == new_name {
        return Err(
            FabioError::invalid_input("--new-name must differ from --name".to_string()).into(),
        );
    }
    let parts = fetch_parts(client, workspace, id, op).await?;
    if table_exists(&parts, new_name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("A table named '{new_name}' already exists."),
            "Pick a different --new-name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = rename_table_bim(bim, name, new_name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, name)?;
        let old_content = parts[idx].1.clone();
        let new_content = rename_table_decl(&old_content, name, new_name);
        // Move the part to the new file path.
        let mut next = remove_part(&parts, &table_path(name));
        next = upsert_part(&next, &table_path(new_name), &new_content);
        // Update the model.tmdl ref.
        let model = part_content(&next, MODEL_TMDL).unwrap_or("");
        let new_model = add_model_ref(&remove_model_ref(model, "table", name), "table", new_name);
        replace_part(&next, MODEL_TMDL, &new_model)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": name, "newName": new_name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "table_renamed", "id": id, "table": name, "newName": new_name }),
        "status",
    );
    Ok(())
}

/// Rewrite the top-level `table <old>` declaration to `<new>`.
fn rename_table_decl(content: &str, old: &str, new: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    for line in content.lines() {
        if !done && line.starts_with("table ") {
            // Preserve any inline ` = expr` (calculated-table declarations don't
            // usually have one, but be safe).
            let after = &line["table ".len()..];
            let rest = after
                .find('=')
                .map_or(String::new(), |i| format!(" {}", &after[i..]));
            out.push(format!("table {}{}", quote_tmdl_name(new), rest));
            done = true;
        } else {
            out.push(line.to_string());
        }
    }
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    let _ = old;
    result
}

fn table_not_found(name: &str) -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::NotFound,
        format!("Table '{name}' not found in the model definition."),
        "List tables with `fabio semantic-model list-tables`.".to_string(),
    )
    .into()
}

// ── update-table (scalar properties) ──────────────────────────────────────────

/// Properties `update-table` can set on a table.
#[derive(Default)]
pub(super) struct TableProps<'a> {
    pub hidden: Option<bool>,
    pub data_category: Option<&'a str>,
    pub description: Option<&'a str>,
}

pub(super) async fn update_table(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    props: &TableProps<'_>,
) -> Result<()> {
    let op = "semantic-model update-table";
    if props.hidden.is_none() && props.data_category.is_none() && props.description.is_none() {
        return Err(FabioError::invalid_input(
            "Provide at least one of --hidden / --data-category / --description".to_string(),
        )
        .into());
    }
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = update_table_bim(bim, name, props)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, name)?;
        let new_content = update_table_tmdl(&parts[idx].1, props);
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(cli, op, &serde_json::json!({ "id": id, "table": name })) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "table_updated", "id": id, "table": name }),
        "status",
    );
    Ok(())
}

/// Set/replace the table's scalar properties (`isHidden`, `dataCategory:`) and
/// `///` description. Managed scalar props are dropped and re-inserted right
/// after the `table` declaration (before its child objects).
fn update_table_tmdl(content: &str, props: &TableProps<'_>) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let decl = lines
        .iter()
        .position(|l| tab_indent(l) == 0 && l.trim_start().starts_with("table "))
        .unwrap_or(0);

    // Description: replace `///` comment lines (indent 0) immediately before decl.
    let mut desc_start = decl;
    while desc_start > 0
        && tab_indent(lines[desc_start - 1]) == 0
        && lines[desc_start - 1].trim_start().starts_with("///")
    {
        desc_start -= 1;
    }

    let mut out: Vec<String> = Vec::new();
    // Lines before the description block are kept verbatim.
    out.extend(lines[..desc_start].iter().map(|s| (*s).to_string()));
    // Re-emit the description (new, or the original comments if not overriding).
    if let Some(d) = props.description {
        for dl in d.split('\n') {
            out.push(format!("/// {}", dl.trim_end()));
        }
    } else {
        out.extend(lines[desc_start..decl].iter().map(|s| (*s).to_string()));
    }
    // The `table` declaration line.
    out.push(lines[decl].to_string());
    // Managed scalar props right after the decl.
    if props.hidden == Some(true) {
        out.push("\tisHidden".to_string());
    }
    if let Some(cat) = props.data_category.filter(|c| !c.is_empty()) {
        out.push(format!("\tdataCategory: {cat}"));
    }
    // Remaining table body, dropping any existing managed scalar props that sit
    // in the scalar region (before the first child object).
    let mut in_scalar_region = true;
    for l in &lines[decl + 1..] {
        let t = l.trim_start();
        if in_scalar_region && tab_indent(l) == 1 && super::tmdl::is_child_object_decl(l) {
            in_scalar_region = false;
        }
        let drop_managed = in_scalar_region
            && tab_indent(l) == 1
            && ((props.hidden.is_some() && t == "isHidden")
                || (props.data_category.is_some() && t.starts_with("dataCategory:")));
        if drop_managed {
            continue;
        }
        out.push((*l).to_string());
    }
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn update_table_bim(bim: &str, name: &str, props: &TableProps<'_>) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let t = bim_tables_mut(&mut j)?
        .iter_mut()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| table_not_found(name))?;
    if let Some(h) = props.hidden {
        t["isHidden"] = Value::Bool(h);
    }
    if let Some(cat) = props.data_category {
        t["dataCategory"] = Value::from(cat);
    }
    if let Some(d) = props.description {
        t["description"] = Value::from(d);
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

// ── model.bim editors ─────────────────────────────────────────────────────────

fn bim_tables_mut(j: &mut Value) -> Result<&mut Vec<Value>> {
    j.get_mut("model")
        .and_then(|m| m.get_mut("tables"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no tables").into())
}

fn add_calculated_table_bim(bim: &str, name: &str, expression: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    bim_tables_mut(&mut j)?.push(serde_json::json!({
        "name": name,
        "partitions": [{
            "name": name,
            "source": { "type": "calculated", "expression": expression }
        }]
    }));
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_table_bim(bim: &str, name: &str) -> Result<(String, Vec<String>)> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let tables = bim_tables_mut(&mut j)?;
    let before = tables.len();
    tables.retain(|t| t.get("name").and_then(Value::as_str) != Some(name));
    if tables.len() == before {
        return Err(table_not_found(name));
    }
    // Cascade: drop relationships referencing the table.
    let mut removed_rels = Vec::new();
    if let Some(rels) = j
        .get_mut("model")
        .and_then(|m| m.get_mut("relationships"))
        .and_then(Value::as_array_mut)
    {
        rels.retain(|r| {
            let refs = r.get("fromTable").and_then(Value::as_str) == Some(name)
                || r.get("toTable").and_then(Value::as_str) == Some(name);
            if refs && let Some(id) = r.get("name").and_then(Value::as_str) {
                removed_rels.push(id.to_string());
            }
            !refs
        });
    }
    Ok((
        serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()),
        removed_rels,
    ))
}

fn rename_table_bim(bim: &str, name: &str, new_name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let t = bim_tables_mut(&mut j)?
        .iter_mut()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| table_not_found(name))?;
    t["name"] = Value::from(new_name);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_calculated_table_shape() {
        let out = render_calculated_table("Dates", "CALENDAR(DATE(2020,1,1), DATE(2020,12,31))");
        assert!(out.starts_with("table Dates\n"));
        assert!(out.contains("\tpartition Dates = calculated"));
        assert!(out.contains("\t\tsource = CALENDAR(DATE(2020,1,1), DATE(2020,12,31))"));
        // quoted name
        let out2 = render_calculated_table("Date Table", "{1}");
        assert!(out2.contains("table 'Date Table'"));
        assert!(out2.contains("partition 'Date Table' = calculated"));
    }

    #[test]
    fn rename_table_decl_rewrites_only_header() {
        let content = "table Sales\n\tlineageTag: x\n\n\tcolumn A\n\t\tdataType: string\n";
        let out = rename_table_decl(content, "Sales", "Fact Sales");
        assert!(out.starts_with("table 'Fact Sales'\n"));
        assert!(out.contains("column A")); // body untouched
    }

    #[test]
    fn update_table_sets_scalar_props_and_description() {
        let content = "table Dates\n\tlineageTag: x\n\n\tcolumn D\n\t\tdataType: dateTime\n";
        let props = TableProps {
            hidden: Some(true),
            data_category: Some("Time"),
            description: Some("Date dimension"),
        };
        let out = update_table_tmdl(content, &props);
        assert!(out.starts_with("/// Date dimension\ntable Dates\n"));
        assert!(out.contains("\tisHidden"));
        assert!(out.contains("\tdataCategory: Time"));
        assert!(out.contains("\tlineageTag: x")); // preserved
        assert!(out.contains("column D")); // child untouched
        // Re-applying replaces (no dupes).
        let props2 = TableProps {
            hidden: Some(false),
            data_category: Some("Regular"),
            ..Default::default()
        };
        let out2 = update_table_tmdl(&out, &props2);
        assert!(!out2.contains("isHidden")); // hidden=false removes it
        assert!(out2.contains("\tdataCategory: Regular"));
        assert_eq!(out2.matches("dataCategory").count(), 1);
    }

    #[test]
    fn update_table_bim_sets_props() {
        let bim = r#"{"model":{"tables":[{"name":"Dates"}]}}"#;
        let props = TableProps {
            hidden: Some(true),
            data_category: Some("Time"),
            description: Some("d"),
        };
        let out = update_table_bim(bim, "Dates", &props).unwrap();
        let j: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(j["model"]["tables"][0]["isHidden"], true);
        assert_eq!(j["model"]["tables"][0]["dataCategory"], "Time");
    }

    #[test]
    fn bim_add_delete_rename_table() {
        let bim = r#"{"model":{"tables":[{"name":"Sales"},{"name":"Customer"}],"relationships":[{"name":"r1","fromTable":"Sales","fromColumn":"CustomerKey","toTable":"Customer","toColumn":"CustomerKey"}]}}"#;
        // add
        let added = add_calculated_table_bim(bim, "Dates", "CALENDAR(1,2)").unwrap();
        let ja: Value = serde_json::from_str(&added).unwrap();
        assert_eq!(ja["model"]["tables"].as_array().unwrap().len(), 3);
        // delete Customer → cascades r1
        let (deleted, rels) = delete_table_bim(bim, "Customer").unwrap();
        assert_eq!(rels, vec!["r1".to_string()]);
        let jd: Value = serde_json::from_str(&deleted).unwrap();
        assert_eq!(jd["model"]["relationships"].as_array().unwrap().len(), 0);
        // rename
        let renamed = rename_table_bim(bim, "Sales", "Fact").unwrap();
        assert!(renamed.contains("\"Fact\""));
    }
}
