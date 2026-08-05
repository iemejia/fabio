//! `semantic-model analyze` and `semantic-model measure-dependencies` — read-only
//! model-quality tooling inspired by the Fabric "Semantic model best practices"
//! guidance (Best Practice Analyzer / Memory Analyzer / measure dependencies).
//!
//! Both work purely over the model's metadata via the DAX `INFO.VIEW.*` functions
//! (the Analysis Services Schema Rowsets) plus optional `DISTINCTCOUNT`
//! cardinality probes — the same `executeQueries` surface fabio already uses. No
//! new API, no definition parsing.

use std::collections::{BTreeSet, HashSet};

use anyhow::{Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use regex::Regex;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_forbidden;
use crate::output;

use super::operations::{fetch_info_view, run_dax_rows};

// ── small metadata accessors ──────────────────────────────────────────────────

fn s<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or_default()
}
fn b(row: &Value, key: &str) -> bool {
    row.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A Power BI column `DataType` (from `INFO.VIEW.COLUMNS`) that aggregates numerically.
fn is_numeric_type(dt: &str) -> bool {
    matches!(
        dt,
        "Integer" | "Int64" | "Whole Number" | "Decimal" | "Double" | "Number" | "Currency"
    )
}

/// A date/time-ish column (by type or name) — used for the ambiguous-dates rule.
fn is_dateish(name: &str, dt: &str) -> bool {
    matches!(dt, "DateTime" | "Date" | "Time" | "Date/Time")
        || name.to_ascii_lowercase().contains("date")
}

// ── best-practice rules ───────────────────────────────────────────────────────

const SEVERITY_ORDER: [&str; 3] = ["info", "warning", "error"];

fn severity_rank(sev: &str) -> usize {
    SEVERITY_ORDER.iter().position(|s| *s == sev).unwrap_or(0)
}

fn issue(
    rule: &str,
    severity: &str,
    object_type: &str,
    object: &str,
    message: &str,
    fix: &str,
) -> Value {
    serde_json::json!({
        "rule": rule,
        "severity": severity,
        "objectType": object_type,
        "object": object,
        "message": message,
        "fix": fix,
    })
}

/// Heuristic: is this a cryptic, non-business-friendly name (e.g. `TR_AMT`,
/// `F_SLS`, `DIM_GEO_01`)? Conservative to avoid false positives on real names
/// like `StoreName` or `Total Revenue`.
pub(super) fn is_non_descriptive(name: &str) -> bool {
    let letters: Vec<char> = name.chars().filter(char::is_ascii_alphabetic).collect();
    if letters.is_empty() {
        return false;
    }
    let upper = letters.iter().filter(|c| c.is_ascii_uppercase()).count();
    let upper_ratio = f64::from(u32::try_from(upper).unwrap_or(u32::MAX))
        / f64::from(u32::try_from(letters.len()).unwrap_or(u32::MAX));
    let has_underscore = name.contains('_');
    let has_digit = name.chars().any(|c| c.is_ascii_digit());
    let all_upper = upper_ratio > 0.99;
    // Underscore + (a digit or mostly-uppercase) => coded name; or a very short
    // all-caps token with no spaces (an abbreviation).
    (has_underscore && (has_digit || upper_ratio > 0.6))
        || (all_upper && !name.contains(' ') && letters.len() <= 6)
}

/// Run every applicable rule and return the issues. `cardinality` (optional) maps
/// `"Table[Column]"` → distinct-value count for the high-cardinality rule.
#[allow(clippy::too_many_lines)]
pub(super) fn run_rules(
    tables: &[Value],
    columns: &[Value],
    measures: &[Value],
    relationships: &[Value],
    cardinality: Option<&std::collections::HashMap<String, u64>>,
    high_card_threshold: u64,
) -> Vec<Value> {
    let mut issues = Vec::new();

    // Visible, non-synthetic tables/columns/measures only (AI ignores hidden).
    let visible_tables: Vec<&Value> = tables.iter().filter(|t| !b(t, "IsHidden")).collect();

    // 1. Missing descriptions (AI grounding).
    for t in &visible_tables {
        if s(t, "Description").is_empty() {
            issues.push(issue("missing-description", "info", "table", s(t, "Name"),
                "Table has no description; the DAX-generation tool uses descriptions to ground answers.",
                "Add a description to this table (helps AI and report authors)."));
        }
    }
    for c in columns {
        if b(c, "IsHidden") || s(c, "Type") == "RowNumber" {
            continue;
        }
        if s(c, "Description").is_empty() {
            issues.push(issue(
                "missing-description",
                "info",
                "column",
                &format!("{}[{}]", s(c, "Table"), s(c, "Name")),
                "Column has no description.",
                "Add a business-friendly description, especially for AI data schemas.",
            ));
        }
    }
    for m in measures {
        if b(m, "IsHidden") {
            continue;
        }
        if s(m, "Description").is_empty() {
            issues.push(issue(
                "missing-description",
                "info",
                "measure",
                s(m, "Name"),
                "Measure has no description.",
                "Describe what the measure computes so the AI/query tool interprets it correctly.",
            ));
        }
    }

    // 2. Non-descriptive names.
    for t in &visible_tables {
        if is_non_descriptive(s(t, "Name")) {
            issues.push(issue("non-descriptive-name", "warning", "table", s(t, "Name"),
                "Cryptic table name provides no context for the DAX-generation tool.",
                "Rename to a business-friendly name (e.g. 'Sales' not 'F_SLS'), or add a synonym/description."));
        }
    }
    for c in columns {
        if b(c, "IsHidden") || s(c, "Type") == "RowNumber" {
            continue;
        }
        if is_non_descriptive(s(c, "Name")) {
            issues.push(issue(
                "non-descriptive-name",
                "warning",
                "column",
                &format!("{}[{}]", s(c, "Table"), s(c, "Name")),
                "Cryptic column name (e.g. TR_AMT, DIM_GEO_01).",
                "Rename to a clear business term, or provide a description/synonym.",
            ));
        }
    }
    for m in measures {
        if is_non_descriptive(s(m, "Name")) {
            issues.push(issue(
                "non-descriptive-name",
                "warning",
                "measure",
                s(m, "Name"),
                "Cryptic measure name.",
                "Use a business-friendly measure name (e.g. 'Total Revenue').",
            ));
        }
    }

    // 3. Implicit-aggregation risk: an identifier column that still aggregates.
    for c in columns {
        if b(c, "IsHidden") {
            continue;
        }
        let name = s(c, "Name");
        let looks_like_key = b(c, "IsKey")
            || name.eq_ignore_ascii_case("id")
            || name.to_ascii_lowercase().ends_with("id")
            || name.to_ascii_lowercase().ends_with("key")
            || name.to_ascii_lowercase().ends_with("code");
        let summarize = s(c, "SummarizeBy");
        if looks_like_key
            && is_numeric_type(s(c, "DataType"))
            && !summarize.eq_ignore_ascii_case("none")
        {
            issues.push(issue("implicit-aggregation", "warning", "column",
                &format!("{}[{}]", s(c, "Table"), name),
                &format!("Identifier column has SummarizeBy='{summarize}', so it aggregates by default (implicit measure)."),
                "Set the column's default summarization to None, and create explicit measures for real metrics."));
        }
    }

    // 4. Duplicate / overlapping measure names.
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for m in measures {
        let name = s(m, "Name");
        let norm: String = name
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .flat_map(char::to_lowercase)
            .collect();
        if norm.is_empty() {
            continue;
        }
        if let Some(first) = seen.get(&norm) {
            issues.push(issue("duplicate-measure", "warning", "measure", name,
                &format!("Measure name is a near-duplicate of '{first}' (ambiguous for the DAX tool)."),
                "Consolidate duplicate measures or exclude the redundant one from the AI data schema."));
        } else {
            seen.insert(norm, name.to_string());
        }
    }

    // 5. Ambiguous date fields.
    let date_cols: Vec<String> = columns
        .iter()
        .filter(|c| !b(c, "IsHidden") && s(c, "Type") != "RowNumber")
        .filter(|c| is_dateish(s(c, "Name"), s(c, "DataType")))
        .map(|c| format!("{}[{}]", s(c, "Table"), s(c, "Name")))
        .collect();
    if date_cols.len() > 1 {
        issues.push(issue("ambiguous-dates", "info", "model", "(model)",
            &format!("Multiple date fields ({}) can confuse the AI about which to use.", date_cols.join(", ")),
            "Mark a date table and/or add AI instructions specifying the default date field per question type."));
    }

    // 6. Relationship hygiene.
    for r in relationships {
        let name = s(r, "Name");
        let rel = format!(
            "{}[{}] -> {}[{}]",
            s(r, "FromTable"),
            s(r, "FromColumn"),
            s(r, "ToTable"),
            s(r, "ToColumn")
        );
        let label = if name.is_empty() {
            rel.clone()
        } else {
            name.to_string()
        };
        if !b(r, "IsActive") {
            issues.push(issue("inactive-relationship", "info", "relationship", &label,
                "Inactive relationship — only used via USERELATIONSHIP; can confuse auto-generated DAX.",
                "Confirm the inactive relationship is intentional; document the intended USERELATIONSHIP path."));
        }
        if s(r, "CrossFilteringBehavior").eq_ignore_ascii_case("BothDirections") {
            issues.push(issue("bidirectional-relationship", "warning", "relationship", &label,
                "Bidirectional cross-filtering can cause ambiguous filter propagation and slow queries.",
                "Prefer single-direction relationships; use bidirectional only when required."));
        }
        if s(r, "FromCardinality").eq_ignore_ascii_case("Many")
            && s(r, "ToCardinality").eq_ignore_ascii_case("Many")
        {
            issues.push(issue("many-to-many", "warning", "relationship", &label,
                "Many-to-many relationship — often a sign of a missing dimension (non-star schema).",
                "Introduce a bridge/dimension table to model this as a star schema."));
        }
    }

    // 7. Schema shape (star schema).
    if visible_tables.len() == 1 {
        issues.push(issue(
            "flat-schema",
            "info",
            "model",
            "(model)",
            "Single flat/denormalized table — DAX is optimized for a star schema.",
            "Split into fact + dimension tables where practical (unpivot wide tables).",
        ));
    } else if visible_tables.len() > 1 && relationships.is_empty() {
        issues.push(issue(
            "no-relationships",
            "warning",
            "model",
            "(model)",
            "Multiple tables but no relationships — the model is not a star schema.",
            "Define relationships between fact and dimension tables.",
        ));
    }

    // 8. Calculated columns (materialize upstream, esp. Direct Lake).
    for c in columns {
        if s(c, "Type") == "Calculated" {
            issues.push(issue(
                "calculated-column",
                "info",
                "column",
                &format!("{}[{}]", s(c, "Table"), s(c, "Name")),
                "Calculated column increases model size and is not supported in Direct Lake.",
                "Materialize the computation upstream (in the lakehouse/warehouse) where possible.",
            ));
        }
    }

    // 9. High cardinality (opt-in — needs DISTINCTCOUNT probes).
    if let Some(card) = cardinality {
        for c in columns {
            if b(c, "IsHidden") || s(c, "Type") == "RowNumber" {
                continue;
            }
            let key = format!("{}[{}]", s(c, "Table"), s(c, "Name"));
            if let Some(&n) = card.get(&key)
                && n > high_card_threshold
            {
                issues.push(issue("high-cardinality", "warning", "column", &key,
                    &format!("High-cardinality column ({n} distinct values) inflates model memory."),
                    "Reduce precision, split, or remove the column if not needed for analysis/reporting."));
            }
        }
    }

    issues
}

// ── analyze command ───────────────────────────────────────────────────────────

pub(super) async fn analyze(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    with_cardinality: bool,
    min_severity: &str,
    strict: bool,
) -> Result<()> {
    let tables = fetch_info_view(client, workspace, id, "TABLES").await?;
    let columns = fetch_info_view(client, workspace, id, "COLUMNS").await?;
    let measures = fetch_info_view(client, workspace, id, "MEASURES").await?;
    let relationships = fetch_info_view(client, workspace, id, "RELATIONSHIPS").await?;

    let cardinality = if with_cardinality {
        Some(probe_cardinality(client, workspace, id, &columns).await?)
    } else {
        None
    };

    let mut issues = run_rules(
        &tables,
        &columns,
        &measures,
        &relationships,
        cardinality.as_ref(),
        HIGH_CARDINALITY_THRESHOLD,
    );

    // Filter to >= min_severity.
    let min_rank = severity_rank(min_severity);
    issues.retain(|i| severity_rank(i["severity"].as_str().unwrap_or("info")) >= min_rank);

    let count = |sev: &str| issues.iter().filter(|i| i["severity"] == sev).count();
    let (errors, warnings, infos) = (count("error"), count("warning"), count("info"));

    let out = serde_json::json!({
        "id": id,
        "issueCount": issues.len(),
        "summary": { "error": errors, "warning": warnings, "info": infos },
        "issues": issues,
        "cardinalityProbed": with_cardinality,
    });
    output::render_object(cli, &out, "issueCount");

    if strict && !issues.is_empty() {
        bail!(
            "semantic-model analyze found {} issue(s) at or above '{min_severity}' severity",
            issues.len()
        );
    }
    Ok(())
}

const HIGH_CARDINALITY_THRESHOLD: u64 = 100_000;

/// Probe distinct-value counts for every visible data column in ONE DAX query
/// (`EVALUATE ROW("c0", DISTINCTCOUNT('T'[C]), ...)`). Returns `"Table[Col]"` →
/// count. Best-effort: on any failure returns an empty map (cardinality is opt-in).
async fn probe_cardinality(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    columns: &[Value],
) -> Result<std::collections::HashMap<String, u64>> {
    let data_cols: Vec<(String, String)> = columns
        .iter()
        .filter(|c| !b(c, "IsHidden") && s(c, "Type") == "Data")
        .map(|c| (s(c, "Table").to_string(), s(c, "Name").to_string()))
        .take(200)
        .collect();
    if data_cols.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let exprs: Vec<String> = data_cols
        .iter()
        .enumerate()
        .map(|(i, (t, c))| {
            let te = t.replace('\'', "''");
            let ce = c.replace(']', "]]");
            format!("\"c{i}\", DISTINCTCOUNT('{te}'[{ce}])")
        })
        .collect();
    let dax = format!("EVALUATE ROW({})", exprs.join(", "));
    let Ok(rows) = run_dax_rows(client, workspace, id, &dax).await else {
        return Ok(std::collections::HashMap::new());
    };
    let mut map = std::collections::HashMap::new();
    if let Some(row) = rows.first().and_then(Value::as_object) {
        for (i, (t, c)) in data_cols.iter().enumerate() {
            // executeQueries returns keys bracketed like "[c0]".
            let n = row
                .get(&format!("[c{i}]"))
                .and_then(Value::as_u64)
                .or_else(|| row.get(&format!("c{i}")).and_then(Value::as_u64));
            if let Some(n) = n {
                map.insert(format!("{t}[{c}]"), n);
            }
        }
    }
    Ok(map)
}

// ── measure-dependencies command ──────────────────────────────────────────────

/// Parse a measure's DAX `Expression` into the objects it references.
///
/// Returns `(measures, columns, tables)`:
/// * qualified refs `'Table'[Col]` / `Table[Col]` → a column `"Table[Col]"` and a table;
/// * bare refs `[X]` → a measure if `X` is a known measure, else an unqualified column.
pub(super) fn parse_measure_refs(
    expression: &str,
    measure_names: &HashSet<String>,
    column_names: &HashSet<String>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    // Table-qualified column reference.
    let re_q = Regex::new(r"(?:'([^']*)'|([A-Za-z_][A-Za-z0-9_ ]*))\[([^\]]+)\]").unwrap();
    let mut columns: BTreeSet<String> = BTreeSet::new();
    let mut tables: BTreeSet<String> = BTreeSet::new();
    for cap in re_q.captures_iter(expression) {
        let table = cap
            .get(1)
            .or_else(|| cap.get(2))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let col = cap.get(3).map(|m| m.as_str()).unwrap_or_default();
        if !table.is_empty() {
            columns.insert(format!("{table}[{col}]"));
            tables.insert(table);
        }
    }
    // Strip qualified refs, then bare `[X]` are measures (or unqualified columns).
    let stripped = re_q.replace_all(expression, " ");
    let re_b = Regex::new(r"\[([^\]]+)\]").unwrap();
    let mut measures: BTreeSet<String> = BTreeSet::new();
    for cap in re_b.captures_iter(&stripped) {
        let name = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
        if measure_names.contains(name) {
            measures.insert(name.to_string());
        } else if column_names.contains(name) {
            columns.insert(format!("[{name}]"));
        }
    }
    (
        measures.into_iter().collect(),
        columns.into_iter().collect(),
        tables.into_iter().collect(),
    )
}

pub(super) async fn measure_dependencies(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    // Measure DAX expressions are NOT exposed by INFO.VIEW.MEASURES over
    // executeQueries (the Expression column comes back null, and the raw
    // INFO.MEASURES() DMV is rejected), so read them from the model definition.
    let data = client
        .post(
            &format!("/workspaces/{workspace}/semanticModels/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model measure-dependencies", "Contributor"))?;
    let parts = decode_parts(&data);
    let (measures, measure_names, column_names) = extract_measures(&parts);

    let deps: Vec<Value> = measures
        .iter()
        .map(|m| {
            let (dep_measures, dep_columns, dep_tables) =
                parse_measure_refs(&m.expr, &measure_names, &column_names);
            serde_json::json!({
                "measure": m.name,
                "table": m.table,
                "dependsOnMeasures": dep_measures,
                "dependsOnColumns": dep_columns,
                "dependsOnTables": dep_tables,
            })
        })
        .collect();

    output::render_list(
        cli,
        &deps,
        &["measure", "table", "dependsOnMeasures", "dependsOnColumns"],
        &[
            "MEASURE",
            "TABLE",
            "DEPENDS ON MEASURES",
            "DEPENDS ON COLUMNS",
        ],
        "measure",
    );
    Ok(())
}

/// A measure and its DAX expression, extracted from the model definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MeasureDef {
    pub name: String,
    pub table: String,
    pub expr: String,
}

/// Decode a `getDefinition` response's parts into `(path, text)` pairs.
fn decode_parts(data: &Value) -> Vec<(String, String)> {
    data.get("definition")
        .and_then(|d| d.get("parts"))
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| {
                    let path = p.get("path")?.as_str()?.to_string();
                    let bytes = BASE64.decode(p.get("payload")?.as_str()?).ok()?;
                    Some((path, String::from_utf8_lossy(&bytes).into_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Extract measures + the model's measure/column name sets from the definition
/// parts. Handles both `model.bim` (TMSL JSON) and the TMDL `tables/*.tmdl` form.
pub(super) fn extract_measures(
    parts: &[(String, String)],
) -> (Vec<MeasureDef>, HashSet<String>, HashSet<String>) {
    if let Some((_, bim)) = parts.iter().find(|(p, _)| p == "model.bim") {
        return extract_measures_bim(bim);
    }
    let mut measures = Vec::new();
    let mut mnames = HashSet::new();
    let mut cnames = HashSet::new();
    for (path, content) in parts {
        let is_table_tmdl = path.starts_with("definition/tables/")
            && std::path::Path::new(path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("tmdl"));
        if is_table_tmdl {
            extract_measures_tmdl(content, &mut measures, &mut mnames, &mut cnames);
        }
    }
    (measures, mnames, cnames)
}

fn strip_tmdl_name(raw: &str) -> String {
    raw.trim().trim_matches('\'').trim().to_string()
}

/// Count leading tab characters.
fn tab_indent(line: &str) -> usize {
    line.chars().take_while(|c| *c == '\t').count()
}

/// Parse measures + column names out of one `definition/tables/<T>.tmdl` file.
/// Handles single-line (`measure X = <expr>`) and multi-line measure bodies.
fn extract_measures_tmdl(
    content: &str,
    measures: &mut Vec<MeasureDef>,
    mnames: &mut HashSet<String>,
    cnames: &mut HashSet<String>,
) {
    let lines: Vec<&str> = content.lines().collect();
    let table = lines
        .iter()
        .find(|l| l.starts_with("table "))
        .map(|l| strip_tmdl_name(&l[6..]))
        .unwrap_or_default();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start_matches('\t');
        if tab_indent(line) == 1 && trimmed.starts_with("measure ") {
            let rest = &trimmed["measure ".len()..];
            let (name_part, expr_part) = match rest.split_once('=') {
                Some((n, e)) => (n, e),
                None => (rest, ""),
            };
            let name = strip_tmdl_name(name_part);
            let mut expr = expr_part.trim().to_string();
            // Multi-line body: collect deeper-indented following lines.
            if expr.is_empty() {
                let mut j = i + 1;
                let mut body = Vec::new();
                while j < lines.len() && (lines[j].trim().is_empty() || tab_indent(lines[j]) >= 2) {
                    if !lines[j].trim().is_empty() {
                        body.push(lines[j].trim());
                    }
                    j += 1;
                }
                expr = body.join(" ");
                i = j;
            } else {
                i += 1;
            }
            if !name.is_empty() {
                mnames.insert(name.clone());
                measures.push(MeasureDef {
                    name,
                    table: table.clone(),
                    expr,
                });
            }
            continue;
        }
        if tab_indent(line) == 1 && trimmed.starts_with("column ") {
            let name = strip_tmdl_name(&trimmed["column ".len()..]);
            if !name.is_empty() {
                cnames.insert(name);
            }
        }
        i += 1;
    }
}

fn extract_measures_bim(bim: &str) -> (Vec<MeasureDef>, HashSet<String>, HashSet<String>) {
    let mut measures = Vec::new();
    let mut mnames = HashSet::new();
    let mut cnames = HashSet::new();
    let Ok(j) = serde_json::from_str::<Value>(bim) else {
        return (measures, mnames, cnames);
    };
    let empty = Vec::new();
    let tables = j
        .get("model")
        .and_then(|m| m.get("tables"))
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for t in tables {
        let table = t.get("name").and_then(Value::as_str).unwrap_or_default();
        for c in t.get("columns").and_then(Value::as_array).unwrap_or(&empty) {
            if let Some(n) = c.get("name").and_then(Value::as_str) {
                cnames.insert(n.to_string());
            }
        }
        for m in t
            .get("measures")
            .and_then(Value::as_array)
            .unwrap_or(&empty)
        {
            let Some(name) = m.get("name").and_then(Value::as_str) else {
                continue;
            };
            // expression is a string or an array of strings.
            let expr = match m.get("expression") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => String::new(),
            };
            mnames.insert(name.to_string());
            measures.push(MeasureDef {
                name: name.to_string(),
                table: table.to_string(),
                expr,
            });
        }
    }
    (measures, mnames, cnames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn non_descriptive_name_heuristic() {
        assert!(is_non_descriptive("TR_AMT"));
        assert!(is_non_descriptive("F_SLS"));
        assert!(is_non_descriptive("DIM_GEO_01"));
        assert!(is_non_descriptive("FSLS"));
        // Real names should not be flagged.
        assert!(!is_non_descriptive("StoreName"));
        assert!(!is_non_descriptive("Total Revenue"));
        assert!(!is_non_descriptive("dimstore"));
        assert!(!is_non_descriptive("SalesRegion"));
    }

    fn cols() -> Vec<Value> {
        vec![
            json!({"Name": "StoreId", "Table": "Sales", "DataType": "Integer", "Type": "Data", "IsHidden": false, "IsKey": false, "SummarizeBy": "Sum", "Description": ""}),
            json!({"Name": "Amount", "Table": "Sales", "DataType": "Decimal", "Type": "Data", "IsHidden": false, "SummarizeBy": "Sum", "Description": "Sales amount"}),
            json!({"Name": "OrderDate", "Table": "Sales", "DataType": "DateTime", "Type": "Data", "IsHidden": false, "SummarizeBy": "None", "Description": ""}),
            json!({"Name": "ShipDate", "Table": "Sales", "DataType": "DateTime", "Type": "Data", "IsHidden": false, "SummarizeBy": "None", "Description": ""}),
            json!({"Name": "Margin", "Table": "Sales", "DataType": "Decimal", "Type": "Calculated", "IsHidden": false, "SummarizeBy": "Sum", "Description": "x"}),
            json!({"Name": "RowNumber-x", "Table": "Sales", "DataType": "Integer", "Type": "RowNumber", "IsHidden": true, "Description": ""}),
        ]
    }

    #[test]
    fn rules_flag_expected_issues() {
        let tables = vec![
            json!({"Name": "Sales", "IsHidden": false, "Description": ""}),
            json!({"Name": "Store", "IsHidden": false, "Description": "Stores"}),
        ];
        let measures = vec![
            json!({"Name": "Total Sales", "IsHidden": false, "Description": ""}),
            json!({"Name": "TotalSales", "IsHidden": false, "Description": "dup"}),
        ];
        let rels: Vec<Value> = vec![];
        let issues = run_rules(&tables, &cols(), &measures, &rels, None, 100_000);
        let rules: Vec<&str> = issues.iter().map(|i| i["rule"].as_str().unwrap()).collect();

        // Identifier column (StoreId) that still sums -> implicit-aggregation.
        assert!(rules.contains(&"implicit-aggregation"));
        // Two DateTime columns -> ambiguous-dates.
        assert!(rules.contains(&"ambiguous-dates"));
        // Calculated column (Margin).
        assert!(rules.contains(&"calculated-column"));
        // Duplicate measure names (Total Sales vs TotalSales).
        assert!(rules.contains(&"duplicate-measure"));
        // >1 table but no relationships -> no-relationships.
        assert!(rules.contains(&"no-relationships"));
        // Missing description on the Sales table + StoreId column.
        assert!(rules.contains(&"missing-description"));
        // RowNumber column must be ignored (hidden + synthetic).
        assert!(
            !issues
                .iter()
                .any(|i| i["object"].as_str().unwrap_or("").contains("RowNumber"))
        );
    }

    #[test]
    fn high_cardinality_rule_uses_probe() {
        let tables = vec![json!({"Name": "Sales", "IsHidden": false, "Description": "d"})];
        let mut card = std::collections::HashMap::new();
        card.insert("Sales[StoreId]".to_string(), 5_000_000u64);
        let issues = run_rules(&tables, &cols(), &[], &[], Some(&card), 100_000);
        assert!(
            issues
                .iter()
                .any(|i| i["rule"] == "high-cardinality" && i["object"] == "Sales[StoreId]")
        );
    }

    #[test]
    fn parse_measure_refs_classifies_columns_and_measures() {
        let measures: HashSet<String> = ["Total Sales".to_string(), "Cost".to_string()]
            .into_iter()
            .collect();
        let columns: HashSet<String> = ["Amount".to_string(), "Qty".to_string()]
            .into_iter()
            .collect();
        // A measure referencing another measure + a qualified column + a bare column.
        let expr = "DIVIDE([Total Sales] - [Cost], SUM('Sales'[Amount])) + [Qty]";
        let (m, c, t) = parse_measure_refs(expr, &measures, &columns);
        assert!(m.contains(&"Total Sales".to_string()));
        assert!(m.contains(&"Cost".to_string()));
        assert!(c.contains(&"Sales[Amount]".to_string()));
        assert!(c.contains(&"[Qty]".to_string())); // unqualified column
        assert!(t.contains(&"Sales".to_string()));
    }

    #[test]
    fn parse_measure_refs_handles_unquoted_table() {
        let measures: HashSet<String> = HashSet::new();
        let columns: HashSet<String> = HashSet::new();
        let (_m, c, t) = parse_measure_refs("SUM(Sales[Amount])", &measures, &columns);
        assert_eq!(c, vec!["Sales[Amount]".to_string()]);
        assert_eq!(t, vec!["Sales".to_string()]);
    }

    #[test]
    fn extract_measures_from_tmdl_single_and_multiline() {
        // Mirrors the live TMDL Fabric returns for a model with measures.
        let tmdl = "table Sales\n\n\tmeasure 'Total Amount' = SUM('Sales'[Amount])\n\n\tmeasure 'Avg Price' =\n\t\tDIVIDE([Total Amount], [Total Qty])\n\n\tcolumn Amount\n\t\tdataType: double\n\t\tsourceColumn: Amount\n";
        let parts = vec![("definition/tables/Sales.tmdl".to_string(), tmdl.to_string())];
        let (measures, mnames, cnames) = extract_measures(&parts);
        assert_eq!(measures.len(), 2);
        let avg = measures.iter().find(|m| m.name == "Avg Price").unwrap();
        assert_eq!(avg.table, "Sales");
        assert!(avg.expr.contains("DIVIDE([Total Amount], [Total Qty])"));
        assert!(mnames.contains("Total Amount"));
        assert!(cnames.contains("Amount"));
    }

    #[test]
    fn extract_measures_from_model_bim() {
        let bim = r#"{"model":{"tables":[{"name":"Sales","columns":[{"name":"Amount"}],"measures":[{"name":"Total","expression":"SUM('Sales'[Amount])"}]}]}}"#;
        let parts = vec![("model.bim".to_string(), bim.to_string())];
        let (measures, mnames, cnames) = extract_measures(&parts);
        assert_eq!(measures.len(), 1);
        assert_eq!(measures[0].name, "Total");
        assert!(mnames.contains("Total"));
        assert!(cnames.contains("Amount"));
    }
}
