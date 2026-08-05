//! `semantic-model` hierarchy authoring — `add-hierarchy`, `delete-hierarchy`,
//! `list-hierarchies`.
//!
//! User hierarchies live inside a table's `definition/tables/<T>.tmdl` as a
//! `hierarchy <name>` child block with ordered `level <name>` / `column: <col>`
//! entries. fabio edits them via the shared definition read-modify-write (no
//! XMLA/TOM).

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

/// A `(levelName, columnName)` pair.
pub(super) struct Level {
    pub name: String,
    pub column: String,
}

/// Parse a `--level` spec: `Name=Column`, `Name:Column`, or bare `Column`
/// (level name defaults to the column name).
pub(super) fn parse_level_spec(spec: &str) -> Result<Level> {
    let (name, column) = if let Some((n, c)) = spec.split_once('=') {
        (n.trim(), c.trim())
    } else if let Some((n, c)) = spec.split_once(':') {
        (n.trim(), c.trim())
    } else {
        (spec.trim(), spec.trim())
    };
    if column.is_empty() {
        return Err(FabioError::invalid_input(format!("Invalid --level '{spec}'")).into());
    }
    Ok(Level {
        name: name.to_string(),
        column: column.to_string(),
    })
}

// ── add-hierarchy ─────────────────────────────────────────────────────────────

pub(super) async fn add_hierarchy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    name: &str,
    levels: &[Level],
) -> Result<()> {
    let op = "semantic-model add-hierarchy";
    if levels.is_empty() {
        return Err(FabioError::invalid_input(
            "A hierarchy needs at least one --level".to_string(),
        )
        .into());
    }
    let parts = fetch_parts(client, workspace, id, op).await?;

    if hierarchy_exists(&parts, table, name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("Hierarchy '{table}.{name}' already exists."),
            "Pick a different name, or delete it first.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_hierarchy_bim(bim, table, name, levels)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, table)?;
        let block = build_hierarchy_lines(name, levels);
        let new_content = insert_table_child_lines(&parts[idx].1, &block);
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({
            "id": id,
            "table": table,
            "hierarchy": name,
            "levels": levels.iter().map(|l| &l.name).collect::<Vec<_>>(),
        }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "hierarchy_added", "id": id, "table": table, "hierarchy": name }),
        "status",
    );
    Ok(())
}

fn build_hierarchy_lines(name: &str, levels: &[Level]) -> Vec<String> {
    let mut block: Vec<String> = Vec::new();
    block.push(format!("\thierarchy {}", quote_tmdl_name(name)));
    for l in levels {
        block.push(String::new());
        block.push(format!("\t\tlevel {}", quote_tmdl_name(&l.name)));
        block.push(format!("\t\t\tcolumn: {}", quote_tmdl_name(&l.column)));
    }
    block
}

// ── delete-hierarchy ──────────────────────────────────────────────────────────

pub(super) async fn delete_hierarchy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model delete-hierarchy";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_hierarchy_bim(bim, table, name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let idx = find_table_file(&parts, table)?;
        let (new_content, removed) = delete_hierarchy_tmdl(&parts[idx].1, name);
        if !removed {
            return Err(
                FabioError::not_found(format!("Hierarchy '{table}.{name}' not found")).into(),
            );
        }
        let mut out = parts.clone();
        out[idx].1 = new_content;
        out
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "table": table, "hierarchy": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "hierarchy_deleted", "id": id, "table": table, "hierarchy": name }),
        "status",
    );
    Ok(())
}

fn delete_hierarchy_tmdl(content: &str, name: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = child_span(&lines, "hierarchy", name) else {
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

// ── list-hierarchies ──────────────────────────────────────────────────────────

pub(super) async fn list_hierarchies(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: Option<&str>,
) -> Result<()> {
    let op = "semantic-model list-hierarchies";
    let parts = fetch_parts(client, workspace, id, op).await?;
    let hierarchies = collect_hierarchies(&parts, table);
    output::render_list(
        cli,
        &hierarchies,
        &["table", "name", "levelCount"],
        &["TABLE", "NAME", "LEVELS"],
        "name",
    );
    Ok(())
}

fn collect_hierarchies(parts: &[(String, String)], table: Option<&str>) -> Vec<Value> {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_hierarchies_bim(bim, table);
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
        for (hname, levels) in parse_hierarchies_tmdl(c) {
            out.push(serde_json::json!({
                "table": tname,
                "name": hname,
                "levelCount": levels.len(),
                "levels": levels,
            }));
        }
    }
    out
}

/// Parse `(hierarchyName, [levelName])` pairs from a table file.
fn parse_hierarchies_tmdl(content: &str) -> Vec<(String, Vec<String>)> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if tab_indent(lines[i]) == 1
            && let Some(hname) = decl_name(lines[i].trim_start_matches('\t'), "hierarchy")
        {
            let mut levels = Vec::new();
            let mut j = i + 1;
            while j < lines.len() && (lines[j].trim().is_empty() || tab_indent(lines[j]) >= 2) {
                if tab_indent(lines[j]) == 2
                    && let Some(lname) = decl_name(lines[j].trim_start_matches('\t'), "level")
                {
                    levels.push(lname);
                }
                j += 1;
            }
            out.push((hname, levels));
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn hierarchy_exists(parts: &[(String, String)], table: &str, name: &str) -> bool {
    collect_hierarchies(parts, Some(table))
        .iter()
        .any(|h| h.get("name").and_then(Value::as_str) == Some(name))
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

fn add_hierarchy_bim(bim: &str, table: &str, name: &str, levels: &[Level]) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let level_json: Vec<Value> = levels
        .iter()
        .enumerate()
        .map(|(i, l)| serde_json::json!({ "name": l.name, "ordinal": i, "column": l.column }))
        .collect();
    bim_table(&mut j, table)?
        .as_object_mut()
        .unwrap()
        .entry("hierarchies")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({ "name": name, "levels": level_json }));
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_hierarchy_bim(bim: &str, table: &str, name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let hs = bim_table(&mut j, table)?
        .get_mut("hierarchies")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| FabioError::not_found(format!("Hierarchy '{table}.{name}' not found")))?;
    let before = hs.len();
    hs.retain(|h| h.get("name").and_then(Value::as_str) != Some(name));
    if hs.len() == before {
        return Err(FabioError::not_found(format!("Hierarchy '{table}.{name}' not found")).into());
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn collect_hierarchies_bim(bim: &str, table: Option<&str>) -> Vec<Value> {
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
            if let Some(hs) = t.get("hierarchies").and_then(Value::as_array) {
                for h in hs {
                    let levels: Vec<String> = h
                        .get("levels")
                        .and_then(Value::as_array)
                        .map(|ls| {
                            ls.iter()
                                .filter_map(|l| l.get("name").and_then(Value::as_str))
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default();
                    out.push(serde_json::json!({
                        "table": tname,
                        "name": h.get("name").and_then(Value::as_str).unwrap_or(""),
                        "levelCount": levels.len(),
                        "levels": levels,
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
        "table Geo\n\n\tcolumn Country\n\t\tdataType: string\n\t\tsourceColumn: Country\n\n\tcolumn City\n\t\tdataType: string\n\t\tsourceColumn: City\n\n\tpartition Geo = m\n\t\tsource = let x = 1 in x\n".to_string()
    }

    #[test]
    fn parse_level_spec_forms() {
        let a = parse_level_spec("Country").unwrap();
        assert_eq!((a.name.as_str(), a.column.as_str()), ("Country", "Country"));
        let b = parse_level_spec("Yr=Year").unwrap();
        assert_eq!((b.name.as_str(), b.column.as_str()), ("Yr", "Year"));
        let c = parse_level_spec("Mon:Month").unwrap();
        assert_eq!((c.name.as_str(), c.column.as_str()), ("Mon", "Month"));
    }

    #[test]
    fn add_hierarchy_inserts_after_scalar_props() {
        let levels = vec![
            Level {
                name: "Country".into(),
                column: "Country".into(),
            },
            Level {
                name: "City".into(),
                column: "City".into(),
            },
        ];
        let block = build_hierarchy_lines("Geography", &levels);
        let out = insert_table_child_lines(&table_tmdl(), &block);
        assert!(out.contains("\thierarchy Geography"));
        assert!(out.contains("\t\tlevel Country"));
        assert!(out.contains("\t\t\tcolumn: Country"));
        assert!(out.contains("\t\tlevel City"));
    }

    #[test]
    fn parse_and_delete_hierarchy() {
        let with = "table Geo\n\n\thierarchy Geography\n\n\t\tlevel Country\n\t\t\tcolumn: Country\n\n\t\tlevel City\n\t\t\tcolumn: City\n\n\tcolumn Country\n\t\tdataType: string\n\t\tsourceColumn: Country\n";
        let parsed = parse_hierarchies_tmdl(with);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "Geography");
        assert_eq!(parsed[0].1, vec!["Country".to_string(), "City".to_string()]);
        let (out, removed) = delete_hierarchy_tmdl(with, "Geography");
        assert!(removed);
        assert!(!out.contains("hierarchy Geography"));
        assert!(out.contains("column Country"));
    }

    #[test]
    fn bim_hierarchy_lifecycle() {
        let bim = r#"{"model":{"tables":[{"name":"Geo","columns":[{"name":"Country"},{"name":"City"}]}]}}"#;
        let levels = vec![
            Level {
                name: "Country".into(),
                column: "Country".into(),
            },
            Level {
                name: "City".into(),
                column: "City".into(),
            },
        ];
        let added = add_hierarchy_bim(bim, "Geo", "Geography", &levels).unwrap();
        let j: Value = serde_json::from_str(&added).unwrap();
        assert_eq!(
            j["model"]["tables"][0]["hierarchies"][0]["name"],
            "Geography"
        );
        assert_eq!(
            j["model"]["tables"][0]["hierarchies"][0]["levels"][1]["ordinal"],
            1
        );
        let listed = collect_hierarchies_bim(&added, Some("Geo"));
        assert_eq!(listed[0]["levelCount"], 2);
        let deleted = delete_hierarchy_bim(&added, "Geo", "Geography").unwrap();
        let jd: Value = serde_json::from_str(&deleted).unwrap();
        assert_eq!(
            jd["model"]["tables"][0]["hierarchies"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
}
