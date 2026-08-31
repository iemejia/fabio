use anyhow::Result;
use serde_json::Value;

use super::stage_prefix;
use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::resolve_datasource_id;

/// List elements (tables/columns) in a datasource via the elements API.
///
/// Uses: `GET /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}/elements` (staging)
///   or: `GET /workspaces/{ws}/dataAgents/{id}/datasources/{dsId}/elements` (published)
/// Navigates the schema tree level-by-level (schemas → tables → columns).
pub(super) async fn list_elements(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    stage: &str,
) -> Result<()> {
    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let prefix = stage_prefix(stage);
    let base_path =
        format!("/workspaces/{workspace}/dataAgents/{id}{prefix}/datasources/{ds_id}/elements");

    // Get root-level elements (typically schemas)
    let root_resp = client.get_list(&base_path, "value", true, None).await?;

    let mut flat: Vec<Value> = Vec::new();

    // Flatten the tree by navigating level-by-level
    for elem in &root_resp.items {
        flatten_element(&mut flat, elem, client, &base_path, 0).await?;
    }

    output::render_list_with_token(
        cli,
        &flat,
        &["path", "type", "selected", "description"],
        &["PATH", "TYPE", "SELECTED", "DESCRIPTION"],
        "path",
        None,
    );
    Ok(())
}

/// Set or clear a description on a specific element identified by dot-path.
///
/// Uses: `PATCH /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}/elements?id={elementId}`
#[allow(clippy::too_many_arguments)]
pub(super) async fn describe_element(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    path: &str,
    description: Option<&str>,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "data-agent describe-element",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
            "path": path,
            "description": description,
        }),
    ) {
        return Ok(());
    }

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let base_path =
        format!("/workspaces/{workspace}/dataAgents/{id}/staging/datasources/{ds_id}/elements");

    // Navigate tree to find the element by dot-path
    let element_id = resolve_element_id_by_path(client, &base_path, path).await?;

    // PATCH the element to set/clear description
    let patch_body = match description {
        Some(desc) if !desc.is_empty() => serde_json::json!({ "description": desc }),
        _ => serde_json::json!({ "description": "" }),
    };

    client
        .patch(&format!("{base_path}?id={element_id}"), &patch_body)
        .await?;

    let result = serde_json::json!({
        "status": if description.is_some_and(|d| !d.is_empty()) { "description_set" } else { "description_cleared" },
        "path": path,
        "description": description.unwrap_or(""),
    });
    output::render_object(cli, &result, "status");
    Ok(())
}

/// Delete a stale schema element from a datasource.
///
/// Uses: `DELETE /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}/elements?id={elementId}`
pub(super) async fn delete_element(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    element_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "data-agent delete-element",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
            "elementId": element_id,
        }),
    ) {
        return Ok(());
    }

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;

    client
        .delete(&format!(
            "/workspaces/{workspace}/dataAgents/{id}/staging/datasources/{ds_id}/elements?id={element_id}"
        ))
        .await?;

    let result = serde_json::json!({
        "id": id,
        "status": "element_deleted",
        "datasource": datasource,
        "elementId": element_id,
    });
    output::render_object(cli, &result, "status");
    Ok(())
}

// ─── Private Helpers ─────────────────────────────────────────────────────────

/// Recursively flatten an element and its children into a flat list for display.
async fn flatten_element(
    output_vec: &mut Vec<Value>,
    elem: &Value,
    client: &FabricClient,
    base_path: &str,
    depth: usize,
) -> Result<()> {
    let display_name = elem
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or("");
    let elem_type = elem.get("type").and_then(Value::as_str).unwrap_or("");
    let is_selected = elem.get("isSelected").and_then(Value::as_bool);
    let description = elem
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let elem_id = elem
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(display_name);
    let has_sub = elem
        .get("hasSubElements")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let indent = "  ".repeat(depth);
    let display_path = format!("{indent}{display_name}");

    let mut row = serde_json::json!({
        "path": elem_id,
        "displayPath": display_path,
        "name": display_name,
        "type": elem_type,
    });

    if let Some(selected) = is_selected {
        row["selected"] = Value::Bool(selected);
    }
    if !description.is_empty() {
        row["description"] = Value::from(description);
    }
    if let Some(dt) = elem.get("dataType").and_then(Value::as_str) {
        row["dataType"] = Value::from(dt);
    }
    if let Some(state) = elem.get("state").and_then(Value::as_str) {
        row["state"] = Value::from(state);
    }

    output_vec.push(row);

    // Navigate into sub-elements if present
    if has_sub && !elem_id.is_empty() {
        let sub_resp = client
            .get_list(
                &format!("{base_path}?rootId={elem_id}"),
                "value",
                true,
                None,
            )
            .await;
        if let Ok(sub) = sub_resp {
            for child in &sub.items {
                Box::pin(flatten_element(
                    output_vec,
                    child,
                    client,
                    base_path,
                    depth + 1,
                ))
                .await?;
            }
        }
    }

    Ok(())
}

/// Resolve a dot-path (e.g., `dbo.orders.total_amount`) to an element ID
/// by navigating the schema tree level-by-level.
async fn resolve_element_id_by_path(
    client: &FabricClient,
    base_path: &str,
    dot_path: &str,
) -> Result<String> {
    let path_parts: Vec<&str> = dot_path.split('.').collect();
    let mut current_root: Option<String> = None;

    for (i, part_name) in path_parts.iter().enumerate() {
        let url = current_root.as_ref().map_or_else(
            || base_path.to_string(),
            |root_id| format!("{base_path}?rootId={root_id}"),
        );

        let resp = client.get_list(&url, "value", true, None).await?;

        let found = resp.items.iter().find(|elem| {
            elem.get("displayName")
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case(part_name))
        });

        let Some(elem) = found else {
            return Err(FabioError::with_hint(
                ErrorCode::NotFound,
                format!("Element not found at path '{dot_path}' (failed at segment '{part_name}')"),
                "Use 'fabio data-agent list-elements' to see available paths. Path format: schema.table or schema.table.column",
            ).into());
        };

        let elem_id = elem
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        if i == path_parts.len() - 1 {
            // This is the target element
            if elem_id.is_empty() {
                return Err(FabioError::new(
                    ErrorCode::ApiError,
                    format!("Element at path '{dot_path}' has no ID"),
                )
                .into());
            }
            return Ok(elem_id);
        }
        // Navigate deeper
        current_root = Some(elem_id);
    }

    Err(FabioError::new(
        ErrorCode::NotFound,
        format!("Element not found at path '{dot_path}'"),
    )
    .into())
}

#[cfg(test)]
mod tests {
    // Integration tests in tests/e2e_dataagent.rs cover the full flow.
    // Unit tests for tree navigation helpers require mocking the HTTP client.
}
