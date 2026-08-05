//! `semantic-model` perspective authoring — `add-perspective`,
//! `delete-perspective`, `add-perspective-member`, `remove-perspective-member`,
//! `list-perspectives`.
//!
//! Perspectives (filtered views of the model for different audiences) live in
//! `definition/perspectives/<name>.tmdl` and are `ref`-ed from `model.tmdl`
//! (`ref perspective <name>`). A perspective file is a nested tree:
//! `perspective <name>` → `perspectiveTable <T>` → `perspectiveColumn <C>` /
//! `perspectiveMeasure <M>` / `perspectiveHierarchy <H>`. fabio parses that tree
//! into a small model, edits it, and re-renders — the same robust approach as
//! translations. Edits go through the shared definition read-modify-write.

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::analyze::tab_indent;
use super::tmdl::{
    add_model_ref, fetch_parts, part_content, push_parts, quote_tmdl_name, remove_model_ref,
    remove_part, replace_part, upsert_part,
};

const MODEL_TMDL: &str = "definition/model.tmdl";

fn perspective_path(name: &str) -> String {
    format!("definition/perspectives/{name}.tmdl")
}

fn is_perspective_file(path: &str) -> bool {
    path.starts_with("definition/perspectives/")
        && std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tmdl"))
}

/// The kind of a member within a perspective table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberKind {
    Column,
    Measure,
    Hierarchy,
}

// ── parsed perspective model ──────────────────────────────────────────────────

#[derive(Default)]
struct PerspTable {
    name: String,
    columns: Vec<String>,
    measures: Vec<String>,
    hierarchies: Vec<String>,
}

#[derive(Default)]
struct Perspective {
    name: String,
    tables: Vec<PerspTable>,
}

impl Perspective {
    fn table_mut(&mut self, name: &str) -> &mut PerspTable {
        if let Some(pos) = self.tables.iter().position(|t| t.name == name) {
            &mut self.tables[pos]
        } else {
            self.tables.push(PerspTable {
                name: name.to_string(),
                ..Default::default()
            });
            self.tables.last_mut().unwrap()
        }
    }
}

fn strip_bare(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        s[1..s.len() - 1].replace("''", "'")
    } else {
        s.to_string()
    }
}

fn parse_perspective(content: &str) -> Perspective {
    let mut p = Perspective::default();
    let mut cur: Option<usize> = None;
    for line in content.lines() {
        let indent = tab_indent(line);
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("perspective ") {
            p.name = strip_bare(rest);
        } else if indent == 1
            && let Some(rest) = t.strip_prefix("perspectiveTable ")
        {
            p.tables.push(PerspTable {
                name: strip_bare(rest),
                ..Default::default()
            });
            cur = Some(p.tables.len() - 1);
        } else if indent == 2
            && let Some(ti) = cur
        {
            if let Some(rest) = t.strip_prefix("perspectiveColumn ") {
                p.tables[ti].columns.push(strip_bare(rest));
            } else if let Some(rest) = t.strip_prefix("perspectiveMeasure ") {
                p.tables[ti].measures.push(strip_bare(rest));
            } else if let Some(rest) = t.strip_prefix("perspectiveHierarchy ") {
                p.tables[ti].hierarchies.push(strip_bare(rest));
            }
        }
    }
    p
}

fn render_perspective(p: &Perspective) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "perspective {}", quote_tmdl_name(&p.name));
    for t in &p.tables {
        s.push('\n');
        let _ = writeln!(s, "\tperspectiveTable {}", quote_tmdl_name(&t.name));
        for c in &t.columns {
            let _ = writeln!(s, "\n\t\tperspectiveColumn {}", quote_tmdl_name(c));
        }
        for m in &t.measures {
            let _ = writeln!(s, "\n\t\tperspectiveMeasure {}", quote_tmdl_name(m));
        }
        for h in &t.hierarchies {
            let _ = writeln!(s, "\n\t\tperspectiveHierarchy {}", quote_tmdl_name(h));
        }
    }
    s
}

// ── add-perspective / delete-perspective ──────────────────────────────────────

pub(super) async fn add_perspective(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model add-perspective";
    let parts = fetch_parts(client, workspace, id, op).await?;

    if perspective_exists(&parts, name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("Perspective '{name}' already exists."),
            "Use `add-perspective-member`, or pick a different name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_perspective_bim(bim, name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let content = format!("perspective {}\n", quote_tmdl_name(name));
        let with_p = upsert_part(&parts, &perspective_path(name), &content);
        let model = part_content(&with_p, MODEL_TMDL).unwrap_or("");
        let new_model = add_model_ref(model, "perspective", name);
        replace_part(&with_p, MODEL_TMDL, &new_model)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "perspective": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "perspective_added", "id": id, "perspective": name }),
        "status",
    );
    Ok(())
}

pub(super) async fn delete_perspective(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model delete-perspective";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_perspective_bim(bim, name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        if !perspective_exists(&parts, name) {
            return Err(perspective_not_found(name));
        }
        let without = remove_part(&parts, &perspective_path(name));
        let model = part_content(&without, MODEL_TMDL).unwrap_or("");
        let new_model = remove_model_ref(model, "perspective", name);
        replace_part(&without, MODEL_TMDL, &new_model)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "perspective": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "perspective_deleted", "id": id, "perspective": name }),
        "status",
    );
    Ok(())
}

// ── add-perspective-member / remove-perspective-member ────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn add_perspective_member(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    perspective: &str,
    table: &str,
    member: Option<(MemberKind, &str)>,
) -> Result<()> {
    let op = "semantic-model add-perspective-member";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_perspective_member_bim(bim, perspective, table, member)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let content = part_content(&parts, &perspective_path(perspective))
            .ok_or_else(|| perspective_not_found(perspective))?;
        let mut p = parse_perspective(content);
        let t = p.table_mut(table);
        if let Some((kind, name)) = member {
            let list = match kind {
                MemberKind::Column => &mut t.columns,
                MemberKind::Measure => &mut t.measures,
                MemberKind::Hierarchy => &mut t.hierarchies,
            };
            if !list.iter().any(|x| x == name) {
                list.push(name.to_string());
            }
        }
        replace_part(
            &parts,
            &perspective_path(perspective),
            &render_perspective(&p),
        )
    };

    let target = member.map_or_else(|| table.to_string(), |(_, n)| format!("{table}.{n}"));
    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "perspective": perspective, "member": target }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "perspective_member_added", "id": id, "perspective": perspective, "member": target }),
        "status",
    );
    Ok(())
}

pub(super) async fn remove_perspective_member(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    perspective: &str,
    table: &str,
    member: Option<(MemberKind, &str)>,
) -> Result<()> {
    let op = "semantic-model remove-perspective-member";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = remove_perspective_member_bim(bim, perspective, table, member)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let content = part_content(&parts, &perspective_path(perspective))
            .ok_or_else(|| perspective_not_found(perspective))?;
        let mut p = parse_perspective(content);
        let removed = remove_member(&mut p, table, member);
        if !removed {
            return Err(FabioError::not_found(format!(
                "Perspective member not found in '{perspective}'."
            ))
            .into());
        }
        replace_part(
            &parts,
            &perspective_path(perspective),
            &render_perspective(&p),
        )
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "perspective": perspective, "table": table }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "perspective_member_removed", "id": id, "perspective": perspective }),
        "status",
    );
    Ok(())
}

fn remove_member(p: &mut Perspective, table: &str, member: Option<(MemberKind, &str)>) -> bool {
    let Some(ti) = p.tables.iter().position(|t| t.name == table) else {
        return false;
    };
    match member {
        None => {
            p.tables.remove(ti);
            true
        }
        Some((kind, name)) => {
            let list = match kind {
                MemberKind::Column => &mut p.tables[ti].columns,
                MemberKind::Measure => &mut p.tables[ti].measures,
                MemberKind::Hierarchy => &mut p.tables[ti].hierarchies,
            };
            let before = list.len();
            list.retain(|x| x != name);
            list.len() != before
        }
    }
}

// ── list-perspectives ─────────────────────────────────────────────────────────

pub(super) async fn list_perspectives(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let op = "semantic-model list-perspectives";
    let parts = fetch_parts(client, workspace, id, op).await?;
    let perspectives = collect_perspectives(&parts);
    output::render_list(
        cli,
        &perspectives,
        &["name", "tableCount"],
        &["NAME", "TABLES"],
        "name",
    );
    Ok(())
}

fn collect_perspectives(parts: &[(String, String)]) -> Vec<Value> {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_perspectives_bim(bim);
    }
    parts
        .iter()
        .filter(|(p, _)| is_perspective_file(p))
        .map(|(_, c)| {
            let p = parse_perspective(c);
            serde_json::json!({ "name": p.name, "tableCount": p.tables.len() })
        })
        .collect()
}

fn perspective_exists(parts: &[(String, String)], name: &str) -> bool {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_perspectives_bim(bim)
            .iter()
            .any(|p| p.get("name").and_then(Value::as_str) == Some(name));
    }
    parts.iter().any(|(p, _)| p == &perspective_path(name))
}

fn perspective_not_found(name: &str) -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::NotFound,
        format!("Perspective '{name}' not found in the model definition."),
        "List perspectives with `fabio semantic-model list-perspectives`, or add it with `add-perspective`."
            .to_string(),
    )
    .into()
}

// ── model.bim editors ─────────────────────────────────────────────────────────

fn bim_perspectives_mut(j: &mut Value) -> Result<&mut Vec<Value>> {
    j.get_mut("model")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no model object"))?
        .entry("perspectives")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| FabioError::invalid_input("perspectives is not an array").into())
}

fn add_perspective_bim(bim: &str, name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    if collect_perspectives_bim(bim)
        .iter()
        .any(|p| p.get("name").and_then(Value::as_str) == Some(name))
    {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("Perspective '{name}' already exists."),
            "Pick a different name.".to_string(),
        )
        .into());
    }
    bim_perspectives_mut(&mut j)?.push(serde_json::json!({ "name": name, "tables": [] }));
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_perspective_bim(bim: &str, name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let ps = bim_perspectives_mut(&mut j)?;
    let before = ps.len();
    ps.retain(|p| p.get("name").and_then(Value::as_str) != Some(name));
    if ps.len() == before {
        return Err(perspective_not_found(name));
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn bim_perspective_table<'a>(
    j: &'a mut Value,
    perspective: &str,
    table: &str,
) -> Result<&'a mut Value> {
    let tables = bim_perspectives_mut(j)?
        .iter_mut()
        .find(|p| p.get("name").and_then(Value::as_str) == Some(perspective))
        .ok_or_else(|| perspective_not_found(perspective))?
        .as_object_mut()
        .unwrap()
        .entry("tables")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .unwrap();
    if !tables
        .iter()
        .any(|t| t.get("name").and_then(Value::as_str) == Some(table))
    {
        tables.push(serde_json::json!({ "name": table }));
    }
    Ok(tables
        .iter_mut()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(table))
        .unwrap())
}

const fn member_bim_key(kind: MemberKind) -> &'static str {
    match kind {
        MemberKind::Column => "columns",
        MemberKind::Measure => "measures",
        MemberKind::Hierarchy => "hierarchies",
    }
}

fn add_perspective_member_bim(
    bim: &str,
    perspective: &str,
    table: &str,
    member: Option<(MemberKind, &str)>,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let t = bim_perspective_table(&mut j, perspective, table)?;
    if let Some((kind, name)) = member {
        let arr = t
            .as_object_mut()
            .unwrap()
            .entry(member_bim_key(kind))
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .unwrap();
        if !arr
            .iter()
            .any(|x| x.get("name").and_then(Value::as_str) == Some(name))
        {
            arr.push(serde_json::json!({ "name": name }));
        }
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn remove_perspective_member_bim(
    bim: &str,
    perspective: &str,
    table: &str,
    member: Option<(MemberKind, &str)>,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let tables = bim_perspectives_mut(&mut j)?
        .iter_mut()
        .find(|p| p.get("name").and_then(Value::as_str) == Some(perspective))
        .ok_or_else(|| perspective_not_found(perspective))?
        .get_mut("tables")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::not_found("Perspective member not found".to_string()))?;
    match member {
        None => {
            let before = tables.len();
            tables.retain(|t| t.get("name").and_then(Value::as_str) != Some(table));
            if tables.len() == before {
                return Err(
                    FabioError::not_found("Perspective table not found".to_string()).into(),
                );
            }
        }
        Some((kind, name)) => {
            let t = tables
                .iter_mut()
                .find(|t| t.get("name").and_then(Value::as_str) == Some(table))
                .ok_or_else(|| FabioError::not_found("Perspective table not found".to_string()))?;
            let removed = t
                .get_mut(member_bim_key(kind))
                .and_then(Value::as_array_mut)
                .is_some_and(|arr| {
                    let before = arr.len();
                    arr.retain(|x| x.get("name").and_then(Value::as_str) != Some(name));
                    arr.len() != before
                });
            if !removed {
                return Err(
                    FabioError::not_found("Perspective member not found".to_string()).into(),
                );
            }
        }
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn collect_perspectives_bim(bim: &str) -> Vec<Value> {
    let Ok(j) = serde_json::from_str::<Value>(bim) else {
        return Vec::new();
    };
    j.get("model")
        .and_then(|m| m.get("perspectives"))
        .and_then(Value::as_array)
        .map(|ps| {
            ps.iter()
                .map(|p| {
                    let count = p
                        .get("tables")
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    serde_json::json!({
                        "name": p.get("name").and_then(Value::as_str).unwrap_or(""),
                        "tableCount": count,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persp_tmdl() -> String {
        "perspective SalesView\n\n\tperspectiveTable Geo\n\n\t\tperspectiveColumn Country\n"
            .to_string()
    }

    #[test]
    fn parse_and_render_roundtrip() {
        let p = parse_perspective(&persp_tmdl());
        assert_eq!(p.name, "SalesView");
        assert_eq!(p.tables[0].name, "Geo");
        assert_eq!(p.tables[0].columns, vec!["Country".to_string()]);
        let r = render_perspective(&p);
        let p2 = parse_perspective(&r);
        assert_eq!(p2.tables[0].columns, vec!["Country".to_string()]);
    }

    #[test]
    fn add_and_remove_members() {
        let mut p = parse_perspective(&persp_tmdl());
        p.table_mut("Geo").columns.push("City".to_string());
        assert!(p.tables[0].columns.contains(&"City".to_string()));
        // add a measure to a new table
        p.table_mut("Sales").measures.push("Total".to_string());
        assert!(p.tables.iter().any(|t| t.name == "Sales"));
        // remove a column
        assert!(remove_member(
            &mut p,
            "Geo",
            Some((MemberKind::Column, "City"))
        ));
        assert!(!p.tables[0].columns.contains(&"City".to_string()));
        // remove the whole table
        assert!(remove_member(&mut p, "Sales", None));
        assert!(!p.tables.iter().any(|t| t.name == "Sales"));
    }

    #[test]
    fn bim_perspective_lifecycle() {
        let bim = r#"{"model":{"tables":[]}}"#;
        let a = add_perspective_bim(bim, "View1").unwrap();
        let m =
            add_perspective_member_bim(&a, "View1", "Geo", Some((MemberKind::Column, "Country")))
                .unwrap();
        let j: Value = serde_json::from_str(&m).unwrap();
        assert_eq!(
            j["model"]["perspectives"][0]["tables"][0]["columns"][0]["name"],
            "Country"
        );
        let listed = collect_perspectives_bim(&m);
        assert_eq!(listed[0]["tableCount"], 1);
        let rm = remove_perspective_member_bim(&m, "View1", "Geo", None).unwrap();
        let jr: Value = serde_json::from_str(&rm).unwrap();
        assert_eq!(
            jr["model"]["perspectives"][0]["tables"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        let d = delete_perspective_bim(&a, "View1").unwrap();
        let jd: Value = serde_json::from_str(&d).unwrap();
        assert_eq!(jd["model"]["perspectives"].as_array().unwrap().len(), 0);
    }
}
