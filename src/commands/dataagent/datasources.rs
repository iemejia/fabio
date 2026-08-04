use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::resolve_datasource_id;

/// List configured data sources via the datasources API.
///
/// Uses: `GET /workspaces/{ws}/dataAgents/{id}/staging/datasources` (staging)
///   or: `GET /workspaces/{ws}/dataAgents/{id}/datasources` (published)
pub(super) async fn list_datasources(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    stage: &str,
) -> Result<()> {
    let prefix = stage_prefix(stage);
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/dataAgents/{id}{prefix}/datasources"),
            "value",
            true,
            None,
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "displayName", "type"],
        &["ID", "NAME", "TYPE"],
        "displayName",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

/// Show details of a specific data source.
///
/// Uses: `GET /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}` (staging)
///   or: `GET /workspaces/{ws}/dataAgents/{id}/datasources/{dsId}` (published)
pub(super) async fn show_datasource(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    stage: &str,
) -> Result<()> {
    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let prefix = stage_prefix(stage);

    let data = client
        .get(&format!(
            "/workspaces/{workspace}/dataAgents/{id}{prefix}/datasources/{ds_id}"
        ))
        .await?;

    output::render_object(cli, &data, "displayName");
    Ok(())
}

/// Add a data source to the agent via the staging datasources API.
///
/// Uses: `POST /workspaces/{ws}/dataAgents/{id}/staging/datasources` (LRO)
#[allow(clippy::too_many_arguments)]
pub(super) async fn add_datasource(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    artifact: &str,
    artifact_workspace: Option<&str>,
    artifact_type: Option<&str>,
    instructions: Option<&str>,
) -> Result<()> {
    let ds_workspace = artifact_workspace.unwrap_or(workspace);

    // Resolve artifact type and ID
    let (resolved_type, artifact_id, artifact_name) =
        resolve_artifact(client, ds_workspace, artifact, artifact_type).await?;

    if output::dry_run_guard(
        cli,
        "data-agent add-datasource",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "artifactId": artifact_id,
            "artifactName": artifact_name,
            "artifactWorkspace": ds_workspace,
            "fabricItemType": resolved_type,
        }),
    ) {
        return Ok(());
    }

    // Build request body per the new API schema
    // The API uses DatasourceType discriminator: "FabricItem" or "LakehouseTables"
    let mut body = serde_json::json!({
        "type": "FabricItem",
        "itemReference": {
            "itemId": artifact_id,
            "workspaceId": ds_workspace,
        },
        "fabricItemType": resolved_type,
    });
    if let Some(instr) = instructions {
        body["instructions"] = Value::from(instr);
    }

    // Datasource creation triggers async schema discovery (LRO).
    // The server may take 1-3 minutes to complete schema indexing.
    let resp = client
        .post(
            &format!("/workspaces/{workspace}/dataAgents/{id}/staging/datasources"),
            &body,
            true,
        )
        .await?;

    // API returns the created datasource object (or empty on LRO)
    let result = if resp.is_null() || resp.as_object().is_some_and(serde_json::Map::is_empty) {
        serde_json::json!({
            "status": "datasource_added",
            "artifactId": artifact_id,
            "displayName": artifact_name,
            "fabricItemType": resolved_type,
        })
    } else {
        // The staging POST response returns the datasource object with PascalCase
        // keys (FabricItemType, Id, DisplayName, ItemReference), unlike the
        // camelCase used by list-datasources, fabio's empty-LRO fallback above,
        // and every documented example. Normalize so the output contract is
        // stable regardless of LRO timing — agents always read `data.fabricItemType`.
        let mut r = camel_case_keys(resp);
        if let Some(obj) = r.as_object_mut() {
            obj.insert("status".to_string(), Value::from("datasource_added"));
        }
        r
    };
    output::render_object(cli, &result, "status");
    Ok(())
}

/// Recursively lower-case the first character of every object key.
///
/// The Fabric data-agent staging `POST .../datasources` response uses `PascalCase`
/// keys while the rest of the API (and fabio's output contract) is camelCase.
/// This makes the two agree without dropping any server-provided fields. Keys
/// already in camelCase are unchanged (lower-casing an already-lowercase initial
/// is a no-op), so it is safe to apply to mixed payloads.
fn camel_case_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (lower_first(&k), camel_case_keys(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(camel_case_keys).collect()),
        other => other,
    }
}

/// Lower-case only the first character of a key (ASCII), leaving the rest intact.
fn lower_first(key: &str) -> String {
    let mut chars = key.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_ascii_lowercase().to_string() + chars.as_str()
    })
}

/// Remove a data source from the agent.
///
/// Uses: `DELETE /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}`
pub(super) async fn remove_datasource(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "data-agent remove-datasource",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
        }),
    ) {
        return Ok(());
    }

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;

    client
        .delete(&format!(
            "/workspaces/{workspace}/dataAgents/{id}/staging/datasources/{ds_id}"
        ))
        .await?;

    let result = serde_json::json!({
        "id": id,
        "status": "datasource_removed",
        "datasource": datasource,
        "datasourceId": ds_id,
    });
    output::render_object(cli, &result, "status");
    Ok(())
}

/// Update a data source's metadata (instructions, description).
///
/// Uses: `PATCH /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}`
#[allow(clippy::too_many_arguments)]
pub(super) async fn update_datasource(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    instructions: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if instructions.is_none() && description.is_none() {
        return Err(FabioError::invalid_input(
            "At least one of --instructions or --description must be provided",
        )
        .into());
    }

    if output::dry_run_guard(
        cli,
        "data-agent update-datasource",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
            "instructions": instructions,
            "description": description,
        }),
    ) {
        return Ok(());
    }

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;

    let mut body = serde_json::Map::new();
    if let Some(instr) = instructions {
        body.insert("instructions".to_string(), Value::from(instr));
    }
    if let Some(desc) = description {
        body.insert("userDescription".to_string(), Value::from(desc));
    }

    let resp = client
        .patch(
            &format!("/workspaces/{workspace}/dataAgents/{id}/staging/datasources/{ds_id}"),
            &Value::Object(body),
        )
        .await?;

    let result = if resp.is_null() || resp.as_object().is_some_and(serde_json::Map::is_empty) {
        serde_json::json!({
            "id": id,
            "status": "datasource_updated",
            "datasourceId": ds_id,
            "instructions": instructions,
            "description": description,
        })
    } else {
        let mut r = resp;
        if let Some(obj) = r.as_object_mut() {
            obj.insert("status".to_string(), Value::from("datasource_updated"));
        }
        r
    };
    output::render_object(cli, &result, "status");
    Ok(())
}

/// A grouping (container) staging-element type that `select-tables` must drill
/// one level into to reach the selectable leaves nested beneath it.
///
/// Lakehouse/Warehouse sources group tables under `Schema`/`Schemas`; KQL
/// database sources group their tables/functions/etc. under `Tables`,
/// `Functions`, `Shortcuts`, and `MaterializedViews` containers. Without
/// recognizing the KQL containers, `--tables Sales` never reaches the nested
/// `Sales` table (regression this guards against).
fn is_grouping_container(t: &str) -> bool {
    matches!(
        t.to_ascii_lowercase().as_str(),
        "schema" | "schemas" | "tables" | "functions" | "shortcuts" | "materializedviews"
    )
}

/// Select or unselect elements in a data source via the elements API.
///
/// Tables mode (`--tables`/`--all-tables`) restricts to table-typed elements.
/// Element mode (`--elements`/`--all-elements`) matches elements of ANY leaf
/// type by name — this is how you scope an Ontology/GraphModel data source to
/// specific entity types. Selection is a type-agnostic PATCH on the element id.
///
/// Uses: `GET .../staging/datasources/{dsId}/elements` to list elements,
/// then `PATCH .../staging/datasources/{dsId}/elements?id={elementId}` per element.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn select_tables(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    tables: Option<&str>,
    elements: Option<&str>,
    all_tables: bool,
    all_elements: bool,
    unselect: bool,
) -> Result<()> {
    if tables.is_none() && elements.is_none() && !all_tables && !all_elements {
        return Err(FabioError::invalid_input(
            "Provide one of --tables, --elements, --all-tables, or --all-elements",
        )
        .into());
    }

    if output::dry_run_guard(
        cli,
        "data-agent select-tables",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
            "tables": tables,
            "elements": elements,
            "allTables": all_tables,
            "allElements": all_elements,
            "unselect": unselect,
        }),
    ) {
        return Ok(());
    }

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let base_path =
        format!("/workspaces/{workspace}/dataAgents/{id}/staging/datasources/{ds_id}/elements");

    // Fetch all elements (paginated).
    let elements_resp = client.get_list(&base_path, "value", true, None).await?;

    // Names to match (from --tables and/or --elements).
    let names: Vec<String> = tables
        .into_iter()
        .chain(elements)
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect();
    let select_all = all_tables || all_elements;
    let target_selected = !unselect;

    // Element mode (any leaf type) whenever an element flag is used; otherwise
    // restrict to table-typed elements to preserve the table-only semantics.
    let restrict_to_tables = elements.is_none() && !all_elements;
    let table_types = ["Table", "ExternalTable", "MaterializedView", "View"];
    let is_container = |t: &str| is_grouping_container(t);
    let is_selectable = |t: &str| -> bool {
        if restrict_to_tables {
            table_types.iter().any(|x| x.eq_ignore_ascii_case(t))
        } else {
            !is_container(t)
        }
    };

    let mut modified = 0;

    for elem in &elements_resp.items {
        let elem_type = elem.get("type").and_then(Value::as_str).unwrap_or("");
        if !is_selectable(elem_type) {
            // Drill one level into schema containers to reach nested elements.
            if is_container(elem_type) {
                let elem_id = elem.get("id").and_then(Value::as_str).unwrap_or("");
                if !elem_id.is_empty() {
                    let sub_resp = client
                        .get_list(
                            &format!("{base_path}?rootId={elem_id}"),
                            "value",
                            true,
                            None,
                        )
                        .await?;
                    for sub_elem in &sub_resp.items {
                        let sub_type = sub_elem.get("type").and_then(Value::as_str).unwrap_or("");
                        if !is_selectable(sub_type) {
                            continue;
                        }
                        let display_name = sub_elem
                            .get("displayName")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let sub_id = sub_elem.get("id").and_then(Value::as_str).unwrap_or("");
                        let should_modify =
                            select_all || names.iter().any(|t| t == &display_name.to_lowercase());
                        if should_modify && !sub_id.is_empty() {
                            client
                                .patch(
                                    &format!("{base_path}?id={sub_id}"),
                                    &serde_json::json!({ "isSelected": target_selected }),
                                )
                                .await?;
                            modified += 1;
                        }
                    }
                }
            }
            continue;
        }

        let display_name = elem
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or("");
        let elem_id = elem.get("id").and_then(Value::as_str).unwrap_or("");
        let should_modify = select_all || names.iter().any(|t| t == &display_name.to_lowercase());

        if should_modify && !elem_id.is_empty() {
            client
                .patch(
                    &format!("{base_path}?id={elem_id}"),
                    &serde_json::json!({ "isSelected": target_selected }),
                )
                .await?;
            modified += 1;
        }
    }

    if modified == 0 && !select_all {
        return Err(FabioError::with_hint(
            ErrorCode::NotFound,
            format!("No matching elements found: {}", names.join(", ")),
            "List available elements: fabio data-agent list-elements -w <workspace> --id <id> --datasource <ds>",
        )
        .into());
    }

    let noun = if restrict_to_tables {
        "tables"
    } else {
        "elements"
    };
    let result = serde_json::json!({
        "status": if unselect { format!("{noun}_unselected") } else { format!("{noun}_selected") },
        "modified": modified,
        "allSelected": select_all,
    });
    output::render_object(cli, &result, "status");
    Ok(())
}

// ─── Private Helpers ─────────────────────────────────────────────────────────

const fn stage_prefix(stage: &str) -> &str {
    if stage.eq_ignore_ascii_case("published") {
        ""
    } else {
        "/staging"
    }
}

/// Resolve an artifact (name or ID) to its type, ID, and display name.
async fn resolve_artifact(
    client: &FabricClient,
    ds_workspace: &str,
    artifact: &str,
    artifact_type: Option<&str>,
) -> Result<(String, String, String)> {
    // Auto-detect artifact type if not provided
    let resolved_type = if let Some(t) = artifact_type {
        t.to_string()
    } else {
        let items = client
            .get_list(
                &format!("/workspaces/{ds_workspace}/items"),
                "value",
                true,
                None,
            )
            .await?;

        let found = items.items.iter().find(|item| {
            let item_name = item
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or("");
            let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
            item_name.eq_ignore_ascii_case(artifact) || item_id == artifact
        });

        match found {
            Some(item) => item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            None => {
                return Err(FabioError::with_hint(
                    ErrorCode::NotFound,
                    format!("Artifact '{artifact}' not found in workspace '{ds_workspace}'"),
                    "Specify the artifact type with --artifact-type, or check the workspace items: fabio item list -w <workspace>",
                ).into());
            }
        }
    };

    // Resolve artifact ID
    let items = client
        .get_list(
            &format!("/workspaces/{ds_workspace}/items?type={resolved_type}"),
            "value",
            true,
            None,
        )
        .await?;

    let artifact_item = items
        .items
        .iter()
        .find(|item| {
            let item_name = item.get("displayName").and_then(Value::as_str).unwrap_or("");
            let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
            item_name.eq_ignore_ascii_case(artifact) || item_id == artifact
        })
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                format!("Artifact '{artifact}' of type '{resolved_type}' not found"),
                format!("List items of this type: fabio item list -w {ds_workspace} --type {resolved_type}"),
            )
        })?;

    let artifact_id = artifact_item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let artifact_name = artifact_item
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or(artifact)
        .to_string();

    Ok((resolved_type, artifact_id, artifact_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Integration tests in tests/e2e_dataagent.rs cover the full flow.
    // Unit tests for `resolve_artifact` and other async helpers require mocking.

    #[test]
    fn camel_case_keys_normalizes_pascal_top_level() {
        let server = json!({
            "Id": "abc",
            "DisplayName": "MyOntology",
            "FabricItemType": "Ontology",
            "Type": "FabricItem"
        });
        let got = camel_case_keys(server);
        assert_eq!(got["id"], "abc");
        assert_eq!(got["displayName"], "MyOntology");
        assert_eq!(got["fabricItemType"], "Ontology");
        assert_eq!(got["type"], "FabricItem");
        // The PascalCase keys must be gone (stable contract).
        assert!(got.get("FabricItemType").is_none());
    }

    #[test]
    fn camel_case_keys_recurses_into_nested_objects_and_arrays() {
        let server = json!({
            "ItemReference": {"ItemId": "i1", "WorkspaceId": "w1"},
            "Elements": [{"IsSelected": true}]
        });
        let got = camel_case_keys(server);
        assert_eq!(got["itemReference"]["itemId"], "i1");
        assert_eq!(got["itemReference"]["workspaceId"], "w1");
        assert_eq!(got["elements"][0]["isSelected"], true);
    }

    #[test]
    fn camel_case_keys_leaves_camel_case_untouched() {
        let already = json!({"fabricItemType": "Lakehouse", "id": "x"});
        let got = camel_case_keys(already.clone());
        assert_eq!(got, already);
    }

    #[test]
    fn is_grouping_container_recognizes_schema_and_kql_containers() {
        // Lakehouse/Warehouse grouping.
        assert!(is_grouping_container("Schema"));
        assert!(is_grouping_container("Schemas"));
        // KQL database grouping containers (the bug this guards): tables are
        // nested under these, so select-tables must drill in.
        assert!(is_grouping_container("Tables"));
        assert!(is_grouping_container("Functions"));
        assert!(is_grouping_container("Shortcuts"));
        assert!(is_grouping_container("MaterializedViews"));
        // Case-insensitive.
        assert!(is_grouping_container("tables"));
        // Leaf/selectable types are NOT containers (singular vs plural).
        assert!(!is_grouping_container("Table"));
        assert!(!is_grouping_container("MaterializedView"));
        assert!(!is_grouping_container("Column"));
        assert!(!is_grouping_container("View"));
    }
}
