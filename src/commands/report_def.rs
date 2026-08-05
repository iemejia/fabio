//! Power BI **report** definition read-modify-write authoring.
//!
//! Reports have no XMLA/TOM surface; like semantic models, fabio edits them by
//! round-tripping the item definition: `getDefinition` → edit the PBIR parts in
//! memory → `updateDefinition`. This module holds that plumbing plus the
//! page-level introspection (`list-pages`, `list-visuals`) and authoring
//! (`add-page`, `delete-page`, `rename-page`, `set-active-page`) commands.
//!
//! It targets the enhanced **PBIR** format (the `definition/` folder documented
//! at <https://learn.microsoft.com/power-bi/developer/projects/projects-report>).
//! A PBIR-Legacy report (single `report.json`) is rejected with a clear message,
//! since page/visual objects only exist as separate files in PBIR.

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

pub(super) const PAGE_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/page/2.1.0/schema.json";
pub(super) const PAGES_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/pagesMetadata/1.1.0/schema.json";

const PAGES_JSON: &str = "definition/pages/pages.json";
const DEFAULT_WIDTH: i64 = 1280;
const DEFAULT_HEIGHT: i64 = 720;

// ── definition round-trip plumbing ────────────────────────────────────────────

/// Fetch a report's definition and decode its parts into `(path, text)` pairs.
pub(super) async fn fetch_parts(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    op: &str,
) -> Result<Vec<(String, String)>> {
    let def = client
        .post(
            &format!("/workspaces/{workspace}/reports/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, op, "Contributor"))?;
    Ok(decode_parts(&def))
}

fn decode_parts(def: &Value) -> Vec<(String, String)> {
    def.get("definition")
        .and_then(|d| d.get("parts"))
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| {
                    let path = p.get("path").and_then(Value::as_str)?;
                    let payload = p.get("payload").and_then(Value::as_str)?;
                    let bytes = BASE64.decode(payload).ok()?;
                    let text = String::from_utf8(bytes).ok()?;
                    Some((path.to_string(), text))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Push a new set of report definition parts via `updateDefinition` (LRO).
pub(super) async fn push_parts(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    parts: &[(String, String)],
    op: &str,
) -> Result<()> {
    let definition_parts: Vec<Value> = parts
        .iter()
        .map(|(path, content)| {
            serde_json::json!({
                "path": path,
                "payload": BASE64.encode(content.as_bytes()),
                "payloadType": "InlineBase64"
            })
        })
        .collect();
    client
        .post(
            &format!("/workspaces/{workspace}/reports/{id}/updateDefinition"),
            &serde_json::json!({ "definition": { "parts": definition_parts } }),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, op, "Contributor"))?;
    Ok(())
}

pub(super) fn part_content<'a>(parts: &'a [(String, String)], path: &str) -> Option<&'a str> {
    parts
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, c)| c.as_str())
}

pub(super) fn upsert_part(
    parts: &[(String, String)],
    path: &str,
    content: &str,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = parts.iter().filter(|(p, _)| p != path).cloned().collect();
    out.push((path.to_string(), content.to_string()));
    out
}

/// Remove every part whose path starts with `prefix` (used to drop a page folder).
pub(super) fn remove_prefix(parts: &[(String, String)], prefix: &str) -> Vec<(String, String)> {
    parts
        .iter()
        .filter(|(p, _)| !p.starts_with(prefix))
        .cloned()
        .collect()
}

// ── PBIR structure helpers ────────────────────────────────────────────────────

/// A report is in enhanced PBIR format when it has a `definition/pages/` folder.
pub(super) fn is_pbir(parts: &[(String, String)]) -> bool {
    parts
        .iter()
        .any(|(p, _)| p.starts_with("definition/pages/"))
}

fn require_pbir(parts: &[(String, String)]) -> Result<()> {
    if is_pbir(parts) {
        Ok(())
    } else {
        Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "This report is in PBIR-Legacy format (single report.json).".to_string(),
            "Page/visual authoring requires the enhanced PBIR format. Open and save the report in the Power BI Service (or Desktop with the PBIR preview) to convert it, then retry."
                .to_string(),
        )
        .into())
    }
}

/// The distinct page folder names, in a stable (pageOrder-first) order.
pub(super) fn page_folders(parts: &[(String, String)]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (p, _) in parts {
        if let Some(rest) = p.strip_prefix("definition/pages/")
            && let Some(folder) = rest.split('/').next()
            && folder != "pages.json"
            && !folder.is_empty()
            && !names.contains(&folder.to_string())
        {
            names.push(folder.to_string());
        }
    }
    // Order by pages.json pageOrder when present.
    if let Some(order) = part_content(parts, PAGES_JSON)
        .and_then(|c| serde_json::from_str::<Value>(c).ok())
        .and_then(|v| v.get("pageOrder").and_then(Value::as_array).cloned())
    {
        let ordered: Vec<String> = order
            .iter()
            .filter_map(|e| e.as_str().map(str::to_owned))
            .filter(|e| names.contains(e))
            .collect();
        let mut rest: Vec<String> = names.into_iter().filter(|n| !ordered.contains(n)).collect();
        let mut result = ordered;
        result.append(&mut rest);
        return result;
    }
    names.sort();
    names
}

fn active_page(parts: &[(String, String)]) -> Option<String> {
    part_content(parts, PAGES_JSON)
        .and_then(|c| serde_json::from_str::<Value>(c).ok())
        .and_then(|v| {
            v.get("activePageName")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

fn page_json_path(name: &str) -> String {
    format!("definition/pages/{name}/page.json")
}

/// Generate a Fabric-style 20-hex object name.
fn new_object_name() -> String {
    Uuid::new_v4().simple().to_string()[..20].to_string()
}

// ── list-pages ────────────────────────────────────────────────────────────────

pub(super) async fn list_pages(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let op = "report list-pages";
    let parts = fetch_parts(client, workspace, id, op).await?;
    require_pbir(&parts)?;
    let active = active_page(&parts);
    let rows: Vec<Value> = page_folders(&parts)
        .iter()
        .map(|name| {
            let pj = part_content(&parts, &page_json_path(name))
                .and_then(|c| serde_json::from_str::<Value>(c).ok())
                .unwrap_or(Value::Null);
            let visual_count = parts
                .iter()
                .filter(|(p, _)| {
                    p.starts_with(&format!("definition/pages/{name}/visuals/"))
                        && p.ends_with("/visual.json")
                })
                .count();
            serde_json::json!({
                "name": name,
                "displayName": pj.get("displayName").and_then(Value::as_str).unwrap_or(""),
                "displayOption": pj.get("displayOption").and_then(Value::as_str).unwrap_or(""),
                "width": pj.get("width").and_then(Value::as_i64).unwrap_or(0),
                "height": pj.get("height").and_then(Value::as_i64).unwrap_or(0),
                "visualCount": visual_count,
                "active": active.as_deref() == Some(name.as_str()),
            })
        })
        .collect();
    output::render_list(
        cli,
        &rows,
        &["name", "displayName", "visualCount", "active"],
        &["NAME", "DISPLAY NAME", "VISUALS", "ACTIVE"],
        "name",
    );
    Ok(())
}

// ── list-visuals ──────────────────────────────────────────────────────────────

pub(super) async fn list_visuals(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    page: Option<&str>,
) -> Result<()> {
    let op = "report list-visuals";
    let parts = fetch_parts(client, workspace, id, op).await?;
    require_pbir(&parts)?;
    if let Some(pg) = page
        && !page_folders(&parts).iter().any(|n| n == pg)
    {
        return Err(page_not_found(pg));
    }
    let rows = collect_visuals(&parts, page);
    output::render_list(
        cli,
        &rows,
        &["page", "name", "visualType", "title"],
        &["PAGE", "NAME", "TYPE", "TITLE"],
        "name",
    );
    Ok(())
}

fn collect_visuals(parts: &[(String, String)], page: Option<&str>) -> Vec<Value> {
    let mut rows = Vec::new();
    for (p, c) in parts {
        let Some(rest) = p.strip_prefix("definition/pages/") else {
            continue;
        };
        if !rest.ends_with("/visual.json") {
            continue;
        }
        let segs: Vec<&str> = rest.split('/').collect();
        // <page>/visuals/<visual>/visual.json
        if segs.len() != 4 || segs[1] != "visuals" {
            continue;
        }
        let (page_name, visual_name) = (segs[0], segs[2]);
        if page.is_some_and(|pg| pg != page_name) {
            continue;
        }
        let v: Value = serde_json::from_str(c).unwrap_or(Value::Null);
        let pos = v.get("position");
        rows.push(serde_json::json!({
            "page": page_name,
            "name": visual_name,
            "visualType": v.pointer("/visual/visualType").and_then(Value::as_str).unwrap_or(""),
            "title": visual_title(&v),
            "x": pos.and_then(|p| p.get("x")).and_then(Value::as_f64).unwrap_or(0.0),
            "y": pos.and_then(|p| p.get("y")).and_then(Value::as_f64).unwrap_or(0.0),
            "width": pos.and_then(|p| p.get("width")).and_then(Value::as_f64).unwrap_or(0.0),
            "height": pos.and_then(|p| p.get("height")).and_then(Value::as_f64).unwrap_or(0.0),
        }));
    }
    rows
}

/// Best-effort extraction of a visual's title literal.
fn visual_title(v: &Value) -> String {
    v.pointer("/visual/visualContainerObjects/title/0/properties/text/expr/Literal/Value")
        .and_then(Value::as_str)
        .map(|s| s.trim_matches('\'').to_string())
        .unwrap_or_default()
}

// ── add-page ──────────────────────────────────────────────────────────────────

pub(super) async fn add_page(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    display_name: &str,
    name: Option<&str>,
    active: bool,
) -> Result<()> {
    let op = "report add-page";
    let parts = fetch_parts(client, workspace, id, op).await?;
    require_pbir(&parts)?;

    let page_name = name.map_or_else(new_object_name, str::to_owned);
    if page_folders(&parts).iter().any(|n| n == &page_name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("A page named '{page_name}' already exists."),
            "Pick a different --name.".to_string(),
        )
        .into());
    }

    let page_json = build_page_json(&page_name, display_name);
    let mut new_parts = upsert_part(&parts, &page_json_path(&page_name), &page_json);
    let pages_meta = update_pages_json(&new_parts, &page_name, active, false);
    new_parts = upsert_part(&new_parts, PAGES_JSON, &pages_meta);

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "page": page_name, "displayName": display_name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "page_added", "id": id, "page": page_name, "displayName": display_name }),
        "status",
    );
    Ok(())
}

pub(super) fn build_page_json(name: &str, display_name: &str) -> String {
    let v = serde_json::json!({
        "$schema": PAGE_SCHEMA,
        "name": name,
        "displayName": display_name,
        "displayOption": "FitToPage",
        "height": DEFAULT_HEIGHT,
        "width": DEFAULT_WIDTH,
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// Build/refresh pages.json: ensure `page` is registered in pageOrder; set it as
/// active when `active`; if `removing`, drop it (and repoint active if needed).
pub(super) fn update_pages_json(
    parts: &[(String, String)],
    page: &str,
    active: bool,
    removing: bool,
) -> String {
    let existing: Value = part_content(parts, PAGES_JSON)
        .and_then(|c| serde_json::from_str(c).ok())
        .unwrap_or(Value::Null);

    let mut order: Vec<String> = existing
        .get("pageOrder")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    // Reconcile with the actual page folders present after the edit.
    let folders = page_folders(parts);
    order.retain(|p| folders.contains(p));
    if !removing && !order.contains(&page.to_string()) {
        order.push(page.to_string());
    }
    // Append any folders missing from the order.
    for f in &folders {
        if !order.contains(f) {
            order.push(f.clone());
        }
    }

    let mut active_name = existing
        .get("activePageName")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if removing {
        if active_name.as_deref() == Some(page) {
            active_name = order.first().cloned();
        }
    } else if active || active_name.is_none() {
        active_name = Some(page.to_string());
    }

    let mut obj = serde_json::Map::new();
    obj.insert("$schema".to_string(), Value::from(PAGES_SCHEMA));
    obj.insert(
        "pageOrder".to_string(),
        Value::Array(order.into_iter().map(Value::from).collect()),
    );
    if let Some(a) = active_name {
        obj.insert("activePageName".to_string(), Value::from(a));
    }
    serde_json::to_string_pretty(&Value::Object(obj)).unwrap_or_default()
}

// ── delete-page ───────────────────────────────────────────────────────────────

pub(super) async fn delete_page(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    let op = "report delete-page";
    let parts = fetch_parts(client, workspace, id, op).await?;
    require_pbir(&parts)?;

    let folders = page_folders(&parts);
    if !folders.iter().any(|n| n == name) {
        return Err(page_not_found(name));
    }
    if folders.len() <= 1 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Cannot delete the only page '{name}'."),
            "A report must keep at least one page. Add another page first, or delete the report."
                .to_string(),
        )
        .into());
    }

    let without = remove_prefix(&parts, &format!("definition/pages/{name}/"));
    let pages_json = update_pages_json(&without, name, false, true);
    let new_parts = upsert_part(&without, PAGES_JSON, &pages_json);

    if output::dry_run_guard(cli, op, &serde_json::json!({ "id": id, "page": name })) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "page_deleted", "id": id, "page": name }),
        "status",
    );
    Ok(())
}

// ── rename-page ───────────────────────────────────────────────────────────────

pub(super) async fn rename_page(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    display_name: &str,
) -> Result<()> {
    let op = "report rename-page";
    let parts = fetch_parts(client, workspace, id, op).await?;
    require_pbir(&parts)?;

    let path = page_json_path(name);
    let content = part_content(&parts, &path).ok_or_else(|| page_not_found(name))?;
    let mut pj: Value = serde_json::from_str(content)
        .map_err(|e| FabioError::invalid_input(format!("page.json is not valid JSON: {e}")))?;
    pj["displayName"] = Value::from(display_name);
    let new_content = serde_json::to_string_pretty(&pj).unwrap_or_default();
    let new_parts = upsert_part(&parts, &path, &new_content);

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "page": name, "displayName": display_name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "page_renamed", "id": id, "page": name, "displayName": display_name }),
        "status",
    );
    Ok(())
}

// ── set-active-page ───────────────────────────────────────────────────────────

pub(super) async fn set_active_page(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    let op = "report set-active-page";
    let parts = fetch_parts(client, workspace, id, op).await?;
    require_pbir(&parts)?;
    if !page_folders(&parts).iter().any(|n| n == name) {
        return Err(page_not_found(name));
    }
    let pages_json = update_pages_json(&parts, name, true, false);
    let new_parts = upsert_part(&parts, PAGES_JSON, &pages_json);

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "activePage": name }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "active_page_set", "id": id, "activePage": name }),
        "status",
    );
    Ok(())
}

// ── shared ────────────────────────────────────────────────────────────────────

pub(super) fn page_not_found(name: &str) -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::NotFound,
        format!("Page '{name}' not found in the report definition."),
        "List pages with `fabio report list-pages`.".to_string(),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_parts() -> Vec<(String, String)> {
        vec![
            ("definition.pbir".into(), "{}".into()),
            ("definition/report.json".into(), "{}".into()),
            ("definition/version.json".into(), r#"{"version":"2.0.0"}"#.into()),
            (
                PAGES_JSON.into(),
                r#"{"pageOrder":["p1","p2"],"activePageName":"p2"}"#.into(),
            ),
            (
                "definition/pages/p1/page.json".into(),
                r#"{"name":"p1","displayName":"One","displayOption":"FitToPage","width":1280,"height":720}"#.into(),
            ),
            (
                "definition/pages/p1/visuals/v1/visual.json".into(),
                r#"{"name":"v1","visual":{"visualType":"card"},"position":{"x":1,"y":2,"width":3,"height":4}}"#.into(),
            ),
            (
                "definition/pages/p2/page.json".into(),
                r#"{"name":"p2","displayName":"Two"}"#.into(),
            ),
        ]
    }

    #[test]
    fn is_pbir_and_page_folders() {
        let parts = sample_parts();
        assert!(is_pbir(&parts));
        assert_eq!(
            page_folders(&parts),
            vec!["p1".to_string(), "p2".to_string()]
        );
        assert_eq!(active_page(&parts).as_deref(), Some("p2"));
    }

    #[test]
    fn legacy_is_not_pbir() {
        let parts = vec![
            ("definition.pbir".into(), "{}".into()),
            ("report.json".into(), r#"{"sections":[]}"#.into()),
        ];
        assert!(!is_pbir(&parts));
        assert!(require_pbir(&parts).is_err());
    }

    #[test]
    fn collect_visuals_reads_type_and_page() {
        let parts = sample_parts();
        let rows = collect_visuals(&parts, None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["page"], "p1");
        assert_eq!(rows[0]["visualType"], "card");
        assert_eq!(collect_visuals(&parts, Some("p2")).len(), 0);
    }

    #[test]
    fn build_page_json_shape() {
        let s = build_page_json("abc", "My Page");
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["name"], "abc");
        assert_eq!(v["displayName"], "My Page");
        assert_eq!(v["displayOption"], "FitToPage");
        assert_eq!(v["width"], 1280);
        assert!(v["$schema"].as_str().unwrap().contains("page/2.1.0"));
    }

    #[test]
    fn update_pages_json_add_and_active() {
        let mut parts = sample_parts();
        // Simulate a new page part being present before recomputing pages.json.
        parts.push((
            "definition/pages/p3/page.json".into(),
            r#"{"name":"p3"}"#.into(),
        ));
        let out = update_pages_json(&parts, "p3", true, false);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(
            v["pageOrder"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x.as_str() == Some("p3"))
        );
        assert_eq!(v["activePageName"], "p3");
    }

    #[test]
    fn update_pages_json_remove_repoints_active() {
        // Remove p2 (the active page); active should repoint to a remaining page.
        let parts: Vec<(String, String)> = sample_parts()
            .into_iter()
            .filter(|(p, _)| !p.starts_with("definition/pages/p2/"))
            .collect();
        let out = update_pages_json(&parts, "p2", false, true);
        let v: Value = serde_json::from_str(&out).unwrap();
        let order: Vec<&str> = v["pageOrder"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(order, vec!["p1"]);
        assert_eq!(v["activePageName"], "p1");
    }

    #[test]
    fn remove_prefix_drops_page_folder() {
        let parts = sample_parts();
        let out = remove_prefix(&parts, "definition/pages/p1/");
        assert!(
            !out.iter()
                .any(|(p, _)| p.starts_with("definition/pages/p1/"))
        );
        assert!(
            out.iter()
                .any(|(p, _)| p.starts_with("definition/pages/p2/"))
        );
    }
}
