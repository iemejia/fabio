use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::cli::Cli;
use crate::client::FabricClient;

use super::platform::{DefinitionPart, PlatformMetadata, write_source_directory};

/// Exportable item with governance metadata.
struct ExportableItem {
    id: String,
    item_type: String,
    name: String,
    description: Option<String>,
    sensitivity_label: Option<Value>,
    tags: Option<Vec<Value>>,
}

/// Export a workspace's item definitions to a local `.platform` directory.
///
/// For each item in the workspace:
/// 1. Fetches the item metadata (type, name)
/// 2. Fetches the definition via `getDefinition` LRO (in parallel)
/// 3. Writes the directory structure with `.platform` + definition files
pub async fn export_workspace(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    output_dir: &std::path::Path,
    item_types: Option<&[String]>,
    overwrite: bool,
    concurrency: usize,
) -> Result<ExportResult> {
    let items_to_export = collect_exportable_items(client, workspace_id, item_types).await?;
    let total = items_to_export.len();

    // Fetch definitions in parallel with bounded concurrency
    let (exported, skipped) = fetch_definitions_parallel(
        client,
        workspace_id,
        &items_to_export,
        concurrency,
        cli.quiet,
    )
    .await?;

    // Write to disk
    let count = if cli.dry_run {
        exported.len()
    } else {
        write_source_directory(output_dir, &exported, overwrite)?
    };

    // Export shortcuts for Lakehouse items
    if !cli.dry_run {
        export_lakehouse_shortcuts(client, workspace_id, output_dir, &items_to_export).await;
    }

    // Export SQL analytics endpoint identity for Lakehouse items (enables
    // Direct Lake connection rewiring on deploy apply).
    if !cli.dry_run {
        export_lakehouse_sql_endpoints(client, workspace_id, output_dir, &items_to_export).await;
    }

    // Export governance metadata (sensitivity labels + tags)
    if !cli.dry_run {
        write_governance_metadata(output_dir, &items_to_export);
    }

    // Export job schedules for items that have them
    if !cli.dry_run {
        export_item_schedules(client, workspace_id, output_dir, &items_to_export).await;
    }

    Ok(ExportResult {
        total_items: total,
        exported: count,
        skipped,
    })
}

/// Collect all exportable items from the workspace, optionally filtering by type.
async fn collect_exportable_items(
    client: &FabricClient,
    workspace_id: &str,
    item_types: Option<&[String]>,
) -> Result<Vec<ExportableItem>> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace_id}/items"),
            "value",
            true,
            None,
        )
        .await?;

    let mut items: Vec<ExportableItem> = Vec::new();

    for item in &resp.items {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        let item_type = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let name = item
            .get("displayName")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        if id.is_empty() || item_type.is_empty() || name.is_empty() {
            continue;
        }

        // Filter by item types if specified
        if let Some(types) = item_types
            && !types.iter().any(|t| t.eq_ignore_ascii_case(item_type))
        {
            continue;
        }

        // Exclude auto-provisioned types unless explicitly requested via --item-types
        if item_types.is_none() && is_auto_provisioned_type(item_type) {
            continue;
        }

        // Extract governance metadata from the item response
        let sensitivity_label = item.get("sensitivityLabel").cloned();
        let tags = item.get("tags").and_then(|v| v.as_array()).cloned();

        items.push(ExportableItem {
            id: id.to_owned(),
            item_type: item_type.to_owned(),
            name: name.to_owned(),
            description,
            sensitivity_label,
            tags,
        });
    }

    // Exclude the auto-created child items an Ontology spawns (an internal
    // Lakehouse `<name>_lh_<id>` and a GraphModel `<name>_graph_<id>`). These are
    // provisioned automatically when the Ontology item is (re)created, so they
    // must NOT be exported as independent items — deploying them would create
    // orphan duplicates. Names are derived exactly from each Ontology's id, so a
    // user's own `*_lh_*`/`*_graph_*` item is never falsely excluded.
    let child_names = ontology_child_names(&items);
    if !child_names.is_empty() {
        items.retain(|it| {
            !((it.item_type == "Lakehouse" || it.item_type == "GraphModel")
                && child_names.contains(&it.name))
        });
    }

    Ok(items)
}

/// The exact display names of the child items each Ontology auto-provisions:
/// `<ontologyName>_lh_<idNoDashes>` (internal Lakehouse) and
/// `<ontologyName>_graph_<idNoDashes>` (backing `GraphModel`).
fn ontology_child_names(items: &[ExportableItem]) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for it in items {
        if it.item_type == "Ontology" {
            let id_no_dashes = it.id.replace('-', "");
            names.insert(format!("{}_lh_{id_no_dashes}", it.name));
            names.insert(format!("{}_graph_{id_no_dashes}", it.name));
        }
    }
    names
}

/// Fetch item definitions in parallel using bounded concurrency.
///
/// Returns `(exported_items, skipped_messages)`.
async fn fetch_definitions_parallel(
    client: &FabricClient,
    workspace_id: &str,
    items: &[ExportableItem],
    concurrency: usize,
    quiet: bool,
) -> Result<(Vec<(PlatformMetadata, Vec<DefinitionPart>)>, Vec<String>)> {
    let batch_concurrency = concurrency.max(1);
    let semaphore = Arc::new(Semaphore::new(batch_concurrency));
    let mut handles = Vec::with_capacity(items.len());

    for item in items {
        let sem = Arc::clone(&semaphore);
        let client_clone = client.clone();
        let ws_id = workspace_id.to_owned();
        let item_id = item.id.clone();
        let item_type_owned = item.item_type.clone();
        let name_owned = item.name.clone();
        let description_owned = item.description.clone();

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();

            let path = format!("/workspaces/{ws_id}/items/{item_id}/getDefinition");
            let result = client_clone.post(&path, &serde_json::json!({}), true).await;

            if !quiet {
                eprintln!(
                    "[deploy export] fetched definition for {item_type_owned} \"{name_owned}\""
                );
            }

            (
                item_type_owned,
                name_owned,
                description_owned,
                item_id,
                result,
            )
        }));
    }

    let mut exported = Vec::new();
    let mut skipped = Vec::new();

    for handle in handles {
        let (item_type, name, description, item_id, result) = handle.await?;

        match result {
            Ok(data) => {
                let parts = extract_definition_parts(&data);
                if parts.is_empty() && !is_shell_only_type(&item_type) {
                    skipped.push(format!("{item_type} \"{name}\" (no definition parts)"));
                    continue;
                }

                let definition_format = data
                    .get("definition")
                    .and_then(|d| d.get("format"))
                    .and_then(|f| f.as_str())
                    .map(str::to_owned);

                let metadata = PlatformMetadata {
                    item_type,
                    display_name: name,
                    logical_id: Some(stable_logical_id(&data, &item_id)),
                    description,
                    definition_format,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                };

                exported.push((metadata, parts));
            }
            Err(_) => {
                // Shell-only items (Warehouse, SQLDatabase, etc.) don't support
                // getDefinition but should still be exported with just a .platform
                // file so deploy apply can recreate the container.
                if is_shell_only_type(&item_type) {
                    let metadata = PlatformMetadata {
                        item_type,
                        display_name: name,
                        logical_id: Some(item_id),
                        description,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    };
                    exported.push((metadata, Vec::new()));
                } else {
                    skipped.push(format!(
                        "{item_type} \"{name}\" (getDefinition not supported)"
                    ));
                }
            }
        }
    }

    Ok((exported, skipped))
}

/// Result of an export operation.
#[derive(Debug, serde::Serialize)]
pub struct ExportResult {
    pub total_items: usize,
    pub exported: usize,
    pub skipped: Vec<String>,
}

/// Item types that are exported as metadata-only (`.platform` file, no definition parts).
///
/// These types don't support `getDefinition` but are still valid deployment targets:
/// - `deploy apply` creates them with just `displayName` + `type` (no definition body)
/// - `deploy apply` skips `updateDefinition` when parts are empty
///
/// Matches fabric-cicd's `SHELL_ONLY_PUBLISH` concept.
const SHELL_ONLY_TYPES: &[&str] = &["Warehouse", "SQLDatabase", "MLExperiment", "MLModel"];

/// Item types that are auto-provisioned by Fabric and not independently deployable.
///
/// These are excluded from export by default (not counted in `total_items`).
/// They can still be explicitly requested via `--item-types`.
const AUTO_PROVISIONED_TYPES: &[&str] = &["SQLEndpoint"];

/// Returns true if the item type should be exported as shell-only (just `.platform`).
fn is_shell_only_type(item_type: &str) -> bool {
    SHELL_ONLY_TYPES
        .iter()
        .any(|t| t.eq_ignore_ascii_case(item_type))
}

/// Returns true if the item type is auto-provisioned by Fabric and not independently deployable.
fn is_auto_provisioned_type(item_type: &str) -> bool {
    AUTO_PROVISIONED_TYPES
        .iter()
        .any(|t| t.eq_ignore_ascii_case(item_type))
}

/// Extract definition parts from a `getDefinition` API response.
fn extract_definition_parts(data: &Value) -> Vec<DefinitionPart> {
    let Some(parts) = data
        .get("definition")
        .and_then(|d| d.get("parts"))
        .and_then(|p| p.as_array())
    else {
        return Vec::new();
    };

    parts
        .iter()
        .filter_map(|p| {
            let path = p.get("path")?.as_str()?.to_owned();
            let payload = p.get("payload")?.as_str()?.to_owned();
            let payload_type = p
                .get("payloadType")
                .and_then(|v| v.as_str())
                .unwrap_or("InlineBase64")
                .to_owned();

            // Skip .platform files from export (we generate our own)
            if path == ".platform" {
                return None;
            }

            Some(DefinitionPart {
                path,
                payload,
                payload_type,
            })
        })
        .collect()
}

/// Try to extract a logical ID from the definition response.
///
/// Some definitions include `.platform` as a part with `config.logicalId`.
fn extract_logical_id(data: &Value) -> Option<String> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    let parts = data.get("definition")?.get("parts")?.as_array()?;

    for part in parts {
        let path = part.get("path")?.as_str()?;
        if path == ".platform" {
            let payload = part.get("payload")?.as_str()?;
            let decoded = BASE64.decode(payload).ok()?;
            let json: Value = serde_json::from_slice(&decoded).ok()?;
            return json
                .get("config")
                .and_then(|c| c.get("logicalId"))
                .and_then(|v| v.as_str())
                .map(str::to_owned);
        }
    }

    None
}

/// The all-zero GUID the Fabric API returns as a placeholder `logicalId` for
/// items with no Git-integration logical id.
const ZERO_LOGICAL_ID: &str = "00000000-0000-0000-0000-000000000000";

/// Resolve a STABLE logical id for an exported item: prefer a real
/// Git-integration `logicalId` from the `.platform` part, but when that is
/// absent or the all-zero placeholder, fall back to the item's own source GUID.
///
/// Using the source GUID as the logical id is what makes cross-item references
/// resolvable on `deploy apply`: a reference in one item's definition that holds
/// another item's source GUID (e.g. an Ontology `DataBinding.itemId` pointing at
/// a Lakehouse) is rewritten to the newly-deployed GUID by the existing
/// logical-id resolver (`build_resolution_map` + `resolve_logical_ids_in_payload`),
/// because the referenced item's logical id now equals that same source GUID.
fn stable_logical_id(data: &Value, source_item_id: &str) -> String {
    match extract_logical_id(data) {
        Some(lid) if !lid.is_empty() && lid != ZERO_LOGICAL_ID => lid,
        _ => source_item_id.to_owned(),
    }
}

/// Export shortcuts for all Lakehouse items in the workspace.
///
/// For each Lakehouse item, fetches deployed shortcuts via `GET /items/{id}/shortcuts`
/// and writes `shortcuts.metadata.json` to the item's export directory.
/// Failures are silently ignored (shortcuts are optional).
async fn export_lakehouse_shortcuts(
    client: &FabricClient,
    workspace_id: &str,
    output_dir: &std::path::Path,
    items: &[ExportableItem],
) {
    for item in items {
        if !item.item_type.eq_ignore_ascii_case("Lakehouse") {
            continue;
        }

        let url = format!("/workspaces/{workspace_id}/items/{}/shortcuts", item.id);
        let Ok(data) = client.get(&url).await else {
            continue;
        };

        let Some(shortcuts) = data.get("value").and_then(|v| v.as_array()) else {
            continue;
        };

        if shortcuts.is_empty() {
            continue;
        }

        let dir_name = format!("{}.{}", item.name, item.item_type);
        let item_dir = output_dir.join(&dir_name);

        // Only write if the item directory already exists (was exported successfully)
        if !item_dir.exists() {
            continue;
        }

        let content = serde_json::to_string_pretty(shortcuts).unwrap_or_default();
        let _ = std::fs::write(item_dir.join("shortcuts.metadata.json"), content);
    }
}

/// Export the SQL analytics endpoint identity (`sqlendpoint.metadata.json`) for
/// each Lakehouse: `{ "id": <endpoint item id>, "server": <connectionString> }`.
///
/// Deploy apply uses this to map the SOURCE endpoint id + server to the newly
/// provisioned TARGET endpoint, so a Direct Lake `SemanticModel`'s
/// `Sql.Database("<server>","<endpointId>")` connection is rewired to the
/// deployed lakehouse. Failures are silently ignored (optional metadata).
async fn export_lakehouse_sql_endpoints(
    client: &FabricClient,
    workspace_id: &str,
    output_dir: &std::path::Path,
    items: &[ExportableItem],
) {
    for item in items {
        if !item.item_type.eq_ignore_ascii_case("Lakehouse") {
            continue;
        }

        let url = format!("/workspaces/{workspace_id}/lakehouses/{}", item.id);
        let Ok(data) = client.get(&url).await else {
            continue;
        };
        let ep = data
            .get("properties")
            .and_then(|p| p.get("sqlEndpointProperties"));
        let (Some(id), Some(server)) = (
            ep.and_then(|e| e.get("id")).and_then(|v| v.as_str()),
            ep.and_then(|e| e.get("connectionString"))
                .and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        if id.is_empty() || server.is_empty() {
            continue;
        }

        let item_dir = output_dir.join(format!("{}.{}", item.name, item.item_type));
        if !item_dir.exists() {
            continue;
        }
        let content = serde_json::to_string_pretty(&serde_json::json!({
            "id": id,
            "server": server,
        }))
        .unwrap_or_default();
        let _ = std::fs::write(item_dir.join("sqlendpoint.metadata.json"), content);
    }
}

/// Write `governance.metadata.json` for items that have tags or sensitivity labels.
///
/// Only writes the file when the item has at least one tag or a sensitivity label.
/// Failures are silently ignored (governance metadata is optional).
fn write_governance_metadata(output_dir: &std::path::Path, items: &[ExportableItem]) {
    for item in items {
        let has_label = item.sensitivity_label.is_some();
        let has_tags = item.tags.as_ref().is_some_and(|t| !t.is_empty());

        if !has_label && !has_tags {
            continue;
        }

        let dir_name = format!("{}.{}", item.name, item.item_type);
        let item_dir = output_dir.join(&dir_name);

        // Only write if the item directory already exists (was exported successfully)
        if !item_dir.exists() {
            continue;
        }

        let mut metadata = serde_json::Map::new();
        if let Some(ref label) = item.sensitivity_label {
            metadata.insert("sensitivityLabel".to_owned(), label.clone());
        }
        if let Some(ref tags) = item.tags
            && !tags.is_empty()
        {
            metadata.insert("tags".to_owned(), Value::Array(tags.clone()));
        }

        let content = serde_json::to_string_pretty(&Value::Object(metadata)).unwrap_or_default();
        let _ = std::fs::write(item_dir.join("governance.metadata.json"), content);
    }
}

/// Known job types to check for schedules per item type.
/// Maps item type (case-insensitive) to the job type string used in the API path.
const SCHEDULE_JOB_TYPES: &[(&str, &str)] = &[
    ("Notebook", "DefaultJob"),
    ("DataPipeline", "Pipeline"),
    ("SparkJobDefinition", "SparkJob"),
    ("Lakehouse", "DefaultJob"),
    ("SemanticModel", "DefaultJob"),
    ("Dataflow", "DefaultJob"),
    ("CopyJob", "DefaultJob"),
];

/// Export job schedules for items that support them.
///
/// For each item, queries the Fabric Job Scheduler API to get existing schedules.
/// Writes `schedules.metadata.json` to the item directory if schedules exist.
/// Failures are silently skipped (not all items support schedules).
async fn export_item_schedules(
    client: &FabricClient,
    workspace_id: &str,
    output_dir: &std::path::Path,
    items: &[ExportableItem],
) {
    for item in items {
        // Determine the job type for this item
        let job_type = SCHEDULE_JOB_TYPES
            .iter()
            .find(|(t, _)| t.eq_ignore_ascii_case(&item.item_type))
            .map(|(_, jt)| *jt);

        let Some(job_type) = job_type else {
            continue;
        };

        let url = format!(
            "/workspaces/{workspace_id}/items/{}/jobs/{job_type}/schedules",
            item.id
        );
        let Ok(data) = client.get(&url).await else {
            continue;
        };

        let Some(schedules) = data.get("value").and_then(|v| v.as_array()) else {
            continue;
        };

        if schedules.is_empty() {
            continue;
        }

        let dir_name = format!("{}.{}", item.name, item.item_type);
        let item_dir = output_dir.join(&dir_name);

        // Only write if the item directory already exists (was exported successfully)
        if !item_dir.exists() {
            continue;
        }

        // Strip server-generated fields (id, createdDateTime, owner) to make it portable
        let portable_schedules: Vec<Value> = schedules
            .iter()
            .map(|s| {
                let mut schedule = s.clone();
                if let Some(obj) = schedule.as_object_mut() {
                    obj.remove("id");
                    obj.remove("createdDateTime");
                    obj.remove("owner");
                }
                // Add the job type so we know what to create on import
                schedule
                    .as_object_mut()
                    .unwrap()
                    .insert("jobType".to_owned(), Value::from(job_type));
                schedule
            })
            .collect();

        let content = serde_json::to_string_pretty(&portable_schedules).unwrap_or_default();
        let _ = std::fs::write(item_dir.join("schedules.metadata.json"), content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use serde_json::json;

    // ── is_shell_only_type ──────────────────────────────────────────────────

    #[test]
    fn shell_only_types_recognized() {
        assert!(is_shell_only_type("Warehouse"));
        assert!(is_shell_only_type("SQLDatabase"));
        assert!(is_shell_only_type("MLExperiment"));
        assert!(is_shell_only_type("MLModel"));
    }

    #[test]
    fn shell_only_type_case_insensitive() {
        assert!(is_shell_only_type("warehouse"));
        assert!(is_shell_only_type("WAREHOUSE"));
        assert!(is_shell_only_type("sqldatabase"));
        assert!(is_shell_only_type("mlmodel"));
    }

    #[test]
    fn non_shell_only_types_rejected() {
        assert!(!is_shell_only_type("Notebook"));
        assert!(!is_shell_only_type("Lakehouse"));
        assert!(!is_shell_only_type("SemanticModel"));
        assert!(!is_shell_only_type("SQLEndpoint"));
        assert!(!is_shell_only_type("Report"));
        assert!(!is_shell_only_type("DataPipeline"));
    }

    #[test]
    fn sql_endpoint_is_not_shell_only() {
        // SQLEndpoint is auto-provisioned by Fabric and should remain fully skipped
        assert!(!is_shell_only_type("SQLEndpoint"));
        assert!(!is_shell_only_type("sqlendpoint"));
    }

    // ── SHELL_ONLY_TYPES constant ───────────────────────────────────────────

    #[test]
    fn shell_only_types_has_expected_count() {
        assert_eq!(
            SHELL_ONLY_TYPES.len(),
            4,
            "SHELL_ONLY_TYPES should have 4 entries; update this test if intentionally changed"
        );
    }

    #[test]
    fn shell_only_types_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for entry in SHELL_ONLY_TYPES {
            assert!(
                seen.insert(entry.to_lowercase()),
                "Duplicate entry in SHELL_ONLY_TYPES: {entry}"
            );
        }
    }

    // ── is_auto_provisioned_type ────────────────────────────────────────────

    #[test]
    fn auto_provisioned_types_recognized() {
        assert!(is_auto_provisioned_type("SQLEndpoint"));
        assert!(is_auto_provisioned_type("sqlendpoint"));
        assert!(is_auto_provisioned_type("SQLENDPOINT"));
    }

    #[test]
    fn non_auto_provisioned_types_rejected() {
        assert!(!is_auto_provisioned_type("Warehouse"));
        assert!(!is_auto_provisioned_type("Lakehouse"));
        assert!(!is_auto_provisioned_type("Notebook"));
        assert!(!is_auto_provisioned_type("SQLDatabase"));
    }

    #[test]
    fn auto_provisioned_and_shell_only_are_disjoint() {
        // No type should be in both lists
        for t in AUTO_PROVISIONED_TYPES {
            assert!(
                !is_shell_only_type(t),
                "{t} should not be in both AUTO_PROVISIONED_TYPES and SHELL_ONLY_TYPES"
            );
        }
        for t in SHELL_ONLY_TYPES {
            assert!(
                !is_auto_provisioned_type(t),
                "{t} should not be in both SHELL_ONLY_TYPES and AUTO_PROVISIONED_TYPES"
            );
        }
    }

    // ── extract_definition_parts ────────────────────────────────────────────

    #[test]
    fn extract_parts_from_valid_response() {
        let data = json!({
            "definition": {
                "parts": [
                    {"path": "notebook.ipynb", "payload": "abc123", "payloadType": "InlineBase64"},
                    {"path": "settings.json", "payload": "def456", "payloadType": "InlineBase64"}
                ]
            }
        });
        let parts = extract_definition_parts(&data);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].path, "notebook.ipynb");
        assert_eq!(parts[0].payload, "abc123");
        assert_eq!(parts[1].path, "settings.json");
    }

    #[test]
    fn extract_parts_filters_out_platform_file() {
        let data = json!({
            "definition": {
                "parts": [
                    {"path": ".platform", "payload": "metadata", "payloadType": "InlineBase64"},
                    {"path": "content.json", "payload": "real_content", "payloadType": "InlineBase64"}
                ]
            }
        });
        let parts = extract_definition_parts(&data);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].path, "content.json");
    }

    #[test]
    fn extract_parts_returns_empty_for_missing_definition() {
        let data = json!({"something": "else"});
        let parts = extract_definition_parts(&data);
        assert!(parts.is_empty());
    }

    #[test]
    fn extract_parts_returns_empty_for_null_parts() {
        let data = json!({"definition": {"parts": null}});
        let parts = extract_definition_parts(&data);
        assert!(parts.is_empty());
    }

    #[test]
    fn extract_parts_defaults_payload_type_to_inline_base64() {
        let data = json!({
            "definition": {
                "parts": [
                    {"path": "file.json", "payload": "data"}
                ]
            }
        });
        let parts = extract_definition_parts(&data);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].payload_type, "InlineBase64");
    }

    // ── extract_logical_id ──────────────────────────────────────────────────

    #[test]
    fn extract_logical_id_from_platform_part() {
        let platform_json = json!({
            "config": {"logicalId": "abc-123-def", "version": "2.0"},
            "metadata": {"type": "Notebook", "displayName": "Test"}
        });
        let encoded = BASE64.encode(serde_json::to_string(&platform_json).unwrap());
        let data = json!({
            "definition": {
                "parts": [
                    {"path": ".platform", "payload": encoded, "payloadType": "InlineBase64"},
                    {"path": "notebook.ipynb", "payload": "content", "payloadType": "InlineBase64"}
                ]
            }
        });
        let logical_id = extract_logical_id(&data);
        assert_eq!(logical_id.as_deref(), Some("abc-123-def"));
    }

    #[test]
    fn extract_logical_id_returns_none_when_no_platform() {
        let data = json!({
            "definition": {
                "parts": [
                    {"path": "notebook.ipynb", "payload": "content", "payloadType": "InlineBase64"}
                ]
            }
        });
        let logical_id = extract_logical_id(&data);
        assert!(logical_id.is_none());
    }

    #[test]
    fn extract_logical_id_returns_none_for_invalid_base64() {
        let data = json!({
            "definition": {
                "parts": [
                    {"path": ".platform", "payload": "not_valid_base64!!!", "payloadType": "InlineBase64"}
                ]
            }
        });
        let logical_id = extract_logical_id(&data);
        assert!(logical_id.is_none());
    }

    #[test]
    fn extract_logical_id_returns_none_when_no_logical_id_field() {
        let platform_json = json!({
            "config": {"version": "2.0"},
            "metadata": {"type": "Notebook"}
        });
        let encoded = BASE64.encode(serde_json::to_string(&platform_json).unwrap());
        let data = json!({
            "definition": {
                "parts": [
                    {"path": ".platform", "payload": encoded, "payloadType": "InlineBase64"}
                ]
            }
        });
        let logical_id = extract_logical_id(&data);
        assert!(logical_id.is_none());
    }

    // ── stable_logical_id ───────────────────────────────────────────────────

    fn platform_data(logical_id: Option<&str>) -> Value {
        let mut cfg = json!({"version": "2.0"});
        if let Some(l) = logical_id {
            cfg["logicalId"] = Value::from(l);
        }
        let platform = json!({"config": cfg, "metadata": {"type": "Lakehouse"}});
        let encoded = BASE64.encode(serde_json::to_string(&platform).unwrap());
        json!({"definition": {"parts": [
            {"path": ".platform", "payload": encoded, "payloadType": "InlineBase64"}
        ]}})
    }

    #[test]
    fn stable_logical_id_prefers_real_git_logical_id() {
        let data = platform_data(Some("real-git-lid"));
        assert_eq!(stable_logical_id(&data, "src-guid"), "real-git-lid");
    }

    #[test]
    fn stable_logical_id_falls_back_to_source_guid_for_zero_placeholder() {
        let data = platform_data(Some(ZERO_LOGICAL_ID));
        assert_eq!(stable_logical_id(&data, "src-guid-123"), "src-guid-123");
    }

    #[test]
    fn stable_logical_id_falls_back_to_source_guid_when_absent() {
        let data = platform_data(None);
        assert_eq!(stable_logical_id(&data, "src-guid-456"), "src-guid-456");
    }

    // ── ontology_child_names ────────────────────────────────────────────────

    fn item(item_type: &str, name: &str, id: &str) -> ExportableItem {
        ExportableItem {
            id: id.to_owned(),
            item_type: item_type.to_owned(),
            name: name.to_owned(),
            description: None,
            sensitivity_label: None,
            tags: None,
        }
    }

    #[test]
    fn ontology_child_names_derived_from_ontology_id() {
        let items = vec![item(
            "Ontology",
            "RetailOntology",
            "3c341d94-1f51-438f-ae78-d09942d8e81b",
        )];
        let names = ontology_child_names(&items);
        assert!(names.contains("RetailOntology_lh_3c341d941f51438fae78d09942d8e81b"));
        assert!(names.contains("RetailOntology_graph_3c341d941f51438fae78d09942d8e81b"));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn ontology_child_names_empty_without_ontology() {
        let items = vec![item("Lakehouse", "MyLH", "id-1")];
        assert!(ontology_child_names(&items).is_empty());
    }

    // ── write_governance_metadata ───────────────────────────────────────────

    #[test]
    fn governance_metadata_written_when_has_label() {
        let dir = tempfile::TempDir::new().unwrap();
        let item_dir = dir.path().join("MyNb.Notebook");
        std::fs::create_dir_all(&item_dir).unwrap();

        let items = vec![ExportableItem {
            id: "id-1".to_owned(),
            item_type: "Notebook".to_owned(),
            name: "MyNb".to_owned(),
            description: None,
            sensitivity_label: Some(json!({"id": "label-uuid"})),
            tags: None,
        }];

        write_governance_metadata(dir.path(), &items);

        let gov_path = item_dir.join("governance.metadata.json");
        assert!(gov_path.exists());
        let content = std::fs::read_to_string(&gov_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["sensitivityLabel"]["id"], "label-uuid");
    }

    #[test]
    fn governance_metadata_written_when_has_tags() {
        let dir = tempfile::TempDir::new().unwrap();
        let item_dir = dir.path().join("MyNb.Notebook");
        std::fs::create_dir_all(&item_dir).unwrap();

        let items = vec![ExportableItem {
            id: "id-1".to_owned(),
            item_type: "Notebook".to_owned(),
            name: "MyNb".to_owned(),
            description: None,
            sensitivity_label: None,
            tags: Some(vec![json!({"id": "tag-1", "displayName": "Prod"})]),
        }];

        write_governance_metadata(dir.path(), &items);

        let gov_path = item_dir.join("governance.metadata.json");
        assert!(gov_path.exists());
        let content = std::fs::read_to_string(&gov_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["tags"][0]["id"], "tag-1");
        assert_eq!(parsed["tags"][0]["displayName"], "Prod");
    }

    #[test]
    fn governance_metadata_not_written_when_no_governance() {
        let dir = tempfile::TempDir::new().unwrap();
        let item_dir = dir.path().join("MyNb.Notebook");
        std::fs::create_dir_all(&item_dir).unwrap();

        let items = vec![ExportableItem {
            id: "id-1".to_owned(),
            item_type: "Notebook".to_owned(),
            name: "MyNb".to_owned(),
            description: None,
            sensitivity_label: None,
            tags: None,
        }];

        write_governance_metadata(dir.path(), &items);

        let gov_path = item_dir.join("governance.metadata.json");
        assert!(!gov_path.exists());
    }

    #[test]
    fn governance_metadata_not_written_when_empty_tags() {
        let dir = tempfile::TempDir::new().unwrap();
        let item_dir = dir.path().join("MyNb.Notebook");
        std::fs::create_dir_all(&item_dir).unwrap();

        let items = vec![ExportableItem {
            id: "id-1".to_owned(),
            item_type: "Notebook".to_owned(),
            name: "MyNb".to_owned(),
            description: None,
            sensitivity_label: None,
            tags: Some(vec![]),
        }];

        write_governance_metadata(dir.path(), &items);

        let gov_path = item_dir.join("governance.metadata.json");
        assert!(!gov_path.exists());
    }

    #[test]
    fn governance_metadata_not_written_if_dir_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        // Don't create item_dir

        let items = vec![ExportableItem {
            id: "id-1".to_owned(),
            item_type: "Notebook".to_owned(),
            name: "MyNb".to_owned(),
            description: None,
            sensitivity_label: Some(json!({"id": "label-uuid"})),
            tags: None,
        }];

        write_governance_metadata(dir.path(), &items);

        // Should not crash, just skip
        let gov_path = dir.path().join("MyNb.Notebook/governance.metadata.json");
        assert!(!gov_path.exists());
    }
}
