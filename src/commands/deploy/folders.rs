//! Workspace folder management for deployments.
//!
//! Infers folder hierarchy from the source directory structure and reconciles
//! with the deployed workspace folders (create, move items, delete orphans).

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::Cli;
use crate::client::FabricClient;

/// A folder discovered from the source directory structure.
#[derive(Debug, Clone)]
pub struct SourceFolder {
    /// Display name of the folder (last path segment).
    pub display_name: String,
    /// Full folder path from workspace root (e.g., "/ETL/Bronze").
    pub path: String,
    /// Parent folder path (e.g., "/ETL"), or empty string for root-level folders.
    pub parent_path: String,
    /// Depth level (1 = direct child of root, 2 = grandchild, etc.).
    pub depth: usize,
}

/// A folder deployed in the workspace.
#[derive(Debug, Clone)]
pub struct DeployedFolder {
    /// Folder ID from the API.
    pub id: String,
    /// Display name.
    pub display_name: String,
    /// Parent folder ID (None for root-level folders).
    pub parent_id: Option<String>,
}

/// Plan for folder operations during deployment.
#[derive(Debug, Clone, Default)]
pub struct FolderPlan {
    /// Folders to create (in depth-first order so parents are created first).
    pub to_create: Vec<SourceFolder>,
    /// Items that need to be moved to a different folder: (`item_id`, `target_folder_path`).
    pub to_move: Vec<(String, String)>,
    /// Folders to delete (deepest-first, only when `--delete-orphans` is active).
    pub to_delete: Vec<String>,
    /// Map from folder path → deployed folder ID (after creation).
    pub folder_ids: HashMap<String, String>,
}

/// Discover folder hierarchy from the source directory structure.
///
/// A directory is a "folder" if:
/// 1. It does NOT contain a `.platform` file (those are item directories)
/// 2. It contains at least one descendant that has a `.platform` file
/// 3. It is not named `.children` (KQL database internal structure)
pub fn discover_source_folders(source_dir: &Path) -> Result<Vec<SourceFolder>> {
    let mut folders = Vec::new();
    discover_folders_recursive(source_dir, source_dir, &mut folders)?;

    // Sort by depth (parents first)
    folders.sort_by_key(|f| f.depth);
    Ok(folders)
}

fn discover_folders_recursive(
    root: &Path,
    current: &Path,
    folders: &mut Vec<SourceFolder>,
) -> Result<bool> {
    // Check if this directory is an item directory
    if current.join(".platform").exists() {
        return Ok(true); // This is an item, not a folder
    }

    let dir_name = current
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Skip .children directories (KQL database internal)
    if dir_name == ".children" {
        return Ok(false);
    }

    let mut has_items = false;

    let entries = std::fs::read_dir(current)
        .with_context(|| format!("Failed to read directory: {}", current.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let child_has_items = discover_folders_recursive(root, &path, folders)?;
            if child_has_items {
                has_items = true;
            }
        }
    }

    // Only add as folder if it contains item descendants AND is not the root
    if has_items && current != root {
        let relative = current
            .strip_prefix(root)
            .unwrap_or(current)
            .to_string_lossy()
            .replace('\\', "/");
        let path = format!("/{relative}");
        let depth = path.matches('/').count();

        let parent_path = path.rfind('/').map_or_else(String::new, |last_slash| {
            if last_slash == 0 {
                String::new() // root-level folder
            } else {
                path[..last_slash].to_owned()
            }
        });

        folders.push(SourceFolder {
            display_name: dir_name,
            path,
            parent_path,
            depth,
        });
    }

    Ok(has_items)
}

/// Determine the folder path for an item based on its position in the source directory.
///
/// Returns the folder path (e.g., "/ETL/Bronze") or empty string if at root level.
pub fn item_folder_path(source_dir: &Path, item_dir: &Path) -> String {
    let parent = item_dir.parent().unwrap_or(source_dir);
    if parent == source_dir {
        return String::new();
    }

    let relative = parent
        .strip_prefix(source_dir)
        .unwrap_or(parent)
        .to_string_lossy()
        .replace('\\', "/");

    format!("/{relative}")
}

/// Fetch deployed folders from a workspace.
pub async fn fetch_deployed_folders(
    client: &FabricClient,
    workspace_id: &str,
) -> Result<HashMap<String, DeployedFolder>> {
    let url = format!("/workspaces/{workspace_id}/folders");
    let resp = client.get(&url).await;

    match resp {
        Ok(body) => {
            let folders_array = body
                .get("value")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut result = HashMap::new();
            for folder in folders_array {
                let id = folder
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let display_name = folder
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let parent_id = folder
                    .get("parentFolderId")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);

                result.insert(
                    id.clone(),
                    DeployedFolder {
                        id,
                        display_name,
                        parent_id,
                    },
                );
            }
            Ok(result)
        }
        Err(e) => {
            // Folders API might not be available on all workspaces
            // Degrade gracefully
            eprintln!("[deploy] warning: could not fetch workspace folders: {e}");
            Ok(HashMap::new())
        }
    }
}

/// Build a path → ID lookup from deployed folders.
/// Reconstructs full paths by walking the parent chain.
pub fn build_folder_path_map(
    deployed: &HashMap<String, DeployedFolder>,
) -> HashMap<String, String> {
    let mut path_map: HashMap<String, String> = HashMap::new();

    for folder in deployed.values() {
        let path = reconstruct_folder_path(deployed, &folder.id);
        path_map.insert(path, folder.id.clone());
    }

    path_map
}

/// Reconstruct the full path for a folder by walking its parent chain.
fn reconstruct_folder_path(deployed: &HashMap<String, DeployedFolder>, folder_id: &str) -> String {
    let mut segments = Vec::new();
    let mut current_id = Some(folder_id.to_owned());

    while let Some(ref id) = current_id {
        if let Some(folder) = deployed.get(id) {
            segments.push(folder.display_name.clone());
            current_id = folder.parent_id.clone();
        } else {
            break;
        }
    }

    segments.reverse();
    format!("/{}", segments.join("/"))
}

/// Create a folder in the workspace, returning its ID.
pub async fn create_folder(
    client: &FabricClient,
    workspace_id: &str,
    display_name: &str,
    parent_folder_id: Option<&str>,
) -> Result<String> {
    let url = format!("/workspaces/{workspace_id}/folders");
    let mut body = json!({ "displayName": display_name });

    if let Some(pid) = parent_folder_id {
        body.as_object_mut()
            .unwrap()
            .insert("parentFolderId".to_owned(), serde_json::Value::from(pid));
    }

    let resp = client.post(&url, &body, false).await?;

    let folder_id = resp
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();

    Ok(folder_id)
}

/// Move an item to a folder (or to workspace root if `folder_id` is None).
pub async fn move_item_to_folder(
    client: &FabricClient,
    workspace_id: &str,
    item_id: &str,
    folder_id: Option<&str>,
) -> Result<()> {
    let url = format!("/workspaces/{workspace_id}/items/{item_id}/move");
    let body = folder_id.map_or_else(|| json!({}), |fid| json!({ "targetFolderId": fid }));

    let _ = client.post(&url, &body, false).await?;
    Ok(())
}

/// Delete a folder from the workspace.
pub async fn delete_folder(
    client: &FabricClient,
    workspace_id: &str,
    folder_id: &str,
) -> Result<()> {
    let url = format!("/workspaces/{workspace_id}/folders/{folder_id}");
    let _ = client.delete(&url).await?;
    Ok(())
}

/// Execute the folder plan: create folders, move items, and optionally delete orphans.
pub async fn execute_folder_plan(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    source_folders: &[SourceFolder],
    folder_path_map: &mut HashMap<String, String>,
    delete_orphans: bool,
    deployed_folders: &HashMap<String, DeployedFolder>,
) -> Result<FolderPlan> {
    let mut plan = FolderPlan::default();

    // Create folders that don't exist (depth-first order, parents already sorted first)
    for folder in source_folders {
        if folder_path_map.contains_key(&folder.path) {
            continue; // Already exists
        }

        if cli.dry_run {
            plan.to_create.push(folder.clone());
            continue;
        }

        // Find parent ID
        let parent_id = if folder.parent_path.is_empty() {
            None
        } else {
            folder_path_map.get(&folder.parent_path).map(String::as_str)
        };

        if !cli.quiet {
            eprintln!("[deploy] creating folder \"{}\"", folder.path);
        }

        match create_folder(client, workspace_id, &folder.display_name, parent_id).await {
            Ok(id) => {
                folder_path_map.insert(folder.path.clone(), id.clone());
                plan.folder_ids.insert(folder.path.clone(), id);
                plan.to_create.push(folder.clone());
            }
            Err(e) => {
                eprintln!(
                    "[deploy] warning: failed to create folder \"{}\": {e}",
                    folder.path
                );
            }
        }
    }

    // Delete orphaned folders (only if --delete-orphans)
    if delete_orphans {
        let source_paths: HashSet<&str> = source_folders.iter().map(|f| f.path.as_str()).collect();
        let deployed_path_map = build_folder_path_map(deployed_folders);

        // Find deployed folders NOT in source — sort deepest first for deletion
        let mut orphans: Vec<(String, String)> = deployed_path_map
            .iter()
            .filter(|(path, _)| !source_paths.contains(path.as_str()))
            .map(|(path, id)| (path.clone(), id.clone()))
            .collect();

        // Sort by depth descending (delete children before parents)
        orphans.sort_by_key(|b| std::cmp::Reverse(b.0.matches('/').count()));

        for (path, id) in &orphans {
            if cli.dry_run {
                plan.to_delete.push(id.clone());
                continue;
            }

            if !cli.quiet {
                eprintln!("[deploy] deleting orphaned folder \"{path}\"");
            }

            if let Err(e) = delete_folder(client, workspace_id, id).await {
                eprintln!("[deploy] warning: failed to delete folder \"{path}\": {e}");
            } else {
                plan.to_delete.push(id.clone());
            }
        }
    }

    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_discover_source_folders_flat() {
        let dir = TempDir::new().unwrap();

        // Create flat items (no folders)
        let nb_dir = dir.path().join("MyNb.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        fs::write(nb_dir.join(".platform"), "{}").unwrap();

        let folders = discover_source_folders(dir.path()).unwrap();
        assert!(folders.is_empty(), "Flat structure should have no folders");
    }

    #[test]
    fn test_discover_source_folders_nested() {
        let dir = TempDir::new().unwrap();

        // Create nested structure: ETL/Bronze/MyNb.Notebook
        let nb_dir = dir.path().join("ETL").join("Bronze").join("MyNb.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        fs::write(nb_dir.join(".platform"), "{}").unwrap();

        let folders = discover_source_folders(dir.path()).unwrap();
        assert_eq!(folders.len(), 2);

        // First should be ETL (depth 1), then Bronze (depth 2)
        assert_eq!(folders[0].display_name, "ETL");
        assert_eq!(folders[0].path, "/ETL");
        assert_eq!(folders[0].parent_path, "");
        assert_eq!(folders[0].depth, 1);

        assert_eq!(folders[1].display_name, "Bronze");
        assert_eq!(folders[1].path, "/ETL/Bronze");
        assert_eq!(folders[1].parent_path, "/ETL");
        assert_eq!(folders[1].depth, 2);
    }

    #[test]
    fn test_discover_source_folders_skips_children() {
        let dir = TempDir::new().unwrap();

        // .children directory should be skipped
        let child_dir = dir.path().join(".children").join("MyDB.KQLDatabase");
        fs::create_dir_all(&child_dir).unwrap();
        fs::write(child_dir.join(".platform"), "{}").unwrap();

        let folders = discover_source_folders(dir.path()).unwrap();
        assert!(folders.is_empty());
    }

    #[test]
    fn test_item_folder_path_at_root() {
        let source = Path::new("/workspace");
        let item = Path::new("/workspace/MyNb.Notebook");
        assert_eq!(item_folder_path(source, item), "");
    }

    #[test]
    fn test_item_folder_path_nested() {
        let source = Path::new("/workspace");
        let item = Path::new("/workspace/ETL/Bronze/MyNb.Notebook");
        assert_eq!(item_folder_path(source, item), "/ETL/Bronze");
    }

    #[test]
    fn test_reconstruct_folder_path() {
        let mut deployed = HashMap::new();
        deployed.insert(
            "id-root".to_owned(),
            DeployedFolder {
                id: "id-root".to_owned(),
                display_name: "ETL".to_owned(),
                parent_id: None,
            },
        );
        deployed.insert(
            "id-child".to_owned(),
            DeployedFolder {
                id: "id-child".to_owned(),
                display_name: "Bronze".to_owned(),
                parent_id: Some("id-root".to_owned()),
            },
        );

        let path = reconstruct_folder_path(&deployed, "id-child");
        assert_eq!(path, "/ETL/Bronze");
    }
}
