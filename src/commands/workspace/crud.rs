use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;
use anyhow::Result;
use serde_json::Value;
pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    roles: Option<&str>,
    capacity: Option<&str>,
) -> Result<()> {
    let path = roles.map_or_else(
        || "/workspaces".to_string(),
        |r| format!("/workspaces?roles={r}"),
    );
    let resp = client
        .get_list(&path, "value", cli.all, cli.continuation_token.as_deref())
        .await?;
    let items = if let Some(cap_id) = capacity {
        resp.items
            .into_iter()
            .filter(|item| {
                item.get("capacityId")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id.eq_ignore_ascii_case(cap_id))
            })
            .collect()
    } else {
        resp.items
    };

    let has_labels = items
        .iter()
        .any(|item| item.get("sensitivityLabel").is_some_and(|v| !v.is_null()));
    let has_tags = output::has_tags(&items);

    let display_items;
    let items_ref: &[Value] = if has_tags {
        display_items = output::enrich_with_tags_display(&items);
        &display_items
    } else {
        &items
    };

    match (has_labels, has_tags) {
        (true, true) => output::render_list_with_token(
            cli,
            items_ref,
            &[
                "displayName",
                "id",
                "type",
                "sensitivityLabel.id",
                "_tagsDisplay",
            ],
            &["NAME", "ID", "TYPE", "SENSITIVITY LABEL", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (true, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "type", "sensitivityLabel.id"],
            &["NAME", "ID", "TYPE", "SENSITIVITY LABEL"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, true) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "type", "_tagsDisplay"],
            &["NAME", "ID", "TYPE", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "type"],
            &["NAME", "ID", "TYPE"],
            "id",
            resp.continuation_token.as_deref(),
        ),
    }
    Ok(())
}
pub(super) async fn show(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    let data = client.get(&format!("/workspaces/{id}")).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}
#[allow(clippy::unnecessary_wraps)]
pub(super) fn url(cli: &Cli, id: &str) -> Result<()> {
    let data = serde_json::json!({ "url": format!("https://app.fabric.microsoft.com/groups/{id}"), "workspaceId": id });
    output::render_object(cli, &data, "url");
    Ok(())
}
pub(super) async fn create(
    cli: &Cli,
    client: &FabricClient,
    name: &str,
    description: Option<&str>,
    capacity_id: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(cap) = capacity_id {
        body["capacityId"] = Value::from(cap);
    }
    if output::dry_run_guard(cli, "workspace create", &body) {
        return Ok(());
    }
    let data = client
        .post("/workspaces", &body, false)
        .await
        .map_err(|e| {
            let e = enrich_forbidden(e, "workspace create", "Fabric user (tenant-level)");
            match capacity_id {
                Some(cap) => super::enrich_assign_capacity_error(e, cap),
                None => e,
            }
        })?;
    output::render_object(cli, &data, "id");
    Ok(())
}
pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput, "At least one of --name or --description must be provided".to_string(), "Example: fabio workspace update --id <ID> --name \"New Name\" --description \"New description\"".to_string()).into());
    }
    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::from(n);
    }
    if let Some(d) = description {
        body["description"] = Value::from(d);
    }
    if output::dry_run_guard(cli, "workspace update", &body) {
        return Ok(());
    }
    let data = client
        .patch(&format!("/workspaces/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "workspace update", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}
pub(super) async fn delete(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    if output::dry_run_guard(cli, "workspace delete", &serde_json::json!({ "id": id })) {
        return Ok(());
    }
    client
        .delete(&format!("/workspaces/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "workspace delete", "Admin"))?;
    output::render_object(
        cli,
        &serde_json::json!({ "id": id, "status": "deleted" }),
        "status",
    );
    Ok(())
}

/// Clone workspace items from source to destination using the Bulk APIs.
///
/// Flow:
/// 1. Resolve workspace names to IDs
/// 2. Call `bulkExportDefinitions` on source (LRO)
/// 3. Transform the export response into a `bulkImportDefinitions` request
/// 4. Call `bulkImportDefinitions` on destination (LRO)
#[allow(clippy::too_many_lines)]
pub(super) async fn clone_workspace(
    cli: &Cli,
    client: &FabricClient,
    source: &str,
    dest: &str,
    item_types: Option<&[String]>,
    allow_pairing_by_name: bool,
) -> Result<()> {
    use crate::commands::deploy::plan::resolve_workspace;

    let source_id = resolve_workspace(client, source).await?;
    let dest_id = resolve_workspace(client, dest).await?;

    if source_id == dest_id {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Source and destination workspaces cannot be the same".to_string(),
            "Provide different --source and --dest workspace IDs or names.".to_string(),
        )
        .into());
    }

    if output::dry_run_guard(
        cli,
        "workspace clone",
        &serde_json::json!({
            "source_workspace": source_id,
            "dest_workspace": dest_id,
            "item_types": item_types,
            "allow_pairing_by_name": allow_pairing_by_name,
        }),
    ) {
        return Ok(());
    }

    // Step 1: Build bulk export request
    // The API requires {"mode":"All"} or {"mode":"Selective","items":[{"id":"<uuid>"}]}
    let export_body = if let Some(types) = item_types {
        // Need to list items first to get IDs for selective export
        if !cli.quiet {
            eprintln!("[workspace clone] listing items to filter by type...");
        }
        let resp = client
            .get_list(
                &format!("/workspaces/{source_id}/items"),
                "value",
                true,
                None,
            )
            .await?;

        let selected_items: Vec<Value> = resp
            .items
            .iter()
            .filter(|item| {
                item.get("type")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| types.iter().any(|ft| ft.eq_ignore_ascii_case(t)))
            })
            .filter_map(|item| {
                item.get("id")
                    .and_then(|v| v.as_str())
                    .map(|id| serde_json::json!({"id": id}))
            })
            .collect();

        if selected_items.is_empty() {
            output::render_object(
                cli,
                &serde_json::json!({
                    "status": "empty",
                    "message": format!("No items of types {:?} found in source workspace", types),
                    "source_workspace": source_id,
                    "dest_workspace": dest_id,
                }),
                "status",
            );
            return Ok(());
        }

        serde_json::json!({
            "mode": "Selective",
            "items": selected_items
        })
    } else {
        serde_json::json!({"mode": "All"})
    };

    if !cli.quiet {
        eprintln!("[workspace clone] exporting definitions from source workspace...");
    }

    // Step 2: Call bulkExportDefinitions on source (requires beta=True query param)
    let export_result = client
        .post(
            &format!("/workspaces/{source_id}/items/bulkExportDefinitions?beta=True"),
            &export_body,
            true, // LRO poll
        )
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("ActiveCiCdOperation") {
                FabioError::with_hint(
                    ErrorCode::ApiError,
                    "Source workspace has an active CI/CD operation (Git integration connected)".to_string(),
                    "Bulk APIs cannot run while Git integration is active. Either: (1) disconnect Git first: fabio git disconnect --workspace <WS>, or (2) use deploy export/apply instead: fabio deploy export --workspace <WS> --dir ./export && fabio deploy apply --source ./export --workspace <DEST>".to_string(),
                ).into()
            } else {
                enrich_forbidden(e, "workspace clone (export)", "Contributor")
            }
        })?;

    // Step 3: The import API uses the same flat definitionParts format as the export response.
    // Export returns: { itemDefinitionsIndex: [{id, rootPath, displayName, type}], definitionParts: [{path, payload, payloadType}] }
    // Import expects: { definitionParts: [{path, payload, payloadType}], options: { allowPairingByName: bool } }
    // Parts already have full paths (e.g., /Name.Type/file.ext), so we pass them through directly.
    let parts = export_result
        .get("definitionParts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let index = export_result
        .get("itemDefinitionsIndex")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let items_count = index.len();
    if items_count == 0 {
        output::render_object(
            cli,
            &serde_json::json!({
                "status": "empty",
                "message": "No exportable items found in source workspace",
                "source_workspace": source_id,
                "dest_workspace": dest_id,
            }),
            "status",
        );
        return Ok(());
    }

    if !cli.quiet {
        eprintln!("[workspace clone] importing {items_count} item(s) to destination workspace...");
    }

    let import_body = serde_json::json!({
        "definitionParts": parts,
        "options": {
            "allowPairingByName": allow_pairing_by_name
        }
    });

    // Step 4: Call bulkImportDefinitions on destination (requires beta=True)
    let import_result = client
        .post(
            &format!("/workspaces/{dest_id}/items/bulkImportDefinitions?beta=True"),
            &import_body,
            true, // LRO poll
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace clone (import)", "Contributor"))?;

    // Render result
    let mut result = serde_json::json!({
        "status": "succeeded",
        "source_workspace": source_id,
        "dest_workspace": dest_id,
        "items_exported": items_count,
    });

    // Include import details if available
    if let Some(details) = import_result.get("importItemDefinitionsDetails") {
        result["import_details"] = details.clone();
    }

    output::render_object(cli, &result, "status");
    Ok(())
}
