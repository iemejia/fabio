use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::output;

use super::{matches_filters, parse_filter_patterns, parse_size_value};

/// File metadata extracted from DFS listing.
struct FileInfo {
    size: u64,
    etag: String,
}

/// Sync files between source and destination paths in `OneLake`.
/// Source can be another `OneLake` lakehouse or a local directory (`--local`).
/// By default, compares files using `ETag` (remote-to-remote) or size (local-to-remote).
/// With `--checksum`, uses `Content-MD5` via HEAD requests for content-level verification.
/// Optionally deletes files at dest that don't exist in source (`--delete`).
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::fn_params_excessive_bools
)]
pub(super) async fn sync_files(
    cli: &Cli,
    client: &FabricClient,
    src_ws: Option<&str>,
    src_id: Option<&str>,
    src_path: Option<&str>,
    local_path: Option<&str>,
    dst_ws: &str,
    dst_id: &str,
    dst_path: &str,
    delete_extra: bool,
    checksum: bool,
    include: Option<&str>,
    exclude: Option<&str>,
    size_only: bool,
    no_overwrite: bool,
    force: bool,
    no_recursive: bool,
    max_delete: Option<usize>,
    existing: bool,
    remove_source_files: bool,
    min_size: Option<&str>,
    max_size: Option<&str>,
    itemize: bool,
) -> Result<()> {
    use crate::parallel::{self, BatchSummary};

    let concurrency = parallel::default_concurrency();
    let is_local = local_path.is_some();

    // Parse size limits
    let min_bytes = min_size.map(parse_size_value).transpose()?;
    let max_bytes = max_size.map(parse_size_value).transpose()?;

    // Build source file map (local directory or remote OneLake listing)
    let mut src_map = if let Some(local_dir) = local_path {
        build_local_file_map(local_dir, !no_recursive)?
    } else {
        let sw = src_ws.unwrap();
        let si = src_id.unwrap();
        let sp = src_path.unwrap();
        build_file_map(client, sw, si, sp).await?
    };

    // Build destination file map (always remote)
    let mut dst_map = build_file_map(client, dst_ws, dst_id, dst_path).await?;

    // Apply --no-recursive: filter out files in subdirectories
    if no_recursive {
        src_map.retain(|rel, _| !rel.contains('/'));
        dst_map.retain(|rel, _| !rel.contains('/'));
    }

    // Apply --min-size / --max-size filters
    if min_bytes.is_some() || max_bytes.is_some() {
        src_map.retain(|_, info| {
            if let Some(min) = min_bytes
                && info.size < min
            {
                return false;
            }
            if let Some(max) = max_bytes
                && info.size > max
            {
                return false;
            }
            true
        });
    }

    // Apply --include/--exclude filters to the source map
    if include.is_some() || exclude.is_some() {
        let include_patterns = include.map(parse_filter_patterns);
        let exclude_patterns = exclude.map(parse_filter_patterns);
        src_map.retain(|rel, _| {
            matches_filters(rel, include_patterns.as_ref(), exclude_patterns.as_ref())
        });
        // Also filter dst_map for consistent --delete behavior (only delete files
        // that would have been considered in scope)
        if delete_extra {
            dst_map.retain(|rel, _| {
                matches_filters(rel, include_patterns.as_ref(), exclude_patterns.as_ref())
            });
        }
    }

    // Apply --existing: limit source to files that already exist at destination
    if existing {
        src_map.retain(|rel, _| dst_map.contains_key(rel));
    }

    let total_source = src_map.len();

    // Determine files to copy based on comparison strategy
    let to_copy: Vec<String> = if force {
        // --force: copy ALL source files regardless of comparison
        src_map.keys().cloned().collect()
    } else if no_overwrite {
        // --no-overwrite: only copy files that don't exist at destination
        src_map
            .keys()
            .filter(|rel| !dst_map.contains_key(*rel))
            .cloned()
            .collect()
    } else if size_only {
        // --size-only: copy if file doesn't exist or has different size
        src_map
            .keys()
            .filter(|rel| {
                dst_map.get(*rel).is_none_or(|dst_info| {
                    let src_info = &src_map[*rel];
                    src_info.size != dst_info.size
                })
            })
            .cloned()
            .collect()
    } else if checksum {
        // MD5-based: need HEAD requests for files that exist in both
        eprintln!("  Using Content-MD5 checksums (HEAD per file)...");
        if is_local {
            // Local mode: compute local MD5 and compare with remote Content-MD5
            compute_local_checksum_diff(
                client,
                &src_map,
                &dst_map,
                local_path.unwrap(),
                dst_ws,
                dst_id,
                dst_path,
                concurrency,
            )
            .await?
        } else {
            compute_checksum_diff(
                client,
                &src_map,
                &dst_map,
                src_ws.unwrap(),
                src_id.unwrap(),
                src_path.unwrap(),
                dst_ws,
                dst_id,
                dst_path,
                concurrency,
            )
            .await?
        }
    } else if is_local {
        // Local mode default: compare by size (local files have no ETags)
        src_map
            .keys()
            .filter(|rel| {
                dst_map.get(*rel).is_none_or(|dst_info| {
                    let src_info = &src_map[*rel];
                    src_info.size != dst_info.size
                })
            })
            .cloned()
            .collect()
    } else {
        // ETag-based (default): compare ETags from listing (free)
        src_map
            .keys()
            .filter(|rel| {
                dst_map.get(*rel).is_none_or(|dst_info| {
                    let src_info = &src_map[*rel];
                    src_info.etag != dst_info.etag
                })
            })
            .cloned()
            .collect()
    };

    // Determine files to delete (at dest but not in source)
    let to_delete: Vec<String> = if delete_extra {
        dst_map
            .keys()
            .filter(|rel| !src_map.contains_key(*rel))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    // --max-delete safety: if more files would be deleted than allowed, skip deletions
    let (to_delete, deletions_skipped) = if let Some(max) = max_delete {
        if to_delete.len() > max {
            eprintln!(
                "  WARNING: {} files would be deleted (exceeds --max-delete={}), skipping all deletions",
                to_delete.len(),
                max
            );
            (Vec::new(), true)
        } else {
            (to_delete, false)
        }
    } else {
        (to_delete, false)
    };

    // Rename detection: if --delete is active and source is remote, find source_only
    // files whose ETag matches a dest_only file. Skipped for local sources.
    let (mut to_rename, to_copy, to_delete) = if delete_extra && !deletions_skipped && !is_local {
        detect_renames(&to_copy, &to_delete, &src_map, &dst_map)
    } else {
        (Vec::new(), to_copy, to_delete)
    };

    // Second pass: Content-MD5 based rename detection (when --checksum + --delete).
    // Skipped for local sources (no concept of server-side rename from local).
    let (to_copy, to_delete) =
        if checksum && delete_extra && !is_local && !to_copy.is_empty() && !to_delete.is_empty() {
            let md5_renames = detect_renames_by_checksum(
                client,
                &to_copy,
                &to_delete,
                &src_map,
                src_ws.unwrap(),
                src_id.unwrap(),
                src_path.unwrap(),
                dst_ws,
                dst_id,
                dst_path,
                concurrency,
            )
            .await?;
            if md5_renames.is_empty() {
                (to_copy, to_delete)
            } else {
                // Remove matched files from copy/delete lists
                let matched_src: std::collections::HashSet<&str> =
                    md5_renames.iter().map(|(_, new)| new.as_str()).collect();
                let matched_dst: std::collections::HashSet<&str> =
                    md5_renames.iter().map(|(old, _)| old.as_str()).collect();
                let remaining_copy = to_copy
                    .into_iter()
                    .filter(|r| !matched_src.contains(r.as_str()))
                    .collect();
                let remaining_delete = to_delete
                    .into_iter()
                    .filter(|r| !matched_dst.contains(r.as_str()))
                    .collect();
                to_rename.extend(md5_renames);
                (remaining_copy, remaining_delete)
            }
        } else {
            (to_copy, to_delete)
        };

    // Server-side deduplication: skipped for local sources (must upload from local).
    // For remote sources, check if existing dest files have same content hash.
    let (dedup_copies, remote_copies) = if is_local {
        (Vec::new(), to_copy.clone())
    } else if checksum {
        find_dedup_copies_by_checksum(
            client,
            &to_copy,
            &src_map,
            &dst_map,
            src_ws.unwrap(),
            src_id.unwrap(),
            src_path.unwrap(),
            dst_ws,
            dst_id,
            dst_path,
            concurrency,
        )
        .await?
    } else {
        find_dedup_copies(&to_copy, &src_map, &dst_map)
    };

    let strategy = if force {
        "force"
    } else if no_overwrite {
        "no-overwrite"
    } else if size_only {
        "size-only"
    } else if checksum {
        "Content-MD5"
    } else if is_local {
        "size"
    } else {
        "ETag"
    };
    eprintln!(
        "  Sync ({strategy}): {} to copy ({} dedup, {} remote), {} to rename, {} to delete, concurrency={concurrency}",
        to_copy.len(),
        dedup_copies.len(),
        remote_copies.len(),
        to_rename.len(),
        to_delete.len()
    );

    // Sync can overwrite AND delete files (with --delete-extra). Guard AFTER the
    // read-only diff so the dry-run plan enumerates exactly what would change,
    // before any copy/rename/delete network call.
    if output::dry_run_guard(
        cli,
        "lakehouse sync",
        &serde_json::json!({
            "destWorkspace": dst_ws,
            "destId": dst_id,
            "destPath": dst_path,
            "toCopy": to_copy.len(),
            "toRename": to_rename.len(),
            "toDelete": to_delete.len(),
            "deleteExtra": delete_extra,
            "copyFiles": to_copy,
            "deleteFiles": to_delete,
        }),
    ) {
        return Ok(());
    }

    // Execute renames first (atomic, O(1) per file)
    let (renamed, rename_failed) = if to_rename.is_empty() {
        (0, 0)
    } else {
        let rename_tasks: Vec<(String, String)> = to_rename
            .iter()
            .map(|(old, new)| (format!("{dst_path}/{old}"), format!("{dst_path}/{new}")))
            .collect();
        let item_names: Vec<String> = to_rename
            .iter()
            .map(|(old, new)| format!("{old} -> {new}"))
            .collect();
        let dw: Arc<str> = Arc::from(dst_ws);
        let di: Arc<str> = Arc::from(dst_id);
        let cc = client.clone();

        let results = parallel::execute_parallel(rename_tasks, concurrency, move |(src, dst)| {
            let c = cc.clone();
            let dw = Arc::clone(&dw);
            let di = Arc::clone(&di);
            async move {
                // Atomic rename within the destination item
                let result = c.rename_onelake_file(&dw, &di, &src, &dst).await?;
                if result.is_some() {
                    Ok(())
                } else {
                    // Rename not supported (shouldn't happen for same-item) — fall back
                    // to copy + delete in a future pass
                    Err(anyhow::anyhow!("atomic rename failed, fallback needed"))
                }
            }
        })
        .await;
        let summary = BatchSummary::from_results(&results, &item_names);
        (summary.succeeded, summary.failed)
    };

    // Dedup copies: same-lakehouse copy (existing dest file -> new dest path)
    let (n_dedup, dedup_fail) = if dedup_copies.is_empty() {
        (0, 0)
    } else {
        // dedup_copies: Vec<(source_rel_at_dest, target_rel)>
        let dedup_tasks: Vec<(String, String)> = dedup_copies
            .iter()
            .map(|(src_rel, dst_rel)| {
                (
                    format!("{dst_path}/{src_rel}"),
                    format!("{dst_path}/{dst_rel}"),
                )
            })
            .collect();
        let item_names: Vec<String> = dedup_copies
            .iter()
            .map(|(src_rel, dst_rel)| format!("{src_rel} -> {dst_rel} (dedup)"))
            .collect();
        let dw: Arc<str> = Arc::from(dst_ws);
        let di: Arc<str> = Arc::from(dst_id);
        let cc = client.clone();

        let results = parallel::execute_parallel(dedup_tasks, concurrency, move |(src, dst)| {
            let c = cc.clone();
            let dw = Arc::clone(&dw);
            let di = Arc::clone(&di);
            async move {
                // Same-lakehouse copy: source and dest are both in the dest lakehouse
                c.copy_onelake_file(&dw, &di, &src, &dw, &di, &dst).await?;
                Ok(())
            }
        })
        .await;
        let summary = BatchSummary::from_results(&results, &item_names);
        (summary.succeeded, summary.failed)
    };

    // Remote copies: either upload from local or cross-lakehouse server-side copy
    let (n_remote, remote_fail) = if remote_copies.is_empty() {
        (0, 0)
    } else if is_local {
        // Local mode: read files from disk and upload via DFS
        let local_dir = local_path.unwrap().to_string();
        let upload_tasks: Vec<(String, String)> = remote_copies
            .iter()
            .map(|rel| (rel.clone(), format!("{dst_path}/{rel}")))
            .collect();
        let item_names: Vec<String> = remote_copies.clone();
        let dw: Arc<str> = Arc::from(dst_ws);
        let di: Arc<str> = Arc::from(dst_id);
        let local_base: Arc<str> = Arc::from(local_dir.as_str());
        let cc = client.clone();

        let results =
            parallel::execute_parallel(upload_tasks, concurrency, move |(rel, dst_remote)| {
                let c = cc.clone();
                let dw = Arc::clone(&dw);
                let di = Arc::clone(&di);
                let local_base = Arc::clone(&local_base);
                async move {
                    let local_file = Path::new(local_base.as_ref())
                        .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                    let data = tokio::fs::read(&local_file).await.map_err(|e| {
                        anyhow::anyhow!("Failed to read {}: {e}", local_file.display())
                    })?;
                    c.upload_onelake_file(&dw, &di, &dst_remote, data).await?;
                    Ok(())
                }
            })
            .await;
        let summary = BatchSummary::from_results(&results, &item_names);
        (summary.succeeded, summary.failed)
    } else {
        // Remote mode: cross-lakehouse server-side copy
        let sp = src_path.unwrap();
        let copy_tasks: Vec<(String, String)> = remote_copies
            .iter()
            .map(|rel| (format!("{sp}/{rel}"), format!("{dst_path}/{rel}")))
            .collect();
        let item_names: Vec<String> = remote_copies.clone();
        let sw: Arc<str> = Arc::from(src_ws.unwrap());
        let si: Arc<str> = Arc::from(src_id.unwrap());
        let dw: Arc<str> = Arc::from(dst_ws);
        let di: Arc<str> = Arc::from(dst_id);
        let cc = client.clone();

        let results = parallel::execute_parallel(copy_tasks, concurrency, move |(src, dst)| {
            let c = cc.clone();
            let sw = Arc::clone(&sw);
            let si = Arc::clone(&si);
            let dw = Arc::clone(&dw);
            let di = Arc::clone(&di);
            async move {
                c.copy_onelake_file(&sw, &si, &src, &dw, &di, &dst).await?;
                Ok(())
            }
        })
        .await;
        let summary = BatchSummary::from_results(&results, &item_names);
        (summary.succeeded, summary.failed)
    };

    let copied = n_dedup + n_remote;
    let copy_failed = dedup_fail + remote_fail;

    // Delete extra files in parallel
    let (deleted, delete_failed) = if to_delete.is_empty() {
        (0, 0)
    } else {
        let delete_tasks: Vec<String> = to_delete
            .iter()
            .map(|rel| format!("{dst_path}/{rel}"))
            .collect();
        let item_names: Vec<String> = to_delete.clone();
        let dw: Arc<str> = Arc::from(dst_ws);
        let di: Arc<str> = Arc::from(dst_id);
        let cc = client.clone();

        let results = parallel::execute_parallel(delete_tasks, concurrency, move |path| {
            let c = cc.clone();
            let dw = Arc::clone(&dw);
            let di = Arc::clone(&di);
            async move {
                c.delete_onelake_file(&dw, &di, &path).await?;
                Ok(())
            }
        })
        .await;
        let summary = BatchSummary::from_results(&results, &item_names);
        (summary.succeeded, summary.failed)
    };

    let total_failed = copy_failed + delete_failed + rename_failed;

    // --itemize: output per-file actions to stderr
    if itemize {
        for (old, new) in &to_rename {
            eprintln!("  [rename] {old} -> {new}");
        }
        for rel in &to_copy {
            let mode = if dedup_copies.iter().any(|(_, t)| t == rel) {
                "dedup"
            } else {
                "remote"
            };
            eprintln!("  [copy]   {rel} ({mode})");
        }
        for rel in &to_delete {
            eprintln!("  [delete] {rel}");
        }
        let unchanged_count = total_source - to_copy.len() - to_rename.len();
        if unchanged_count > 0 {
            eprintln!("  [skip]   {unchanged_count} unchanged file(s)");
        }
    }

    // --remove-source-files: delete source files that were successfully copied
    let source_removed = if remove_source_files && copied > 0 {
        if is_local {
            // Local mode: delete local files
            let local_dir = local_path.unwrap();
            let mut removed = 0usize;
            for rel in &to_copy {
                let local_file =
                    Path::new(local_dir).join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                if std::fs::remove_file(&local_file).is_ok() {
                    removed += 1;
                }
            }
            removed
        } else {
            // Remote mode: delete via DFS
            let sp = src_path.unwrap();
            let remove_tasks: Vec<String> =
                to_copy.iter().map(|rel| format!("{sp}/{rel}")).collect();
            let item_names: Vec<String> = to_copy.clone();
            let sw: Arc<str> = Arc::from(src_ws.unwrap());
            let si: Arc<str> = Arc::from(src_id.unwrap());
            let cc = client.clone();

            let results = parallel::execute_parallel(remove_tasks, concurrency, move |path| {
                let c = cc.clone();
                let sw = Arc::clone(&sw);
                let si = Arc::clone(&si);
                async move {
                    c.delete_onelake_file(&sw, &si, &path).await?;
                    Ok(())
                }
            })
            .await;
            let summary = BatchSummary::from_results(&results, &item_names);
            summary.succeeded
        }
    } else {
        0
    };

    let status = if total_failed == 0 {
        "synced"
    } else {
        "partial_failure"
    };
    let mut obj = serde_json::json!({
        "sourceFiles": src_map.len(),
        "destFiles": dst_map.len(),
        "copied": copied,
        "dedupCopied": n_dedup,
        "renamed": renamed,
        "deleted": deleted,
        "unchanged": total_source - to_copy.len() - to_rename.len(),
        "failed": total_failed,
        "strategy": strategy,
        "status": status
    });
    if source_removed > 0 {
        obj["sourceRemoved"] = serde_json::json!(source_removed);
    }
    if deletions_skipped {
        obj["deletionsSkipped"] = serde_json::json!(true);
    }
    output::render_object(cli, &obj, "status");

    if total_failed > 0 {
        return Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::ApiError,
            format!("Sync partially failed: {total_failed} operations failed"),
            "Retry the sync command to process remaining files. Use --verbose for detailed failure information.",
        )
        .into());
    }

    Ok(())
}

// ─── Sync Helper Functions ───────────────────────────────────────────────────

/// Build a file map from a local directory (recursive walk).
/// Returns relative paths using forward slashes (cross-platform).
fn build_local_file_map(
    dir: &str,
    recursive: bool,
) -> Result<std::collections::HashMap<String, FileInfo>> {
    let base = Path::new(dir);
    if !base.is_dir() {
        return Err(crate::errors::FabioError::invalid_input(format!(
            "Local path '{dir}' is not a directory",
        ))
        .into());
    }

    let mut map = std::collections::HashMap::new();
    collect_local_files(base, base, recursive, &mut map)?;
    Ok(map)
}

/// Recursively collect files from a local directory.
fn collect_local_files(
    base: &Path,
    current: &Path,
    recursive: bool,
    map: &mut std::collections::HashMap<String, FileInfo>,
) -> Result<()> {
    let entries = std::fs::read_dir(current).map_err(|e| {
        crate::errors::FabioError::invalid_input(format!(
            "Cannot read directory {}: {e}",
            current.display()
        ))
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| {
            crate::errors::FabioError::invalid_input(format!("Directory entry error: {e}"))
        })?;
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_local_files(base, &path, recursive, map)?;
            }
        } else if path.is_file() {
            let metadata = std::fs::metadata(&path).map_err(|e| {
                crate::errors::FabioError::invalid_input(format!(
                    "Cannot read metadata for {}: {e}",
                    path.display()
                ))
            })?;
            // Compute relative path with forward slashes
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            map.insert(
                rel,
                FileInfo {
                    size: metadata.len(),
                    etag: String::new(), // local files have no ETag
                },
            );
        }
    }
    Ok(())
}

/// Compute diff using local MD5 checksums vs remote `Content-MD5`.
/// Returns list of relative paths that need uploading.
#[allow(clippy::too_many_arguments)]
async fn compute_local_checksum_diff(
    client: &FabricClient,
    src_map: &std::collections::HashMap<String, FileInfo>,
    dst_map: &std::collections::HashMap<String, FileInfo>,
    local_dir: &str,
    dst_ws: &str,
    dst_id: &str,
    dst_path: &str,
    concurrency: usize,
) -> Result<Vec<String>> {
    use crate::parallel;

    // Files only in source — always upload
    let mut to_copy: Vec<String> = src_map
        .keys()
        .filter(|rel| !dst_map.contains_key(*rel))
        .cloned()
        .collect();

    // Files in both — compare MD5
    let common: Vec<String> = src_map
        .keys()
        .filter(|rel| dst_map.contains_key(*rel))
        .cloned()
        .collect();

    if common.is_empty() {
        return Ok(to_copy);
    }

    eprintln!("  Checking MD5 for {} files...", common.len());

    // Compute local MD5 for common files
    let local_md5s: Vec<String> = common
        .iter()
        .map(|rel| {
            let path = Path::new(local_dir).join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            std::fs::read(&path).map_or_else(
                |_| String::new(),
                |data| {
                    let hash = md5::compute(&data);
                    BASE64.encode(hash.0)
                },
            )
        })
        .collect();

    // Get remote MD5 via HEAD requests
    let dst_tasks: Vec<String> = common
        .iter()
        .map(|rel| format!("{dst_path}/{rel}"))
        .collect();
    let dw: Arc<str> = Arc::from(dst_ws);
    let di: Arc<str> = Arc::from(dst_id);
    let cc = client.clone();
    let dst_results = parallel::execute_parallel(dst_tasks, concurrency, move |path| {
        let c = cc.clone();
        let dw = Arc::clone(&dw);
        let di = Arc::clone(&di);
        async move {
            let props = c.get_file_properties(&dw, &di, &path).await?;
            Ok(props)
        }
    })
    .await;

    // Compare
    for (i, rel) in common.iter().enumerate() {
        let src_md5 = &local_md5s[i];
        let dst_md5 = dst_results
            .get(i)
            .and_then(|r| r.result.as_ref().ok())
            .and_then(|v| v.get("contentMD5"))
            .and_then(Value::as_str)
            .unwrap_or("");

        if src_md5.is_empty() || dst_md5.is_empty() {
            // Fallback to size comparison
            let src_info = &src_map[rel];
            let dst_info = &dst_map[rel];
            if src_info.size != dst_info.size {
                to_copy.push(rel.clone());
            }
        } else if src_md5 != dst_md5 {
            to_copy.push(rel.clone());
        }
    }

    Ok(to_copy)
}

/// Build a file map (`relative_path` -> `FileInfo`) from a remote listing.
/// Lists from root (no directory param) to avoid the DFS virtual view doubling.
async fn build_file_map(
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    path: &str,
) -> Result<std::collections::HashMap<String, FileInfo>> {
    let files = client.list_onelake_files(workspace, item_id, None).await?;
    let prefix = format!("{item_id}/{path}/");

    let mut map = std::collections::HashMap::new();
    for file in &files {
        let Some(name) = file.get("name").and_then(Value::as_str) else {
            continue;
        };
        let is_dir = file
            .get("isDirectory")
            .and_then(Value::as_str)
            .unwrap_or("false")
            == "true";
        if is_dir {
            continue;
        }
        if let Some(relative) = name.strip_prefix(&prefix) {
            // Reject paths with traversal sequences from API responses
            if relative.contains("../") || relative.contains("..\\") || relative == ".." {
                continue;
            }
            let size = file
                .get("contentLength")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let etag = file
                .get("eTag")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            map.insert(relative.to_string(), FileInfo { size, etag });
        }
    }
    Ok(map)
}

/// Server-side deduplication: find files that need copying but already have a
/// content-identical twin at the destination (same `ETag` + size). For these files,
/// we can perform a same-lakehouse copy instead of a cross-lakehouse transfer.
///
/// Returns `(dedup_copies, remote_copies)` where:
/// - `dedup_copies`: `Vec<(existing_dest_rel_path, target_rel_path)>` — copy within dest
/// - `remote_copies`: `Vec<target_rel_path>` — normal cross-lakehouse copy
fn find_dedup_copies(
    to_copy: &[String],
    src_map: &std::collections::HashMap<String, FileInfo>,
    dst_map: &std::collections::HashMap<String, FileInfo>,
) -> (Vec<(String, String)>, Vec<String>) {
    use std::collections::HashMap;

    if to_copy.is_empty() {
        return (Vec::new(), Vec::new());
    }

    // Build index of ALL destination files by ETag (includes files not being deleted).
    // These are potential dedup sources — files already at the destination with known content.
    let mut dest_by_etag: HashMap<&str, Vec<&str>> = HashMap::new();
    for (rel, info) in dst_map {
        if !info.etag.is_empty() {
            dest_by_etag.entry(&info.etag).or_default().push(rel);
        }
    }

    let mut dedup_copies: Vec<(String, String)> = Vec::new();
    let mut remote_copies: Vec<String> = Vec::new();

    for rel in to_copy {
        let Some(src_info) = src_map.get(rel) else {
            remote_copies.push(rel.clone());
            continue;
        };

        if src_info.etag.is_empty() {
            remote_copies.push(rel.clone());
            continue;
        }

        // Look for a destination file with the same ETag and size
        let found = dest_by_etag
            .get(src_info.etag.as_str())
            .and_then(|candidates| {
                candidates.iter().find(|&&c| {
                    // Don't use the target path itself as source (it may be stale/overwritten)
                    c != rel && dst_map.get(c).is_some_and(|d| d.size == src_info.size)
                })
            })
            .copied();

        if let Some(existing_path) = found {
            dedup_copies.push((existing_path.to_string(), rel.clone()));
        } else {
            remote_copies.push(rel.clone());
        }
    }

    (dedup_copies, remote_copies)
}

/// Server-side deduplication using `Content-MD5` checksums (parallel HEAD requests).
///
/// Fetches MD5 for source files that need copying and for ALL destination files,
/// then matches by MD5 + size. Files whose content already exists at the destination
/// are copied locally (same-lakehouse) instead of cross-lakehouse.
#[allow(clippy::too_many_arguments)]
async fn find_dedup_copies_by_checksum(
    client: &FabricClient,
    to_copy: &[String],
    src_map: &std::collections::HashMap<String, FileInfo>,
    dst_map: &std::collections::HashMap<String, FileInfo>,
    src_ws: &str,
    src_id: &str,
    src_path: &str,
    dst_ws: &str,
    dst_id: &str,
    dst_path: &str,
    concurrency: usize,
) -> Result<(Vec<(String, String)>, Vec<String>)> {
    use crate::parallel;
    use std::collections::HashMap;

    if to_copy.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // Fetch MD5 for source files that need copying
    let src_tasks: Vec<String> = to_copy
        .iter()
        .map(|rel| format!("{src_path}/{rel}"))
        .collect();
    let sw: Arc<str> = Arc::from(src_ws);
    let si: Arc<str> = Arc::from(src_id);
    let cc = client.clone();
    let src_results = parallel::execute_parallel(src_tasks, concurrency, move |path| {
        let c = cc.clone();
        let sw = Arc::clone(&sw);
        let si = Arc::clone(&si);
        async move {
            let props = c.get_file_properties(&sw, &si, &path).await?;
            Ok(props)
        }
    })
    .await;

    // Fetch MD5 for ALL destination files (potential dedup sources)
    let dst_rels: Vec<&String> = dst_map.keys().collect();
    let dst_tasks: Vec<String> = dst_rels
        .iter()
        .map(|rel| format!("{dst_path}/{rel}"))
        .collect();
    let dw: Arc<str> = Arc::from(dst_ws);
    let di: Arc<str> = Arc::from(dst_id);
    let cc = client.clone();
    let dst_results = parallel::execute_parallel(dst_tasks, concurrency, move |path| {
        let c = cc.clone();
        let dw = Arc::clone(&dw);
        let di = Arc::clone(&di);
        async move {
            let props = c.get_file_properties(&dw, &di, &path).await?;
            Ok(props)
        }
    })
    .await;

    // Build dest index: MD5 -> [(rel_path, size)]
    let mut dest_by_md5: HashMap<String, Vec<(&str, u64)>> = HashMap::new();
    for (i, rel) in dst_rels.iter().enumerate() {
        let md5 = dst_results
            .get(i)
            .and_then(|r| r.result.as_ref().ok())
            .and_then(|v| v.get("contentMD5"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let size = dst_results
            .get(i)
            .and_then(|r| r.result.as_ref().ok())
            .and_then(|v| v.get("contentLength"))
            .and_then(Value::as_u64)
            .unwrap_or(0);

        if !md5.is_empty() {
            dest_by_md5.entry(md5).or_default().push((rel, size));
        }
    }

    let mut dedup_copies: Vec<(String, String)> = Vec::new();
    let mut remote_copies: Vec<String> = Vec::new();

    for (i, rel) in to_copy.iter().enumerate() {
        let src_md5 = src_results
            .get(i)
            .and_then(|r| r.result.as_ref().ok())
            .and_then(|v| v.get("contentMD5"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let src_size = src_map.get(rel).map_or(0, |info| info.size);

        if !src_md5.is_empty()
            && let Some(candidates) = dest_by_md5.get(src_md5)
        {
            let match_found = candidates
                .iter()
                .find(|(path, size)| *path != rel && *size == src_size)
                .map(|(path, _)| *path);

            if let Some(existing_path) = match_found {
                dedup_copies.push((existing_path.to_string(), rel.clone()));
                continue;
            }
        }

        remote_copies.push(rel.clone());
    }

    if !dedup_copies.is_empty() {
        eprintln!(
            "  Dedup: {} files can use existing dest content (same MD5)",
            dedup_copies.len()
        );
    }

    Ok((dedup_copies, remote_copies))
}

/// Detect renames by matching source-only files with dest-only files that have
/// the same `ETag`. Returns `(renames, remaining_to_copy, remaining_to_delete)`.
///
/// A rename is detected when a file in `to_copy` (source-only or changed) has
/// an `ETag` matching a file in `to_delete` (dest-only). In this case, the file
/// was renamed at the source — we can do an atomic O(1) rename at the destination
/// instead of a full copy + delete.
fn detect_renames(
    to_copy: &[String],
    to_delete: &[String],
    src_map: &std::collections::HashMap<String, FileInfo>,
    dst_map: &std::collections::HashMap<String, FileInfo>,
) -> (Vec<(String, String)>, Vec<String>, Vec<String>) {
    use std::collections::HashMap;

    // Build an index of dest-only files keyed by ETag -> dest relative path
    // Only include files with non-empty ETags
    let mut dest_by_etag: HashMap<&str, Vec<&str>> = HashMap::new();
    for rel in to_delete {
        if let Some(info) = dst_map.get(rel)
            && !info.etag.is_empty()
        {
            dest_by_etag.entry(&info.etag).or_default().push(rel);
        }
    }

    let mut renames: Vec<(String, String)> = Vec::new();
    let mut matched_dest: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut remaining_copy: Vec<String> = Vec::new();

    for rel in to_copy {
        if let Some(src_info) = src_map.get(rel)
            && !src_info.etag.is_empty()
        {
            // Look for a dest-only file with the same ETag that hasn't been matched yet
            if let Some(candidates) = dest_by_etag.get(src_info.etag.as_str()) {
                let match_found = candidates
                    .iter()
                    .find(|&&c| !matched_dest.contains(c))
                    .copied();

                if let Some(old_path) = match_found {
                    // Also verify size matches as a safety check
                    let size_match = dst_map
                        .get(old_path)
                        .is_some_and(|d| d.size == src_info.size);

                    if size_match {
                        renames.push((old_path.to_string(), rel.clone()));
                        matched_dest.insert(old_path);
                        continue;
                    }
                }
            }
        }
        remaining_copy.push(rel.clone());
    }

    // Remove matched dest paths from the to_delete list
    let remaining_delete: Vec<String> = to_delete
        .iter()
        .filter(|rel| !matched_dest.contains(rel.as_str()))
        .cloned()
        .collect();

    (renames, remaining_copy, remaining_delete)
}

/// Detect renames using `Content-MD5` comparison via parallel HEAD requests.
///
/// Called as a second pass after `ETag`-based detection when `--checksum` is active.
/// Fetches MD5 for remaining unmatched source-only and dest-only files, then matches
/// by MD5 + size. Falls back to size-only matching when MD5 is not available
/// (which is the case for `OneLake` DFS where `Content-MD5` headers are not returned).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn detect_renames_by_checksum(
    client: &FabricClient,
    to_copy: &[String],
    to_delete: &[String],
    _src_map: &std::collections::HashMap<String, FileInfo>,
    src_ws: &str,
    src_id: &str,
    src_path: &str,
    dst_ws: &str,
    dst_id: &str,
    dst_path: &str,
    concurrency: usize,
) -> Result<Vec<(String, String)>> {
    use crate::parallel;
    use std::collections::HashMap;

    eprintln!(
        "  Checking checksums for rename detection ({} source + {} dest candidates)...",
        to_copy.len(),
        to_delete.len()
    );

    // Fetch properties for source-only files
    let src_tasks: Vec<String> = to_copy
        .iter()
        .map(|rel| format!("{src_path}/{rel}"))
        .collect();
    let sw: Arc<str> = Arc::from(src_ws);
    let si: Arc<str> = Arc::from(src_id);
    let cc = client.clone();
    let src_results = parallel::execute_parallel(src_tasks, concurrency, move |path| {
        let c = cc.clone();
        let sw = Arc::clone(&sw);
        let si = Arc::clone(&si);
        async move {
            let props = c.get_file_properties(&sw, &si, &path).await?;
            Ok(props)
        }
    })
    .await;

    // Fetch properties for dest-only files
    let dst_tasks: Vec<String> = to_delete
        .iter()
        .map(|rel| format!("{dst_path}/{rel}"))
        .collect();
    let dw: Arc<str> = Arc::from(dst_ws);
    let di: Arc<str> = Arc::from(dst_id);
    let cc = client.clone();
    let dst_results = parallel::execute_parallel(dst_tasks, concurrency, move |path| {
        let c = cc.clone();
        let dw = Arc::clone(&dw);
        let di = Arc::clone(&di);
        async move {
            let props = c.get_file_properties(&dw, &di, &path).await?;
            Ok(props)
        }
    })
    .await;

    // Build dest index: (md5_or_empty, size) -> [rel_path]
    // When MD5 is available, match by MD5+size. When not, match by size alone
    // (only for unique sizes to avoid false positives).
    let mut dest_by_md5: HashMap<String, Vec<(&str, u64)>> = HashMap::new();
    let mut dest_by_size: HashMap<u64, Vec<&str>> = HashMap::new();
    let mut has_any_md5 = false;

    for (i, rel) in to_delete.iter().enumerate() {
        let md5 = dst_results
            .get(i)
            .and_then(|r| r.result.as_ref().ok())
            .and_then(|v| v.get("contentMD5"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let dst_size = dst_results
            .get(i)
            .and_then(|r| r.result.as_ref().ok())
            .and_then(|v| v.get("contentLength"))
            .and_then(Value::as_u64)
            .unwrap_or(0);

        if !md5.is_empty() {
            has_any_md5 = true;
            dest_by_md5.entry(md5).or_default().push((rel, dst_size));
        }
        if dst_size > 0 {
            dest_by_size.entry(dst_size).or_default().push(rel);
        }
    }

    let mut renames: Vec<(String, String)> = Vec::new();
    let mut matched_dest: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for (i, rel) in to_copy.iter().enumerate() {
        let src_md5 = src_results
            .get(i)
            .and_then(|r| r.result.as_ref().ok())
            .and_then(|v| v.get("contentMD5"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let src_size = src_results
            .get(i)
            .and_then(|r| r.result.as_ref().ok())
            .and_then(|v| v.get("contentLength"))
            .and_then(Value::as_u64)
            .unwrap_or(0);

        // Try MD5 match first (strongest signal)
        if !src_md5.is_empty()
            && has_any_md5
            && let Some(candidates) = dest_by_md5.get(src_md5)
        {
            let match_found = candidates
                .iter()
                .find(|(path, size)| !matched_dest.contains(*path) && *size == src_size)
                .map(|(path, _)| *path);

            if let Some(old_path) = match_found {
                renames.push((old_path.to_string(), rel.clone()));
                matched_dest.insert(old_path);
                continue;
            }
        }

        // Fallback: size-only match (only when the size is unique among dest orphans
        // to avoid false positives from files that happen to have the same size)
        if src_size > 0
            && let Some(candidates) = dest_by_size.get(&src_size)
        {
            // Only match when there's exactly ONE dest file with this size
            // (avoids ambiguity)
            let unmatched: Vec<&str> = candidates
                .iter()
                .filter(|p| !matched_dest.contains(**p))
                .copied()
                .collect();
            if unmatched.len() == 1 {
                let old_path = unmatched[0];
                renames.push((old_path.to_string(), rel.clone()));
                matched_dest.insert(old_path);
            }
        }
    }

    if !renames.is_empty() {
        eprintln!(
            "  Checksum rename detection: {} matches found",
            renames.len()
        );
    }

    Ok(renames)
}

/// Compute diff using `Content-MD5` checksums (parallel HEAD requests).
/// Returns list of relative paths that need copying.
#[allow(clippy::too_many_arguments)]
async fn compute_checksum_diff(
    client: &FabricClient,
    src_map: &std::collections::HashMap<String, FileInfo>,
    dst_map: &std::collections::HashMap<String, FileInfo>,
    src_ws: &str,
    src_id: &str,
    src_path: &str,
    dst_ws: &str,
    dst_id: &str,
    dst_path: &str,
    concurrency: usize,
) -> Result<Vec<String>> {
    use crate::parallel;

    // Files only in source — always copy
    let mut to_copy: Vec<String> = src_map
        .keys()
        .filter(|rel| !dst_map.contains_key(*rel))
        .cloned()
        .collect();

    // Files in both — compare MD5 via HEAD
    let common: Vec<String> = src_map
        .keys()
        .filter(|rel| dst_map.contains_key(*rel))
        .cloned()
        .collect();

    if common.is_empty() {
        return Ok(to_copy);
    }

    eprintln!("  Checking MD5 for {} files...", common.len());

    // Get MD5 for source files
    let src_tasks: Vec<String> = common
        .iter()
        .map(|rel| format!("{src_path}/{rel}"))
        .collect();
    let sw: Arc<str> = Arc::from(src_ws);
    let si: Arc<str> = Arc::from(src_id);
    let cc = client.clone();
    let src_results = parallel::execute_parallel(src_tasks, concurrency, move |path| {
        let c = cc.clone();
        let sw = Arc::clone(&sw);
        let si = Arc::clone(&si);
        async move {
            let props = c.get_file_properties(&sw, &si, &path).await?;
            Ok(props)
        }
    })
    .await;

    // Get MD5 for dest files
    let dst_tasks: Vec<String> = common
        .iter()
        .map(|rel| format!("{dst_path}/{rel}"))
        .collect();
    let dw: Arc<str> = Arc::from(dst_ws);
    let di: Arc<str> = Arc::from(dst_id);
    let cc = client.clone();
    let dst_results = parallel::execute_parallel(dst_tasks, concurrency, move |path| {
        let c = cc.clone();
        let dw = Arc::clone(&dw);
        let di = Arc::clone(&di);
        async move {
            let props = c.get_file_properties(&dw, &di, &path).await?;
            Ok(props)
        }
    })
    .await;

    // Compare MD5s
    for (i, rel) in common.iter().enumerate() {
        let src_md5 = src_results
            .get(i)
            .and_then(|r| r.result.as_ref().ok())
            .and_then(|v| v.get("contentMD5"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let dst_md5 = dst_results
            .get(i)
            .and_then(|r| r.result.as_ref().ok())
            .and_then(|v| v.get("contentMD5"))
            .and_then(Value::as_str)
            .unwrap_or("");

        // If either MD5 is empty (not provided by API), fall back to size comparison
        if src_md5.is_empty() || dst_md5.is_empty() {
            let src_info = &src_map[rel];
            let dst_info = &dst_map[rel];
            if src_info.size != dst_info.size {
                to_copy.push(rel.clone());
            }
        } else if src_md5 != dst_md5 {
            to_copy.push(rel.clone());
        }
    }

    Ok(to_copy)
}
