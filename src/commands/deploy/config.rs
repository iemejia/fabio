//! Configuration file support for deployment.
//!
//! Supports both JSON and YAML formats (auto-detected by file extension).
//! Provides per-environment workspace mapping, filtering, and option defaults.
//!
//! ```yaml
//! # deploy-config.yml
//! source: "./workspace"
//! parameters: "./parameters.json"
//!
//! environments:
//!   dev:
//!     workspace: "Dev-Workspace-Name"
//!   prod:
//!     workspace: "aaaabbbb-cccc-dddd-eeee-ffffffffffff"
//!
//! filters:
//!   item_types: [Notebook, DataPipeline]
//!   exclude_regex: "^DEBUG_.*"
//!   include_folders: ["/ETL", "/Reports"]
//!   include_items: ["MyNotebook.Notebook"]
//!   shortcut_exclude_regex: "^temp_"
//!
//! options:
//!   delete_orphans: false
//!   hard_delete: false
//!   no_folders: false
//!   no_workspace_id_replace: false
//!   no_post_hooks: false
//!   concurrency: 8
//!   fail_fast: false
//!   allow_delete_types: [Lakehouse, Warehouse]
//! ```

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::errors::{ErrorCode, FabioError};

/// Top-level deploy configuration file.
#[derive(Debug, Clone, Deserialize)]
pub struct DeployConfig {
    /// Source directory (relative to config file location).
    pub source: Option<String>,

    /// Parameters file path (relative to config file location).
    pub parameters: Option<String>,

    /// Per-environment configuration.
    #[serde(default)]
    pub environments: HashMap<String, EnvironmentConfig>,

    /// Filtering options.
    #[serde(default)]
    pub filters: Option<FilterConfig>,

    /// Default options for plan/apply.
    #[serde(default)]
    pub options: Option<OptionsConfig>,
}

/// Per-environment configuration block.
#[derive(Debug, Clone, Deserialize)]
pub struct EnvironmentConfig {
    /// Target workspace ID or name for this environment.
    pub workspace: String,
}

/// Filtering configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FilterConfig {
    /// Only deploy specific item types (case-insensitive).
    #[serde(default)]
    pub item_types: Option<Vec<String>>,

    /// Exclude items whose display name matches this regex.
    #[serde(default)]
    pub exclude_regex: Option<String>,

    /// Only include items in these folder paths.
    #[serde(default)]
    pub include_folders: Option<Vec<String>>,

    /// Exclude items in these folder paths (mutually exclusive with `include_folders`).
    #[serde(default)]
    pub exclude_folders: Option<Vec<String>>,

    /// Only include specific items by "name.Type" format.
    #[serde(default)]
    pub include_items: Option<Vec<String>>,

    /// Exclude shortcuts matching this regex during reconciliation.
    #[serde(default)]
    pub shortcut_exclude_regex: Option<String>,

    /// Only deploy items changed since this git ref (e.g., "HEAD~1", "main").
    #[serde(default)]
    pub git_diff: Option<String>,
}

/// Default option values for plan/apply.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OptionsConfig {
    /// Include delete actions for items not in source.
    #[serde(default)]
    pub delete_orphans: Option<bool>,

    /// Use permanent delete (skip recycle bin).
    #[serde(default)]
    pub hard_delete: Option<bool>,

    /// Skip folder management.
    #[serde(default)]
    pub no_folders: Option<bool>,

    /// Skip automatic workspace ID replacement.
    #[serde(default)]
    pub no_workspace_id_replace: Option<bool>,

    /// Skip post-deploy hooks.
    #[serde(default)]
    pub no_post_hooks: Option<bool>,

    /// Max parallel operations per type batch.
    #[serde(default)]
    pub concurrency: Option<usize>,

    /// Stop on first failure.
    #[serde(default)]
    pub fail_fast: Option<bool>,

    /// Item types that may be deleted (when `delete_orphans` is true).
    /// Protected types (Lakehouse, Warehouse, etc.) require explicit listing here.
    #[serde(default)]
    pub allow_delete_types: Option<Vec<String>>,
}

/// Resolved deploy configuration with absolute paths.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    /// Absolute source directory path.
    pub source: Option<PathBuf>,

    /// Absolute parameters file path.
    pub parameters: Option<PathBuf>,

    /// Workspace ID or name for the selected environment.
    pub workspace: Option<String>,

    /// Filter configuration.
    pub filters: FilterConfig,

    /// Options configuration.
    pub options: OptionsConfig,
}

/// Parse a deploy config file (JSON or YAML, auto-detected by extension).
pub fn parse_config(path: &Path) -> Result<DeployConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let ext = path
        .extension()
        .map_or_default(|e| e.to_string_lossy().to_lowercase());

    let config: DeployConfig = match ext.as_str() {
        "yml" | "yaml" => serde_yaml::from_str(&content)
            .with_context(|| format!("Invalid YAML in config file: {}", path.display()))?,
        "json" | "jsonc" => serde_json::from_str(&content)
            .with_context(|| format!("Invalid JSON in config file: {}", path.display()))?,
        _ => {
            // Try JSON first, then YAML
            serde_json::from_str(&content).or_else(|_| {
                serde_yaml::from_str(&content).with_context(|| {
                    format!(
                        "Config file is neither valid JSON nor YAML: {}",
                        path.display()
                    )
                })
            })?
        }
    };

    // Validate mutual exclusivity of include_folders and exclude_folders
    if let Some(ref filters) = config.filters
        && filters.include_folders.is_some()
        && filters.exclude_folders.is_some()
    {
        return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Config file error: 'include_folders' and 'exclude_folders' are mutually exclusive",
                "Use only one: 'include_folders' to deploy specific folders, or 'exclude_folders' to skip specific folders.",
            )
            .into());
    }

    Ok(config)
}

/// Resolve a config file into absolute paths and select the environment.
pub fn resolve_config(
    config: &DeployConfig,
    config_path: &Path,
    env: &str,
) -> Result<ResolvedConfig> {
    let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

    // Resolve workspace from environment block
    let workspace = config
        .environments
        .get(env)
        .map(|e| e.workspace.clone())
        .or_else(|| {
            // Case-insensitive fallback
            config
                .environments
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(env))
                .map(|(_, v)| v.workspace.clone())
        });

    if workspace.is_none() && !config.environments.is_empty() {
        let available: Vec<&str> = config.environments.keys().map(String::as_str).collect();
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Environment \"{env}\" not found in config. Available: {}",
                available.join(", ")
            ),
            format!(
                "Use one of the defined environments: --env {}",
                available.first().unwrap_or(&"<name>")
            ),
        )
        .into());
    }

    // Resolve source path (relative to config file)
    let source = config.source.as_ref().map(|s| {
        let p = Path::new(s);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            config_dir.join(p)
        }
    });

    // Resolve parameters path (relative to config file)
    let parameters = config.parameters.as_ref().map(|s| {
        let p = Path::new(s);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            config_dir.join(p)
        }
    });

    Ok(ResolvedConfig {
        source,
        parameters,
        workspace,
        filters: config.filters.clone().unwrap_or_default(),
        options: config.options.clone().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_json_config() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("deploy-config.json");
        fs::write(
            &config_path,
            r#"{
                "source": "./workspace",
                "parameters": "./params.json",
                "environments": {
                    "dev": { "workspace": "dev-ws-name" },
                    "prod": { "workspace": "aaaabbbb-cccc-dddd-eeee-ffffffffffff" }
                },
                "filters": {
                    "item_types": ["Notebook", "DataPipeline"],
                    "exclude_regex": "^DEBUG_"
                },
                "options": {
                    "concurrency": 4,
                    "delete_orphans": true,
                    "allow_delete_types": ["Lakehouse"]
                }
            }"#,
        )
        .unwrap();

        let config = parse_config(&config_path).unwrap();
        assert_eq!(config.environments.len(), 2);
        assert_eq!(config.environments["dev"].workspace, "dev-ws-name");
        assert_eq!(config.source.as_deref(), Some("./workspace"));

        let filters = config.filters.unwrap();
        assert_eq!(filters.item_types.as_ref().unwrap().len(), 2);
        assert_eq!(filters.exclude_regex.as_deref(), Some("^DEBUG_"));

        let options = config.options.unwrap();
        assert_eq!(options.concurrency, Some(4));
        assert_eq!(options.delete_orphans, Some(true));
        assert_eq!(options.allow_delete_types.as_ref().unwrap(), &["Lakehouse"]);
    }

    #[test]
    fn test_parse_yaml_config() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("deploy-config.yml");
        fs::write(
            &config_path,
            r#"
source: "./workspace"
parameters: "./params.json"
environments:
  dev:
    workspace: dev-ws-name
  prod:
    workspace: aaaabbbb-cccc-dddd-eeee-ffffffffffff
filters:
  item_types:
    - Notebook
    - DataPipeline
options:
  concurrency: 8
"#,
        )
        .unwrap();

        let config = parse_config(&config_path).unwrap();
        assert_eq!(config.environments.len(), 2);
        assert_eq!(
            config.environments["prod"].workspace,
            "aaaabbbb-cccc-dddd-eeee-ffffffffffff"
        );

        let filters = config.filters.unwrap();
        assert_eq!(filters.item_types.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_mutual_exclusivity_folders() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("bad.json");
        fs::write(
            &config_path,
            r#"{
                "environments": {},
                "filters": {
                    "include_folders": ["/a"],
                    "exclude_folders": ["/b"]
                }
            }"#,
        )
        .unwrap();

        let result = parse_config(&config_path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("mutually exclusive")
        );
    }

    #[test]
    fn test_resolve_config_paths() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("deploy.json");
        fs::write(
            &config_path,
            r#"{
                "source": "./src",
                "parameters": "../params.json",
                "environments": {
                    "dev": { "workspace": "my-dev-ws" }
                }
            }"#,
        )
        .unwrap();

        let config = parse_config(&config_path).unwrap();
        let resolved = resolve_config(&config, &config_path, "dev").unwrap();

        assert_eq!(resolved.workspace.as_deref(), Some("my-dev-ws"));
        assert!(resolved.source.unwrap().ends_with("src"));
        assert!(
            resolved
                .parameters
                .unwrap()
                .to_string_lossy()
                .contains("params.json")
        );
    }

    #[test]
    fn test_resolve_config_missing_env() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("deploy.json");
        fs::write(
            &config_path,
            r#"{
                "environments": {
                    "dev": { "workspace": "ws" }
                }
            }"#,
        )
        .unwrap();

        let config = parse_config(&config_path).unwrap();
        let result = resolve_config(&config, &config_path, "prod");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_empty_config() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("minimal.json");
        fs::write(&config_path, r#"{ "environments": {} }"#).unwrap();

        let config = parse_config(&config_path).unwrap();
        assert!(config.source.is_none());
        assert!(config.parameters.is_none());
        assert!(config.filters.is_none());
        assert!(config.options.is_none());
    }
}
