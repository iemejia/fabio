use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::{expand_local_glob, expand_remote_glob, render_batch_result};

// ─── Data Operations (Listing + File I/O) ────────────────────────────────────

pub(super) async fn tables(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/lakehouses/{id}/tables"),
            "data",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["name", "type", "format"],
        &["NAME", "TYPE", "FORMAT"],
        "name",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn files(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    path: Option<&str>,
) -> Result<()> {
    let items = client.list_onelake_files(workspace, id, path).await?;
    output::render_list(
        cli,
        &items,
        &["name", "contentLength", "lastModified"],
        &["NAME", "SIZE", "MODIFIED"],
        "name",
    );
    Ok(())
}

pub(super) async fn upload(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    source_path: &str,
    dest_path: &str,
) -> Result<()> {
    use crate::parallel::{self, BatchSummary};

    // Expand glob patterns for local files
    let local_files = expand_local_glob(source_path)?;

    if local_files.len() == 1 {
        // Single file: upload directly
        let data = std::fs::read(&local_files[0]).map_err(|e| {
            crate::errors::FabioError::invalid_input(format!(
                "Cannot read file {}: {e}",
                local_files[0]
            ))
        })?;
        let result = client
            .upload_onelake_file(workspace, id, dest_path, data)
            .await?;
        output::render_object(cli, &result, "status");
        return Ok(());
    }

    // Multiple files: upload in parallel to dest_path as directory
    let concurrency = parallel::default_concurrency();
    eprintln!(
        "  Uploading {} files with concurrency={concurrency}...",
        local_files.len()
    );

    let upload_tasks: Vec<(String, String)> = local_files
        .into_iter()
        .map(|local| {
            let filename = Path::new(&local)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let remote = format!("{dest_path}/{filename}");
            (local, remote)
        })
        .collect();
    let item_names: Vec<String> = upload_tasks.iter().map(|(l, _)| l.clone()).collect();

    let workspace: Arc<str> = Arc::from(workspace);
    let id: Arc<str> = Arc::from(id);
    let client = client.clone();

    let results = parallel::execute_parallel(upload_tasks, concurrency, move |(local, remote)| {
        let client = client.clone();
        let workspace = Arc::clone(&workspace);
        let id = Arc::clone(&id);
        async move {
            let data = tokio::fs::read(&local).await.map_err(|e| {
                anyhow::anyhow!(
                    "{}",
                    crate::errors::FabioError::invalid_input(format!(
                        "Cannot read file {local}: {e}"
                    ))
                )
            })?;
            client
                .upload_onelake_file(&workspace, &id, &remote, data)
                .await?;
            Ok(())
        }
    })
    .await;

    let summary = BatchSummary::from_results(&results, &item_names);
    render_batch_result(cli, &summary, "uploaded")
}

pub(super) async fn download(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    source_path: &str,
    dest_path: &str,
) -> Result<()> {
    // Security: reject symlinks at destination to prevent arbitrary file overwrite
    if let Ok(meta) = std::fs::symlink_metadata(dest_path)
        && meta.file_type().is_symlink()
    {
        return Err(crate::errors::FabioError::invalid_input(format!(
            "Destination path is a symlink (refusing to follow): {dest_path}"
        ))
        .into());
    }

    let data = client
        .download_onelake_file(workspace, id, source_path)
        .await?;

    std::fs::write(dest_path, &data).map_err(|e| {
        crate::errors::FabioError::invalid_input(format!("Cannot write to {dest_path}: {e}"))
    })?;

    let obj = serde_json::json!({
        "source": source_path,
        "destination": dest_path,
        "size": data.len(),
        "status": "downloaded"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Copy/Move/Delete File Operations ────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn copy_file(
    cli: &Cli,
    client: &FabricClient,
    src_ws: &str,
    src_id: &str,
    src_path: &str,
    dst_ws: &str,
    dst_id: &str,
    dst_path: &str,
) -> Result<()> {
    use crate::parallel::{self, BatchSummary};

    // Check if source path contains a glob pattern
    let matched_files = expand_remote_glob(client, src_ws, src_id, src_path).await?;

    if matched_files.len() == 1 && matched_files[0] == src_path {
        // Single file: copy directly
        let result = client
            .copy_onelake_file(src_ws, src_id, src_path, dst_ws, dst_id, dst_path)
            .await?;
        output::render_object(cli, &result, "status");
        return Ok(());
    }

    // Multiple files: copy in parallel, dest_path is a directory
    let concurrency = parallel::default_concurrency();
    eprintln!(
        "  Copying {} files with concurrency={concurrency}...",
        matched_files.len()
    );

    let copy_tasks: Vec<(String, String)> = matched_files
        .into_iter()
        .map(|src| {
            let filename = src.rsplit('/').next().unwrap_or(&src).to_string();
            let dest = format!("{dst_path}/{filename}");
            (src, dest)
        })
        .collect();
    let item_names: Vec<String> = copy_tasks.iter().map(|(s, _)| s.clone()).collect();

    let src_ws: Arc<str> = Arc::from(src_ws);
    let src_id: Arc<str> = Arc::from(src_id);
    let dst_ws: Arc<str> = Arc::from(dst_ws);
    let dst_id: Arc<str> = Arc::from(dst_id);
    let client = client.clone();

    let results = parallel::execute_parallel(copy_tasks, concurrency, move |(src, dest)| {
        let client = client.clone();
        let src_ws = Arc::clone(&src_ws);
        let src_id = Arc::clone(&src_id);
        let dst_ws = Arc::clone(&dst_ws);
        let dst_id = Arc::clone(&dst_id);
        async move {
            client
                .copy_onelake_file(&src_ws, &src_id, &src, &dst_ws, &dst_id, &dest)
                .await?;
            Ok(())
        }
    })
    .await;

    let summary = BatchSummary::from_results(&results, &item_names);
    render_batch_result(cli, &summary, "copied")
}

pub(super) async fn create_directory(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    path: &str,
) -> Result<()> {
    let result = client.create_onelake_directory(workspace, id, path).await?;
    output::render_object(cli, &result, "status");
    Ok(())
}

pub(super) async fn delete_file(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    path: &str,
) -> Result<()> {
    let result = client.delete_onelake_file(workspace, id, path).await?;
    output::render_object(cli, &result, "status");
    Ok(())
}

/// Recursively delete a `OneLake` directory (and everything under it).
///
/// This is irreversible, so it honors `--dry-run`. Uses the DFS recursive
/// delete (`DELETE {item}/{path}?recursive=true`).
pub(super) async fn delete_directory(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    path: &str,
) -> Result<()> {
    validate_delete_directory_path(path)?;
    let preview = serde_json::json!({
        "workspace": workspace,
        "id": id,
        "path": path,
        "recursive": true,
    });
    if output::dry_run_guard(cli, "lakehouse delete-directory", &preview) {
        return Ok(());
    }
    client.delete_onelake_directory(workspace, id, path).await?;
    let result = serde_json::json!({
        "path": path,
        "recursive": true,
        "status": "deleted",
    });
    output::render_object(cli, &result, "status");
    Ok(())
}

/// Reject a directory path that would resolve to the item filesystem root.
///
/// A recursive delete of the root (empty path, `/`, `.`, `..`) would erase the
/// ENTIRE lakehouse (all Files and Tables), so it is blocked outright. Concrete
/// subdirectories — including the high-level `Files` or `Tables` roots — are
/// allowed (they are legitimate, if large, deletes guarded by `--dry-run`).
/// Pure for unit testing.
fn validate_delete_directory_path(path: &str) -> Result<()> {
    let trimmed = path.trim().trim_matches('/').trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Refusing to recursively delete the item root (path: '{path}')"),
            "Specify a concrete directory under the item, e.g. 'Files/staging' or 'Tables/foo'. \
             A recursive delete of the root would erase the entire lakehouse.",
        )
        .into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn move_file(
    cli: &Cli,
    client: &FabricClient,
    src_ws: &str,
    src_id: &str,
    src_path: &str,
    dst_ws: &str,
    dst_id: &str,
    dst_path: &str,
) -> Result<()> {
    use crate::parallel::{self, BatchSummary};

    // Check if source path contains a glob pattern
    let matched_files = expand_remote_glob(client, src_ws, src_id, src_path).await?;

    if matched_files.len() == 1 && matched_files[0] == src_path {
        // Single file move
        let is_same_item = src_ws == dst_ws && src_id == dst_id;

        let obj = if is_same_item {
            // Same item: use atomic rename (falls back to copy+delete internally)
            client
                .move_onelake_file(src_ws, src_id, src_path, dst_path)
                .await?
        } else {
            // Cross-item: must use copy + delete
            client
                .copy_onelake_file(src_ws, src_id, src_path, dst_ws, dst_id, dst_path)
                .await?;
            client.delete_onelake_file(src_ws, src_id, src_path).await?;
            serde_json::json!({
                "source": src_path,
                "destination": dst_path,
                "status": "moved",
                "method": "copy_delete"
            })
        };

        output::render_object(cli, &obj, "status");
        return Ok(());
    }

    // Multiple files: use rename for same-item, copy+delete for cross-item
    let concurrency = parallel::default_concurrency();
    eprintln!(
        "  Moving {} files with concurrency={concurrency}...",
        matched_files.len()
    );

    let copy_tasks: Vec<(String, String)> = matched_files
        .iter()
        .map(|src| {
            let filename = src.rsplit('/').next().unwrap_or(src).to_string();
            let dest = format!("{dst_path}/{filename}");
            (src.clone(), dest)
        })
        .collect();
    let item_names: Vec<String> = matched_files.clone();

    let is_same_item = src_ws == dst_ws && src_id == dst_id;

    if is_same_item {
        // Same item: use atomic rename for each file (no copy needed)
        let src_ws_arc: Arc<str> = Arc::from(src_ws);
        let src_id_arc: Arc<str> = Arc::from(src_id);
        let client_clone = client.clone();

        let results = parallel::execute_parallel(copy_tasks, concurrency, move |(src, dest)| {
            let client = client_clone.clone();
            let ws = Arc::clone(&src_ws_arc);
            let id = Arc::clone(&src_id_arc);
            async move {
                client.move_onelake_file(&ws, &id, &src, &dest).await?;
                Ok(())
            }
        })
        .await;

        let summary = BatchSummary::from_results(&results, &item_names);
        return render_batch_result(cli, &summary, "moved");
    }

    // Cross-item: copy in parallel, then delete sources on success
    let src_ws_arc: Arc<str> = Arc::from(src_ws);
    let src_id_arc: Arc<str> = Arc::from(src_id);
    let dst_ws_arc: Arc<str> = Arc::from(dst_ws);
    let dst_id_arc: Arc<str> = Arc::from(dst_id);
    let client_clone = client.clone();

    let results = parallel::execute_parallel(copy_tasks, concurrency, move |(src, dest)| {
        let client = client_clone.clone();
        let sw = Arc::clone(&src_ws_arc);
        let si = Arc::clone(&src_id_arc);
        let dw = Arc::clone(&dst_ws_arc);
        let di = Arc::clone(&dst_id_arc);
        async move {
            client
                .copy_onelake_file(&sw, &si, &src, &dw, &di, &dest)
                .await?;
            Ok(())
        }
    })
    .await;

    let summary = BatchSummary::from_results(&results, &item_names);

    if !summary.all_succeeded() {
        return render_batch_result(cli, &summary, "move_failed");
    }

    // All copies succeeded — now delete sources in parallel
    let src_ws_arc: Arc<str> = Arc::from(src_ws);
    let src_id_arc: Arc<str> = Arc::from(src_id);
    let client_clone = client.clone();

    let delete_results =
        parallel::execute_parallel(matched_files.clone(), concurrency, move |src| {
            let client = client_clone.clone();
            let sw = Arc::clone(&src_ws_arc);
            let si = Arc::clone(&src_id_arc);
            async move {
                client.delete_onelake_file(&sw, &si, &src).await?;
                Ok(())
            }
        })
        .await;

    let delete_summary = BatchSummary::from_results(&delete_results, &item_names);
    if !delete_summary.all_succeeded() {
        eprintln!(
            "  Warning: {} source files could not be deleted after successful copy",
            delete_summary.failed
        );
    }

    render_batch_result(cli, &summary, "moved")
}

#[cfg(test)]
mod tests {
    use super::validate_delete_directory_path;

    #[test]
    fn delete_directory_path_rejects_root_variants() {
        for p in ["", "   ", "/", "//", ".", "..", " / "] {
            assert!(
                validate_delete_directory_path(p).is_err(),
                "path {p:?} should be rejected as root"
            );
        }
    }

    #[test]
    fn delete_directory_path_allows_concrete_dirs() {
        for p in [
            "Files/staging",
            "Tables/foo",
            "Files",
            "Tables",
            "Files/a/b",
        ] {
            assert!(
                validate_delete_directory_path(p).is_ok(),
                "path {p:?} should be allowed"
            );
        }
    }
}
