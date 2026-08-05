//! `semantic-model` relationship authoring — `add-relationship`,
//! `delete-relationship`, `update-relationship`.
//!
//! Relationships live in `definition/relationships.tmdl` as top-level
//! `relationship <guid>` blocks (or in `model.bim` under `model.relationships[]`).
//! fabio edits them via the shared definition read-modify-write (no XMLA/TOM).
//! Non-default properties only are emitted, matching how Fabric serializes:
//! `isActive` (default true), `crossFilteringBehavior` (default oneDirection),
//! `fromCardinality` (default many), `toCardinality` (default one).

use anyhow::Result;
use serde_json::Value;
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::tmdl::{
    column_ref, fetch_parts, find_table_file, part_content, push_parts, quote_tmdl_name,
    remove_part, upsert_part,
};

const RELATIONSHIPS_PATH: &str = "definition/relationships.tmdl";

/// Fields describing a relationship to create / match.
pub(super) struct RelSpec<'a> {
    pub from_table: &'a str,
    pub from_column: &'a str,
    pub to_table: &'a str,
    pub to_column: &'a str,
}

/// Mutable properties that `add`/`update` can set.
#[derive(Default)]
pub(super) struct RelProps<'a> {
    pub cross_filter: Option<&'a str>,
    pub is_active: Option<bool>,
    pub from_cardinality: Option<&'a str>,
    pub to_cardinality: Option<&'a str>,
}

fn normalize_cross_filter(v: &str) -> Result<&'static str> {
    match v.to_ascii_lowercase().as_str() {
        "onedirection" | "single" | "one" => Ok("oneDirection"),
        "bothdirections" | "both" | "bidirectional" => Ok("bothDirections"),
        "automatic" | "auto" => Ok("automatic"),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --cross-filter value '{v}'."),
            "Valid values: oneDirection, bothDirections, automatic.".to_string(),
        )
        .into()),
    }
}

fn normalize_cardinality(v: &str) -> Result<&'static str> {
    match v.to_ascii_lowercase().as_str() {
        "one" => Ok("one"),
        "many" => Ok("many"),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid cardinality value '{v}'."),
            "Valid values: one, many.".to_string(),
        )
        .into()),
    }
}

// ── add-relationship ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn add_relationship(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    spec: &RelSpec<'_>,
    props: &RelProps<'_>,
) -> Result<()> {
    let op = "semantic-model add-relationship";
    // Validate enum values up front (offline).
    let cross = props.cross_filter.map(normalize_cross_filter).transpose()?;
    let from_card = props
        .from_cardinality
        .map(normalize_cardinality)
        .transpose()?;
    let to_card = props
        .to_cardinality
        .map(normalize_cardinality)
        .transpose()?;

    let parts = fetch_parts(client, workspace, id, op).await?;

    // Validate the referenced tables exist (nice error before mutating).
    if parts.iter().all(|(p, _)| p != "model.bim") {
        find_table_file(&parts, spec.from_table)?;
        find_table_file(&parts, spec.to_table)?;
    }

    let rel_id = Uuid::new_v4().to_string();
    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_relationship_bim(bim, &rel_id, spec, cross, from_card, to_card)?;
        super::tmdl::replace_part(&parts, "model.bim", &new_bim)
    } else {
        let existing = part_content(&parts, RELATIONSHIPS_PATH).unwrap_or("");
        let block =
            render_relationship_block(&rel_id, spec, cross, props.is_active, from_card, to_card);
        let updated = append_relationship_block(existing, &block);
        upsert_part(&parts, RELATIONSHIPS_PATH, &updated)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({
            "id": id,
            "relationshipId": rel_id,
            "from": format!("{}[{}]", spec.from_table, spec.from_column),
            "to": format!("{}[{}]", spec.to_table, spec.to_column),
        }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({
            "status": "relationship_added",
            "id": id,
            "relationshipId": rel_id,
        }),
        "status",
    );
    Ok(())
}

/// Render a TMDL `relationship` block (non-default properties only).
fn render_relationship_block(
    rel_id: &str,
    spec: &RelSpec<'_>,
    cross: Option<&str>,
    is_active: Option<bool>,
    from_card: Option<&str>,
    to_card: Option<&str>,
) -> String {
    use std::fmt::Write as _;
    let mut b = String::new();
    let _ = writeln!(b, "relationship {}", quote_tmdl_name(rel_id));
    let _ = writeln!(
        b,
        "\tfromColumn: {}",
        column_ref(spec.from_table, spec.from_column)
    );
    let _ = writeln!(
        b,
        "\ttoColumn: {}",
        column_ref(spec.to_table, spec.to_column)
    );
    if is_active == Some(false) {
        b.push_str("\tisActive: false\n");
    }
    if let Some(c) = cross.filter(|c| *c != "oneDirection") {
        let _ = writeln!(b, "\tcrossFilteringBehavior: {c}");
    }
    if let Some(c) = from_card.filter(|c| *c != "many") {
        let _ = writeln!(b, "\tfromCardinality: {c}");
    }
    if let Some(c) = to_card.filter(|c| *c != "one") {
        let _ = writeln!(b, "\ttoCardinality: {c}");
    }
    b
}

/// Append a relationship block to the (possibly empty) relationships.tmdl body.
fn append_relationship_block(existing: &str, block: &str) -> String {
    let trimmed = existing.trim_end();
    if trimmed.is_empty() {
        return block.to_string();
    }
    format!("{trimmed}\n\n{block}")
}

// ── delete-relationship ───────────────────────────────────────────────────────

pub(super) async fn delete_relationship(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    relationship_id: Option<&str>,
    spec: Option<&RelSpec<'_>>,
) -> Result<()> {
    let op = "semantic-model delete-relationship";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let existing = part_content(&parts, RELATIONSHIPS_PATH)
        .unwrap_or("")
        .to_string();
    let (updated, removed_id) = remove_relationship_block(&existing, relationship_id, spec)
        .ok_or_else(|| relationship_not_found(relationship_id, spec))?;

    let new_parts = if updated.trim().is_empty() {
        remove_part(&parts, RELATIONSHIPS_PATH)
    } else {
        upsert_part(&parts, RELATIONSHIPS_PATH, &updated)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "relationshipId": removed_id }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "relationship_deleted", "id": id, "relationshipId": removed_id }),
        "status",
    );
    Ok(())
}

// ── update-relationship ───────────────────────────────────────────────────────

pub(super) async fn update_relationship(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    relationship_id: Option<&str>,
    spec: Option<&RelSpec<'_>>,
    props: &RelProps<'_>,
) -> Result<()> {
    let op = "semantic-model update-relationship";
    if props.cross_filter.is_none() && props.is_active.is_none() {
        return Err(FabioError::invalid_input(
            "Provide at least one of --active/--inactive or --cross-filter".to_string(),
        )
        .into());
    }
    let cross = props.cross_filter.map(normalize_cross_filter).transpose()?;
    let parts = fetch_parts(client, workspace, id, op).await?;

    let existing = part_content(&parts, RELATIONSHIPS_PATH)
        .unwrap_or("")
        .to_string();
    let (updated, matched_id) =
        update_relationship_block(&existing, relationship_id, spec, cross, props.is_active)
            .ok_or_else(|| relationship_not_found(relationship_id, spec))?;
    let new_parts = upsert_part(&parts, RELATIONSHIPS_PATH, &updated);

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "relationshipId": matched_id }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "relationship_updated", "id": id, "relationshipId": matched_id }),
        "status",
    );
    Ok(())
}

fn relationship_not_found(
    relationship_id: Option<&str>,
    spec: Option<&RelSpec<'_>>,
) -> anyhow::Error {
    let what = relationship_id.map_or_else(
        || {
            spec.map_or_else(
                || "the relationship".to_string(),
                |s| {
                    format!(
                        "relationship {}[{}] -> {}[{}]",
                        s.from_table, s.from_column, s.to_table, s.to_column
                    )
                },
            )
        },
        |rid| format!("relationship '{rid}'"),
    );
    FabioError::with_hint(
        ErrorCode::NotFound,
        format!("Could not find {what} in the model definition."),
        "List relationships with `fabio semantic-model list-relationships`.".to_string(),
    )
    .into()
}

// ── block parsing (pure) ──────────────────────────────────────────────────────

/// A parsed relationship block: its id and the range of source lines it spans.
struct RelBlock {
    id: String,
    from: (String, String),
    to: (String, String),
    start: usize,
    end: usize, // exclusive
}

/// Parse `Table.Column` (each part possibly single-quoted) into `(table, column)`.
fn parse_column_ref(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    // Split on the first '.' that is not inside quotes.
    let mut in_quote = false;
    let mut dot = None;
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            '\'' => {
                // doubled '' is an escaped quote inside a quoted identifier
                if in_quote && i + 1 < bytes.len() && bytes[i + 1] == '\'' {
                    i += 2;
                    continue;
                }
                in_quote = !in_quote;
            }
            '.' if !in_quote => {
                dot = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let d = dot?;
    let table: String = bytes[..d].iter().collect();
    let column: String = bytes[d + 1..].iter().collect();
    Some((unquote(table.trim()), unquote(column.trim())))
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        s[1..s.len() - 1].replace("''", "'")
    } else {
        s.to_string()
    }
}

/// Parse all relationship blocks in a relationships.tmdl body.
fn parse_relationship_blocks(content: &str) -> Vec<RelBlock> {
    let lines: Vec<&str> = content.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("relationship ") {
            let rid = unquote(rest.trim());
            let start = i;
            let mut from = (String::new(), String::new());
            let mut to = (String::new(), String::new());
            i += 1;
            while i < lines.len() && !lines[i].starts_with("relationship ") {
                let t = lines[i].trim_start();
                if let Some(v) = t.strip_prefix("fromColumn:")
                    && let Some(c) = parse_column_ref(v)
                {
                    from = c;
                } else if let Some(v) = t.strip_prefix("toColumn:")
                    && let Some(c) = parse_column_ref(v)
                {
                    to = c;
                }
                i += 1;
            }
            blocks.push(RelBlock {
                id: rid,
                from,
                to,
                start,
                end: i,
            });
        } else {
            i += 1;
        }
    }
    blocks
}

/// Remove every relationship block whose `fromColumn`/`toColumn` references
/// `table` (used by `delete-table` cascade). Returns `(new_content, removed_ids)`.
pub(super) fn remove_relationships_referencing_table(
    content: &str,
    table: &str,
) -> (String, Vec<String>) {
    let lines: Vec<&str> = content.lines().collect();
    let blocks = parse_relationship_blocks(content);
    let to_remove: Vec<&RelBlock> = blocks
        .iter()
        .filter(|b| b.from.0.eq_ignore_ascii_case(table) || b.to.0.eq_ignore_ascii_case(table))
        .collect();
    if to_remove.is_empty() {
        return (content.to_string(), Vec::new());
    }
    let removed_ids: Vec<String> = to_remove.iter().map(|b| b.id.clone()).collect();
    let drop: std::collections::HashSet<usize> =
        to_remove.iter().flat_map(|b| b.start..b.end).collect();
    let out: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| !drop.contains(i))
        .map(|(_, l)| *l)
        .collect();
    let mut joined = out.join("\n");
    while joined.contains("\n\n\n") {
        joined = joined.replace("\n\n\n", "\n\n");
    }
    let joined = joined.trim().to_string();
    let result = if joined.is_empty() {
        String::new()
    } else if content.ends_with('\n') {
        format!("{joined}\n")
    } else {
        joined
    };
    (result, removed_ids)
}

fn block_matches(b: &RelBlock, relationship_id: Option<&str>, spec: Option<&RelSpec<'_>>) -> bool {
    if let Some(rid) = relationship_id {
        return b.id.eq_ignore_ascii_case(rid);
    }
    if let Some(s) = spec {
        return b.from.0.eq_ignore_ascii_case(s.from_table)
            && b.from.1.eq_ignore_ascii_case(s.from_column)
            && b.to.0.eq_ignore_ascii_case(s.to_table)
            && b.to.1.eq_ignore_ascii_case(s.to_column);
    }
    false
}

/// Remove the matching relationship block. Returns `(new_content, removed_id)`.
fn remove_relationship_block(
    content: &str,
    relationship_id: Option<&str>,
    spec: Option<&RelSpec<'_>>,
) -> Option<(String, String)> {
    let lines: Vec<&str> = content.lines().collect();
    let blocks = parse_relationship_blocks(content);
    let b = blocks
        .iter()
        .find(|b| block_matches(b, relationship_id, spec))?;
    let mut out: Vec<&str> = Vec::new();
    for (idx, l) in lines.iter().enumerate() {
        if idx >= b.start && idx < b.end {
            continue;
        }
        out.push(l);
    }
    // Trim leading/trailing/double blank lines.
    let mut joined = out.join("\n");
    while joined.contains("\n\n\n") {
        joined = joined.replace("\n\n\n", "\n\n");
    }
    let joined = joined.trim().to_string();
    let result = if joined.is_empty() {
        String::new()
    } else if content.ends_with('\n') {
        format!("{joined}\n")
    } else {
        joined
    };
    Some((result, b.id.clone()))
}

/// Set `isActive` / `crossFilteringBehavior` on the matching block in place.
fn update_relationship_block(
    content: &str,
    relationship_id: Option<&str>,
    spec: Option<&RelSpec<'_>>,
    cross: Option<&str>,
    is_active: Option<bool>,
) -> Option<(String, String)> {
    let lines: Vec<&str> = content.lines().collect();
    let blocks = parse_relationship_blocks(content);
    let b = blocks
        .iter()
        .find(|b| block_matches(b, relationship_id, spec))?;

    let mut body: Vec<String> = Vec::new();
    for l in &lines[b.start..b.end] {
        let t = l.trim_start();
        // Drop the properties we are going to (re)write.
        if is_active.is_some() && t.starts_with("isActive:") {
            continue;
        }
        if cross.is_some() && t.starts_with("crossFilteringBehavior:") {
            continue;
        }
        body.push((*l).to_string());
    }
    // Re-insert the managed properties right after the toColumn line (or at end).
    let insert_at = body
        .iter()
        .position(|l| l.trim_start().starts_with("toColumn:"))
        .map_or(body.len(), |p| p + 1);
    let mut extra: Vec<String> = Vec::new();
    if let Some(active) = is_active
        && !active
    {
        extra.push("\tisActive: false".to_string());
    }
    if let Some(c) = cross.filter(|c| *c != "oneDirection") {
        extra.push(format!("\tcrossFilteringBehavior: {c}"));
    }
    for (k, e) in extra.into_iter().enumerate() {
        body.insert(insert_at + k, e);
    }

    let mut out: Vec<String> = Vec::new();
    out.extend(lines[..b.start].iter().map(|s| (*s).to_string()));
    out.extend(body);
    out.extend(lines[b.end..].iter().map(|s| (*s).to_string()));

    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    Some((result, b.id.clone()))
}

// ── model.bim path ────────────────────────────────────────────────────────────

fn add_relationship_bim(
    bim: &str,
    rel_id: &str,
    spec: &RelSpec<'_>,
    cross: Option<&str>,
    from_card: Option<&str>,
    to_card: Option<&str>,
) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let mut rel = serde_json::json!({
        "name": rel_id,
        "fromTable": spec.from_table,
        "fromColumn": spec.from_column,
        "toTable": spec.to_table,
        "toColumn": spec.to_column,
    });
    if let Some(c) = cross.filter(|c| *c != "oneDirection") {
        rel["crossFilteringBehavior"] = Value::from(c);
    }
    if let Some(c) = from_card {
        rel["fromCardinality"] = Value::from(c);
    }
    if let Some(c) = to_card {
        rel["toCardinality"] = Value::from(c);
    }
    j.get_mut("model")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no model object"))?
        .entry("relationships")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| FabioError::invalid_input("relationships is not an array"))?
        .push(rel);
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        "relationship 11111111-1111-1111-1111-111111111111\n\tfromColumn: Sales.CustomerKey\n\ttoColumn: Customer.CustomerKey\n\nrelationship 22222222-2222-2222-2222-222222222222\n\tfromColumn: Sales.ProductKey\n\ttoColumn: Product.ProductKey\n\tisActive: false\n".to_string()
    }

    #[test]
    fn parse_column_ref_simple_and_quoted() {
        assert_eq!(
            parse_column_ref("Sales.CustomerKey"),
            Some(("Sales".into(), "CustomerKey".into()))
        );
        assert_eq!(
            parse_column_ref("'Sales Fact'.'Net Amount'"),
            Some(("Sales Fact".into(), "Net Amount".into()))
        );
    }

    #[test]
    fn render_block_emits_non_defaults_only() {
        let spec = RelSpec {
            from_table: "Sales",
            from_column: "CustomerKey",
            to_table: "Customer",
            to_column: "CustomerKey",
        };
        let b = render_relationship_block(
            "abc",
            &spec,
            Some("bothDirections"),
            Some(false),
            None,
            None,
        );
        assert!(b.contains("relationship abc"));
        assert!(b.contains("\tfromColumn: Sales.CustomerKey"));
        assert!(b.contains("\ttoColumn: Customer.CustomerKey"));
        assert!(b.contains("\tisActive: false"));
        assert!(b.contains("\tcrossFilteringBehavior: bothDirections"));
        // oneDirection default is NOT emitted
        let b2 =
            render_relationship_block("abc", &spec, Some("oneDirection"), Some(true), None, None);
        assert!(!b2.contains("crossFilteringBehavior"));
        assert!(!b2.contains("isActive"));
    }

    #[test]
    fn append_to_empty_and_existing() {
        let blk = "relationship x\n\tfromColumn: A.B\n\ttoColumn: C.D\n";
        assert_eq!(append_relationship_block("", blk), blk);
        let combined =
            append_relationship_block("relationship y\n\tfromColumn: E.F\n\ttoColumn: G.H", blk);
        assert!(combined.contains("relationship y"));
        assert!(combined.contains("relationship x"));
    }

    #[test]
    fn parse_blocks_counts_two() {
        let blocks = parse_relationship_blocks(&sample());
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].from, ("Sales".into(), "CustomerKey".into()));
        assert_eq!(blocks[1].id, "22222222-2222-2222-2222-222222222222");
    }

    #[test]
    fn remove_by_id_and_by_tuple() {
        let (out, rid) = remove_relationship_block(
            &sample(),
            Some("11111111-1111-1111-1111-111111111111"),
            None,
        )
        .unwrap();
        assert_eq!(rid, "11111111-1111-1111-1111-111111111111");
        assert!(!out.contains("CustomerKey"));
        assert!(out.contains("ProductKey"));

        let spec = RelSpec {
            from_table: "Sales",
            from_column: "ProductKey",
            to_table: "Product",
            to_column: "ProductKey",
        };
        let (out2, _rid2) = remove_relationship_block(&sample(), None, Some(&spec)).unwrap();
        assert!(!out2.contains("ProductKey"));
        assert!(out2.contains("CustomerKey"));
    }

    #[test]
    fn remove_missing_is_none() {
        assert!(remove_relationship_block(&sample(), Some("nope"), None).is_none());
    }

    #[test]
    fn update_sets_and_clears_properties() {
        // Activate the inactive second relationship and make it bidirectional.
        let (out, _id) = update_relationship_block(
            &sample(),
            Some("22222222-2222-2222-2222-222222222222"),
            None,
            Some("bothDirections"),
            Some(true),
        )
        .unwrap();
        // isActive:false removed (now active), crossFilteringBehavior added.
        let second = out.split("relationship 22222222").nth(1).unwrap();
        assert!(!second.contains("isActive: false"));
        assert!(second.contains("crossFilteringBehavior: bothDirections"));
    }

    #[test]
    fn add_relationship_bim_appends() {
        let bim = r#"{"model":{"tables":[]}}"#;
        let spec = RelSpec {
            from_table: "Sales",
            from_column: "CustomerKey",
            to_table: "Customer",
            to_column: "CustomerKey",
        };
        let out =
            add_relationship_bim(bim, "rid1", &spec, Some("bothDirections"), None, None).unwrap();
        let j: Value = serde_json::from_str(&out).unwrap();
        let rels = j["model"]["relationships"].as_array().unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0]["fromTable"], "Sales");
        assert_eq!(rels[0]["crossFilteringBehavior"], "bothDirections");
    }
}
