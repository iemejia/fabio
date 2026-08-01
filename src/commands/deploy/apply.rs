use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};

use super::changeset::{Change, ChangeAction, Changeset, DeployFailure, DeployResult};
use super::ordering::{delete_priority, deploy_priority, topological_sort};
use super::platform::SourceWorkspace;

/// Write a progress line to stderr (diagnostics channel).
/// Only emits when stderr is connected (non-quiet mode).
pub(super) fn emit_progress(quiet: bool, msg: &str) {
    if !quiet {
        eprintln!("[deploy] {msg}");
    }
}

/// Execute the deployment changeset.
///
/// Items are deployed in dependency order (by type), with parallelism within each type batch.
/// Deletes happen last, in reverse dependency order.
#[allow(clippy::too_many_lines)]
pub async fn execute_changeset(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    changeset: &Changeset,
    source: &SourceWorkspace,
    concurrency: usize,
    fail_fast: bool,
) -> Result<DeployResult> {
    let start = Instant::now();

    let mut succeeded: Vec<Change> = Vec::new();
    let mut failed: Vec<DeployFailure> = Vec::new();
    let mut skipped: Vec<Change> = Vec::new();

    // Separate changes by action
    let mut creates_updates: Vec<&Change> = Vec::new();
    let mut deletes: Vec<&Change> = Vec::new();

    for change in &changeset.changes {
        match change.action {
            ChangeAction::Create | ChangeAction::Update | ChangeAction::Rename => {
                creates_updates.push(change);
            }
            ChangeAction::Delete => deletes.push(change),
            ChangeAction::Skip => skipped.push(change.clone()),
        }
    }

    // Group creates/updates by type and sort by deploy priority
    let mut type_groups: HashMap<&str, Vec<&Change>> = HashMap::new();
    for change in &creates_updates {
        type_groups
            .entry(change.item_type.as_str())
            .or_default()
            .push(change);
    }

    let mut sorted_types: Vec<&str> = type_groups.keys().copied().collect();
    sorted_types.sort_by_key(|t| deploy_priority(t));

    // Group types into tiers for cross-type parallelism.
    // Types in the same tier have no mutual dependencies and can run concurrently.
    let mut tier_groups: Vec<Vec<&str>> = Vec::new();
    {
        use super::ordering::deploy_tier;
        let mut current_tier = usize::MAX;
        for &item_type in &sorted_types {
            let tier = deploy_tier(item_type);
            if tier != current_tier {
                tier_groups.push(Vec::new());
                current_tier = tier;
            }
            tier_groups.last_mut().unwrap().push(item_type);
        }
    }

    // Build lookup for source items by (type, name) for definition access
    let source_map: HashMap<(&str, &str), usize> = source
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            (
                (
                    item.metadata.item_type.as_str(),
                    item.metadata.display_name.as_str(),
                ),
                i,
            )
        })
        .collect();

    // Map to track created items: (type, name) → deployed GUID
    // Used for logical ID resolution in subsequent batches.
    // Seed with deployed IDs from the changeset (pre-existing items being updated/skipped).
    // This ensures cross-item references resolve even for items deployed in prior runs.
    let mut created_ids: HashMap<(String, String), String> = HashMap::new();
    for change in &changeset.changes {
        if let Some(ref id) = change.deployed_id {
            created_ids.insert((change.item_type.clone(), change.name.clone()), id.clone());
        }
    }

    // Execute creates/updates in tier order.
    // Types within the same tier are independent and can run concurrently.
    // DataPipeline and Dataflow need topological ordering and run sequentially.
    let total_changes = creates_updates.len();
    let completed = AtomicUsize::new(0);

    for tier_types in &tier_groups {
        // Split tier items into parallel (most types) and sequential (pipeline/dataflow)
        let mut parallel_changes: Vec<&Change> = Vec::new();
        let mut sequential_changes: Vec<Vec<&Change>> = Vec::new();

        for &item_type in tier_types {
            let group = &type_groups[item_type];

            if item_type == "DataPipeline" {
                sequential_changes.push(order_pipelines(group, source, &source_map)?);
            } else if item_type.eq_ignore_ascii_case("Dataflow") {
                sequential_changes.push(order_dataflows(group, source, &source_map)?);
            } else {
                parallel_changes.extend(group.iter());
            }
        }

        let tier_label = tier_types.join(", ");
        if !parallel_changes.is_empty() {
            emit_progress(
                cli.quiet,
                &format!(
                    "deploying {} item(s) [{tier_label}] [{}/{}]",
                    parallel_changes.len(),
                    completed.load(Ordering::Relaxed),
                    total_changes
                ),
            );
        }

        // Execute parallel items with bounded concurrency
        let batch_concurrency = concurrency.max(1);

        if !parallel_changes.is_empty() {
            if batch_concurrency == 1 || parallel_changes.len() <= 1 {
                for change in &parallel_changes {
                    if fail_fast && !failed.is_empty() {
                        break;
                    }

                    let result = execute_single_change(
                        cli,
                        client,
                        workspace_id,
                        change,
                        source,
                        &source_map,
                        &created_ids,
                    )
                    .await;

                    match result {
                        Ok(deployed_id) => {
                            let mut change_result = (*change).clone();
                            if let Some(ref id) = deployed_id {
                                created_ids.insert(
                                    (change.item_type.clone(), change.name.clone()),
                                    id.clone(),
                                );
                                change_result.deployed_id = Some(id.clone());
                            }
                            succeeded.push(change_result);
                            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                            emit_progress(
                                cli.quiet,
                                &format!(
                                    "  {} {} \"{}\" [{}/{}]",
                                    match change.action {
                                        ChangeAction::Create => "created",
                                        ChangeAction::Rename => "renamed",
                                        _ => "updated",
                                    },
                                    change.item_type,
                                    change.name,
                                    done,
                                    total_changes
                                ),
                            );
                        }
                        Err(e) => {
                            completed.fetch_add(1, Ordering::Relaxed);
                            emit_progress(
                                cli.quiet,
                                &format!(
                                    "  FAILED {} \"{}\" : {}",
                                    change.item_type,
                                    change.name,
                                    e.root_cause()
                                ),
                            );
                            failed.push(DeployFailure {
                                change: (*change).clone(),
                                error: e.to_string(),
                                code: extract_error_code(&e),
                            });
                        }
                    }
                }
            } else {
                // Parallel execution across all types in this tier
                let semaphore = Arc::new(Semaphore::new(batch_concurrency));
                let mut handles = Vec::with_capacity(parallel_changes.len());
                let dry_run = cli.dry_run;

                for change in &parallel_changes {
                    let sem = Arc::clone(&semaphore);
                    let change_owned = (*change).clone();
                    let ws_id = workspace_id.to_owned();
                    let client_clone = client.clone();

                    let src_idx = source_map
                        .get(&(change.item_type.as_str(), change.name.as_str()))
                        .copied();

                    let source_item = src_idx.map(|idx| source.items[idx].clone());
                    let res_map = build_resolution_map(source, &created_ids);
                    let mut sm_ids = build_semantic_model_name_map(&created_ids);
                    extend_semantic_model_map_with_dir_aliases(&mut sm_ids, source);

                    handles.push(tokio::spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        let result = execute_single_change_owned(
                            dry_run,
                            &client_clone,
                            &ws_id,
                            &change_owned,
                            source_item.as_ref(),
                            &res_map,
                            &sm_ids,
                        )
                        .await;
                        (change_owned, result)
                    }));
                }

                // Collect results
                for handle in handles {
                    let (mut change_owned, result) = handle.await?;
                    match result {
                        Ok(deployed_id) => {
                            if let Some(ref id) = deployed_id {
                                created_ids.insert(
                                    (change_owned.item_type.clone(), change_owned.name.clone()),
                                    id.clone(),
                                );
                                change_owned.deployed_id = Some(id.clone());
                            }
                            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                            emit_progress(
                                cli.quiet,
                                &format!(
                                    "  {} {} \"{}\" [{}/{}]",
                                    if change_owned.action == ChangeAction::Create {
                                        "created"
                                    } else {
                                        "updated"
                                    },
                                    change_owned.item_type,
                                    change_owned.name,
                                    done,
                                    total_changes
                                ),
                            );
                            succeeded.push(change_owned);
                        }
                        Err(e) => {
                            completed.fetch_add(1, Ordering::Relaxed);
                            emit_progress(
                                cli.quiet,
                                &format!(
                                    "  FAILED {} \"{}\" : {}",
                                    change_owned.item_type,
                                    change_owned.name,
                                    e.root_cause()
                                ),
                            );
                            if fail_fast {
                                failed.push(DeployFailure {
                                    change: change_owned,
                                    error: e.to_string(),
                                    code: extract_error_code(&e),
                                });
                                break;
                            }
                            failed.push(DeployFailure {
                                change: change_owned,
                                error: e.to_string(),
                                code: extract_error_code(&e),
                            });
                        }
                    }
                }
            }
        }

        // Execute sequential items (DataPipeline/Dataflow) in topological order
        for ordered_group in &sequential_changes {
            let group_type = ordered_group
                .first()
                .map_or("unknown", |c| c.item_type.as_str());

            // Auto-create Lakehouses referenced by pipelines but not in source
            if group_type == "DataPipeline" && !cli.dry_run {
                let lakehouse_deps =
                    extract_pipeline_lakehouse_deps(ordered_group, source, &source_map);
                for (lakehouse_name, artifact_ids) in &lakehouse_deps {
                    // Skip if already created/deployed
                    if created_ids.contains_key(&("Lakehouse".to_owned(), lakehouse_name.clone())) {
                        // Map old artifact IDs to the known deployed ID
                        if let Some(deployed_id) = created_ids
                            .get(&("Lakehouse".to_owned(), lakehouse_name.clone()))
                            .cloned()
                        {
                            for old_id in artifact_ids {
                                created_ids.insert(
                                    ("_artifact_alias".to_owned(), old_id.clone()),
                                    deployed_id.clone(),
                                );
                            }
                        }
                        continue;
                    }
                    // Create shell Lakehouse in target workspace
                    emit_progress(
                        cli.quiet,
                        &format!(
                            "  auto-creating Lakehouse \"{lakehouse_name}\" (referenced by pipeline)"
                        ),
                    );
                    let body = serde_json::json!({
                        "displayName": lakehouse_name,
                        "type": "Lakehouse"
                    });
                    match client
                        .post(&format!("/workspaces/{workspace_id}/items"), &body, true)
                        .await
                    {
                        Ok(resp) => {
                            if let Some(id) = resp.get("id").and_then(|v| v.as_str()) {
                                // Register the new Lakehouse and map old artifact IDs
                                created_ids.insert(
                                    ("Lakehouse".to_owned(), lakehouse_name.clone()),
                                    id.to_owned(),
                                );
                                for old_id in artifact_ids {
                                    created_ids.insert(
                                        ("_artifact_alias".to_owned(), old_id.clone()),
                                        id.to_owned(),
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            emit_progress(
                                cli.quiet,
                                &format!(
                                    "  WARNING: failed to auto-create Lakehouse \"{lakehouse_name}\": {e}"
                                ),
                            );
                        }
                    }
                }
            }

            emit_progress(
                cli.quiet,
                &format!(
                    "deploying {} {} item(s) (ordered) [{}/{}]",
                    ordered_group.len(),
                    group_type,
                    completed.load(Ordering::Relaxed),
                    total_changes
                ),
            );

            for change in ordered_group {
                if fail_fast && !failed.is_empty() {
                    break;
                }

                let result = execute_single_change(
                    cli,
                    client,
                    workspace_id,
                    change,
                    source,
                    &source_map,
                    &created_ids,
                )
                .await;

                match result {
                    Ok(deployed_id) => {
                        let mut change_result = (*change).clone();
                        if let Some(ref id) = deployed_id {
                            created_ids.insert(
                                (change.item_type.clone(), change.name.clone()),
                                id.clone(),
                            );
                            change_result.deployed_id = Some(id.clone());
                        }
                        succeeded.push(change_result);
                        let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                        emit_progress(
                            cli.quiet,
                            &format!(
                                "  {} {} \"{}\" [{}/{}]",
                                match change.action {
                                    ChangeAction::Create => "created",
                                    ChangeAction::Rename => "renamed",
                                    _ => "updated",
                                },
                                change.item_type,
                                change.name,
                                done,
                                total_changes
                            ),
                        );
                    }
                    Err(e) => {
                        completed.fetch_add(1, Ordering::Relaxed);
                        emit_progress(
                            cli.quiet,
                            &format!(
                                "  FAILED {} \"{}\" : {}",
                                change.item_type,
                                change.name,
                                e.root_cause()
                            ),
                        );
                        failed.push(DeployFailure {
                            change: (*change).clone(),
                            error: e.to_string(),
                            code: extract_error_code(&e),
                        });
                    }
                }
            }
        }

        // Inter-tier hook: refresh SemanticModels after their tier completes.
        // Reports in the next tier need the model refreshed to bind successfully.
        if !cli.dry_run {
            for change in &succeeded {
                if change.item_type.eq_ignore_ascii_case("SemanticModel")
                    && matches!(change.action, ChangeAction::Create | ChangeAction::Update)
                    && tier_types
                        .iter()
                        .any(|&t| t.eq_ignore_ascii_case("SemanticModel"))
                    && let Some(ref item_id) = change.deployed_id
                {
                    emit_progress(
                        cli.quiet,
                        &format!(
                            "  refreshing semantic model \"{}\" (required for Report binding)",
                            change.name
                        ),
                    );
                    let path =
                        format!("/workspaces/{workspace_id}/semanticModels/{item_id}/refreshes");
                    let _ = client.post(&path, &json!({"type": "Full"}), false).await;
                }
            }
        }
    }

    // Execute deletes in reverse dependency order (always sequential for safety)
    let mut deletes_sorted = deletes;
    deletes_sorted.sort_by_key(|c| delete_priority(&c.item_type));

    if !deletes_sorted.is_empty() {
        emit_progress(
            cli.quiet,
            &format!("deleting {} orphaned item(s)", deletes_sorted.len()),
        );
    }

    for (i, change) in deletes_sorted.iter().enumerate() {
        if fail_fast && !failed.is_empty() {
            break;
        }

        let result = execute_delete(client, workspace_id, change).await;
        match result {
            Ok(()) => {
                emit_progress(
                    cli.quiet,
                    &format!(
                        "  deleted {} \"{}\" [{}/{}]",
                        change.item_type,
                        change.name,
                        i + 1,
                        deletes_sorted.len()
                    ),
                );
                succeeded.push((*change).clone());
            }
            Err(e) => {
                emit_progress(
                    cli.quiet,
                    &format!(
                        "  FAILED delete {} \"{}\" : {}",
                        change.item_type,
                        change.name,
                        e.root_cause()
                    ),
                );
                failed.push(DeployFailure {
                    change: (*change).clone(),
                    error: e.to_string(),
                    code: extract_error_code(&e),
                });
            }
        }
    }

    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    Ok(DeployResult {
        succeeded,
        failed,
        skipped,
        duration_ms,
    })
}

/// Execute a changeset using the Bulk Import Definitions API.
///
/// Batches all creates/updates into a single `bulkImportDefinitions` call for speed.
/// Deletes and renames are still handled per-item (bulk API is additive-only).
///
/// Uses the flat `definitionParts` format: each part's path includes the item directory
/// prefix (`/<displayName>.<itemType>/<relativePath>`), matching the format returned by
/// `bulkExportDefinitions`. The server infers items from directory structure in paths.
///
/// **Limitations vs per-item strategy:**
/// - No mid-session logical ID resolution (`$items.Type.Name.id` won't resolve for items created in the same batch)
/// - Items with duplicate logical IDs (e.g., all `00000000-...`) are rejected (`BadSystemFiles`)
/// - Items with inter-dependencies (`KQLDatabase` -> `Eventhouse`, `Report` -> `SemanticModel`)
///   fail with `DependenciesCouldNotBeResolved` — the bulk API cannot sequence creation order
/// - No per-item error granularity (response includes per-item status in `importItemDefinitionsDetails`)
/// - Requires workspace NOT connected to Git (API limitation: `ActiveCiCdOperationInProgress`)
/// - Renames still executed per-item
///
/// **Best for:** workspace-to-workspace cloning (round-trip with `bulkExportDefinitions`),
/// deploying independent items (standalone Notebooks) to empty workspaces.
#[allow(clippy::too_many_lines)]
pub async fn execute_changeset_bulk(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    changeset: &Changeset,
    source: &SourceWorkspace,
) -> Result<DeployResult> {
    let start = Instant::now();

    let mut succeeded: Vec<Change> = Vec::new();
    let mut failed: Vec<DeployFailure> = Vec::new();
    let mut skipped: Vec<Change> = Vec::new();

    // Separate by action
    let mut bulk_items: Vec<&Change> = Vec::new();
    let mut deletes: Vec<&Change> = Vec::new();
    let mut renames: Vec<&Change> = Vec::new();

    for change in &changeset.changes {
        match change.action {
            ChangeAction::Create | ChangeAction::Update => bulk_items.push(change),
            ChangeAction::Rename => renames.push(change),
            ChangeAction::Delete => deletes.push(change),
            ChangeAction::Skip => skipped.push(change.clone()),
        }
    }

    // Execute renames per-item first (bulk API doesn't support renames)
    for change in &renames {
        if let Some(ref item_id) = change.deployed_id {
            let body = json!({"displayName": change.name});
            match client
                .patch(
                    &format!("/workspaces/{workspace_id}/items/{item_id}"),
                    &body,
                )
                .await
            {
                Ok(_) => {
                    let mut c = (*change).clone();
                    c.deployed_id = Some(item_id.clone());
                    succeeded.push(c);
                }
                Err(e) => {
                    failed.push(DeployFailure {
                        change: (*change).clone(),
                        error: e.to_string(),
                        code: "RENAME_FAILED".to_owned(),
                    });
                }
            }
        }
    }

    // Build bulk import payload using the flat definitionParts format.
    // The API expects: { "definitionParts": [{path, payload, payloadType}, ...], "options": {...} }
    // Each part path includes the item directory prefix: /<displayName>.<itemType>/<relativePath>
    // This matches the format returned by bulkExportDefinitions.
    if !bulk_items.is_empty() {
        emit_progress(
            cli.quiet,
            &format!(
                "[bulk strategy] submitting {} item(s) via bulkImportDefinitions...",
                bulk_items.len()
            ),
        );

        let mut definition_parts: Vec<Value> = Vec::new();

        for change in &bulk_items {
            // Find the source item definition
            let source_item = source.items.iter().find(|si| {
                si.metadata
                    .item_type
                    .eq_ignore_ascii_case(&change.item_type)
                    && si.metadata.display_name == change.name
            });

            let Some(source_item) = source_item else {
                failed.push(DeployFailure {
                    change: (*change).clone(),
                    error: "Source item not found in source workspace".to_owned(),
                    code: "SOURCE_NOT_FOUND".to_owned(),
                });
                continue;
            };

            // Build the item directory prefix: /<displayName>.<itemType>/
            let item_dir = format!("/{}.{}", change.name, change.item_type);

            // Include ALL parts (including .platform) with full paths.
            // The server infers items from the directory structure in the paths.
            for p in &source_item.parts {
                definition_parts.push(json!({
                    "path": format!("{}/{}", item_dir, p.path),
                    "payload": p.payload,
                    "payloadType": p.payload_type,
                }));
            }
        }

        // Submit bulk import with flat definitionParts format
        let import_body = json!({
            "definitionParts": definition_parts,
            "options": {
                "allowPairingByName": true
            }
        });

        match client
            .post(
                &format!("/workspaces/{workspace_id}/items/bulkImportDefinitions?beta=True"),
                &import_body,
                true, // LRO poll
            )
            .await
        {
            Ok(response) => {
                // Parse per-item results from importItemDefinitionsDetails
                let details = response
                    .get("importItemDefinitionsDetails")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                for change in &bulk_items {
                    let mut c = (*change).clone();
                    // Find matching detail by displayName + type
                    let detail = details.iter().find(|d| {
                        d.get("displayName")
                            .and_then(|v| v.as_str())
                            .is_some_and(|n| n.eq_ignore_ascii_case(&change.name))
                            && d.get("type")
                                .and_then(|v| v.as_str())
                                .is_some_and(|t| t.eq_ignore_ascii_case(&change.item_type))
                    });
                    if let Some(detail) = detail {
                        let status = detail.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        if status.eq_ignore_ascii_case("Failed") {
                            let err_msg = detail
                                .get("errorDetails")
                                .and_then(|v| v.get("errorCode"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown bulk import error");
                            failed.push(DeployFailure {
                                change: c,
                                error: err_msg.to_owned(),
                                code: "BULK_IMPORT_FAILED".to_owned(),
                            });
                            continue;
                        }
                        c.deployed_id =
                            detail.get("id").and_then(|v| v.as_str()).map(str::to_owned);
                    }
                    succeeded.push(c);
                }
            }
            Err(e) => {
                let err_msg = e.to_string();
                // If bulk fails, mark ALL items as failed
                if err_msg.contains("ActiveCiCdOperation") {
                    let hint = "Bulk strategy requires workspace not connected to Git. Use --strategy default, or disconnect Git: fabio git disconnect --workspace <WS>";
                    for change in &bulk_items {
                        failed.push(DeployFailure {
                            change: (*change).clone(),
                            error: format!(
                                "Bulk import blocked: workspace has Git integration active. {hint}"
                            ),
                            code: "BULK_GIT_BLOCKED".to_owned(),
                        });
                    }
                } else {
                    for change in &bulk_items {
                        failed.push(DeployFailure {
                            change: (*change).clone(),
                            error: err_msg.clone(),
                            code: "BULK_IMPORT_FAILED".to_owned(),
                        });
                    }
                }
            }
        }
    }

    // Execute deletes per-item (bulk API doesn't support deletes)
    for change in &deletes {
        if let Some(ref item_id) = change.deployed_id {
            emit_progress(
                cli.quiet,
                &format!("  deleting {} \"{}\"", change.item_type, change.name),
            );
            match client
                .delete(&format!("/workspaces/{workspace_id}/items/{item_id}"))
                .await
            {
                Ok(_) => succeeded.push((*change).clone()),
                Err(e) => {
                    failed.push(DeployFailure {
                        change: (*change).clone(),
                        error: e.to_string(),
                        code: "DELETE_FAILED".to_owned(),
                    });
                }
            }
        }
    }

    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(DeployResult {
        succeeded,
        failed,
        skipped,
        duration_ms,
    })
}

/// Execute post-deploy hooks for items that were successfully deployed.
///
/// Hooks:
/// - **Semantic Model**: Triggers `POST /datasets/{id}/refreshes` (framing refresh for Direct Lake)
/// - **Environment**: Triggers `POST /environments/{id}/staging/publish` (publishes staged changes)
///
/// Returns a list of hook result objects (for inclusion in the deploy output).
#[allow(clippy::too_many_lines)]
pub async fn execute_post_hooks(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    succeeded: &[Change],
) -> Vec<Value> {
    let mut results: Vec<Value> = Vec::new();

    for change in succeeded {
        match change.action {
            ChangeAction::Create | ChangeAction::Update | ChangeAction::Rename => {}
            _ => continue,
        }

        let Some(ref item_id) = change.deployed_id else {
            continue;
        };

        match change.item_type.as_str() {
            "Lakehouse" if change.action == ChangeAction::Create => {
                // Feature 3: Poll SQL endpoint provisioning after Lakehouse creation
                emit_progress(
                    cli.quiet,
                    &format!(
                        "post-hook: polling SQL endpoint for lakehouse \"{}\"",
                        change.name
                    ),
                );
                match poll_lakehouse_sql_endpoint(client, workspace_id, item_id).await {
                    Ok(()) => {
                        results.push(json!({
                            "hook": "sql_endpoint_poll",
                            "item_type": "Lakehouse",
                            "item_name": change.name,
                            "status": "ready"
                        }));
                    }
                    Err(e) => {
                        emit_progress(
                            cli.quiet,
                            &format!(
                                "  post-hook WARNING: SQL endpoint polling for \"{}\": {}",
                                change.name,
                                e.root_cause()
                            ),
                        );
                        results.push(json!({
                            "hook": "sql_endpoint_poll",
                            "item_type": "Lakehouse",
                            "item_name": change.name,
                            "status": "timeout",
                            "error": e.to_string()
                        }));
                    }
                }
            }
            "SemanticModel" => {
                // Feature 9: Bind connection first (if semantic_model_binding is configured)
                // Connection binding is handled externally via parameter substitution
                // Here we just trigger the refresh for Direct Lake framing
                emit_progress(
                    cli.quiet,
                    &format!("post-hook: refreshing semantic model \"{}\"", change.name),
                );
                let path = format!("/workspaces/{workspace_id}/semanticModels/{item_id}/refreshes");
                let body = json!({"type": "Full"});
                match client.post(&path, &body, false).await {
                    Ok(_) => {
                        results.push(json!({
                            "hook": "refresh",
                            "item_type": "SemanticModel",
                            "item_name": change.name,
                            "status": "triggered"
                        }));
                    }
                    Err(e) => {
                        emit_progress(
                            cli.quiet,
                            &format!(
                                "  post-hook FAILED: refresh semantic model \"{}\": {}",
                                change.name,
                                e.root_cause()
                            ),
                        );
                        results.push(json!({
                            "hook": "refresh",
                            "item_type": "SemanticModel",
                            "item_name": change.name,
                            "status": "failed",
                            "error": e.to_string()
                        }));
                    }
                }
            }
            "Environment" => {
                // Feature 10: Trigger publish and poll for completion
                emit_progress(
                    cli.quiet,
                    &format!("post-hook: publishing environment \"{}\"", change.name),
                );
                let path =
                    format!("/workspaces/{workspace_id}/environments/{item_id}/staging/publish");
                let body = json!({});
                match client.post(&path, &body, false).await {
                    Ok(_) => {
                        // Poll for publish completion
                        let poll_result = poll_environment_publish(
                            cli,
                            client,
                            workspace_id,
                            item_id,
                            &change.name,
                        )
                        .await;
                        results.push(json!({
                            "hook": "publish",
                            "item_type": "Environment",
                            "item_name": change.name,
                            "status": match poll_result {
                                Ok(()) => "succeeded",
                                Err(_) => "triggered"
                            }
                        }));
                    }
                    Err(e) => {
                        emit_progress(
                            cli.quiet,
                            &format!(
                                "  post-hook FAILED: publish environment \"{}\": {}",
                                change.name,
                                e.root_cause()
                            ),
                        );
                        results.push(json!({
                            "hook": "publish",
                            "item_type": "Environment",
                            "item_name": change.name,
                            "status": "failed",
                            "error": e.to_string()
                        }));
                    }
                }
            }
            _ => {}
        }
    }

    results
}

/// Post-hook: Activate variable library value sets matching the --env name.
///
/// Microsoft best practices recommend that value set names match environment
/// names (e.g., "dev", "test", "prod"). After deploying variable libraries,
/// this hook activates the value set whose name matches the deploy environment.
/// Uses PATCH /workspaces/{ws}/variableLibraries/{id} with properties.activeValueSetName.
///
/// Non-fatal: failures are reported but don't fail the deployment.
pub async fn activate_variable_library_value_sets(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    succeeded: &[Change],
    env_name: &str,
) -> Vec<Value> {
    let mut results: Vec<Value> = Vec::new();

    for change in succeeded {
        if !change.item_type.eq_ignore_ascii_case("VariableLibrary") {
            continue;
        }
        // Only activate on create or update (skip/delete/rename irrelevant)
        match change.action {
            ChangeAction::Create | ChangeAction::Update => {}
            _ => continue,
        }

        let Some(ref item_id) = change.deployed_id else {
            continue;
        };

        emit_progress(
            cli.quiet,
            &format!(
                "post-hook: activating value set \"{}\" for variable library \"{}\"",
                env_name, change.name
            ),
        );

        let body = serde_json::json!({
            "properties": {
                "activeValueSetName": env_name
            }
        });

        match client
            .patch(
                &format!("/workspaces/{workspace_id}/variableLibraries/{item_id}"),
                &body,
            )
            .await
        {
            Ok(_) => {
                results.push(json!({
                    "hook": "activate_value_set",
                    "item_type": "VariableLibrary",
                    "item_name": change.name,
                    "value_set": env_name,
                    "status": "activated"
                }));
            }
            Err(e) => {
                // Non-fatal: if the value set doesn't exist, warn but don't fail
                let err_msg = e.root_cause().to_string();
                emit_progress(
                    cli.quiet,
                    &format!(
                        "  post-hook WARNING: activate value set \"{}\" for \"{}\": {}",
                        env_name, change.name, err_msg
                    ),
                );
                results.push(json!({
                    "hook": "activate_value_set",
                    "item_type": "VariableLibrary",
                    "item_name": change.name,
                    "value_set": env_name,
                    "status": "failed",
                    "error": err_msg
                }));
            }
        }
    }

    results
}

/// Post-hook: Apply job schedules from `schedules.metadata.json` to deployed items.
///
/// For each successfully deployed item that has schedules defined in the source,
/// creates the schedules via the Fabric Job Scheduler API. Existing schedules
/// on the item are left untouched (additive, not reconciling).
///
/// Non-fatal: failures are reported but don't fail the deployment.
pub async fn apply_item_schedules(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    succeeded: &[Change],
    source: &super::platform::SourceWorkspace,
) -> Vec<Value> {
    let mut results: Vec<Value> = Vec::new();

    for change in succeeded {
        match change.action {
            ChangeAction::Create | ChangeAction::Update => {}
            _ => continue,
        }

        let Some(ref item_id) = change.deployed_id else {
            continue;
        };

        // Find the source item with matching type + name
        let source_item = source.items.iter().find(|si| {
            si.metadata
                .item_type
                .eq_ignore_ascii_case(&change.item_type)
                && si.metadata.display_name == change.name
        });

        let Some(source_item) = source_item else {
            continue;
        };

        let Some(ref schedules) = source_item.schedules else {
            continue;
        };

        emit_progress(
            cli.quiet,
            &format!(
                "post-hook: applying {} schedule(s) for \"{}\"",
                schedules.len(),
                change.name
            ),
        );

        for schedule in schedules {
            let job_type = schedule
                .get("jobType")
                .and_then(|v| v.as_str())
                .unwrap_or("DefaultJob");

            // Build the create body (strip our internal jobType field)
            let mut body = schedule.clone();
            if let Some(obj) = body.as_object_mut() {
                obj.remove("jobType");
            }

            let path =
                format!("/workspaces/{workspace_id}/items/{item_id}/jobs/{job_type}/schedules");

            match client.post(&path, &body, false).await {
                Ok(_) => {
                    results.push(json!({
                        "hook": "create_schedule",
                        "item_type": change.item_type,
                        "item_name": change.name,
                        "job_type": job_type,
                        "status": "created"
                    }));
                }
                Err(e) => {
                    let err_msg = e.root_cause().to_string();
                    emit_progress(
                        cli.quiet,
                        &format!(
                            "  post-hook WARNING: create schedule for \"{}\": {}",
                            change.name, err_msg
                        ),
                    );
                    results.push(json!({
                        "hook": "create_schedule",
                        "item_type": change.item_type,
                        "item_name": change.name,
                        "job_type": job_type,
                        "status": "failed",
                        "error": err_msg
                    }));
                }
            }
        }
    }

    results
}

/// Feature 3: Poll SQL endpoint provisioning status after Lakehouse creation.
///
/// The SQL analytics endpoint takes time to provision after a lakehouse is created.
/// This polls the lakehouse properties until the endpoint is ready.
async fn poll_lakehouse_sql_endpoint(
    client: &FabricClient,
    workspace_id: &str,
    lakehouse_id: &str,
) -> Result<()> {
    let url = format!("workspaces/{workspace_id}/lakehouses/{lakehouse_id}");
    let max_wait = std::time::Duration::from_mins(5);
    let poll_interval = std::time::Duration::from_secs(5);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > max_wait {
            return Err(FabioError::with_hint(ErrorCode::Timeout, "SQL endpoint provisioning timed out after 300 seconds", "The lakehouse SQL endpoint is still provisioning. Wait and retry, or check status in the Fabric portal.").into());
        }

        let resp = client.get(&url).await?;

        let status = resp
            .get("properties")
            .and_then(|p| p.get("sqlEndpointProperties"))
            .and_then(|ep| ep.get("provisioningStatus"))
            .and_then(|s| s.as_str())
            .unwrap_or("Unknown");

        match status {
            "Success" => return Ok(()),
            "Failed" => return Err(FabioError::with_hint(ErrorCode::ApiError, "SQL endpoint provisioning failed", "Check capacity state and lakehouse health. Ensure capacity is active: fabio capacity list").into()),
            _ => tokio::time::sleep(poll_interval).await,
        }
    }
}

/// Feature 10: Poll environment publish state until completion.
///
/// After triggering `staging/publish`, polls the environment status until
/// the publish succeeds, fails, or times out.
async fn poll_environment_publish(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    environment_id: &str,
    env_name: &str,
) -> Result<()> {
    let url = format!("workspaces/{workspace_id}/environments/{environment_id}");
    let max_wait = std::time::Duration::from_mins(5);
    let poll_interval = std::time::Duration::from_secs(5);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > max_wait {
            emit_progress(
                cli.quiet,
                &format!(
                    "  environment \"{env_name}\" publish still in progress (timed out waiting)"
                ),
            );
            return Err(FabioError::with_hint(ErrorCode::Timeout, "Environment publish polling timed out", "The environment is still publishing. Check status: fabio environment show --workspace <WS> --id <ID>").into());
        }

        tokio::time::sleep(poll_interval).await;

        let resp = client.get(&url).await;
        let Ok(body) = resp else {
            continue; // Retry on transient errors
        };

        let state = body
            .get("properties")
            .and_then(|p| p.get("publishInfo"))
            .and_then(|pi| pi.get("state"))
            .and_then(|s| s.as_str())
            .unwrap_or("");

        match state {
            "Succeeded" | "Completed" => {
                emit_progress(
                    cli.quiet,
                    &format!("  environment \"{env_name}\" publish succeeded"),
                );
                return Ok(());
            }
            "Failed" => {
                emit_progress(
                    cli.quiet,
                    &format!("  environment \"{env_name}\" publish failed"),
                );
                return Err(FabioError::with_hint(ErrorCode::ApiError, "Environment publish failed", "Check staging settings: fabio environment get-staging-spark-settings --workspace <WS> --id <ID>").into());
            }
            "Cancelled" => {
                emit_progress(
                    cli.quiet,
                    &format!("  environment \"{env_name}\" publish was cancelled"),
                );
                return Err(FabioError::with_hint(
                    ErrorCode::ApiError,
                    "Environment publish was cancelled",
                    "Retry: fabio environment publish --workspace <WS> --id <ID>",
                )
                .into());
            }
            _ => {} // Still in progress, continue polling
        }
    }
}

/// Reconcile lakehouse shortcuts after deployment.
///
/// For each Lakehouse item that was deployed (Create/Update/Rename) and has
/// a `shortcuts.metadata.json` in the source, this function:
/// 1. Lists currently deployed shortcuts from the live workspace
/// 2. Deletes orphan shortcuts (deployed but not in source)
/// 3. Creates/overwrites shortcuts from the source definition
///
/// Shortcut failures are non-fatal (same as other post-hooks).
pub async fn execute_shortcut_hooks(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    succeeded: &[Change],
    source: &SourceWorkspace,
) -> Vec<Value> {
    let mut results: Vec<Value> = Vec::new();

    for change in succeeded {
        // Only process Lakehouse items that were successfully deployed
        if !change.item_type.eq_ignore_ascii_case("Lakehouse") {
            continue;
        }

        match change.action {
            ChangeAction::Create | ChangeAction::Update | ChangeAction::Rename => {}
            _ => continue,
        }

        let Some(ref item_id) = change.deployed_id else {
            continue;
        };

        // Find the source item to get its shortcuts definition
        let source_item = source
            .type_name_index
            .get(&(change.item_type.clone(), change.name.clone()))
            .and_then(|&idx| source.items.get(idx));

        let Some(source_item) = source_item else {
            continue;
        };

        let Some(ref shortcuts) = source_item.shortcuts else {
            continue;
        };

        emit_progress(
            cli.quiet,
            &format!(
                "post-hook: reconciling shortcuts for lakehouse \"{}\"",
                change.name
            ),
        );

        // Replace default GUID placeholder in shortcut itemId with the lakehouse's own GUID.
        // fabric-cicd does this via `_replace_default_lakehouse_id` — the itemId of
        // "00000000-0000-0000-0000-000000000000" means "this lakehouse itself".
        let resolved_shortcuts: Vec<Value> = shortcuts
            .iter()
            .map(|sc| {
                let mut s = sc.to_string();
                // Only replace in oneLake.itemId context (self-referencing shortcut)
                if s.contains("00000000-0000-0000-0000-000000000000") {
                    s = s.replace("00000000-0000-0000-0000-000000000000", item_id);
                }
                serde_json::from_str(&s).unwrap_or_else(|_| sc.clone())
            })
            .collect();

        match reconcile_shortcuts(client, workspace_id, item_id, &resolved_shortcuts).await {
            Ok(summary) => {
                results.push(json!({
                    "hook": "shortcuts",
                    "item_type": "Lakehouse",
                    "item_name": change.name,
                    "status": "completed",
                    "created": summary.created,
                    "deleted": summary.deleted,
                    "total": summary.total
                }));
            }
            Err(e) => {
                emit_progress(
                    cli.quiet,
                    &format!(
                        "  post-hook FAILED: reconcile shortcuts for \"{}\": {}",
                        change.name,
                        e.root_cause()
                    ),
                );
                results.push(json!({
                    "hook": "shortcuts",
                    "item_type": "Lakehouse",
                    "item_name": change.name,
                    "status": "failed",
                    "error": e.to_string()
                }));
            }
        }
    }

    results
}

struct ShortcutSummary {
    created: usize,
    deleted: usize,
    total: usize,
}

/// Reconcile shortcuts for a single lakehouse item.
///
/// Lists deployed shortcuts, computes diff against source, deletes orphans,
/// and creates/overwrites all source shortcuts.
async fn reconcile_shortcuts(
    client: &FabricClient,
    workspace_id: &str,
    item_id: &str,
    source_shortcuts: &[Value],
) -> Result<ShortcutSummary> {
    // 1. List currently deployed shortcuts
    let list_url = format!("/workspaces/{workspace_id}/items/{item_id}/shortcuts");
    let deployed = client.get(&list_url).await.map_or_else(
        |_| Vec::new(),
        |data| {
            data.get("value")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
        },
    );

    // Build a set of deployed shortcut keys: "path/name"
    let deployed_keys: std::collections::HashSet<String> = deployed
        .iter()
        .filter_map(|sc| {
            let path = sc.get("path")?.as_str()?;
            let name = sc.get("name")?.as_str()?;
            // Normalize: trim leading slash from path for consistent matching
            let normalized_path = path.trim_start_matches('/');
            Some(format!("{normalized_path}/{name}"))
        })
        .collect();

    // Build a map of source shortcuts keyed by "path/name"
    let source_keys: std::collections::HashSet<String> = source_shortcuts
        .iter()
        .filter_map(|sc| {
            let path = sc.get("path")?.as_str()?;
            let name = sc.get("name")?.as_str()?;
            let normalized_path = path.trim_start_matches('/');
            Some(format!("{normalized_path}/{name}"))
        })
        .collect();

    // 2. Delete orphans (deployed but not in source)
    let mut deleted = 0;
    for key in &deployed_keys {
        if !source_keys.contains(key) {
            let delete_url = format!("/workspaces/{workspace_id}/items/{item_id}/shortcuts/{key}");
            if client.delete(&delete_url).await.is_ok() {
                deleted += 1;
            }
        }
    }

    // 3. Create/overwrite all source shortcuts
    let mut created = 0;
    let create_url = format!(
        "/workspaces/{workspace_id}/items/{item_id}/shortcuts?shortcutConflictPolicy=CreateOrOverwrite"
    );
    for shortcut in source_shortcuts {
        if client.post(&create_url, shortcut, false).await.is_ok() {
            created += 1;
        }
    }

    Ok(ShortcutSummary {
        created,
        deleted,
        total: source_shortcuts.len(),
    })
}

/// Apply governance tags to items that were created during deployment.
///
/// Non-fatal: failures are reported but don't fail the deploy.
/// Only applies to newly created items — existing items retain their governance state.
pub async fn apply_governance_tags(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    succeeded: &[Change],
    source: &super::platform::SourceWorkspace,
) -> Vec<Value> {
    let mut results = Vec::new();

    for change in succeeded {
        if change.action != ChangeAction::Create {
            continue;
        }

        let Some(ref item_id) = change.deployed_id else {
            continue;
        };

        // Find the source item
        let source_item = source
            .type_name_index
            .get(&(change.item_type.clone(), change.name.clone()))
            .and_then(|&idx| source.items.get(idx));

        let Some(source_item) = source_item else {
            continue;
        };

        let Some(ref governance) = source_item.governance else {
            continue;
        };

        if governance.tags.is_empty() {
            continue;
        }

        let tag_ids: Vec<&str> = governance.tags.iter().map(|t| t.id.as_str()).collect();

        emit_progress(
            cli.quiet,
            &format!(
                "governance: applying {} tag(s) to {} \"{}\"",
                tag_ids.len(),
                change.item_type,
                change.name
            ),
        );

        let url = format!("/workspaces/{workspace_id}/items/{item_id}/tags");
        let body = json!({"tags": tag_ids.iter().map(|id| json!({"id": id})).collect::<Vec<_>>()});
        match client.post(&url, &body, false).await {
            Ok(_) => {
                results.push(json!({
                    "hook": "apply_tags",
                    "item_type": change.item_type,
                    "item_name": change.name,
                    "status": "succeeded",
                    "tags_applied": tag_ids.len()
                }));
            }
            Err(e) => {
                emit_progress(
                    cli.quiet,
                    &format!(
                        "  governance WARNING: apply tags for \"{}\": {}",
                        change.name,
                        e.root_cause()
                    ),
                );
                results.push(json!({
                    "hook": "apply_tags",
                    "item_type": change.item_type,
                    "item_name": change.name,
                    "status": "failed",
                    "error": e.to_string()
                }));
            }
        }
    }

    results
}

///
/// Returns the deployed item GUID on success.
async fn execute_single_change(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    change: &Change,
    source: &SourceWorkspace,
    source_map: &HashMap<(&str, &str), usize>,
    created_ids: &HashMap<(String, String), String>,
) -> Result<Option<String>> {
    let source_idx = source_map
        .get(&(change.item_type.as_str(), change.name.as_str()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Source item not found for {} \"{}\"",
                change.item_type,
                change.name
            )
        })?;

    let source_item = &source.items[*source_idx];

    // Build logical ID resolution map from created_ids + source workspace info
    let resolution_map = build_resolution_map(source, created_ids);

    // Build semantic model name → deployed_id map for report byConnection resolution
    let mut semantic_model_ids = build_semantic_model_name_map(created_ids);
    extend_semantic_model_map_with_dir_aliases(&mut semantic_model_ids, source);

    deploy_change(
        cli.dry_run,
        client,
        workspace_id,
        change,
        source_item,
        &resolution_map,
        &semantic_model_ids,
    )
    .await
}

/// Execute a single create or update with owned data (for parallel spawned tasks).
async fn execute_single_change_owned(
    dry_run: bool,
    client: &FabricClient,
    workspace_id: &str,
    change: &Change,
    source_item: Option<&super::platform::SourceItem>,
    resolution_map: &HashMap<String, String>,
    semantic_model_ids: &HashMap<String, String>,
) -> Result<Option<String>> {
    let source_item = source_item.ok_or_else(|| {
        anyhow::anyhow!(
            "Source item not found for {} \"{}\"",
            change.item_type,
            change.name
        )
    })?;
    deploy_change(
        dry_run,
        client,
        workspace_id,
        change,
        source_item,
        resolution_map,
        semantic_model_ids,
    )
    .await
}

/// Core deploy logic shared by sequential and parallel paths.
#[allow(clippy::option_if_let_else, clippy::too_many_lines)]
async fn deploy_change(
    dry_run: bool,
    client: &FabricClient,
    workspace_id: &str,
    change: &Change,
    source_item: &super::platform::SourceItem,
    resolution_map: &HashMap<String, String>,
    semantic_model_ids: &HashMap<String, String>,
) -> Result<Option<String>> {
    // Build definition parts for API, applying transformations:
    // 1. Report byPath → byConnection conversion
    // 2. Report byConnection semanticmodelid GUID resolution
    // 3. Logical ID resolution (replace logical IDs with deployed GUIDs)
    let parts: Vec<Value> = source_item
        .parts
        .iter()
        .map(|p| {
            let mut payload = p.payload.clone();

            // Transform Report definition.pbir: convert byPath to byConnection
            if change.item_type.eq_ignore_ascii_case("Report") && p.path == "definition.pbir" {
                payload =
                    transform_report_bypath_to_byconnection(&payload, source_item, resolution_map);
                // Resolve byConnection semanticmodelid GUIDs by semantic model name
                payload = resolve_report_byconnection_model_id(&payload, semantic_model_ids);
            }

            // Apply logical ID resolution to all payloads
            let payload = resolve_logical_ids_in_payload(&payload, resolution_map);
            json!({
                "path": p.path,
                "payload": payload,
                "payloadType": p.payload_type,
            })
        })
        .collect();

    // Sort parts for Notebook items: .py/.ipynb content files before .json settings files.
    // The Fabric API processes definition parts in order — content must precede settings.
    let parts = if change.item_type.eq_ignore_ascii_case("Notebook") {
        let mut sorted = parts;
        sorted.sort_by_key(|p| {
            let path = p.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let ext = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            match ext {
                "py" | "ipynb" => 0, // Content files first
                "json" => 2,         // JSON settings last
                _ => 1,              // Everything else in between
            }
        });
        sorted
    } else {
        parts
    };

    match change.action {
        ChangeAction::Create => {
            // Omit definition entirely when there are no parts (e.g. Lakehouse, MLModel)
            let mut body = if parts.is_empty() {
                json!({
                    "displayName": change.name,
                    "type": change.item_type
                })
            } else {
                let definition = if let Some(ref fmt) = source_item.metadata.definition_format {
                    json!({ "format": fmt, "parts": parts })
                } else {
                    json!({ "parts": parts })
                };
                json!({
                    "displayName": change.name,
                    "type": change.item_type,
                    "definition": definition
                })
            };

            // Include creationPayload if present (e.g. KQLDatabase eventhouse-id)
            // Apply logical ID resolution to creationPayload (replaces logical IDs with deployed GUIDs)
            if let Some(ref payload) = source_item.creation_payload {
                let resolved_payload = resolve_logical_ids_in_json(payload, resolution_map);
                body.as_object_mut()
                    .unwrap()
                    .insert("creationPayload".to_owned(), resolved_payload);
            }

            // Include description if present in source metadata
            if let Some(ref desc) = source_item.metadata.description {
                body.as_object_mut()
                    .unwrap()
                    .insert("description".to_owned(), Value::from(desc.as_str()));
            }

            // Include sensitivityLabelSettings. Prefer the governance sidecar
            // label; fall back to the label carried in the .platform metadata
            // (platformProperties 2.1.0, used by Fabric Git Integration / fabric-cicd).
            let sensitivity_label_id = source_item
                .governance
                .as_ref()
                .and_then(|g| g.sensitivity_label.as_ref())
                .map(|l| l.id.clone())
                .or_else(|| source_item.metadata.sensitivity_label_id.clone());
            if let Some(label_id) = sensitivity_label_id {
                body.as_object_mut().unwrap().insert(
                    "sensitivityLabelSettings".to_owned(),
                    json!({"sensitivityLabelId": label_id}),
                );
            }

            if dry_run {
                return Ok(None);
            }

            // Retry loop for ItemDisplayNameNotAvailableYet (name recently freed by deletion).
            // The Fabric API may reject creation for up to 5 minutes after item deletion.
            let url = format!("/workspaces/{workspace_id}/items");
            let mut last_err = None;
            for attempt in 0..10 {
                match client.post(&url, &body, true).await {
                    Ok(result) => {
                        let id = result.get("id").and_then(|v| v.as_str()).map(str::to_owned);
                        return Ok(id);
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        if err_str.contains("ItemDisplayNameNotAvailableYet")
                            || err_str.contains("displayName is not available")
                        {
                            // Name not yet freed — wait and retry
                            if attempt < 9 {
                                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                                last_err = Some(e);
                                continue;
                            }
                        }
                        return Err(e);
                    }
                }
            }
            Err(last_err.unwrap_or_else(|| FabioError::with_hint(ErrorCode::Conflict, "Create failed after retries — item name may still be reserved from a recent deletion", "Wait 5 minutes for the name to be released, then retry. Or use a different name.").into()))
        }
        ChangeAction::Update => {
            let deployed_id = change.deployed_id.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "No deployed ID for update of {} \"{}\"",
                    change.item_type,
                    change.name
                )
            })?;

            // Skip updateDefinition when there are no parts (nothing to update)
            if parts.is_empty() {
                return Ok(Some(deployed_id.to_owned()));
            }

            let definition = if let Some(ref fmt) = source_item.metadata.definition_format {
                json!({ "format": fmt, "parts": parts })
            } else {
                json!({ "parts": parts })
            };

            let body = json!({
                "definition": definition
            });

            if dry_run {
                return Ok(Some(deployed_id.to_owned()));
            }

            client
                .post(
                    &format!("/workspaces/{workspace_id}/items/{deployed_id}/updateDefinition?updateMetadata=true"),
                    &body,
                    true,
                )
                .await?;

            Ok(Some(deployed_id.to_owned()))
        }
        ChangeAction::Rename => {
            let deployed_id = change.deployed_id.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "No deployed ID for rename of {} \"{}\"",
                    change.item_type,
                    change.name
                )
            })?;

            if dry_run {
                return Ok(Some(deployed_id.to_owned()));
            }

            // Step 1: Rename the item via PATCH
            let patch_body = if let Some(ref desc) = source_item.metadata.description {
                json!({ "displayName": change.name, "description": desc })
            } else {
                json!({ "displayName": change.name })
            };

            client
                .patch(
                    &format!("/workspaces/{workspace_id}/items/{deployed_id}"),
                    &patch_body,
                )
                .await?;

            // Step 2: Update definition if there are parts
            if !parts.is_empty() {
                let definition = if let Some(ref fmt) = source_item.metadata.definition_format {
                    json!({ "format": fmt, "parts": parts })
                } else {
                    json!({ "parts": parts })
                };

                let body = json!({ "definition": definition });

                client
                    .post(
                        &format!("/workspaces/{workspace_id}/items/{deployed_id}/updateDefinition?updateMetadata=true"),
                        &body,
                        true,
                    )
                    .await?;
            }

            Ok(Some(deployed_id.to_owned()))
        }
        _ => Ok(None),
    }
}

/// Execute a delete operation.
async fn execute_delete(client: &FabricClient, workspace_id: &str, change: &Change) -> Result<()> {
    let deployed_id = change.deployed_id.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "No deployed ID for delete of {} \"{}\"",
            change.item_type,
            change.name
        )
    })?;

    client
        .delete(&format!("/workspaces/{workspace_id}/items/{deployed_id}"))
        .await?;

    Ok(())
}

/// Order `DataPipeline` changes by their internal references (sub-pipeline invocations).
fn order_pipelines<'a>(
    changes: &[&'a Change],
    source: &SourceWorkspace,
    source_map: &HashMap<(&str, &str), usize>,
) -> Result<Vec<&'a Change>> {
    if changes.len() <= 1 {
        return Ok(changes.to_vec());
    }

    // Build a set of pipeline names in this batch for reference matching
    let pipeline_names: std::collections::HashSet<&str> =
        changes.iter().map(|c| c.name.as_str()).collect();

    // Extract pipeline references from definitions
    let mut items_with_refs: Vec<(String, Vec<String>)> = Vec::new();

    for change in changes {
        let raw_refs = source_map
            .get(&("DataPipeline", change.name.as_str()))
            .map_or_else(Vec::new, |idx| {
                extract_pipeline_references(&source.items[*idx])
            });

        // Resolve references: if a ref is a name in our batch, use it directly.
        // If it's a GUID, try to match it to a pipeline name via activity name heuristic.
        let resolved_refs: Vec<String> = raw_refs
            .into_iter()
            .filter_map(|r| {
                if pipeline_names.contains(r.as_str()) {
                    Some(r)
                } else if is_guid_like(&r) {
                    // GUID reference: try to find matching pipeline by activity name
                    // (already extracted as ref, try fuzzy match against batch names)
                    None // Can't resolve here — topological sort will still work
                // because the GUID won't match any name (treated as external ref)
                } else {
                    // Might be a partial name match
                    pipeline_names
                        .iter()
                        .find(|&&name| name.eq_ignore_ascii_case(&r))
                        .map(|&name| name.to_owned())
                }
            })
            .collect();

        items_with_refs.push((change.name.clone(), resolved_refs));
    }

    let sorted_names = topological_sort(&items_with_refs)?;

    // Reorder changes to match sorted order
    let change_map: HashMap<&str, &'a Change> =
        changes.iter().map(|c| (c.name.as_str(), *c)).collect();

    let mut ordered = Vec::with_capacity(changes.len());
    for name in &sorted_names {
        if let Some(change) = change_map.get(name.as_str()) {
            ordered.push(*change);
        }
    }

    Ok(ordered)
}

/// Extract names of other `DataPipelines` referenced by this pipeline's definition.
///
/// Looks for "Execute Pipeline" activity references in the pipeline JSON.
fn extract_pipeline_references(source_item: &super::platform::SourceItem) -> Vec<String> {
    let mut refs = Vec::new();

    for part in &source_item.parts {
        // Only check pipeline content files
        if !part.path.contains("pipeline")
            && !std::path::Path::new(&part.path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            continue;
        }

        let Ok(decoded) = BASE64.decode(&part.payload) else {
            continue;
        };
        let Ok(content) = String::from_utf8(decoded) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<Value>(&content) else {
            continue;
        };

        // Look for ExecutePipeline activities
        if let Some(activities) = json
            .get("properties")
            .and_then(|p| p.get("activities"))
            .and_then(|a| a.as_array())
        {
            for activity in activities {
                let is_execute_pipeline = activity
                    .get("type")
                    .and_then(|t| t.as_str())
                    .is_some_and(|t| t == "ExecutePipeline");

                if is_execute_pipeline
                    && let Some(name) = activity
                        .get("typeProperties")
                        .and_then(|tp| tp.get("pipeline"))
                        .and_then(|p| p.get("referenceName"))
                        .and_then(|n| n.as_str())
                {
                    refs.push(name.to_owned());
                }
            }
        }
    }

    refs
}

/// Order `Dataflow` changes by their internal cross-references.
///
/// Dataflows can reference other dataflows via `PowerPlatform.Dataflows` patterns
/// or by GUIDs/logical IDs that match other dataflows in the batch.
fn order_dataflows<'a>(
    changes: &[&'a Change],
    source: &SourceWorkspace,
    source_map: &HashMap<(&str, &str), usize>,
) -> Result<Vec<&'a Change>> {
    if changes.len() <= 1 {
        return Ok(changes.to_vec());
    }

    // Extract dataflow references from definitions
    let mut items_with_refs: Vec<(String, Vec<String>)> = Vec::new();

    // Build a set of dataflow names and logical IDs for reference detection
    let dataflow_names: std::collections::HashSet<&str> =
        changes.iter().map(|c| c.name.as_str()).collect();
    let dataflow_logical_ids: HashMap<&str, &str> = changes
        .iter()
        .filter_map(|c| {
            source_map
                .get(&("Dataflow", c.name.as_str()))
                .and_then(|idx| source.items[*idx].metadata.logical_id.as_deref())
                .map(|lid| (lid, c.name.as_str()))
        })
        .collect();

    for change in changes {
        let refs = source_map
            .get(&("Dataflow", change.name.as_str()))
            .map_or_else(Vec::new, |idx| {
                extract_dataflow_references(
                    &source.items[*idx],
                    &dataflow_names,
                    &dataflow_logical_ids,
                )
            });
        items_with_refs.push((change.name.clone(), refs));
    }

    let sorted_names = topological_sort(&items_with_refs)?;

    // Reorder changes to match sorted order
    let change_map: HashMap<&str, &'a Change> =
        changes.iter().map(|c| (c.name.as_str(), *c)).collect();

    let mut ordered = Vec::with_capacity(changes.len());
    for name in &sorted_names {
        if let Some(change) = change_map.get(name.as_str()) {
            ordered.push(*change);
        }
    }

    Ok(ordered)
}

/// Extract names of other Dataflows referenced by this dataflow's definition.
///
/// Looks for `PowerPlatform.Dataflows` references and logical ID matches.
fn extract_dataflow_references(
    source_item: &super::platform::SourceItem,
    dataflow_names: &std::collections::HashSet<&str>,
    dataflow_logical_ids: &HashMap<&str, &str>,
) -> Vec<String> {
    let mut refs = Vec::new();

    for part in &source_item.parts {
        let Ok(decoded) = BASE64.decode(&part.payload) else {
            continue;
        };
        let Ok(content) = String::from_utf8(decoded) else {
            continue;
        };

        // Check for PowerPlatform.Dataflows references
        // Pattern: references to other dataflow names in PQ expressions
        for &name in dataflow_names {
            if name != source_item.metadata.display_name && content.contains(name) {
                refs.push(name.to_owned());
            }
        }

        // Check for logical ID references
        for (&lid, &name) in dataflow_logical_ids {
            if name != source_item.metadata.display_name
                && content.contains(lid)
                && !refs.iter().any(|r| r == name)
            {
                refs.push(name.to_owned());
            }
        }
    }

    refs
}

/// Build a map from logical IDs to deployed (runtime) IDs.
///
/// This enables cross-item reference resolution: when item A's definition references
/// item B by its logical ID, we replace it with item B's actual deployed GUID.
///
/// Sources of resolution:
/// 1. `created_ids` — items created earlier in this apply session `(type, name)` → `deployed_id`
/// 2. Source workspace — items that have both a `logical_id` and a `deployed_id` in the changeset
fn build_resolution_map(
    source: &SourceWorkspace,
    created_ids: &HashMap<(String, String), String>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();

    // Map logical_id → deployed_id from created_ids (items created in this session)
    for ((item_type, name), deployed_id) in created_ids {
        // Find the source item's logical_id
        if let Some(&idx) = source
            .type_name_index
            .get(&(item_type.clone(), name.clone()))
            && let Some(ref logical_id) = source.items[idx].metadata.logical_id
        {
            map.insert(logical_id.clone(), deployed_id.clone());
        }
    }

    // Pipeline GUID resolution: scan pipeline definitions for ExecutePipeline
    // activities whose referenceName is a GUID. Map those GUIDs to the deployed
    // ID of the pipeline they reference (matched by activity name ≈ pipeline name).
    for item in &source.items {
        if !item.metadata.item_type.eq_ignore_ascii_case("DataPipeline") {
            continue;
        }
        let activity_refs = extract_pipeline_activity_guid_map(item);
        for (guid, activity_name) in &activity_refs {
            // Try to find the deployed ID for a pipeline matching the activity name
            if let Some(deployed_id) =
                find_item_deployed_id_by_activity_name(activity_name, created_ids)
            {
                map.insert(guid.clone(), deployed_id);
            }
        }
    }

    // Include artifact alias mappings (old artifactId → new deployed ID).
    // These are created by the pipeline Lakehouse auto-creation step.
    for ((key_type, old_id), new_id) in created_ids {
        if key_type == "_artifact_alias" {
            map.insert(old_id.clone(), new_id.clone());
        }
    }

    map
}

/// Extract Lakehouse dependencies from pipeline definitions.
///
/// Scans `linkedService` entries in pipeline activities for Lakehouse references.
/// Returns a map of `lakehouse_name → Vec<old_artifact_id>` for Lakehouses
/// that are NOT in the source directory (need to be auto-created).
fn extract_pipeline_lakehouse_deps(
    pipeline_changes: &[&Change],
    source: &super::platform::SourceWorkspace,
    source_map: &HashMap<(&str, &str), usize>,
) -> HashMap<String, Vec<String>> {
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();

    // Names of Lakehouses already in the source (will be deployed normally)
    let source_lakehouse_names: std::collections::HashSet<&str> = source
        .items
        .iter()
        .filter(|i| i.metadata.item_type.eq_ignore_ascii_case("Lakehouse"))
        .map(|i| i.metadata.display_name.as_str())
        .collect();

    for change in pipeline_changes {
        let Some(&idx) = source_map.get(&(change.item_type.as_str(), change.name.as_str())) else {
            continue;
        };
        let source_item = &source.items[idx];

        for part in &source_item.parts {
            let Ok(decoded) = BASE64.decode(&part.payload) else {
                continue;
            };
            let Ok(content) = String::from_utf8(decoded) else {
                continue;
            };

            // Find all linkedService Lakehouse references via regex for performance
            // Pattern: "type": "Lakehouse" within a linkedService block with "name" and "artifactId"
            // Use JSON parsing for correctness
            let Ok(json) = serde_json::from_str::<Value>(&content) else {
                continue;
            };

            // Recursively find all linkedService entries with type=Lakehouse
            find_lakehouse_refs_recursive(&json, &source_lakehouse_names, &mut deps);
        }
    }

    deps
}

/// Recursively search a JSON value for `linkedService` entries that reference Lakehouses.
fn find_lakehouse_refs_recursive(
    value: &Value,
    source_lakehouse_names: &std::collections::HashSet<&str>,
    deps: &mut HashMap<String, Vec<String>>,
) {
    match value {
        Value::Object(map) => {
            // Check if this object is a linkedService with type=Lakehouse
            if let (Some(name_val), Some(props)) = (map.get("name"), map.get("properties")) {
                let ls_type = props
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or_default();
                if ls_type.eq_ignore_ascii_case("Lakehouse") {
                    let ls_name = name_val.as_str().unwrap_or_default();
                    let artifact_id = props
                        .get("typeProperties")
                        .and_then(|tp| tp.get("artifactId"))
                        .and_then(|a| a.as_str())
                        .unwrap_or_default();

                    if !ls_name.is_empty()
                        && !artifact_id.is_empty()
                        && !source_lakehouse_names.contains(ls_name)
                    {
                        deps.entry(ls_name.to_owned())
                            .or_default()
                            .push(artifact_id.to_owned());
                    }
                }
            }
            // Recurse into all values
            for v in map.values() {
                find_lakehouse_refs_recursive(v, source_lakehouse_names, deps);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                find_lakehouse_refs_recursive(v, source_lakehouse_names, deps);
            }
        }
        _ => {}
    }
}

/// Extract (`referenceName` GUID, activity name) pairs from `ExecutePipeline` activities.
///
/// Also extracts `notebookId` references from `TridentNotebook` activities
/// and `artifactId` references from other activities — these are used to resolve
/// cross-item references in a full first-time deployment.
fn extract_pipeline_activity_guid_map(
    source_item: &super::platform::SourceItem,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();

    for part in &source_item.parts {
        if !part.path.contains("pipeline")
            && !std::path::Path::new(&part.path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            continue;
        }

        let Ok(decoded) = BASE64.decode(&part.payload) else {
            continue;
        };
        let Ok(content) = String::from_utf8(decoded) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<Value>(&content) else {
            continue;
        };

        if let Some(activities) = json
            .get("properties")
            .and_then(|p| p.get("activities"))
            .and_then(|a| a.as_array())
        {
            for activity in activities {
                let act_type = activity
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or_default();
                let act_name = activity
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default();
                let type_props = activity.get("typeProperties");

                match act_type {
                    "ExecutePipeline" => {
                        let ref_name = type_props
                            .and_then(|tp| tp.get("pipeline"))
                            .and_then(|p| p.get("referenceName"))
                            .and_then(|n| n.as_str())
                            .unwrap_or_default();
                        if is_guid_like(ref_name) && !act_name.is_empty() {
                            pairs.push((ref_name.to_owned(), act_name.to_owned()));
                        }
                    }
                    "TridentNotebook" => {
                        // Notebook activity: notebookId references a notebook
                        let notebook_id = type_props
                            .and_then(|tp| tp.get("notebookId"))
                            .and_then(|n| n.as_str())
                            .unwrap_or_default();
                        if is_guid_like(notebook_id) && !act_name.is_empty() {
                            pairs.push((notebook_id.to_owned(), act_name.to_owned()));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    pairs
}
/// Find the deployed ID for an item matching the given activity name.
///
/// Searches across `DataPipeline` and `Notebook` types. Tries exact match first,
/// then case-insensitive, then contains (for plural/prefix variants).
fn find_item_deployed_id_by_activity_name(
    activity_name: &str,
    created_ids: &HashMap<(String, String), String>,
) -> Option<String> {
    // Item types that pipeline activities commonly reference
    let search_types = ["DataPipeline", "Notebook"];

    for item_type in &search_types {
        // Exact match
        if let Some(id) = created_ids.get(&((*item_type).to_owned(), activity_name.to_owned())) {
            return Some(id.clone());
        }
    }

    // Case-insensitive match across all relevant types
    for ((item_type, name), id) in created_ids {
        if search_types
            .iter()
            .any(|t| item_type.eq_ignore_ascii_case(t))
            && name.eq_ignore_ascii_case(activity_name)
        {
            return Some(id.clone());
        }
    }

    // Fuzzy: activity name contains item name or vice versa
    for ((item_type, name), id) in created_ids {
        if search_types
            .iter()
            .any(|t| item_type.eq_ignore_ascii_case(t))
            && (activity_name.contains(name.as_str()) || name.contains(activity_name))
        {
            return Some(id.clone());
        }
    }

    None
}

/// Replace logical IDs found in a base64-encoded definition payload with deployed IDs.
///
/// Decodes the payload, performs string replacement for any GUIDs in the resolution map,
/// and re-encodes. If the payload is not valid UTF-8 or contains no matches, returns
/// the original payload unchanged.
fn resolve_logical_ids_in_payload(
    payload: &str,
    resolution_map: &HashMap<String, String>,
) -> String {
    if resolution_map.is_empty() {
        return payload.to_owned();
    }

    let Ok(decoded) = BASE64.decode(payload) else {
        return payload.to_owned();
    };
    let Ok(mut content) = String::from_utf8(decoded) else {
        return payload.to_owned();
    };

    let mut replaced = false;
    for (logical_id, deployed_id) in resolution_map {
        if content.contains(logical_id.as_str()) {
            content = content.replace(logical_id.as_str(), deployed_id.as_str());
            replaced = true;
        }
    }

    if replaced {
        BASE64.encode(content.as_bytes())
    } else {
        payload.to_owned()
    }
}

/// Transform a Report's `definition.pbir` from `byPath` to `byConnection` format.
///
/// The Fabric REST API does not support `byPath` references (filesystem-relative paths
/// to semantic models). This function detects `byPath`, resolves the referenced semantic
/// model's logical ID from the source directory structure, and converts to `byConnection`.
///
/// If the payload doesn't contain `byPath` or resolution fails, returns the original payload.
fn transform_report_bypath_to_byconnection(
    payload: &str,
    source_item: &super::platform::SourceItem,
    resolution_map: &HashMap<String, String>,
) -> String {
    let Ok(decoded_bytes) = BASE64.decode(payload) else {
        return payload.to_owned();
    };
    let Ok(content) = String::from_utf8(decoded_bytes) else {
        return payload.to_owned();
    };

    let Ok(mut pbir) = serde_json::from_str::<Value>(&content) else {
        return payload.to_owned();
    };

    // Check if this has a byPath reference
    let Some(rel_path) = pbir
        .get("datasetReference")
        .and_then(|dr| dr.get("byPath"))
        .and_then(|bp| bp.get("path"))
        .and_then(|p| p.as_str())
        .map(str::to_owned)
    else {
        return payload.to_owned();
    };

    // Extract semantic model directory name from relative path
    // e.g., "../ABC.SemanticModel" → "ABC.SemanticModel"
    let model_dir_name = rel_path
        .rsplit('/')
        .next()
        .unwrap_or(&rel_path)
        .trim_start_matches("../");

    // Parse "Name.SemanticModel" format to get display name
    let model_name = model_dir_name
        .strip_suffix(".SemanticModel")
        .or_else(|| model_dir_name.strip_suffix(".Dataset"))
        .unwrap_or(model_dir_name);

    // Strategy: resolve the .platform file at the relative path to get the logical ID,
    // then let the subsequent resolve_logical_ids_in_payload step replace it with the
    // deployed GUID. If we can't find the .platform, fall back to direct resolution map lookup.
    let source_dir = source_item
        .source_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let model_platform_path = source_dir.join(&rel_path).join(".platform");

    let model_logical_id = if model_platform_path.exists() {
        std::fs::read_to_string(&model_platform_path)
            .ok()
            .and_then(|c| serde_json::from_str::<Value>(&c).ok())
            .and_then(|v| {
                v.get("config")
                    .and_then(|c| c.get("logicalId"))
                    .and_then(|l| l.as_str())
                    .map(str::to_owned)
            })
    } else {
        None
    };

    // Determine the ID to put in byConnection:
    // 1. If we found logical ID AND it's in resolution map → use deployed GUID directly
    // 2. If we found logical ID but it's not resolved yet → use logical ID (will be resolved later)
    // 3. Fallback: use the model display name (won't work but gives a clear error)
    let database_id = model_logical_id.as_ref().map_or_else(
        || model_name.to_owned(),
        |lid| {
            resolution_map
                .get(lid)
                .cloned()
                .unwrap_or_else(|| lid.clone())
        },
    );

    // Check schema version to determine output format:
    // v2 PBIR (schema 2.0.0+) uses connectionString only.
    // v1 PBIR uses pbiModelDatabaseName + pbiModelVirtualServerName.
    let is_v2 = pbir
        .get("$schema")
        .and_then(|s| s.as_str())
        .is_some_and(|s| s.contains("/2.") || s.contains("/3.") || s.contains("/4."));

    if is_v2 {
        // v2 format: connectionString with semanticmodelid
        let conn_str = format!(
            "Data Source=pbiazure://api.powerbi.com;initial catalog={model_name};\
             integrated security=ClaimsToken;semanticmodelid={database_id}"
        );
        pbir["datasetReference"] = json!({
            "byConnection": {
                "connectionString": conn_str
            }
        });
    } else {
        // v1 format: all properties required
        pbir["datasetReference"] = json!({
            "byConnection": {
                "connectionString": null,
                "pbiServiceModelId": null,
                "pbiModelVirtualServerName": "sobe_wowvirtualserver",
                "pbiModelDatabaseName": database_id,
                "name": "EntityDataSource",
                "connectionType": "pbiServiceXmlaStyleLive"
            }
        });
    }

    let new_content = serde_json::to_string(&pbir).unwrap_or(content);
    BASE64.encode(new_content.as_bytes())
}

/// Resolve `semanticmodelid` GUIDs in a Report's `definition.pbir` `byConnection` reference.
///
/// When a report already has `byConnection` format (not `byPath`), the connection string
/// may contain `semanticmodelid=<source-workspace-GUID>`. This function:
/// 1. Extracts the semantic model name from `initial catalog=<name>` in the connection string
/// 2. Looks up that name in the deployed semantic model map
/// 3. Replaces the `semanticmodelid` GUID with the target workspace's model ID
/// 4. Also rewrites to v1 `byConnection` format which the Fabric API reliably accepts
///
/// If the payload doesn't contain `byConnection` or resolution fails, returns unchanged.
fn resolve_report_byconnection_model_id(
    payload: &str,
    semantic_model_ids: &HashMap<String, String>,
) -> String {
    if semantic_model_ids.is_empty() {
        return payload.to_owned();
    }

    let Ok(decoded_bytes) = BASE64.decode(payload) else {
        return payload.to_owned();
    };
    let Ok(content) = String::from_utf8(decoded_bytes) else {
        return payload.to_owned();
    };

    let Ok(mut pbir) = serde_json::from_str::<Value>(&content) else {
        return payload.to_owned();
    };

    // Check for byConnection with connectionString containing semanticmodelid
    let conn_str = pbir
        .get("datasetReference")
        .and_then(|dr| dr.get("byConnection"))
        .and_then(|bc| bc.get("connectionString"))
        .and_then(|cs| cs.as_str())
        .map(str::to_owned);

    // Also handle v1 format with pbiModelDatabaseName
    let pbi_db_name = pbir
        .get("datasetReference")
        .and_then(|dr| dr.get("byConnection"))
        .and_then(|bc| bc.get("pbiModelDatabaseName"))
        .and_then(|n| n.as_str())
        .map(str::to_owned);

    if conn_str.is_none() && pbi_db_name.is_none() {
        return payload.to_owned();
    }

    // Try to extract semantic model name from connection string
    let model_name = conn_str.as_deref().and_then(|cs| {
        // Parse "initial catalog=ModelName;" pattern (case-insensitive)
        let lower = cs.to_lowercase();
        let start = lower.find("initial catalog=")?;
        let value_start = start + "initial catalog=".len();
        let remaining = &cs[value_start..];
        let end = remaining.find(';').unwrap_or(remaining.len());
        Some(remaining[..end].to_owned())
    });

    // Look up the deployed ID by semantic model name
    let Some(ref name) = model_name else {
        return payload.to_owned();
    };
    let Some(deployed_id) = semantic_model_ids.get(name.as_str()) else {
        return payload.to_owned();
    };

    // Determine the output format based on the input schema version.
    // v2 PBIR (schema 2.0.0+) uses connectionString only.
    // v1 PBIR uses pbiModelDatabaseName + pbiModelVirtualServerName.
    let is_v2 = pbir
        .get("$schema")
        .and_then(|s| s.as_str())
        .is_some_and(|s| s.contains("/2.") || s.contains("/3.") || s.contains("/4."));

    if is_v2 {
        // v2 format: rewrite the connectionString with the resolved semanticmodelid
        let new_conn_str = format!(
            "Data Source=pbiazure://api.powerbi.com;initial catalog={name};\
             integrated security=ClaimsToken;semanticmodelid={deployed_id}"
        );
        pbir["datasetReference"] = json!({
            "byConnection": {
                "connectionString": new_conn_str
            }
        });
    } else {
        // v1 format: use the classic pbiModelDatabaseName approach with all required properties
        pbir["datasetReference"] = json!({
            "byConnection": {
                "connectionString": null,
                "pbiServiceModelId": null,
                "pbiModelVirtualServerName": "sobe_wowvirtualserver",
                "pbiModelDatabaseName": deployed_id,
                "name": "EntityDataSource",
                "connectionType": "pbiServiceXmlaStyleLive"
            }
        });
    }

    let new_content = serde_json::to_string(&pbir).unwrap_or(content);
    BASE64.encode(new_content.as_bytes())
}

/// Build a map of semantic model name → deployed ID from `created_ids`.
///
/// Used by `resolve_report_byconnection_model_id` to resolve `semanticmodelid`
/// GUIDs in report connection strings by matching the `initial catalog` name.
///
/// Includes both display names AND source directory names as aliases, so that
/// reports referencing a semantic model by its original directory name (before a
/// `.platform` rename) can still be resolved.
fn build_semantic_model_name_map(
    created_ids: &HashMap<(String, String), String>,
) -> HashMap<String, String> {
    created_ids
        .iter()
        .filter(|((item_type, _), _)| item_type.eq_ignore_ascii_case("SemanticModel"))
        .map(|((_, name), id)| (name.clone(), id.clone()))
        .collect()
}

/// Extend the semantic model name map with directory-name aliases from source items.
///
/// When a source directory is named `FCA_Core_SM.SemanticModel` but the `.platform`
/// displayName is `FCA`, the report's `initial catalog` may reference either name.
/// This function adds the directory name (minus the type suffix) as an alias.
fn extend_semantic_model_map_with_dir_aliases(
    map: &mut HashMap<String, String>,
    source: &super::platform::SourceWorkspace,
) {
    for item in &source.items {
        if !item
            .metadata
            .item_type
            .eq_ignore_ascii_case("SemanticModel")
        {
            continue;
        }
        // Extract directory name: e.g., "FCA_Core_SM" from "/path/to/FCA_Core_SM.SemanticModel"
        let dir_name = item
            .source_path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| {
                n.strip_suffix(".SemanticModel")
                    .or_else(|| n.strip_suffix(".Dataset"))
            });

        if let Some(dir_name) = dir_name {
            // Only add if different from the display name (avoid duplicates)
            if !dir_name.eq_ignore_ascii_case(&item.metadata.display_name)
                && !map.contains_key(dir_name)
            {
                // Use the same deployed ID as the display name
                if let Some(id) = map.get(&item.metadata.display_name) {
                    map.insert(dir_name.to_owned(), id.clone());
                }
            }
        }
    }
}

/// Apply logical ID resolution to a JSON value (for creationPayload).
/// Serializes to string, replaces logical IDs, and deserializes back.
fn resolve_logical_ids_in_json(value: &Value, resolution_map: &HashMap<String, String>) -> Value {
    if resolution_map.is_empty() {
        return value.clone();
    }

    let mut content = value.to_string();
    let mut replaced = false;

    for (logical_id, deployed_id) in resolution_map {
        if content.contains(logical_id.as_str()) {
            content = content.replace(logical_id.as_str(), deployed_id.as_str());
            replaced = true;
        }
    }

    if replaced {
        serde_json::from_str(&content).unwrap_or_else(|_| value.clone())
    } else {
        value.clone()
    }
}

/// Check if a string looks like a GUID (36 chars, hex + dashes).
fn is_guid_like(s: &str) -> bool {
    s.len() == 36
        && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && s.chars().filter(|&c| c == '-').count() == 4
}

fn extract_error_code(err: &anyhow::Error) -> String {
    err.downcast_ref::<FabioError>().map_or_else(
        || "UNKNOWN".to_owned(),
        |fabio_err| format!("{:?}", fabio_err.code),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    use super::super::changeset::ChangeAction;
    use super::super::platform::{DefinitionPart, SourceItem};

    #[test]
    fn test_extract_pipeline_references_empty() {
        let item = SourceItem {
            metadata: super::super::platform::PlatformMetadata {
                item_type: "DataPipeline".to_owned(),
                display_name: "Test".to_owned(),
                logical_id: None,
                description: None,
                definition_format: None,
                sensitivity_label_id: None,
                platform_creation_payload: None,
            },
            parts: vec![],
            content_hash: "sha256:abc".to_owned(),
            schedules: None,
            folder_path: String::new(),
            source_path: std::path::PathBuf::from("/tmp"),
            creation_payload: None,
            shortcuts: None,
            governance: None,
        };

        let refs = extract_pipeline_references(&item);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_pipeline_references_finds_execute_pipeline() {
        let pipeline_json = serde_json::json!({
            "properties": {
                "activities": [
                    {
                        "name": "Run Sub Pipeline",
                        "type": "ExecutePipeline",
                        "typeProperties": {
                            "pipeline": {
                                "referenceName": "ChildPipeline"
                            }
                        }
                    },
                    {
                        "name": "Copy Data",
                        "type": "Copy",
                        "typeProperties": {}
                    }
                ]
            }
        });
        let payload = BASE64.encode(serde_json::to_string(&pipeline_json).unwrap().as_bytes());

        let item = SourceItem {
            metadata: super::super::platform::PlatformMetadata {
                item_type: "DataPipeline".to_owned(),
                display_name: "ParentPipeline".to_owned(),
                logical_id: None,
                description: None,
                definition_format: None,
                sensitivity_label_id: None,
                platform_creation_payload: None,
            },
            parts: vec![DefinitionPart {
                path: "pipeline-content.json".to_owned(),
                payload,
                payload_type: "InlineBase64".to_owned(),
            }],
            content_hash: "sha256:abc".to_owned(),
            schedules: None,
            folder_path: String::new(),
            source_path: std::path::PathBuf::from("/tmp"),
            creation_payload: None,
            shortcuts: None,
            governance: None,
        };

        let refs = extract_pipeline_references(&item);
        assert_eq!(refs, vec!["ChildPipeline"]);
    }

    #[test]
    fn test_extract_pipeline_references_multiple() {
        let pipeline_json = serde_json::json!({
            "properties": {
                "activities": [
                    {
                        "name": "Step 1",
                        "type": "ExecutePipeline",
                        "typeProperties": {
                            "pipeline": {"referenceName": "PipeA"}
                        }
                    },
                    {
                        "name": "Step 2",
                        "type": "ExecutePipeline",
                        "typeProperties": {
                            "pipeline": {"referenceName": "PipeB"}
                        }
                    }
                ]
            }
        });
        let payload = BASE64.encode(serde_json::to_string(&pipeline_json).unwrap().as_bytes());

        let item = SourceItem {
            metadata: super::super::platform::PlatformMetadata {
                item_type: "DataPipeline".to_owned(),
                display_name: "Orchestrator".to_owned(),
                logical_id: None,
                description: None,
                definition_format: None,
                sensitivity_label_id: None,
                platform_creation_payload: None,
            },
            parts: vec![DefinitionPart {
                path: "pipeline-content.json".to_owned(),
                payload,
                payload_type: "InlineBase64".to_owned(),
            }],
            content_hash: "sha256:abc".to_owned(),
            schedules: None,
            folder_path: String::new(),
            source_path: std::path::PathBuf::from("/tmp"),
            creation_payload: None,
            shortcuts: None,
            governance: None,
        };

        let refs = extract_pipeline_references(&item);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"PipeA".to_owned()));
        assert!(refs.contains(&"PipeB".to_owned()));
    }

    #[test]
    fn test_deploy_priority_ordering() {
        // Lakehouse should deploy before Notebook, which should deploy before DataPipeline
        let lh = deploy_priority("Lakehouse");
        let nb = deploy_priority("Notebook");
        let dp = deploy_priority("DataPipeline");
        assert!(lh < nb);
        assert!(nb < dp);
    }

    #[test]
    fn test_delete_priority_is_reversed() {
        // Deletes happen in reverse: DataPipeline before Notebook before Lakehouse
        let lh = delete_priority("Lakehouse");
        let nb = delete_priority("Notebook");
        let dp = delete_priority("DataPipeline");
        assert!(dp < nb);
        assert!(nb < lh);
    }

    #[test]
    fn test_order_changes_by_type_priority() {
        let changes = vec![
            Change {
                name: "MyPipeline".to_owned(),
                item_type: "DataPipeline".to_owned(),
                action: ChangeAction::Create,
                reason: "new".to_owned(),
                logical_id: None,
                deployed_id: None,
                source_hash: None,
                previous_name: None,
            },
            Change {
                name: "MyNotebook".to_owned(),
                item_type: "Notebook".to_owned(),
                action: ChangeAction::Create,
                reason: "new".to_owned(),
                logical_id: None,
                deployed_id: None,
                source_hash: None,
                previous_name: None,
            },
            Change {
                name: "MyLH".to_owned(),
                item_type: "Lakehouse".to_owned(),
                action: ChangeAction::Create,
                reason: "new".to_owned(),
                logical_id: None,
                deployed_id: None,
                source_hash: None,
                previous_name: None,
            },
        ];

        // Group by type and sort by deploy_priority
        let mut groups: Vec<(&str, Vec<&Change>)> = Vec::new();
        let mut type_groups: HashMap<&str, Vec<&Change>> = HashMap::new();
        for c in &changes {
            type_groups.entry(c.item_type.as_str()).or_default().push(c);
        }
        let mut sorted_types: Vec<&str> = type_groups.keys().copied().collect();
        sorted_types.sort_by_key(|t| deploy_priority(t));
        for t in sorted_types {
            groups.push((t, type_groups.remove(t).unwrap()));
        }

        assert_eq!(groups[0].0, "Lakehouse");
        assert_eq!(groups[1].0, "Notebook");
        assert_eq!(groups[2].0, "DataPipeline");
    }

    #[test]
    fn test_resolve_logical_ids_no_match() {
        let payload = BASE64.encode(b"some content without any guids");
        let map = HashMap::from([(
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
            "11111111-2222-3333-4444-555555555555".to_owned(),
        )]);

        let result = resolve_logical_ids_in_payload(&payload, &map);
        assert_eq!(result, payload); // unchanged
    }

    #[test]
    fn test_resolve_logical_ids_with_match() {
        let content =
            r#"{"lakehouseId": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "other": "value"}"#;
        let payload = BASE64.encode(content.as_bytes());
        let map = HashMap::from([(
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
            "11111111-2222-3333-4444-555555555555".to_owned(),
        )]);

        let result = resolve_logical_ids_in_payload(&payload, &map);
        let decoded = String::from_utf8(BASE64.decode(&result).unwrap()).unwrap();
        assert!(decoded.contains("11111111-2222-3333-4444-555555555555"));
        assert!(!decoded.contains("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"));
    }

    #[test]
    fn test_resolve_logical_ids_multiple_occurrences() {
        let content =
            "ref1=aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee ref2=aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let payload = BASE64.encode(content.as_bytes());
        let map = HashMap::from([(
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
            "99999999-0000-1111-2222-333333333333".to_owned(),
        )]);

        let result = resolve_logical_ids_in_payload(&payload, &map);
        let decoded = String::from_utf8(BASE64.decode(&result).unwrap()).unwrap();
        // Both occurrences replaced
        assert_eq!(
            decoded
                .matches("99999999-0000-1111-2222-333333333333")
                .count(),
            2
        );
    }

    #[test]
    fn test_resolve_logical_ids_empty_map() {
        let payload = BASE64.encode(b"anything here");
        let map: HashMap<String, String> = HashMap::new();

        let result = resolve_logical_ids_in_payload(&payload, &map);
        assert_eq!(result, payload); // no-op
    }

    #[test]
    fn test_resolve_logical_ids_invalid_base64() {
        let payload = "not-valid-base64!!!";
        let map = HashMap::from([("foo".to_owned(), "bar".to_owned())]);

        let result = resolve_logical_ids_in_payload(payload, &map);
        assert_eq!(result, payload); // returns original
    }

    #[test]
    fn test_build_resolution_map_from_created_ids() {
        use super::super::platform::{PlatformMetadata, SourceWorkspace};

        let source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "Lakehouse".to_owned(),
                    display_name: "SalesLH".to_owned(),
                    logical_id: Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned()),
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: "sha256:abc".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::from([(
                "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
                0,
            )]),
            type_name_index: HashMap::from([(("Lakehouse".to_owned(), "SalesLH".to_owned()), 0)]),
        };

        let created_ids = HashMap::from([(
            ("Lakehouse".to_owned(), "SalesLH".to_owned()),
            "11111111-2222-3333-4444-555555555555".to_owned(),
        )]);

        let map = build_resolution_map(&source, &created_ids);
        assert_eq!(
            map.get("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            Some(&"11111111-2222-3333-4444-555555555555".to_owned())
        );
    }

    #[test]
    fn test_build_resolution_map_no_logical_id() {
        use super::super::platform::{PlatformMetadata, SourceWorkspace};

        let source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "Notebook".to_owned(),
                    display_name: "MyNB".to_owned(),
                    logical_id: None, // no logical ID
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: "sha256:abc".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::from([(("Notebook".to_owned(), "MyNB".to_owned()), 0)]),
        };

        let created_ids = HashMap::from([(
            ("Notebook".to_owned(), "MyNB".to_owned()),
            "22222222-3333-4444-5555-666666666666".to_owned(),
        )]);

        let map = build_resolution_map(&source, &created_ids);
        // No entry — item has no logical_id so it can't be referenced
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_resolution_map_multiple_items() {
        use super::super::platform::{PlatformMetadata, SourceWorkspace};

        let source = SourceWorkspace {
            items: vec![
                SourceItem {
                    metadata: PlatformMetadata {
                        item_type: "Lakehouse".to_owned(),
                        display_name: "LH1".to_owned(),
                        logical_id: Some("lid-lh1".to_owned()),
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![],
                    content_hash: "sha256:abc".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
                SourceItem {
                    metadata: PlatformMetadata {
                        item_type: "Lakehouse".to_owned(),
                        display_name: "LH2".to_owned(),
                        logical_id: Some("lid-lh2".to_owned()),
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![],
                    content_hash: "sha256:def".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
                SourceItem {
                    metadata: PlatformMetadata {
                        item_type: "Notebook".to_owned(),
                        display_name: "NB1".to_owned(),
                        logical_id: Some("lid-nb1".to_owned()),
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![],
                    content_hash: "sha256:ghi".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
            ],
            logical_id_index: HashMap::from([
                ("lid-lh1".to_owned(), 0),
                ("lid-lh2".to_owned(), 1),
                ("lid-nb1".to_owned(), 2),
            ]),
            type_name_index: HashMap::from([
                (("Lakehouse".to_owned(), "LH1".to_owned()), 0),
                (("Lakehouse".to_owned(), "LH2".to_owned()), 1),
                (("Notebook".to_owned(), "NB1".to_owned()), 2),
            ]),
        };

        let created_ids = HashMap::from([
            (
                ("Lakehouse".to_owned(), "LH1".to_owned()),
                "deployed-id-lh1".to_owned(),
            ),
            (
                ("Notebook".to_owned(), "NB1".to_owned()),
                "deployed-id-nb1".to_owned(),
            ),
        ]);

        let map = build_resolution_map(&source, &created_ids);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("lid-lh1"), Some(&"deployed-id-lh1".to_owned()));
        assert_eq!(map.get("lid-nb1"), Some(&"deployed-id-nb1".to_owned()));
        // LH2 not in created_ids → not in resolution map
        assert!(!map.contains_key("lid-lh2"));
    }

    #[test]
    fn test_resolve_logical_ids_multiple_different_ids() {
        let content = r#"{"lh": "lid-aaa", "nb": "lid-bbb", "other": "no-match"}"#;
        let payload = BASE64.encode(content.as_bytes());
        let map = HashMap::from([
            ("lid-aaa".to_owned(), "deployed-aaa".to_owned()),
            ("lid-bbb".to_owned(), "deployed-bbb".to_owned()),
        ]);

        let result = resolve_logical_ids_in_payload(&payload, &map);
        let decoded = String::from_utf8(BASE64.decode(&result).unwrap()).unwrap();
        assert!(decoded.contains("deployed-aaa"));
        assert!(decoded.contains("deployed-bbb"));
        assert!(!decoded.contains("lid-aaa"));
        assert!(!decoded.contains("lid-bbb"));
        assert!(decoded.contains("no-match")); // untouched
    }

    #[test]
    fn test_resolve_logical_ids_non_utf8_payload() {
        // Binary payload that is not valid UTF-8
        let binary = vec![0xFF, 0xFE, 0x00, 0x01, 0x80, 0x90];
        let payload = BASE64.encode(&binary);
        let map = HashMap::from([("whatever".to_owned(), "replaced".to_owned())]);

        let result = resolve_logical_ids_in_payload(&payload, &map);
        assert_eq!(result, payload); // returned unchanged
    }

    #[test]
    fn test_resolve_logical_ids_partial_match_substring() {
        // Ensure that a logical ID that is a substring of another doesn't cause issues
        let content = r#"{"id1": "abc-123", "id2": "abc-123-extended"}"#;
        let payload = BASE64.encode(content.as_bytes());
        let map = HashMap::from([("abc-123".to_owned(), "REPLACED".to_owned())]);

        let result = resolve_logical_ids_in_payload(&payload, &map);
        let decoded = String::from_utf8(BASE64.decode(&result).unwrap()).unwrap();
        // Both occurrences of "abc-123" get replaced (including substring within longer string)
        assert!(decoded.contains("REPLACED"));
        assert!(!decoded.contains("abc-123\""));
    }

    #[test]
    fn test_build_resolution_map_ignores_items_not_in_source() {
        use super::super::platform::{PlatformMetadata, SourceWorkspace};

        let source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "Lakehouse".to_owned(),
                    display_name: "LH1".to_owned(),
                    logical_id: Some("lid-lh1".to_owned()),
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: "sha256:abc".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::from([("lid-lh1".to_owned(), 0)]),
            type_name_index: HashMap::from([(("Lakehouse".to_owned(), "LH1".to_owned()), 0)]),
        };

        // created_ids has an item that doesn't exist in source type_name_index
        let created_ids = HashMap::from([
            (
                ("Lakehouse".to_owned(), "LH1".to_owned()),
                "deployed-lh1".to_owned(),
            ),
            (
                ("Notebook".to_owned(), "Ghost".to_owned()),
                "deployed-ghost".to_owned(),
            ),
        ]);

        let map = build_resolution_map(&source, &created_ids);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("lid-lh1"), Some(&"deployed-lh1".to_owned()));
    }

    #[test]
    fn test_extract_error_code_fabio_error() {
        use crate::errors::{ErrorCode, FabioError};
        let err = FabioError {
            code: ErrorCode::NotFound,
            message: "Not found".to_owned(),
            hint: None,
            hint_type: None,
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
        };
        let anyhow_err: anyhow::Error = err.into();
        let code = extract_error_code(&anyhow_err);
        assert_eq!(code, "NotFound");
    }

    #[test]
    fn test_extract_error_code_unknown_error() {
        let err = anyhow::anyhow!("some random error");
        let code = extract_error_code(&err);
        assert_eq!(code, "UNKNOWN");
    }

    // ─── report byConnection semanticmodelid resolution tests ────────────────

    #[test]
    fn test_build_semantic_model_name_map_extracts_only_semantic_models() {
        let created_ids = HashMap::from([
            (
                ("SemanticModel".to_owned(), "Sales_SM".to_owned()),
                "deployed-sm-1".to_owned(),
            ),
            (
                ("SemanticModel".to_owned(), "HR_SM".to_owned()),
                "deployed-sm-2".to_owned(),
            ),
            (
                ("Notebook".to_owned(), "ETL".to_owned()),
                "deployed-nb-1".to_owned(),
            ),
            (
                ("Report".to_owned(), "Sales_Report".to_owned()),
                "deployed-rpt-1".to_owned(),
            ),
        ]);

        let map = build_semantic_model_name_map(&created_ids);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("Sales_SM"), Some(&"deployed-sm-1".to_owned()));
        assert_eq!(map.get("HR_SM"), Some(&"deployed-sm-2".to_owned()));
        assert!(!map.contains_key("ETL"));
        assert!(!map.contains_key("Sales_Report"));
    }

    #[test]
    fn test_build_semantic_model_name_map_empty() {
        let created_ids = HashMap::new();
        let map = build_semantic_model_name_map(&created_ids);
        assert!(map.is_empty());
    }

    #[test]
    fn test_resolve_report_byconnection_resolves_v2_connection_string() {
        use base64::Engine as _;

        let pbir = serde_json::json!({
            "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json",
            "version": "4.0",
            "datasetReference": {
                "byConnection": {
                    "connectionString": "Data Source=powerbi://api.powerbi.com/v1.0/myorg/OldWorkspace;initial catalog=FUAM_Core_SM;integrated security=ClaimsToken;semanticmodelid=old-guid-1234"
                }
            }
        });
        let payload = BASE64.encode(serde_json::to_string(&pbir).unwrap().as_bytes());

        let sm_ids = HashMap::from([(
            "FUAM_Core_SM".to_owned(),
            "new-deployed-guid-5678".to_owned(),
        )]);

        let result = resolve_report_byconnection_model_id(&payload, &sm_ids);
        let decoded = String::from_utf8(BASE64.decode(&result).unwrap()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&decoded).unwrap();

        let conn_str = parsed["datasetReference"]["byConnection"]["connectionString"]
            .as_str()
            .unwrap();
        assert!(conn_str.contains("semanticmodelid=new-deployed-guid-5678"));
        assert!(conn_str.contains("initial catalog=FUAM_Core_SM"));
        assert!(!conn_str.contains("old-guid-1234"));
    }

    #[test]
    fn test_resolve_report_byconnection_no_match_returns_unchanged() {
        use base64::Engine as _;

        let pbir = serde_json::json!({
            "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json",
            "datasetReference": {
                "byConnection": {
                    "connectionString": "Data Source=x;initial catalog=Unknown_SM;semanticmodelid=abc"
                }
            }
        });
        let payload = BASE64.encode(serde_json::to_string(&pbir).unwrap().as_bytes());

        // Map doesn't contain "Unknown_SM"
        let sm_ids = HashMap::from([("Other_SM".to_owned(), "other-id".to_owned())]);

        let result = resolve_report_byconnection_model_id(&payload, &sm_ids);
        // Should return unchanged since model name not found
        assert_eq!(result, payload);
    }

    #[test]
    fn test_resolve_report_byconnection_empty_map_returns_unchanged() {
        use base64::Engine as _;

        let pbir = serde_json::json!({
            "datasetReference": {
                "byConnection": {
                    "connectionString": "initial catalog=X;semanticmodelid=abc"
                }
            }
        });
        let payload = BASE64.encode(serde_json::to_string(&pbir).unwrap().as_bytes());

        let result = resolve_report_byconnection_model_id(&payload, &HashMap::new());
        assert_eq!(result, payload);
    }

    #[test]
    fn test_resolve_report_byconnection_no_byconnection_returns_unchanged() {
        use base64::Engine as _;

        let pbir = serde_json::json!({
            "datasetReference": {
                "byPath": { "path": "../Sales.SemanticModel" }
            }
        });
        let payload = BASE64.encode(serde_json::to_string(&pbir).unwrap().as_bytes());

        let sm_ids = HashMap::from([("Sales".to_owned(), "sm-id".to_owned())]);
        let result = resolve_report_byconnection_model_id(&payload, &sm_ids);
        assert_eq!(result, payload);
    }

    // ─── extract_pipeline_activity_guid_map tests ────────────────────────────

    #[test]
    fn test_extract_pipeline_activity_guid_map_notebook_ref() {
        use super::super::platform::{DefinitionPart, PlatformMetadata, SourceItem};

        let pipeline_json = serde_json::json!({
            "properties": {
                "activities": [
                    {
                        "name": "Run_ETL_Notebook",
                        "type": "TridentNotebook",
                        "typeProperties": {
                            "notebookId": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                            "workspaceId": "11111111-2222-3333-4444-555555555555"
                        }
                    }
                ]
            }
        });
        let encoded = BASE64.encode(serde_json::to_string(&pipeline_json).unwrap().as_bytes());

        let item = SourceItem {
            metadata: PlatformMetadata {
                item_type: "DataPipeline".to_owned(),
                display_name: "TestPipeline".to_owned(),
                logical_id: None,
                description: None,
                definition_format: None,
                sensitivity_label_id: None,
                platform_creation_payload: None,
            },
            parts: vec![DefinitionPart {
                path: "pipeline-content.json".to_owned(),
                payload: encoded,
                payload_type: "InlineBase64".to_owned(),
            }],
            content_hash: String::new(),
            schedules: None,
            folder_path: String::new(),
            source_path: std::path::PathBuf::new(),
            creation_payload: None,
            shortcuts: None,
            governance: None,
        };

        let pairs = extract_pipeline_activity_guid_map(&item);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert_eq!(pairs[0].1, "Run_ETL_Notebook");
    }
}
