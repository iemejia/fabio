use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::output;

use super::{detect_format_from_extension, expand_table_glob, render_batch_result};

// ─── Load / Upload Table ─────────────────────────────────────────────────────

/// Map the Fabric "operation not supported for schemas-enabled lakehouse" error
/// (raised when no schema is supplied to the load API) into a teaching hint that
/// points the caller at `--schema`. Other errors pass through unchanged.
fn map_schemas_enabled_error(e: anyhow::Error, schema: Option<&str>) -> anyhow::Error {
    if schema.is_none() && super::files::is_schemas_enabled_error(&e) {
        return crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::InvalidInput,
            "This lakehouse has schemas enabled, so the load-table API requires a schema."
                .to_string(),
            "Pass --schema <name> (for example --schema dbo). Inspect schemas/tables with: fabio lakehouse list-tables --workspace <WS> --id <ID>."
                .to_string(),
        )
        .into();
    }
    e
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn load_table(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    source_path: &str,
    table: &str,
    mode: &str,
    format: &str,
    header: bool,
    delimiter: &str,
    schema: Option<&str>,
) -> Result<()> {
    const VALID_MODES: &[&str] = &["Overwrite", "Append"];
    const VALID_FORMATS: &[&str] = &["Csv", "Parquet"];

    // Case-insensitive normalization: accept "overwrite", "csv", etc.
    let mode = VALID_MODES
        .iter()
        .find(|v| v.eq_ignore_ascii_case(mode))
        .copied()
        .unwrap_or(mode);
    let format = VALID_FORMATS
        .iter()
        .find(|v| v.eq_ignore_ascii_case(format))
        .copied()
        .unwrap_or(format);

    if !VALID_MODES.contains(&mode) {
        return Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::InvalidInput,
            format!("Invalid load mode: '{mode}'"),
            format!(
                "--mode must be one of: {} (got: '{mode}')",
                VALID_MODES.join(", ")
            ),
        )
        .into());
    }
    if !VALID_FORMATS.contains(&format) {
        let (hint, hint_type) = if format.eq_ignore_ascii_case("json") {
            (
                "JSON format is not supported by the Fabric load-table API. Convert to CSV or Parquet first.".to_string(),
                crate::errors::HintType::SemanticCorrection,
            )
        } else {
            (
                format!(
                    "--format must be one of: {} (got: '{format}')",
                    VALID_FORMATS.join(", ")
                ),
                crate::errors::HintType::SyntaxFix,
            )
        };
        return Err(crate::errors::FabioError::with_typed_hint(
            crate::errors::ErrorCode::InvalidInput,
            format!("Invalid format: '{format}'"),
            hint,
            hint_type,
        )
        .into());
    }

    if output::dry_run_guard(
        cli,
        "lakehouse load-table",
        &serde_json::json!({
            "workspace": workspace,
            "lakehouse": id,
            "source_path": source_path,
            "table": table,
            "mode": mode,
            "format": format
        }),
    ) {
        return Ok(());
    }

    let format_options = match format {
        "Csv" => serde_json::json!({
            "format": format,
            "header": header,
            "delimiter": delimiter
        }),
        _ => serde_json::json!({
            "format": format
        }),
    };

    let body = serde_json::json!({
        "relativePath": source_path,
        "pathType": "File",
        "mode": mode,
        "formatOptions": format_options
    });

    let url = schema.map_or_else(
        || format!("/workspaces/{workspace}/lakehouses/{id}/tables/{table}/load"),
        |schema_name| {
            format!(
                "/workspaces/{workspace}/lakehouses/{id}/schemas/{schema_name}/tables/{table}/load?beta=true"
            )
        },
    );

    let data = client
        .post(&url, &body, true)
        .await
        .map_err(|e| map_schemas_enabled_error(e, schema))?;

    let obj = if data.is_null() {
        serde_json::json!({
            "table": table,
            "source": source_path,
            "mode": mode,
            "status": "loaded"
        })
    } else {
        data
    };

    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Upload a local file to the lakehouse and load it into a Delta table in one step.
/// Auto-detects format from file extension if `--format` is not provided.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn upload_table(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    source_path: &str,
    table: &str,
    mode: &str,
    format: Option<&str>,
    schema: Option<&str>,
) -> Result<()> {
    const VALID_MODES: &[&str] = &["Overwrite", "Append"];
    const VALID_FORMATS: &[&str] = &["Csv", "Parquet"];

    // Case-insensitive normalization: accept "overwrite", "csv", etc.
    let mode = VALID_MODES
        .iter()
        .find(|v| v.eq_ignore_ascii_case(mode))
        .copied()
        .unwrap_or(mode);

    if !VALID_MODES.contains(&mode) {
        return Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::InvalidInput,
            format!("Invalid load mode: '{mode}'"),
            format!(
                "--mode must be one of: {} (got: '{mode}')",
                VALID_MODES.join(", ")
            ),
        )
        .into());
    }

    // Auto-detect format from file extension if not explicitly provided
    let detected_format = match format {
        Some(f) => {
            // Case-insensitive normalization for explicit format
            VALID_FORMATS
                .iter()
                .find(|v| v.eq_ignore_ascii_case(f))
                .map_or_else(|| f.to_string(), |v| (*v).to_string())
        }
        None => detect_format_from_extension(source_path)?,
    };

    if !VALID_FORMATS.contains(&detected_format.as_str()) {
        return Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::InvalidInput,
            format!("Invalid format: '{detected_format}'"),
            format!(
                "--format must be one of: {} (got: '{detected_format}')",
                VALID_FORMATS.join(", ")
            ),
        )
        .into());
    }

    // Derive a staging path in the lakehouse Files area
    let filename = Path::new(source_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let staging_path = format!("Files/.staging/{filename}");

    if output::dry_run_guard(
        cli,
        "lakehouse upload-table",
        &serde_json::json!({
            "workspace": workspace,
            "lakehouse": id,
            "source_path": source_path,
            "staging_path": staging_path,
            "table": table,
            "mode": mode,
            "format": detected_format
        }),
    ) {
        return Ok(());
    }

    // Step 1: Upload the local file to Files/.staging/<filename>
    let data = std::fs::read(source_path).map_err(|e| {
        crate::errors::FabioError::invalid_input(format!("Cannot read file {source_path}: {e}"))
    })?;

    eprintln!("  Uploading {source_path} to {staging_path}...");
    client
        .upload_onelake_file(workspace, id, &staging_path, data)
        .await?;

    // Step 2: Load the uploaded file into the Delta table
    eprintln!("  Loading into table '{table}' (mode={mode}, format={detected_format})...");
    let format_options = match detected_format.as_str() {
        "Csv" => serde_json::json!({
            "format": detected_format,
            "header": true,
            "delimiter": ","
        }),
        _ => serde_json::json!({
            "format": detected_format
        }),
    };
    let body = serde_json::json!({
        "relativePath": staging_path,
        "pathType": "File",
        "mode": mode,
        "formatOptions": format_options
    });

    let load_result = client
        .post(
            &schema.map_or_else(
                || format!("/workspaces/{workspace}/lakehouses/{id}/tables/{table}/load"),
                |schema_name| {
                    format!(
                        "/workspaces/{workspace}/lakehouses/{id}/schemas/{schema_name}/tables/{table}/load?beta=true"
                    )
                },
            ),
            &body,
            true,
        )
        .await
        .map_err(|e| map_schemas_enabled_error(e, schema));

    // Step 3: Clean up the staging file (best-effort)
    let _ = client
        .delete_onelake_file(workspace, id, &staging_path)
        .await;

    // Handle the load result
    load_result?;

    let obj = serde_json::json!({
        "table": table,
        "source": source_path,
        "mode": mode,
        "format": detected_format,
        "status": "loaded"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Delete / Copy / Move Table ──────────────────────────────────────────────

pub(super) async fn delete_table(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
) -> Result<()> {
    use crate::parallel::{self, BatchSummary};

    let tables = expand_table_glob(client, workspace, id, table).await?;

    // Destructive: guard before any delete. `expand_table_glob` above is a
    // read-only list, so the dry-run can show exactly which tables would be
    // deleted.
    if output::dry_run_guard(
        cli,
        "lakehouse delete-table",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "tables": tables,
            "count": tables.len(),
        }),
    ) {
        return Ok(());
    }

    if tables.len() == 1 {
        let path = format!("Tables/{}", tables[0]);
        client
            .delete_onelake_directory(workspace, id, &path)
            .await?;
        let obj = serde_json::json!({
            "table": tables[0],
            "status": "deleted"
        });
        output::render_object(cli, &obj, "status");
        return Ok(());
    }

    // Multiple tables matched — delete in parallel
    let concurrency = parallel::default_concurrency();
    eprintln!(
        "  Deleting {} tables with concurrency={concurrency}...",
        tables.len()
    );

    let item_names = tables.clone();
    let workspace: Arc<str> = Arc::from(workspace);
    let id: Arc<str> = Arc::from(id);
    let client = client.clone();

    let results = parallel::execute_parallel(tables, concurrency, move |tbl| {
        let client = client.clone();
        let workspace = Arc::clone(&workspace);
        let id = Arc::clone(&id);
        async move {
            let path = format!("Tables/{tbl}");
            client
                .delete_onelake_directory(&workspace, &id, &path)
                .await?;
            Ok(())
        }
    })
    .await;

    let summary = BatchSummary::from_results(&results, &item_names);
    render_batch_result(cli, &summary, "deleted")
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn copy_table(
    cli: &Cli,
    client: &FabricClient,
    src_ws: &str,
    src_id: &str,
    src_table: &str,
    dst_ws: &str,
    dst_id: &str,
    dst_table: Option<&str>,
) -> Result<()> {
    let tables = expand_table_glob(client, src_ws, src_id, src_table).await?;

    // A copy can overwrite the destination table — guard after the read-only
    // glob expansion so the dry-run plan lists which tables would be copied.
    if output::dry_run_guard(
        cli,
        "lakehouse copy-table",
        &serde_json::json!({
            "sourceWorkspace": src_ws,
            "sourceId": src_id,
            "sourceTables": tables,
            "destWorkspace": dst_ws,
            "destId": dst_id,
            "destTable": dst_table,
        }),
    ) {
        return Ok(());
    }

    if tables.len() > 1 {
        use crate::parallel::{self, BatchSummary};

        // Multiple tables — list files once and copy all in parallel
        let concurrency = parallel::default_concurrency();
        eprintln!(
            "  Copying {} tables with concurrency={concurrency}...",
            tables.len()
        );

        // Single root listing shared across all table copies
        let files = client.list_onelake_files(src_ws, src_id, None).await?;

        // Build all copy tasks across all tables
        let mut copy_tasks: Vec<(String, String)> = Vec::new();
        for tbl in &tables {
            let prefix = format!("{src_id}/Tables/{tbl}/");
            for file in &files {
                if let Some(name) = file.get("name").and_then(Value::as_str) {
                    let is_dir = file
                        .get("isDirectory")
                        .and_then(Value::as_str)
                        .unwrap_or("false")
                        == "true";
                    if is_dir {
                        continue;
                    }
                    if let Some(relative) = name.strip_prefix(&prefix) {
                        let src_path = format!("Tables/{tbl}/{relative}");
                        let dst_path = format!("Tables/{tbl}/{relative}");
                        copy_tasks.push((src_path, dst_path));
                    }
                }
            }
        }

        if copy_tasks.is_empty() {
            let obj = serde_json::json!({
                "tablesCopied": tables.len(),
                "filesCopied": 0,
                "status": "copied"
            });
            output::render_object(cli, &obj, "status");
            return Ok(());
        }

        let item_names: Vec<String> = copy_tasks.iter().map(|(s, _)| s.clone()).collect();
        let src_ws: Arc<str> = Arc::from(src_ws);
        let src_id: Arc<str> = Arc::from(src_id);
        let dst_ws: Arc<str> = Arc::from(dst_ws);
        let dst_id: Arc<str> = Arc::from(dst_id);
        let client = client.clone();

        let results =
            parallel::execute_parallel(copy_tasks, concurrency, move |(src_path, dst_path)| {
                let client = client.clone();
                let src_ws = Arc::clone(&src_ws);
                let src_id = Arc::clone(&src_id);
                let dst_ws = Arc::clone(&dst_ws);
                let dst_id = Arc::clone(&dst_id);
                async move {
                    client
                        .copy_onelake_file(&src_ws, &src_id, &src_path, &dst_ws, &dst_id, &dst_path)
                        .await?;
                    Ok(())
                }
            })
            .await;

        let summary = BatchSummary::from_results(&results, &item_names);
        return render_batch_result(cli, &summary, "copied");
    }

    let table_name = &tables[0];
    let dest_name = dst_table.unwrap_or(table_name);
    copy_single_table(
        cli, client, src_ws, src_id, table_name, dst_ws, dst_id, dest_name, true,
    )
    .await
}

/// Copy a single table's files in parallel.
/// When `render` is true, outputs result to stdout. When false, stays silent (for `move_table`).
#[allow(clippy::too_many_arguments)]
pub(super) async fn copy_single_table(
    cli: &Cli,
    client: &FabricClient,
    src_ws: &str,
    src_id: &str,
    src_table: &str,
    dst_ws: &str,
    dst_id: &str,
    dest_name: &str,
    render: bool,
) -> Result<()> {
    use crate::parallel::{self, BatchSummary};

    let concurrency = parallel::default_concurrency();

    // List all files from root (no directory param) and filter for this table
    let files = client.list_onelake_files(src_ws, src_id, None).await?;
    let prefix = format!("{src_id}/Tables/{src_table}/");

    // Collect file copy tasks
    let mut copy_tasks: Vec<(String, String)> = Vec::new();
    for file in &files {
        if let Some(name) = file.get("name").and_then(Value::as_str) {
            let is_dir = file
                .get("isDirectory")
                .and_then(Value::as_str)
                .unwrap_or("false")
                == "true";
            if is_dir {
                continue;
            }
            if let Some(relative) = name.strip_prefix(&prefix) {
                let src_path = format!("Tables/{src_table}/{relative}");
                let dst_path = format!("Tables/{dest_name}/{relative}");
                copy_tasks.push((src_path, dst_path));
            }
        }
    }

    let total_files = copy_tasks.len();
    if total_files == 0 {
        if render {
            let obj = serde_json::json!({
                "sourceTable": src_table,
                "destTable": dest_name,
                "filesCopied": 0,
                "status": "copied"
            });
            output::render_object(cli, &obj, "status");
        }
        return Ok(());
    }

    eprintln!(
        "  Copying {total_files} files for table '{src_table}' with concurrency={concurrency}..."
    );

    let item_names: Vec<String> = copy_tasks.iter().map(|(s, _)| s.clone()).collect();

    let src_ws: Arc<str> = Arc::from(src_ws);
    let src_id: Arc<str> = Arc::from(src_id);
    let dst_ws: Arc<str> = Arc::from(dst_ws);
    let dst_id: Arc<str> = Arc::from(dst_id);
    let client = client.clone();

    let results =
        parallel::execute_parallel(copy_tasks, concurrency, move |(src_path, dst_path)| {
            let client = client.clone();
            let src_ws = Arc::clone(&src_ws);
            let src_id = Arc::clone(&src_id);
            let dst_ws = Arc::clone(&dst_ws);
            let dst_id = Arc::clone(&dst_id);
            async move {
                client
                    .copy_onelake_file(&src_ws, &src_id, &src_path, &dst_ws, &dst_id, &dst_path)
                    .await?;
                Ok(())
            }
        })
        .await;

    let summary = BatchSummary::from_results(&results, &item_names);

    if summary.all_succeeded() {
        if render {
            let obj = serde_json::json!({
                "sourceTable": src_table,
                "destTable": dest_name,
                "filesCopied": summary.succeeded,
                "status": "copied"
            });
            output::render_object(cli, &obj, "status");
        }
        Ok(())
    } else {
        if render {
            let obj = serde_json::json!({
                "sourceTable": src_table,
                "destTable": dest_name,
                "filesCopied": summary.succeeded,
                "filesFailed": summary.failed,
                "failures": summary.failures,
                "status": "partial_failure"
            });
            output::render_object(cli, &obj, "status");
        }
        Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::ApiError,
            format!(
                "Table copy partially failed: {}/{} files copied",
                summary.succeeded, summary.total
            ),
            "Retry the copy operation to process remaining files. Some files may be temporarily locked by active Spark sessions.",
        )
        .into())
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn move_table(
    cli: &Cli,
    client: &FabricClient,
    src_ws: &str,
    src_id: &str,
    src_table: &str,
    dst_ws: &str,
    dst_id: &str,
    dst_table: Option<&str>,
) -> Result<()> {
    let tables = expand_table_glob(client, src_ws, src_id, src_table).await?;

    // Destructive (removes the source table): guard after the read-only glob
    // expansion.
    if output::dry_run_guard(
        cli,
        "lakehouse move-table",
        &serde_json::json!({
            "sourceWorkspace": src_ws,
            "sourceId": src_id,
            "sourceTables": tables,
            "destWorkspace": dst_ws,
            "destId": dst_id,
            "destTable": dst_table,
        }),
    ) {
        return Ok(());
    }

    if tables.len() > 1 {
        use crate::parallel::{self, BatchSummary};

        let is_same_item = src_ws == dst_ws && src_id == dst_id;

        // Multiple tables — if same item, try atomic directory rename per table
        if is_same_item {
            let concurrency = parallel::default_concurrency();
            eprintln!(
                "  Moving {} tables via rename with concurrency={concurrency}...",
                tables.len()
            );

            let item_names = tables.clone();
            let ws: Arc<str> = Arc::from(src_ws);
            let id: Arc<str> = Arc::from(src_id);
            let client_c = client.clone();

            let results = parallel::execute_parallel(tables, concurrency, move |tbl| {
                let client = client_c.clone();
                let ws = Arc::clone(&ws);
                let id = Arc::clone(&id);
                async move {
                    let src_dir = format!("Tables/{tbl}");
                    let dst_dir = format!("Tables/{tbl}");
                    // rename_onelake_file works for directories too
                    match client
                        .rename_onelake_file(&ws, &id, &src_dir, &dst_dir)
                        .await?
                    {
                        Some(_) => Ok(()),
                        None => {
                            // Fallback: should not happen for same-item, but handle gracefully
                            Err(anyhow::anyhow!("rename failed for table {tbl}"))
                        }
                    }
                }
            })
            .await;

            let summary = BatchSummary::from_results(&results, &item_names);
            return render_batch_result(cli, &summary, "moved");
        }

        // Cross-item: copy all files in parallel, then delete sources in parallel
        let concurrency = parallel::default_concurrency();
        eprintln!(
            "  Moving {} tables with concurrency={concurrency}...",
            tables.len()
        );

        // Single root listing shared across all table copies
        let files = client.list_onelake_files(src_ws, src_id, None).await?;

        // Build all copy tasks across all tables
        let mut copy_tasks: Vec<(String, String)> = Vec::new();
        for tbl in &tables {
            let prefix = format!("{src_id}/Tables/{tbl}/");
            for file in &files {
                if let Some(name) = file.get("name").and_then(Value::as_str) {
                    let is_dir = file
                        .get("isDirectory")
                        .and_then(Value::as_str)
                        .unwrap_or("false")
                        == "true";
                    if is_dir {
                        continue;
                    }
                    if let Some(relative) = name.strip_prefix(&prefix) {
                        let src_path = format!("Tables/{tbl}/{relative}");
                        let dst_path = format!("Tables/{tbl}/{relative}");
                        copy_tasks.push((src_path, dst_path));
                    }
                }
            }
        }

        // Phase 1: Copy all files in parallel
        if !copy_tasks.is_empty() {
            let item_names: Vec<String> = copy_tasks.iter().map(|(s, _)| s.clone()).collect();
            let src_ws_c: Arc<str> = Arc::from(src_ws);
            let src_id_c: Arc<str> = Arc::from(src_id);
            let dst_ws_c: Arc<str> = Arc::from(dst_ws);
            let dst_id_c: Arc<str> = Arc::from(dst_id);
            let client_c = client.clone();

            let results =
                parallel::execute_parallel(copy_tasks, concurrency, move |(src_path, dst_path)| {
                    let client = client_c.clone();
                    let src_ws = Arc::clone(&src_ws_c);
                    let src_id = Arc::clone(&src_id_c);
                    let dst_ws = Arc::clone(&dst_ws_c);
                    let dst_id = Arc::clone(&dst_id_c);
                    async move {
                        client
                            .copy_onelake_file(
                                &src_ws, &src_id, &src_path, &dst_ws, &dst_id, &dst_path,
                            )
                            .await?;
                        Ok(())
                    }
                })
                .await;

            let summary = BatchSummary::from_results(&results, &item_names);
            if !summary.all_succeeded() {
                let obj = serde_json::json!({
                    "filesCopied": summary.succeeded,
                    "filesFailed": summary.failed,
                    "failures": summary.failures,
                    "status": "partial_failure"
                });
                output::render_object(cli, &obj, "status");
                return Err(crate::errors::FabioError::with_hint(
                    crate::errors::ErrorCode::ApiError,
                    format!(
                        "Move aborted: copy phase partially failed ({}/{} files copied). Source tables not deleted.",
                        summary.succeeded, summary.total
                    ),
                    "Retry the move operation. The source table is intact (no data was deleted).",
                )
                .into());
            }
        }

        // Phase 2: Delete all source tables in parallel (only after ALL copies succeeded)
        let del_item_names = tables.clone();
        let src_ws_d: Arc<str> = Arc::from(src_ws);
        let src_id_d: Arc<str> = Arc::from(src_id);
        let client_d = client.clone();

        let del_results = parallel::execute_parallel(tables, concurrency, move |tbl| {
            let client = client_d.clone();
            let src_ws = Arc::clone(&src_ws_d);
            let src_id = Arc::clone(&src_id_d);
            async move {
                let path = format!("Tables/{tbl}");
                client
                    .delete_onelake_directory(&src_ws, &src_id, &path)
                    .await?;
                Ok(())
            }
        })
        .await;

        let del_summary = BatchSummary::from_results(&del_results, &del_item_names);
        return render_batch_result(cli, &del_summary, "moved");
    }

    let table_name = &tables[0];
    let dest_name = dst_table.unwrap_or(table_name);

    let is_same_item = src_ws == dst_ws && src_id == dst_id;

    if is_same_item {
        // Same item: try atomic directory rename (handles all files at once)
        let src_dir = format!("Tables/{table_name}");
        let dst_dir = format!("Tables/{dest_name}");
        if let Some(_result) = client
            .rename_onelake_file(src_ws, src_id, &src_dir, &dst_dir)
            .await?
        {
            let obj = serde_json::json!({
                "sourceTable": table_name,
                "destTable": dest_name,
                "status": "moved",
                "method": "rename"
            });
            output::render_object(cli, &obj, "status");
            return Ok(());
        }
        // Fallback: per-file copy + directory delete
    }

    // Copy table (parallel) — errors will propagate if any file fails
    copy_single_table(
        cli, client, src_ws, src_id, table_name, dst_ws, dst_id, dest_name, false,
    )
    .await?;

    // Delete source table only after ALL copies succeeded
    let path = format!("Tables/{table_name}");
    client
        .delete_onelake_directory(src_ws, src_id, &path)
        .await?;

    let obj = serde_json::json!({
        "sourceTable": table_name,
        "destTable": dest_name,
        "status": "moved",
        "method": "copy_delete"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::map_schemas_enabled_error;

    #[test]
    fn schemas_enabled_error_becomes_schema_hint_when_no_schema() {
        let e = anyhow::anyhow!(
            "UnsupportedOperationForSchemasEnabledLakehouse: The operation is not supported for Lakehouse with schemas enabled."
        );
        let mapped = map_schemas_enabled_error(e, None);
        let s = mapped.to_string();
        assert!(s.contains("schemas enabled"), "got: {s}");
        // The teaching hint lives on the FabioError; verify it surfaces --schema.
        let fe = mapped
            .downcast_ref::<crate::errors::FabioError>()
            .expect("FabioError");
        assert!(fe.hint.as_deref().unwrap_or("").contains("--schema"));
    }

    #[test]
    fn schemas_enabled_error_passes_through_when_schema_supplied() {
        let e = anyhow::anyhow!("UnsupportedOperationForSchemasEnabledLakehouse: not supported");
        // With a schema already supplied, we must not rewrite the error.
        let mapped = map_schemas_enabled_error(e, Some("dbo"));
        assert!(mapped.downcast_ref::<crate::errors::FabioError>().is_none());
    }

    #[test]
    fn unrelated_error_passes_through() {
        let e = anyhow::anyhow!("ItemNotFound: nope");
        let mapped = map_schemas_enabled_error(e, None);
        assert!(mapped.to_string().contains("ItemNotFound"));
    }
}
