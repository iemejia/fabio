//! `semantic-model` security-role / row-level-security (RLS) authoring —
//! `add-role`, `delete-role`, `set-rls`, `delete-rls`, `list-roles`.
//!
//! Roles live in `definition/roles/<name>.tmdl` (one file per role) and MUST be
//! `ref`-ed from `model.tmdl` (`ref role <name>`). A role holds a
//! `modelPermission:` and zero or more `tablePermission <Table> = <DAX filter>`
//! lines — the DAX filter is the row-level-security predicate. fabio edits these
//! via the shared definition read-modify-write (no XMLA/TOM). This is distinct
//! from `add-user` (which grants dataset *permissions* to a principal, not RLS).

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::tmdl::{
    add_model_ref, fetch_parts, find_table_file, part_content, push_parts, quote_tmdl_name,
    remove_model_ref, remove_part, replace_part, upsert_part,
};

const MODEL_TMDL: &str = "definition/model.tmdl";

fn role_path(name: &str) -> String {
    format!("definition/roles/{name}.tmdl")
}

fn normalize_model_permission(v: &str) -> Result<&'static str> {
    match v.to_ascii_lowercase().as_str() {
        "read" => Ok("read"),
        "none" => Ok("none"),
        "readrefresh" => Ok("readRefresh"),
        "refresh" => Ok("refresh"),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --model-permission '{v}'."),
            "Valid values: read, none, readRefresh, refresh.".to_string(),
        )
        .into()),
    }
}

// ── add-role ──────────────────────────────────────────────────────────────────

pub(super) async fn add_role(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    model_permission: &str,
) -> Result<()> {
    let op = "semantic-model add-role";
    let perm = normalize_model_permission(model_permission)?;
    let parts = fetch_parts(client, workspace, id, op).await?;

    if role_exists(&parts, name) {
        return Err(FabioError::with_hint(
            ErrorCode::Conflict,
            format!("A role named '{name}' already exists."),
            "Use `set-rls` to add filters, or pick a different name.".to_string(),
        )
        .into());
    }

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = add_role_bim(bim, name, perm)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let content = format!(
            "role {}\n\tmodelPermission: {perm}\n",
            quote_tmdl_name(name)
        );
        let with_role = upsert_part(&parts, &role_path(name), &content);
        let model = part_content(&with_role, MODEL_TMDL).unwrap_or("");
        let new_model = add_model_ref(model, "role", name);
        replace_part(&with_role, MODEL_TMDL, &new_model)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "role": name, "modelPermission": perm }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "role_added", "id": id, "role": name }),
        "status",
    );
    Ok(())
}

// ── delete-role ───────────────────────────────────────────────────────────────

pub(super) async fn delete_role(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    let op = "semantic-model delete-role";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_role_bim(bim, name)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        if !role_exists(&parts, name) {
            return Err(role_not_found(name));
        }
        let without = remove_part(&parts, &role_path(name));
        let model = part_content(&without, MODEL_TMDL).unwrap_or("");
        let new_model = remove_model_ref(model, "role", name);
        replace_part(&without, MODEL_TMDL, &new_model)
    };

    if output::dry_run_guard(cli, op, &serde_json::json!({ "id": id, "role": name })) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "role_deleted", "id": id, "role": name }),
        "status",
    );
    Ok(())
}

// ── set-rls / delete-rls ──────────────────────────────────────────────────────

pub(super) async fn set_rls(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    role: &str,
    table: &str,
    filter: &str,
) -> Result<()> {
    let op = "semantic-model set-rls";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = set_rls_bim(bim, role, table, filter)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        find_table_file(&parts, table)?; // nice error if the table is unknown
        let content = part_content(&parts, &role_path(role)).ok_or_else(|| role_not_found(role))?;
        let new_content = set_table_permission(content, table, filter);
        replace_part(&parts, &role_path(role), &new_content)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "role": role, "table": table, "filter": filter }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "rls_set", "id": id, "role": role, "table": table }),
        "status",
    );
    Ok(())
}

pub(super) async fn delete_rls(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    role: &str,
    table: &str,
) -> Result<()> {
    let op = "semantic-model delete-rls";
    let parts = fetch_parts(client, workspace, id, op).await?;

    let new_parts = if let Some(bim) = part_content(&parts, "model.bim") {
        let new_bim = delete_rls_bim(bim, role, table)?;
        replace_part(&parts, "model.bim", &new_bim)
    } else {
        let content = part_content(&parts, &role_path(role)).ok_or_else(|| role_not_found(role))?;
        let (new_content, removed) = remove_table_permission(content, table);
        if !removed {
            return Err(FabioError::not_found(format!(
                "Role '{role}' has no RLS filter on table '{table}'."
            ))
            .into());
        }
        replace_part(&parts, &role_path(role), &new_content)
    };

    if output::dry_run_guard(
        cli,
        op,
        &serde_json::json!({ "id": id, "role": role, "table": table }),
    ) {
        return Ok(());
    }
    push_parts(client, workspace, id, &new_parts, op).await?;
    output::render_object(
        cli,
        &serde_json::json!({ "status": "rls_deleted", "id": id, "role": role, "table": table }),
        "status",
    );
    Ok(())
}

// ── list-roles ────────────────────────────────────────────────────────────────

pub(super) async fn list_roles(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let op = "semantic-model list-roles";
    let parts = fetch_parts(client, workspace, id, op).await?;
    let roles = collect_roles(&parts);
    output::render_list(
        cli,
        &roles,
        &["name", "modelPermission"],
        &["NAME", "MODEL PERMISSION"],
        "name",
    );
    Ok(())
}

fn collect_roles(parts: &[(String, String)]) -> Vec<Value> {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_roles_bim(bim);
    }
    parts
        .iter()
        .filter(|(p, _)| is_role_file(p))
        .map(|(_, c)| parse_role_tmdl(c))
        .collect()
}

fn is_role_file(path: &str) -> bool {
    path.starts_with("definition/roles/")
        && std::path::Path::new(path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tmdl"))
}

/// Remove any `tablePermission <table> = …` lines from every role file (used by
/// `delete-table` cascade). Returns the updated parts and the affected role names.
pub(super) fn cascade_remove_table_from_roles(
    parts: &[(String, String)],
    table: &str,
) -> (Vec<(String, String)>, Vec<String>) {
    let mut out = parts.to_vec();
    let mut affected = Vec::new();
    for (p, c) in &mut out {
        if is_role_file(p) {
            let (new_c, removed) = remove_table_permission(c, table);
            if removed {
                affected.push(
                    parse_role_tmdl(&new_c)["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                );
                *c = new_c;
            }
        }
    }
    (out, affected)
}

fn parse_role_tmdl(content: &str) -> Value {
    let mut name = String::new();
    let mut model_permission = String::new();
    let mut perms: Vec<Value> = Vec::new();
    for line in content.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("role ") {
            name = unquote(rest.trim());
        } else if let Some(rest) = t.strip_prefix("modelPermission:") {
            model_permission = rest.trim().to_string();
        } else if let Some(rest) = t.strip_prefix("tablePermission ")
            && let Some((tbl, filter)) = rest.split_once('=')
        {
            perms.push(serde_json::json!({
                "table": unquote(tbl.trim()),
                "filter": filter.trim(),
            }));
        }
    }
    serde_json::json!({
        "name": name,
        "modelPermission": model_permission,
        "tablePermissions": perms,
    })
}

fn collect_roles_bim(bim: &str) -> Vec<Value> {
    let Ok(j) = serde_json::from_str::<Value>(bim) else {
        return Vec::new();
    };
    j.get("model")
        .and_then(|m| m.get("roles"))
        .and_then(Value::as_array)
        .map(|roles| {
            roles
                .iter()
                .map(|r| {
                    let perms: Vec<Value> = r
                        .get("tablePermissions")
                        .and_then(Value::as_array)
                        .map(|tps| {
                            tps.iter()
                                .map(|tp| {
                                    serde_json::json!({
                                        "table": tp.get("name").and_then(Value::as_str).unwrap_or(""),
                                        "filter": tp.get("filterExpression").and_then(Value::as_str).unwrap_or(""),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    serde_json::json!({
                        "name": r.get("name").and_then(Value::as_str).unwrap_or(""),
                        "modelPermission": r.get("modelPermission").and_then(Value::as_str).unwrap_or(""),
                        "tablePermissions": perms,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

// ── pure TMDL editors ─────────────────────────────────────────────────────────

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        s[1..s.len() - 1].replace("''", "'")
    } else {
        s.to_string()
    }
}

fn role_exists(parts: &[(String, String)], name: &str) -> bool {
    if let Some(bim) = part_content(parts, "model.bim") {
        return collect_roles_bim(bim)
            .iter()
            .any(|r| r.get("name").and_then(Value::as_str) == Some(name));
    }
    parts.iter().any(|(p, _)| p == &role_path(name))
}

fn role_not_found(name: &str) -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::NotFound,
        format!("Role '{name}' not found in the model definition."),
        "List roles with `fabio semantic-model list-roles`.".to_string(),
    )
    .into()
}

/// Add or replace the `tablePermission <Table> = <filter>` line for `table`.
fn set_table_permission(content: &str, table: &str, filter: &str) -> String {
    let new_line = format!("\ttablePermission {} = {filter}", quote_tmdl_name(table));
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in content.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("tablePermission ")
            && let Some((tbl, _)) = rest.split_once('=')
            && unquote(tbl.trim()).eq_ignore_ascii_case(table)
        {
            out.push(new_line.clone());
            replaced = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !replaced {
        // Append after the modelPermission line, with a blank-line separator.
        if let Some(pos) = out
            .iter()
            .position(|l| l.trim_start().starts_with("modelPermission:"))
        {
            let insert_at = pos + 1;
            out.insert(insert_at, String::new());
            out.insert(insert_at + 1, new_line);
        } else {
            out.push(new_line);
        }
    }
    let mut result = out.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Remove the `tablePermission <Table> = …` line. Returns `(new, removed)`.
fn remove_table_permission(content: &str, table: &str) -> (String, bool) {
    let mut removed = false;
    let mut out: Vec<String> = Vec::new();
    for line in content.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("tablePermission ")
            && let Some((tbl, _)) = rest.split_once('=')
            && unquote(tbl.trim()).eq_ignore_ascii_case(table)
        {
            removed = true;
            continue;
        }
        out.push(line.to_string());
    }
    let mut joined = out.join("\n");
    while joined.contains("\n\n\n") {
        joined = joined.replace("\n\n\n", "\n\n");
    }
    if content.ends_with('\n') && !joined.ends_with('\n') {
        joined.push('\n');
    }
    (joined, removed)
}

// ── model.bim editors ─────────────────────────────────────────────────────────

fn bim_roles_mut(j: &mut Value) -> Result<&mut Vec<Value>> {
    j.get_mut("model")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| FabioError::invalid_input("model.bim has no model object"))?
        .entry("roles")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| FabioError::invalid_input("roles is not an array").into())
}

fn add_role_bim(bim: &str, name: &str, perm: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    bim_roles_mut(&mut j)?.push(serde_json::json!({
        "name": name,
        "modelPermission": perm,
    }));
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_role_bim(bim: &str, name: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let roles = bim_roles_mut(&mut j)?;
    let before = roles.len();
    roles.retain(|r| r.get("name").and_then(Value::as_str) != Some(name));
    if roles.len() == before {
        return Err(role_not_found(name));
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn set_rls_bim(bim: &str, role: &str, table: &str, filter: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let r = bim_roles_mut(&mut j)?
        .iter_mut()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(role))
        .ok_or_else(|| role_not_found(role))?;
    let tps = r
        .as_object_mut()
        .unwrap()
        .entry("tablePermissions")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .unwrap();
    if let Some(tp) = tps
        .iter_mut()
        .find(|tp| tp.get("name").and_then(Value::as_str) == Some(table))
    {
        tp["filterExpression"] = Value::from(filter);
    } else {
        tps.push(serde_json::json!({ "name": table, "filterExpression": filter }));
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

fn delete_rls_bim(bim: &str, role: &str, table: &str) -> Result<String> {
    let mut j: Value =
        serde_json::from_str(bim).map_err(|e| FabioError::invalid_input(e.to_string()))?;
    let r = bim_roles_mut(&mut j)?
        .iter_mut()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(role))
        .ok_or_else(|| role_not_found(role))?;
    let removed = r
        .get_mut("tablePermissions")
        .and_then(Value::as_array_mut)
        .is_some_and(|tps| {
            let before = tps.len();
            tps.retain(|tp| tp.get("name").and_then(Value::as_str) != Some(table));
            tps.len() != before
        });
    if !removed {
        return Err(FabioError::not_found(format!(
            "Role '{role}' has no RLS filter on table '{table}'."
        ))
        .into());
    }
    Ok(serde_json::to_string(&j).unwrap_or_else(|_| bim.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role_tmdl() -> String {
        "role WestOnly\n\tmodelPermission: read\n\n\ttablePermission Customer = 'Customer'[Region] = \"West\"\n".to_string()
    }

    #[test]
    fn parse_role_reads_name_permission_and_filters() {
        let v = parse_role_tmdl(&role_tmdl());
        assert_eq!(v["name"], "WestOnly");
        assert_eq!(v["modelPermission"], "read");
        assert_eq!(v["tablePermissions"][0]["table"], "Customer");
        assert_eq!(
            v["tablePermissions"][0]["filter"],
            "'Customer'[Region] = \"West\""
        );
    }

    #[test]
    fn set_table_permission_adds_and_replaces() {
        // add a new table's filter
        let base = "role R\n\tmodelPermission: read\n";
        let out = set_table_permission(base, "Sales", "'Sales'[Region] = \"West\"");
        assert!(out.contains("tablePermission Sales = 'Sales'[Region] = \"West\""));
        // replace existing
        let out2 = set_table_permission(&role_tmdl(), "Customer", "TRUE()");
        assert!(out2.contains("tablePermission Customer = TRUE()"));
        assert!(!out2.contains("\"West\""));
        assert_eq!(out2.matches("tablePermission Customer").count(), 1);
    }

    #[test]
    fn remove_table_permission_works() {
        let (out, removed) = remove_table_permission(&role_tmdl(), "Customer");
        assert!(removed);
        assert!(!out.contains("tablePermission"));
        let (_o2, removed2) = remove_table_permission(&role_tmdl(), "Nope");
        assert!(!removed2);
    }

    #[test]
    fn bim_role_lifecycle() {
        let bim = r#"{"model":{"tables":[]}}"#;
        let added = add_role_bim(bim, "R", "read").unwrap();
        let withrls = set_rls_bim(&added, "R", "Customer", "'Customer'[X]=1").unwrap();
        let j: Value = serde_json::from_str(&withrls).unwrap();
        assert_eq!(j["model"]["roles"][0]["name"], "R");
        assert_eq!(
            j["model"]["roles"][0]["tablePermissions"][0]["filterExpression"],
            "'Customer'[X]=1"
        );
        let deleted = delete_role_bim(&withrls, "R").unwrap();
        let j2: Value = serde_json::from_str(&deleted).unwrap();
        assert_eq!(j2["model"]["roles"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn collect_roles_from_tmdl_parts() {
        let parts = vec![(role_path("WestOnly"), role_tmdl())];
        let roles = collect_roles(&parts);
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0]["name"], "WestOnly");
    }
}
