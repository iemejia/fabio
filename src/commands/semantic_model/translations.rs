//! `semantic-model` translation / culture authoring — `add-culture`,
//! `delete-culture`, `set-translation`, `list-cultures`.
//!
//! Cultures live in `definition/cultures/<culture>.tmdl` and are `ref`-ed from
//! `model.tmdl` (`ref cultureInfo <culture>`). A culture file is a nested
//! translation tree: `cultureInfo <c>` → `translations` → `model <M>` →
//! `table <T>` (with `caption:`) → `column <C>`/`measure <M>` (with `caption:`).
//! fabio parses that tree into a small model, edits it, and re-renders — this is
//! more robust than line-editing the nested indentation. Edits go through the
//! shared definition read-modify-write (no XMLA/TOM).

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::analyze::tab_indent;
use super::tmdl::{
    add_model_ref, fetch_parts, model_name, part_content, push_parts, quote_tmdl_name,
    remove_model_ref, remove_part, replace_part, upsert_part,
};

const MODEL_TMDL: &str = "definition/model.tmdl";

fn culture_path(culture: &str) -> String {
    format!("definition/cultures/{culture}.tmdl")
}

fn is_culture_file(path: &str) -> bool {
    path.starts_with("definition/cultures/")
        && std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tmdl"))
}

// ── parsed culture model ──────────────────────────────────────────────────────

#[derive(Default)]
struct TableTr {
    name: String,
    caption: Option<String>,
    columns: Vec<(String, String)>,
    measures: Vec<(String, String)>,
}

#[derive(Default)]
struct Culture {
    name: String,
    model: String,
    tables: Vec<TableTr>,
}

impl Culture {
    fn table_mut(&mut self, name: &str) -> &mut TableTr {
        if let Some(pos) = self.tables.iter().position(|t| t.name == name) {
            &mut self.tables[pos]
        } else {
            self.tables.push(TableTr {
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

fn parse_culture(content: &str) -> Culture {
    let mut c = Culture {
        model: "Model".to_string(),
        ..Default::default()
    };
    let mut cur_table: Option<usize> = None;
    // Which child (column/measure) the next `caption:` at indent 5 belongs to.
    let mut pending: Option<(bool, String)> = None; // (is_column, name)
    for line in content.lines() {
        let indent = tab_indent(line);
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("cultureInfo ") {
            c.name = strip_bare(rest);
        } else if let Some(rest) = t.strip_prefix("model ") {
            c.model = strip_bare(rest);
        } else if indent == 3 && t.starts_with("table ") {
            let name = strip_bare(&t["table ".len()..]);
            c.tables.push(TableTr {
                name,
                ..Default::default()
            });
            cur_table = Some(c.tables.len() - 1);
            pending = None;
        } else if indent == 4 && t.starts_with("column ") {
            pending = Some((true, strip_bare(&t["column ".len()..])));
        } else if indent == 4 && t.starts_with("measure ") {
            pending = Some((false, strip_bare(&t["measure ".len()..])));
        } else if let Some(cap) = t.strip_prefix("caption:") {
            let cap = cap.trim().to_string();
            if indent == 4 {
                // caption directly on the table
                if let Some(ti) = cur_table {
                    c.tables[ti].caption = Some(cap);
                }
            } else if indent == 5
                && let (Some(ti), Some((is_col, name))) = (cur_table, pending.take())
            {
                if is_col {
                    c.tables[ti].columns.push((name, cap));
                } else {
                    c.tables[ti].measures.push((name, cap));
                }
            }
        }
    }
    c
}

fn render_culture(c: &Culture) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "cultureInfo {}", quote_tmdl_name(&c.name));
    s.push_str("\ttranslations\n");
    let _ = writeln!(s, "\t\tmodel {}", quote_tmdl_name(&c.model));
    for t in &c.tables {
        if t.caption.is_none() && t.columns.is_empty() && t.measures.is_empty() {
            continue;
        }
        let _ = writeln!(s, "\t\t\ttable {}", quote_tmdl_name(&t.name));
        if let Some(cap) = &t.caption {
            let _ = writeln!(s, "\t\t\t\tcaption: {cap}");
        }
        for (name, cap) in &t.columns {
            let _ = writeln!(s, "\t\t\t\tcolumn {}", quote_tmdl_name(name));
            let _ = writeln!(s, "\t\t\t\t\tcaption: {cap}");
        }
        for (name, cap) in &t.measures {
            let _ = writeln!(s, "\t\t\t\tmeasure {}", quote_tmdl_name(name));
            let _ = writeln!(s, "\t\t\t\t\tcaption: {cap}");
        }
    }
    s
}

/// Set a caption on a table/column/measure in the parsed culture. Returns false
/// only for an impossible combination (guarded earlier).
fn apply_translation(
    c: &mut Culture,
    table: &str,
    column: Option<&str>,
    measure: Option<&str>,
    caption: &str,
) {
    let t = c.table_mut(table);
    if let Some(col) = column {
        if let Some(entry) = t.columns.iter_mut().find(|(n, _)| n == col) {
            entry.1 = caption.to_string();
        } else {
            t.columns.push((col.to_string(), caption.to_string()));
        }
    } else if let Some(m) = measure {
        if let Some(entry) = t.measures.iter_mut().find(|(n, _)| n == m) {
            entry.1 = caption.to_string();
        } else {
            t.measures.push((m.to_string(), caption.to_string()));
        }
    } else {
        t.caption = Some(caption.to_string());
    }
}

// ── add-culture ───────────────────────────────────────────────────────────────

pub(super) async fn add_culture(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    culture: &str,
) -> Result<()> {
    let op = "semantic-model add-culture";
    let parts = fetch_parts(client, workspace, id, op).await?;

    if culture_exists(&parts, culture) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("Culture '{culture}' already exists."),
            "Use `set-translation` to add translations, or pick a different culture.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_culture_bim(bim, culture)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let model = model_name(part_content(&parts, MODEL_TMDL).unwrap_or(""));
        let content = format!(
            "cultureInfo {}\n\ttranslations\n\t\tmodel {}\n",
            quote_tmdl_name(culture),
            quote_tmdl_name(&model)
        );
        let with_culture = upsert_part(&parts, &culture_path(culture), &content);
        let model_tmdl = part_content(&with_culture, MODEL_TMDL).unwrap_or("");
        let new_model = add_model_ref(model_tmdl, "cultureInfo", culture);
        replace_part(&with_culture, MODEL_TMDL, &new_model)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "culture": culture }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "culture_added", "id": id, "culture": culture }),
        "status",
    );
    Ok(())
}

// ── delete-culture ────────────────────────────────────────────────────────────

pub(super) async fn delete_culture(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    culture: &str,
) -> Result<()> {
    let op = "semantic-model delete-culture";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_culture_bim(bim, culture)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        if !culture_exists(&parts, culture) {
            return Err(culture_not_found(culture));
        }
        let without = remove_part(&parts, &culture_path(culture));
        let model_tmdl = part_content(&without, MODEL_TMDL).unwrap_or("");
        let new_model = remove_model_ref(model_tmdl, "cultureInfo", culture);
        replace_part(&without, MODEL_TMDL, &new_model)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "culture": culture }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "culture_deleted", "id": id, "culture": culture }),
        "status",
    );
    Ok(())
}

// ── set-translation ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn set_translation(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    culture: &str,
    table: &str,
    column: Option<&str>,
    measure: Option<&str>,
    caption: &str,
) -> Result<()> {
    let op = "semantic-model set-translation";
    if column.is_some() && measure.is_some() {
        return Err(FabioError::invalid_input(
            "Specify at most one of --column / --measure".to_string(),
        )
        .into());
    }
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = set_translation_bim(bim, culture, table, column, measure, caption)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let content = part_content(&parts, &culture_path(culture))
            .ok_or_else(|| culture_not_found(culture))?;
        let mut c = parse_culture(content);
        apply_translation(&mut c, table, column, measure, caption);
        replace_part(&parts, &culture_path(culture), &render_culture(&c))
    };

    let target = column
        .map(|x| format!("{table}[{x}]"))
        .or_else(|| measure.map(std::string::ToString::to_string))
        .unwrap_or_else(|| table.to_string());
    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "culture": culture, "target": target, "caption": caption }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "translation_set", "id": id, "culture": culture, "target": target }),
        "status",
    );
    Ok(())
}

// ── list-cultures ─────────────────────────────────────────────────────────────

pub(super) async fn list_cultures(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let op = "semantic-model list-cultures";
    let parts = fetch_parts(client, workspace, id, op).await?;
    let cultures = collect_cultures(&parts);
    output::render_list(
        cli,
        &cultures,
        &["culture", "translationCount"],
        &["CULTURE", "TRANSLATIONS"],
        "culture",
    );
    Ok(())
}

fn collect_cultures(parts: &[(String, String)]) -> Vec<Value> {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_cultures_bim(bim);
    }
    parts
        .iter()
        .filter(|(p, _)| is_culture_file(p))
        .map(|(_, content)| {
            let c = parse_culture(content);
            let count: usize = c
                .tables
                .iter()
                .map(|t| usize::from(t.caption.is_some()) + t.columns.len() + t.measures.len())
                .sum();
            serde_json::json!({ "culture": c.name, "translationCount": count })
        })
        .collect()
}

fn culture_exists(parts: &[(String, String)], culture: &str) -> bool {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_cultures_bim(bim)
            .iter()
            .any(|c| c.get("culture").and_then(Value::as_str) == Some(culture));
    }
    parts.iter().any(|(p, _)| p == &culture_path(culture))
}

fn culture_not_found(culture: &str) -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::NotFound,
        format!("Culture '{culture}' not found in the model definition."),
        "List cultures with `fabio semantic-model list-cultures`, or add it with `add-culture`."
            .to_string(),
    )
    .into()
}

// ── model.bim editors ─────────────────────────────────────────────────────────

fn bim_cultures_mut(j: &mut Value) -> Result<&mut Vec<Value>> {
    j.get_mut("model")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no model object"))?
        .entry("cultures")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| FabioError::invalid_input("cultures is not an array").into())
}

fn add_culture_bim(bim: &str, culture: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let model_name = j
        .get("model")
        .and_then(|m| m.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("Model")
        .to_string();
    bim_cultures_mut(&mut j)?.push(serde_json::json!({
        "name": culture,
        "translations": { "model": { "name": model_name } }
    }));
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_culture_bim(bim: &str, culture: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let cultures = bim_cultures_mut(&mut j)?;
    let before = cultures.len();
    cultures.retain(|c| c.get("name").and_then(Value::as_str) != Some(culture));
    if cultures.len() == before {
        return Err(culture_not_found(culture));
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn set_translation_bim(
    bim: &str,
    culture: &str,
    table: &str,
    column: Option<&str>,
    measure: Option<&str>,
    caption: &str,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let cult = bim_cultures_mut(&mut j)?
        .iter_mut()
        .find(|c| c.get("name").and_then(Value::as_str) == Some(culture))
        .ok_or_else(|| culture_not_found(culture))?;
    let tables = cult
        .pointer_mut("/translations/model")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| FabioError::invalid_input("culture has no translations.model"))?
        .entry("tables")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .unwrap();
    // find or create the table node
    if !tables
        .iter()
        .any(|t| t.get("name").and_then(Value::as_str) == Some(table))
    {
        tables.push(serde_json::json!({ "name": table }));
    }
    let t = tables
        .iter_mut()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(table))
        .unwrap();
    if let Some(col) = column {
        set_child_caption_bim(t, "columns", col, caption);
    } else if let Some(m) = measure {
        set_child_caption_bim(t, "measures", m, caption);
    } else {
        t["translatedCaption"] = Value::from(caption);
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn set_child_caption_bim(table: &mut Value, key: &str, name: &str, caption: &str) {
    let arr = table
        .as_object_mut()
        .unwrap()
        .entry(key)
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .unwrap();
    if let Some(child) = arr
        .iter_mut()
        .find(|c| c.get("name").and_then(Value::as_str) == Some(name))
    {
        child["translatedCaption"] = Value::from(caption);
    } else {
        arr.push(serde_json::json!({ "name": name, "translatedCaption": caption }));
    }
}

fn collect_cultures_bim(bim: &str) -> Vec<Value> {
    let Ok(j) = serde_json::from_str::<Value>(bim) else {
        return Vec::new();
    };
    j.get("model")
        .and_then(|m| m.get("cultures"))
        .and_then(Value::as_array)
        .map(|cultures| {
            cultures
                .iter()
                .map(|c| {
                    let count = c
                        .pointer("/translations/model/tables")
                        .and_then(Value::as_array)
                        .map_or(0, |tables| {
                            tables
                                .iter()
                                .map(|t| {
                                    usize::from(t.get("translatedCaption").is_some())
                                        + t.get("columns")
                                            .and_then(Value::as_array)
                                            .map_or(0, Vec::len)
                                        + t.get("measures")
                                            .and_then(Value::as_array)
                                            .map_or(0, Vec::len)
                                })
                                .sum()
                        });
                    serde_json::json!({
                        "culture": c.get("name").and_then(Value::as_str).unwrap_or(""),
                        "translationCount": count,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn culture_tmdl() -> String {
        "cultureInfo fr-FR\n\ttranslations\n\t\tmodel Model\n\t\t\ttable Sales\n\t\t\t\tcaption: Ventes\n\t\t\t\tcolumn Amount\n\t\t\t\t\tcaption: Montant\n\t\t\t\tmeasure Total\n\t\t\t\t\tcaption: Somme\n".to_string()
    }

    #[test]
    fn parse_culture_reads_tree() {
        let c = parse_culture(&culture_tmdl());
        assert_eq!(c.name, "fr-FR");
        assert_eq!(c.model, "Model");
        assert_eq!(c.tables.len(), 1);
        assert_eq!(c.tables[0].caption.as_deref(), Some("Ventes"));
        assert_eq!(
            c.tables[0].columns,
            vec![("Amount".into(), "Montant".into())]
        );
        assert_eq!(c.tables[0].measures, vec![("Total".into(), "Somme".into())]);
    }

    #[test]
    fn round_trip_render() {
        let c = parse_culture(&culture_tmdl());
        let rendered = render_culture(&c);
        // re-parse yields the same structure
        let c2 = parse_culture(&rendered);
        assert_eq!(c2.tables[0].caption.as_deref(), Some("Ventes"));
        assert_eq!(c2.tables[0].columns[0].1, "Montant");
    }

    #[test]
    fn apply_translation_adds_and_updates() {
        let mut c = parse_culture(&culture_tmdl());
        // update existing column caption
        apply_translation(&mut c, "Sales", Some("Amount"), None, "Montant total");
        assert_eq!(c.tables[0].columns[0].1, "Montant total");
        // add a new table caption
        apply_translation(&mut c, "Customer", None, None, "Client");
        assert!(
            c.tables
                .iter()
                .any(|t| t.name == "Customer" && t.caption.as_deref() == Some("Client"))
        );
        // add a measure to an existing table
        apply_translation(&mut c, "Sales", None, Some("Avg"), "Moyenne");
        assert!(
            c.tables[0]
                .measures
                .iter()
                .any(|(n, cap)| n == "Avg" && cap == "Moyenne")
        );
    }

    #[test]
    fn bim_culture_lifecycle() {
        let bim = r#"{"model":{"name":"Model","tables":[]}}"#;
        let added = add_culture_bim(bim, "fr-FR").unwrap();
        let withtr =
            set_translation_bim(&added, "fr-FR", "Sales", Some("Amount"), None, "Montant").unwrap();
        let j: Value = serde_json::from_str(&withtr).unwrap();
        let col = &j["model"]["cultures"][0]["translations"]["model"]["tables"][0]["columns"][0];
        assert_eq!(col["translatedCaption"], "Montant");
        let cults = collect_cultures_bim(&withtr);
        assert_eq!(cults[0]["translationCount"], 1);
        let deleted = delete_culture_bim(&withtr, "fr-FR").unwrap();
        let jd: Value = serde_json::from_str(&deleted).unwrap();
        assert_eq!(jd["model"]["cultures"].as_array().unwrap().len(), 0);
    }
}
