use std::collections::HashMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::errors::{ErrorCode, FabioError, HintType};

/// Governance metadata stored in `governance.metadata.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceMetadata {
    /// Sensitivity label ID to apply at creation time.
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "sensitivityLabel",
        default
    )]
    pub sensitivity_label: Option<SensitivityLabelRef>,
    /// Tags to apply post-creation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagRef>,
}

/// Reference to a sensitivity label (just the ID).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitivityLabelRef {
    pub id: String,
}

/// Reference to a tag (ID + display name for readability).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagRef {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

/// A Lakehouse's SQL analytics endpoint identity, captured on export
/// (`sqlendpoint.metadata.json`). Deploy uses it to map the SOURCE endpoint
/// id + server FQDN to the newly-provisioned TARGET endpoint, so a Direct Lake
/// `SemanticModel`'s `Sql.Database("<server>","<endpointId>")` connection is
/// rewired to the deployed lakehouse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlEndpointRef {
    /// The SQL analytics endpoint item id.
    pub id: String,
    /// The endpoint server FQDN (the `connectionString`).
    pub server: String,
}

/// Metadata from a `.platform` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformMetadata {
    pub item_type: String,
    pub display_name: String,
    pub logical_id: Option<String>,
    pub description: Option<String>,
    /// Definition format required by the Fabric API (e.g., "ipynb" for Notebooks).
    pub definition_format: Option<String>,
    /// Sensitivity label UUID carried in `.platform` `metadata.sensitivityLabelId`
    /// (git-integration / fabric-cicd repos use platformProperties 2.1.0). Applied
    /// on create when no governance sidecar label is present.
    pub sensitivity_label_id: Option<String>,
    /// Optional creation payload embedded in `.platform` metadata (fabric-cicd compatible).
    #[serde(skip)]
    pub platform_creation_payload: Option<serde_json::Value>,
}

/// A single definition part (file) belonging to an item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefinitionPart {
    /// Relative path within the item directory (e.g., "notebook-content.py").
    pub path: String,
    /// Base64-encoded payload.
    pub payload: String,
    /// Payload type (always "`InlineBase64`" for Fabric API).
    pub payload_type: String,
}

/// A fully parsed item from a `.platform` source directory.
#[derive(Debug, Clone)]
pub struct SourceItem {
    /// Metadata from `.platform`.
    pub metadata: PlatformMetadata,
    /// Definition parts (files excluding `.platform` and `creationPayload.json`).
    pub parts: Vec<DefinitionPart>,
    /// SHA256 hash of all definition parts (for change detection).
    pub content_hash: String,
    /// Optional creation payload (from `creationPayload.json`).
    /// Included in the creation request body as the `creationPayload` field.
    pub creation_payload: Option<serde_json::Value>,
    /// Optional shortcut definitions (from `shortcuts.metadata.json`).
    /// Only relevant for Lakehouse items. Contains the JSON array of shortcuts.
    pub shortcuts: Option<Vec<serde_json::Value>>,
    /// Optional governance metadata (sensitivity label + tags).
    /// Read from `governance.metadata.json` if it exists.
    pub governance: Option<GovernanceMetadata>,
    /// Optional job schedules (from `schedules.metadata.json`).
    /// Contains an array of schedule configurations to apply after deployment.
    pub schedules: Option<Vec<serde_json::Value>>,
    /// Workspace folder path (e.g., "/ETL/Bronze"). Empty string means root level.
    pub folder_path: String,
    /// Path to the item directory on disk.
    pub source_path: PathBuf,
}

/// All items discovered from a source directory.
#[derive(Debug)]
pub struct SourceWorkspace {
    pub items: Vec<SourceItem>,
    /// Map from `logical_id` → index into items vec.
    #[allow(dead_code)]
    pub logical_id_index: HashMap<String, usize>,
    /// Map from (type, name) → index into items vec.
    #[allow(dead_code)]
    pub type_name_index: HashMap<(String, String), usize>,
}

/// Parse a `.platform` JSON file and extract metadata.
fn parse_platform_file(path: &Path) -> Result<PlatformMetadata> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read .platform file: {}", path.display()))?;

    let parsed: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("Invalid JSON in .platform file: {}", path.display()))?;

    let metadata = parsed
        .get("metadata")
        .with_context(|| format!("Missing 'metadata' in .platform: {}", path.display()))?;

    let item_type = metadata
        .get("type")
        .and_then(|v| v.as_str())
        .with_context(|| format!("Missing 'metadata.type' in .platform: {}", path.display()))?
        .to_owned();

    let display_name = metadata
        .get("displayName")
        .and_then(|v| v.as_str())
        .with_context(|| {
            format!(
                "Missing 'metadata.displayName' in .platform: {}",
                path.display()
            )
        })?
        .to_owned();

    let description = metadata
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    let logical_id = parsed
        .get("config")
        .and_then(|c| c.get("logicalId"))
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    let definition_format = parsed
        .get("config")
        .and_then(|c| c.get("definitionFormat"))
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    // Fallback format detection: if definitionFormat is not in .platform config,
    // use hardcoded defaults for known item types (matches fabric-cicd behavior)
    let definition_format = definition_format.or_else(|| match item_type.as_str() {
        "SparkJobDefinition" => Some("SparkJobDefinitionV2".to_owned()),
        _ => None,
    });

    // Check for creationPayload inside .platform metadata (fabric-cicd compatible)
    let platform_creation_payload = parsed
        .get("metadata")
        .and_then(|m| m.get("creationPayload"))
        .cloned();

    // Sensitivity label carried in .platform metadata (platformProperties 2.1.0,
    // used by Fabric Git Integration / fabric-cicd). Without this, an imported
    // label would be silently dropped.
    let sensitivity_label_id = metadata
        .get("sensitivityLabelId")
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    Ok(PlatformMetadata {
        item_type,
        display_name,
        logical_id,
        description,
        definition_format,
        sensitivity_label_id,
        platform_creation_payload,
    })
}

/// Infer `.platform` metadata for a folder that has NO `.platform` file.
///
/// Power BI Desktop saves a PBIP project as plain folders named
/// `<name>.Report` / `<name>.SemanticModel` WITHOUT the Git-integration
/// `.platform` sidecar (that file is only added by Fabric Git Integration or
/// fabric-cicd). To let such a raw Desktop PBIP folder be deployed directly,
/// we synthesize the metadata from the folder-name suffix, but ONLY when the
/// suffix maps to a known type AND the type's required entry-point definition
/// file is present on disk (so we never misclassify an arbitrary folder).
///
/// Returns `None` when the folder is not a synthesizable item directory (the
/// caller then recurses into it as a plain folder).
fn synthesize_platform_metadata(path: &Path, dir_name: &str) -> Option<PlatformMetadata> {
    // Suffix → (Fabric item type, required entry-point definition file).
    // Limited to the two PBIP item types; Git-integration exports of other
    // types always carry a real `.platform`, so they never reach here.
    let (base, item_type, entry_file) = match dir_name.rsplit_once('.') {
        Some((base, "Report")) => (base, "Report", "definition.pbir"),
        Some((base, "SemanticModel")) => (base, "SemanticModel", "definition.pbism"),
        _ => return None,
    };

    if base.is_empty() || !path.join(entry_file).exists() {
        return None;
    }

    Some(PlatformMetadata {
        item_type: item_type.to_owned(),
        display_name: base.to_owned(),
        // No `.platform` means no authored logicalId; rename/rebind resolution
        // falls back to (type, name) matching, which is correct for PBIP.
        logical_id: None,
        description: None,
        definition_format: None,
        sensitivity_label_id: None,
        platform_creation_payload: None,
    })
}

/// Compute a deterministic content hash over all definition parts.
///
/// The hash is computed over sorted (path, payload) pairs to ensure
/// consistency regardless of filesystem ordering.
/// Format a SHA-256 digest as `"sha256:<64 hex chars>"`.
#[inline]
pub(super) fn sha256_hex(hash: &[u8]) -> String {
    let mut out = String::with_capacity(7 + 64);
    out.push_str("sha256:");
    for b in hash {
        let _ = write!(out, "{b:02x}");
    }
    out
}

pub(super) fn compute_content_hash(parts: &[DefinitionPart]) -> String {
    compute_content_hash_from_pairs(parts.iter().map(|p| (p.path.as_str(), p.payload.as_str())))
}

/// Same as `compute_content_hash` but accepts references (for filtered subsets).
pub(super) fn compute_content_hash_refs(parts: &[&DefinitionPart]) -> String {
    compute_content_hash_from_pairs(parts.iter().map(|p| (p.path.as_str(), p.payload.as_str())))
}

/// Compute content hash excluding `.platform` (API rewrites logicalId in it).
pub(super) fn compute_content_hash_excluding_platform(parts: &[DefinitionPart]) -> String {
    compute_content_hash_from_pairs(
        parts
            .iter()
            .filter(|p| p.path != ".platform")
            .map(|p| (p.path.as_str(), p.payload.as_str())),
    )
}

/// Core hashing logic: sort (path, payload) pairs and SHA-256 them.
fn compute_content_hash_from_pairs<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut sorted: Vec<(&str, &str)> = pairs.collect();
    sorted.sort_by_key(|(path, _)| *path);

    let mut hasher = Sha256::new();
    for (path, payload) in sorted {
        hasher.update(path.as_bytes());
        hasher.update(b"\x00");
        hasher.update(payload.as_bytes());
        hasher.update(b"\x00");
    }

    sha256_hex(&hasher.finalize())
}

/// Parse a source directory containing Fabric item folders with `.platform` files.
///
/// Supports both flat and nested (folder) structures:
/// ```text
/// source_dir/
/// ├── MyNotebook.Notebook/         (root-level item)
/// │   ├── .platform
/// │   └── notebook-content.py
/// ├── ETL/                          (folder)
/// │   └── Transform.Notebook/
/// │       ├── .platform
/// │       └── notebook-content.py
/// ```
pub fn parse_source_directory(source_dir: &Path) -> Result<SourceWorkspace> {
    if !source_dir.is_dir() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Source directory does not exist: {}", source_dir.display()),
            "Check the path. Create a source directory: fabio deploy export --workspace <WS> --dir <DIR>",
        )
        .into());
    }

    let mut items = Vec::new();
    let mut logical_id_index = HashMap::new();
    let mut type_name_index = HashMap::new();

    // Recursively discover all item directories (those containing .platform)
    discover_items_recursive(source_dir, source_dir, &mut items)?;

    // Build indices
    for (idx, item) in items.iter().enumerate() {
        if let Some(ref lid) = item.metadata.logical_id {
            logical_id_index.insert(lid.clone(), idx);
        }
        type_name_index.insert(
            (
                item.metadata.item_type.clone(),
                item.metadata.display_name.clone(),
            ),
            idx,
        );
    }

    Ok(SourceWorkspace {
        items,
        logical_id_index,
        type_name_index,
    })
}

/// Recursively discover item directories within the source tree.
fn discover_items_recursive(
    root: &Path,
    current: &Path,
    items: &mut Vec<SourceItem>,
) -> Result<()> {
    let entries = std::fs::read_dir(current)
        .with_context(|| format!("Failed to read source directory: {}", current.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let dir_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Skip hidden directories EXCEPT .children (which contains KQL databases)
        if dir_name.starts_with('.') && dir_name != ".children" {
            continue;
        }

        // .children/ is a container for child items (KQL Databases under Eventhouses)
        // — recurse into it to discover items, but don't treat it as a folder
        if dir_name == ".children" {
            discover_items_recursive(root, &path, items)?;
            continue;
        }

        let platform_path = path.join(".platform");
        if platform_path.exists() {
            // This is an item directory — parse it
            let item = parse_item_directory(root, &path)?;
            items.push(item);

            // Also check for .children/ subdirectory containing child items
            // (e.g., KQL Databases nested under Eventhouses)
            let children_dir = path.join(".children");
            if children_dir.is_dir() {
                discover_items_recursive(root, &children_dir, items)?;
            }
        } else if synthesize_platform_metadata(&path, &dir_name).is_some() {
            // A raw Power BI Desktop PBIP item folder (`<name>.Report` /
            // `<name>.SemanticModel`) with no `.platform` sidecar — treat it as
            // an item directory and synthesize its metadata during parsing.
            let item = parse_item_directory(root, &path)?;
            items.push(item);
        } else {
            // This is a folder directory — recurse into it
            discover_items_recursive(root, &path, items)?;
        }
    }

    Ok(())
}

/// Parse a single item directory into a `SourceItem`.
#[allow(clippy::too_many_lines)]
fn parse_item_directory(root: &Path, path: &Path) -> Result<SourceItem> {
    let platform_path = path.join(".platform");
    let mut metadata = if platform_path.exists() {
        parse_platform_file(&platform_path)?
    } else {
        // No `.platform` sidecar: synthesize from the folder-name suffix
        // (raw Power BI Desktop PBIP folder). Discovery already verified this
        // folder is synthesizable before calling us.
        let dir_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        synthesize_platform_metadata(path, &dir_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot determine item type for '{}': no .platform file and the \
                 folder name is not a recognized PBIP item (<name>.Report / <name>.SemanticModel)",
                path.display()
            )
        })?
    };

    // Post-process: for Notebooks without explicit definitionFormat,
    // detect format from the actual content file name
    if metadata.item_type.eq_ignore_ascii_case("Notebook")
        && metadata.definition_format.is_none()
        && path.join("notebook-content.ipynb").exists()
    {
        metadata.definition_format = Some("ipynb".to_owned());
        // .py files don't need a format specifier (server auto-detects)
    }

    // Read all definition parts (includes .platform for the API)
    let parts = read_definition_parts(path)?;

    // Content hash excludes .platform (the API modifies logicalId in .platform,
    // so including it would break idempotent skip detection)
    let hash_parts: Vec<&DefinitionPart> = parts.iter().filter(|p| p.path != ".platform").collect();
    let content_hash = compute_content_hash_refs(&hash_parts);

    // Read optional creationPayload.json
    let creation_payload_path = path.join("creationPayload.json");
    let creation_payload = if creation_payload_path.exists() {
        let content = std::fs::read_to_string(&creation_payload_path).with_context(|| {
            format!(
                "Failed to read creationPayload.json: {}",
                creation_payload_path.display()
            )
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&content).with_context(|| {
            format!(
                "Invalid JSON in creationPayload.json: {}",
                creation_payload_path.display()
            )
        })?;
        Some(parsed)
    } else {
        // Fallback: read creationPayload from .platform metadata (fabric-cicd compatible)
        metadata.platform_creation_payload.clone()
    };

    // For Lakehouse items: detect enableSchemas from lakehouse.metadata.json
    // (fabric-cicd compatible: checks for "defaultSchema" key presence)
    let creation_payload =
        if creation_payload.is_none() && metadata.item_type.eq_ignore_ascii_case("Lakehouse") {
            let lh_metadata_path = path.join("lakehouse.metadata.json");
            if lh_metadata_path.exists() {
                let content = std::fs::read_to_string(&lh_metadata_path).unwrap_or_default();
                if content.contains("defaultSchema") {
                    Some(serde_json::json!({"enableSchemas": true}))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            creation_payload
        };

    // Read optional shortcuts.metadata.json (for Lakehouse items)
    let shortcuts_path = path.join("shortcuts.metadata.json");
    let shortcuts = if shortcuts_path.exists() {
        let content = std::fs::read_to_string(&shortcuts_path).with_context(|| {
            format!(
                "Failed to read shortcuts.metadata.json: {}",
                shortcuts_path.display()
            )
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&content).with_context(|| {
            format!(
                "Invalid JSON in shortcuts.metadata.json: {}",
                shortcuts_path.display()
            )
        })?;
        match parsed {
            serde_json::Value::Array(arr) if !arr.is_empty() => Some(arr),
            _ => None,
        }
    } else {
        None
    };

    // Read optional governance.metadata.json (sensitivity labels + tags)
    let governance_path = path.join("governance.metadata.json");
    let governance = if governance_path.exists() {
        let content = std::fs::read_to_string(&governance_path).with_context(|| {
            format!(
                "Failed to read governance.metadata.json: {}",
                governance_path.display()
            )
        })?;
        let parsed: GovernanceMetadata = serde_json::from_str(&content).with_context(|| {
            format!(
                "Invalid JSON in governance.metadata.json: {}",
                governance_path.display()
            )
        })?;
        // Only keep if there's actually something to apply
        if parsed.sensitivity_label.is_some() || !parsed.tags.is_empty() {
            Some(parsed)
        } else {
            None
        }
    } else {
        None
    };

    // Read optional schedules.metadata.json (job schedules)
    let schedules_path = path.join("schedules.metadata.json");
    let schedules = if schedules_path.exists() {
        let content = std::fs::read_to_string(&schedules_path).with_context(|| {
            format!(
                "Failed to read schedules.metadata.json: {}",
                schedules_path.display()
            )
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&content).with_context(|| {
            format!(
                "Invalid JSON in schedules.metadata.json: {}",
                schedules_path.display()
            )
        })?;
        match parsed {
            serde_json::Value::Array(arr) if !arr.is_empty() => Some(arr),
            _ => None,
        }
    } else {
        None
    };

    // Read optional sqlendpoint.metadata.json (Lakehouse source SQL endpoint identity).
    // Read on-demand during apply from `source_path`; nothing to parse here.

    // Compute folder path from item's parent relative to root
    let folder_path = super::folders::item_folder_path(root, path);

    Ok(SourceItem {
        metadata,
        parts,
        content_hash,
        creation_payload,
        shortcuts,
        governance,
        schedules,
        folder_path,
        source_path: path.to_path_buf(),
    })
}

/// Read all definition files from an item directory (excluding `.platform`).
fn read_definition_parts(item_dir: &Path) -> Result<Vec<DefinitionPart>> {
    let mut parts = Vec::new();
    read_parts_recursive(item_dir, item_dir, &mut parts)?;
    Ok(parts)
}

/// Recursively read files from an item directory, building definition parts.
fn read_parts_recursive(
    base_dir: &Path,
    current_dir: &Path,
    parts: &mut Vec<DefinitionPart>,
) -> Result<()> {
    let entries = std::fs::read_dir(current_dir)
        .with_context(|| format!("Failed to read item directory: {}", current_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let dir_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Skip .pbi/ directories (local Power BI metadata, not part of definition)
            // Skip .children/ directories (handled as separate items in discovery)
            if dir_name == ".pbi" || dir_name == ".children" {
                continue;
            }

            read_parts_recursive(base_dir, &path, parts)?;
        } else {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Skip creationPayload.json, shortcuts.metadata.json, governance.metadata.json,
            // and schedules.metadata.json (not definition parts — handled separately)
            // NOTE: .platform IS included as a definition part (the Fabric API
            // uses it for metadata updates when ?updateMetadata=true is set)
            if file_name == "creationPayload.json"
                || file_name == "shortcuts.metadata.json"
                || file_name == "governance.metadata.json"
                || file_name == "schedules.metadata.json"
                || file_name == "sqlendpoint.metadata.json"
            {
                continue;
            }

            // Compute relative path from base_dir
            let rel_path = path
                .strip_prefix(base_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                // Normalize to forward slashes for Fabric API
                .replace('\\', "/");

            let content = std::fs::read(&path)
                .with_context(|| format!("Failed to read file: {}", path.display()))?;

            let encoded = BASE64.encode(&content);

            parts.push(DefinitionPart {
                path: rel_path,
                payload: encoded,
                payload_type: "InlineBase64".to_owned(),
            });
        }
    }

    Ok(())
}

/// Write a source workspace to disk in the standard `.platform` directory format.
///
/// Used by `deploy export` to create a local copy of a workspace.
pub fn write_source_directory(
    output_dir: &Path,
    items: &[(PlatformMetadata, Vec<DefinitionPart>)],
    overwrite: bool,
) -> Result<usize> {
    if output_dir.exists() && !overwrite {
        // Check if it's non-empty
        let has_entries = std::fs::read_dir(output_dir).is_ok_and(|mut rd| rd.next().is_some());
        if has_entries {
            return Err(FabioError::with_typed_hint(
                ErrorCode::InvalidInput,
                format!("Output directory is not empty: {}", output_dir.display()),
                "Use --overwrite to replace existing content.",
                HintType::SafetyBypass,
            )
            .into());
        }
    }

    std::fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "Failed to create output directory: {}",
            output_dir.display()
        )
    })?;

    let mut count = 0;

    for (metadata, parts) in items {
        let dir_name = format!("{}.{}", metadata.display_name, metadata.item_type);
        let item_dir = output_dir.join(&dir_name);
        std::fs::create_dir_all(&item_dir)?;

        // Write .platform file
        let platform_content = build_platform_json(metadata);
        std::fs::write(item_dir.join(".platform"), platform_content)?;

        // Write definition parts
        for part in parts {
            // Sanitize part path to prevent directory traversal from API responses.
            // Reject paths containing ".." or starting with "/" which could write
            // outside the item directory.
            if part.path.contains("..") || part.path.starts_with('/') || part.path.starts_with('\\')
            {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!(
                        "Refusing to write part with unsafe path '{}' in item '{}'",
                        part.path, metadata.display_name
                    ),
                    "Path contains directory traversal. Validate the source: fabio deploy validate --source <DIR>",
                )
                .into());
            }

            let part_path = item_dir.join(Path::new(&part.path));

            // Defense-in-depth: verify resolved path is inside item directory
            if !part_path.starts_with(&item_dir) {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!(
                        "Refusing to write part '{}' — resolved path escapes item directory '{}'",
                        part.path,
                        item_dir.display()
                    ),
                    "Path resolves outside the item directory. Validate the source: fabio deploy validate --source <DIR>",
                )
                .into());
            }

            // Create parent directories if needed
            if let Some(parent) = part_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let decoded = BASE64
                .decode(&part.payload)
                .with_context(|| format!("Failed to decode base64 for part: {}", part.path))?;

            std::fs::write(&part_path, decoded)?;
        }

        count += 1;
    }

    Ok(count)
}

/// Build the JSON content for a `.platform` file.
fn build_platform_json(metadata: &PlatformMetadata) -> String {
    let mut obj = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/gitIntegration/platformProperties/2.1.0/schema.json",
        "metadata": {
            "type": metadata.item_type,
            "displayName": metadata.display_name,
        },
        "config": {
            "version": "2.0",
        }
    });

    if let Some(ref desc) = metadata.description {
        obj["metadata"]["description"] = serde_json::Value::from(desc.as_str());
    }

    // sensitivityLabelId is a platformProperties 2.1.0 metadata field.
    if let Some(ref label_id) = metadata.sensitivity_label_id {
        obj["metadata"]["sensitivityLabelId"] = serde_json::Value::from(label_id.as_str());
    }

    if let Some(ref lid) = metadata.logical_id {
        obj["config"]["logicalId"] = serde_json::Value::from(lid.as_str());
    }

    if let Some(ref fmt) = metadata.definition_format {
        obj["config"]["definitionFormat"] = serde_json::Value::from(fmt.as_str());
    }

    serde_json::to_string_pretty(&obj).unwrap_or_default()
}

/// Get the git source metadata for the current directory (if in a git repo).
#[derive(Debug, Clone, Serialize)]
pub struct SourceGitMetadata {
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
}

/// Try to extract git metadata from the source directory.
pub fn get_git_metadata(source_dir: &Path) -> Option<SourceGitMetadata> {
    use std::process::Command;

    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(source_dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());

    // If we can't even get a commit, this isn't a git repo
    commit.as_ref()?;

    let branch = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(source_dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty());

    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(source_dir)
        .output()
        .is_ok_and(|o| !o.stdout.is_empty());

    Some(SourceGitMetadata {
        commit,
        branch,
        dirty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_platform_file() {
        let dir = TempDir::new().unwrap();
        let platform = dir.path().join(".platform");
        fs::write(
            &platform,
            r#"{
                "$schema": "https://developer.microsoft.com/json-schemas/fabric/gitIntegration/platformProperties/2.0.0/schema.json",
                "metadata": {
                    "type": "Notebook",
                    "displayName": "Hello World",
                    "description": "A test notebook"
                },
                "config": {
                    "version": "2.0",
                    "logicalId": "99b570c5-0c79-9dc4-4c9b-fa16c621384c"
                }
            }"#,
        )
        .unwrap();

        let meta = parse_platform_file(&platform).unwrap();
        assert_eq!(meta.item_type, "Notebook");
        assert_eq!(meta.display_name, "Hello World");
        assert_eq!(meta.description.as_deref(), Some("A test notebook"));
        assert_eq!(
            meta.logical_id.as_deref(),
            Some("99b570c5-0c79-9dc4-4c9b-fa16c621384c")
        );
        // A 2.0.0 .platform has no sensitivityLabelId.
        assert_eq!(meta.sensitivity_label_id, None);
    }

    #[test]
    fn test_parse_platform_file_sensitivity_label() {
        // platformProperties 2.1.0 carries metadata.sensitivityLabelId (used by
        // Fabric Git Integration / fabric-cicd). It must be parsed, not dropped.
        let dir = TempDir::new().unwrap();
        let platform = dir.path().join(".platform");
        fs::write(
            &platform,
            r#"{
                "$schema": "https://developer.microsoft.com/json-schemas/fabric/gitIntegration/platformProperties/2.1.0/schema.json",
                "metadata": {
                    "type": "Notebook",
                    "displayName": "Labeled",
                    "sensitivityLabelId": "ce9c66be-6a5a-4d0b-9f3f-111122223333"
                },
                "config": { "version": "2.0" }
            }"#,
        )
        .unwrap();

        let meta = parse_platform_file(&platform).unwrap();
        assert_eq!(
            meta.sensitivity_label_id.as_deref(),
            Some("ce9c66be-6a5a-4d0b-9f3f-111122223333")
        );
    }

    #[test]
    fn test_build_platform_json_sensitivity_label_roundtrip() {
        let meta = PlatformMetadata {
            item_type: "Notebook".to_owned(),
            display_name: "Labeled".to_owned(),
            logical_id: None,
            description: None,
            definition_format: None,
            sensitivity_label_id: Some("ce9c66be-6a5a-4d0b-9f3f-111122223333".to_owned()),
            platform_creation_payload: None,
        };
        let json = build_platform_json(&meta);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Emits the 2.1.0 schema (the version that defines sensitivityLabelId).
        assert!(
            v["$schema"]
                .as_str()
                .unwrap()
                .contains("platformProperties/2.1.0"),
            "expected 2.1.0 schema, got {}",
            v["$schema"]
        );
        assert_eq!(
            v["metadata"]["sensitivityLabelId"],
            "ce9c66be-6a5a-4d0b-9f3f-111122223333"
        );

        // Round-trips back through the parser.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".platform");
        fs::write(&path, &json).unwrap();
        let reparsed = parse_platform_file(&path).unwrap();
        assert_eq!(reparsed.sensitivity_label_id, meta.sensitivity_label_id);
    }

    #[test]
    fn test_build_platform_json_omits_label_when_absent() {
        let meta = PlatformMetadata {
            item_type: "Notebook".to_owned(),
            display_name: "Plain".to_owned(),
            logical_id: None,
            description: None,
            definition_format: None,
            sensitivity_label_id: None,
            platform_creation_payload: None,
        };
        let v: serde_json::Value = serde_json::from_str(&build_platform_json(&meta)).unwrap();
        assert!(v["metadata"].get("sensitivityLabelId").is_none());
    }

    #[test]
    fn test_parse_source_directory() {
        let dir = TempDir::new().unwrap();

        // Create a notebook item
        let nb_dir = dir.path().join("MyNotebook.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        fs::write(
            nb_dir.join(".platform"),
            r#"{"metadata":{"type":"Notebook","displayName":"MyNotebook"},"config":{"version":"2.0","logicalId":"aaa-bbb"}}"#,
        )
        .unwrap();
        fs::write(nb_dir.join("notebook-content.py"), "# Hello").unwrap();

        // Create a pipeline item
        let pl_dir = dir.path().join("ETL.DataPipeline");
        fs::create_dir_all(&pl_dir).unwrap();
        fs::write(
            pl_dir.join(".platform"),
            r#"{"metadata":{"type":"DataPipeline","displayName":"ETL"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(pl_dir.join("pipeline-content.json"), r#"{"activities":[]}"#).unwrap();

        // A non-item file at root (should be ignored)
        fs::write(dir.path().join("README.md"), "# Docs").unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        assert_eq!(workspace.items.len(), 2);
        assert!(workspace.logical_id_index.contains_key("aaa-bbb"));
        assert!(
            workspace
                .type_name_index
                .contains_key(&("Notebook".to_owned(), "MyNotebook".to_owned()))
        );
        assert!(
            workspace
                .type_name_index
                .contains_key(&("DataPipeline".to_owned(), "ETL".to_owned()))
        );
    }

    #[test]
    fn test_parse_source_directory_with_creation_payload() {
        let dir = TempDir::new().unwrap();

        // Create a KQL database item with creationPayload.json
        let kql_dir = dir.path().join("MyDB.KQLDatabase");
        fs::create_dir_all(&kql_dir).unwrap();
        fs::write(
            kql_dir.join(".platform"),
            r#"{"metadata":{"type":"KQLDatabase","displayName":"MyDB"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(
            kql_dir.join("creationPayload.json"),
            r#"{"databaseType":"ReadWrite","parentEventhouseItemId":"eh-123"}"#,
        )
        .unwrap();
        fs::write(kql_dir.join("DatabaseProperties.json"), r"{}").unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        assert_eq!(workspace.items.len(), 1);

        let item = &workspace.items[0];
        assert!(item.creation_payload.is_some());
        let payload = item.creation_payload.as_ref().unwrap();
        assert_eq!(payload["databaseType"], "ReadWrite");
        assert_eq!(payload["parentEventhouseItemId"], "eh-123");

        // creationPayload.json should NOT be in the definition parts
        assert_eq!(item.parts.len(), 2); // .platform + DatabaseProperties.json
        assert!(
            item.parts
                .iter()
                .any(|p| p.path == "DatabaseProperties.json")
        );
    }

    #[test]
    fn test_parse_source_directory_without_creation_payload() {
        let dir = TempDir::new().unwrap();

        let nb_dir = dir.path().join("Nb.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        fs::write(
            nb_dir.join(".platform"),
            r#"{"metadata":{"type":"Notebook","displayName":"Nb"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(nb_dir.join("notebook-content.py"), "# code").unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        let item = &workspace.items[0];
        assert!(item.creation_payload.is_none());
        assert_eq!(item.parts.len(), 2); // .platform + notebook-content.py
    }

    #[test]
    fn test_content_hash_deterministic() {
        let parts = vec![
            DefinitionPart {
                path: "b.json".to_owned(),
                payload: BASE64.encode(b"content-b"),
                payload_type: "InlineBase64".to_owned(),
            },
            DefinitionPart {
                path: "a.json".to_owned(),
                payload: BASE64.encode(b"content-a"),
                payload_type: "InlineBase64".to_owned(),
            },
        ];

        // Same parts in different order should produce same hash
        let parts_reversed = vec![parts[1].clone(), parts[0].clone()];

        let hash1 = compute_content_hash(&parts);
        let hash2 = compute_content_hash(&parts_reversed);
        assert_eq!(hash1, hash2);
        assert!(hash1.starts_with("sha256:"));
    }

    #[test]
    fn test_write_source_directory() {
        let dir = TempDir::new().unwrap();
        let output = dir.path().join("export");

        let items = vec![(
            PlatformMetadata {
                item_type: "Notebook".to_owned(),
                display_name: "Test".to_owned(),
                logical_id: Some("lid-123".to_owned()),
                description: None,
                definition_format: None,
                sensitivity_label_id: None,
                platform_creation_payload: None,
            },
            vec![DefinitionPart {
                path: "notebook-content.py".to_owned(),
                payload: BASE64.encode(b"# code"),
                payload_type: "InlineBase64".to_owned(),
            }],
        )];

        let count = write_source_directory(&output, &items, false).unwrap();
        assert_eq!(count, 1);

        let nb_dir = output.join("Test.Notebook");
        assert!(nb_dir.join(".platform").exists());
        assert!(nb_dir.join("notebook-content.py").exists());

        let content = fs::read_to_string(nb_dir.join("notebook-content.py")).unwrap();
        assert_eq!(content, "# code");
    }

    #[test]
    fn test_parse_source_directory_with_shortcuts() {
        let dir = TempDir::new().unwrap();

        // Create a lakehouse item with shortcuts.metadata.json
        let lh_dir = dir.path().join("SalesLH.Lakehouse");
        fs::create_dir_all(&lh_dir).unwrap();
        fs::write(
            lh_dir.join(".platform"),
            r#"{"metadata":{"type":"Lakehouse","displayName":"SalesLH"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(
            lh_dir.join("shortcuts.metadata.json"),
            r#"[
                {
                    "name": "products",
                    "path": "Tables",
                    "target": {
                        "oneLake": {
                            "workspaceId": "00000000-0000-0000-0000-000000000000",
                            "itemId": "aaa-bbb-ccc",
                            "path": "Tables/products"
                        }
                    }
                }
            ]"#,
        )
        .unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        assert_eq!(workspace.items.len(), 1);

        let item = &workspace.items[0];
        assert_eq!(item.metadata.item_type, "Lakehouse");

        // Shortcuts should be parsed
        assert!(item.shortcuts.is_some());
        let shortcuts = item.shortcuts.as_ref().unwrap();
        assert_eq!(shortcuts.len(), 1);
        assert_eq!(shortcuts[0]["name"], "products");
        assert_eq!(shortcuts[0]["path"], "Tables");

        // shortcuts.metadata.json should NOT be in definition parts
        // (.platform IS included as a part)
        assert_eq!(item.parts.len(), 1);
        assert_eq!(item.parts[0].path, ".platform");
    }

    #[test]
    fn test_parse_source_directory_empty_shortcuts_array() {
        let dir = TempDir::new().unwrap();

        let lh_dir = dir.path().join("EmptyLH.Lakehouse");
        fs::create_dir_all(&lh_dir).unwrap();
        fs::write(
            lh_dir.join(".platform"),
            r#"{"metadata":{"type":"Lakehouse","displayName":"EmptyLH"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(lh_dir.join("shortcuts.metadata.json"), "[]").unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        let item = &workspace.items[0];

        // Empty array should result in None (no shortcuts to deploy)
        assert!(item.shortcuts.is_none());
    }

    #[test]
    fn test_parse_discovers_kql_databases_in_children() {
        let dir = TempDir::new().unwrap();

        // Create Eventhouse with .children/ containing a KQL Database
        let eh_dir = dir.path().join("MyEH.Eventhouse");
        fs::create_dir_all(&eh_dir).unwrap();
        fs::write(
            eh_dir.join(".platform"),
            r#"{"metadata":{"type":"Eventhouse","displayName":"MyEH"},"config":{"version":"2.0","logicalId":"eh-lid-001"}}"#,
        ).unwrap();
        fs::write(eh_dir.join("EventhouseProperties.json"), "{}").unwrap();

        let kql_dir = eh_dir.join(".children").join("MyDB.KQLDatabase");
        fs::create_dir_all(&kql_dir).unwrap();
        fs::write(
            kql_dir.join(".platform"),
            r#"{"metadata":{"type":"KQLDatabase","displayName":"MyDB"},"config":{"version":"2.0","logicalId":"kql-lid-001"}}"#,
        ).unwrap();
        fs::write(
            kql_dir.join("DatabaseProperties.json"),
            r#"{"parentEventhouseItemId":"eh-lid-001"}"#,
        )
        .unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();

        // Both Eventhouse and KQL Database should be discovered
        assert_eq!(workspace.items.len(), 2);
        let types: Vec<&str> = workspace
            .items
            .iter()
            .map(|i| i.metadata.item_type.as_str())
            .collect();
        assert!(types.contains(&"Eventhouse"));
        assert!(types.contains(&"KQLDatabase"));
    }

    #[test]
    fn test_parse_excludes_pbi_directory_from_parts() {
        let dir = TempDir::new().unwrap();

        let report_dir = dir.path().join("MyReport.Report");
        fs::create_dir_all(&report_dir).unwrap();
        fs::write(
            report_dir.join(".platform"),
            r#"{"metadata":{"type":"Report","displayName":"MyReport"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(report_dir.join("definition.pbir"), r#"{"version":"4.0"}"#).unwrap();

        // Create .pbi/ directory (should be excluded from parts)
        let pbi_dir = report_dir.join(".pbi");
        fs::create_dir_all(&pbi_dir).unwrap();
        fs::write(pbi_dir.join("localSettings.json"), "{}").unwrap();
        fs::write(pbi_dir.join("cache.abf"), "binary data").unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        assert_eq!(workspace.items.len(), 1);

        let item = &workspace.items[0];
        // Only definition.pbir + .platform should be in parts (not .pbi/ contents)
        assert_eq!(item.parts.len(), 2);
        assert!(item.parts.iter().any(|p| p.path == "definition.pbir"));
        assert!(item.parts.iter().any(|p| p.path == ".platform"));
    }

    #[test]
    fn test_parse_lakehouse_enables_schemas_from_metadata() {
        let dir = TempDir::new().unwrap();

        let lh_dir = dir.path().join("SchemaLH.Lakehouse");
        fs::create_dir_all(&lh_dir).unwrap();
        fs::write(
            lh_dir.join(".platform"),
            r#"{"metadata":{"type":"Lakehouse","displayName":"SchemaLH"},"config":{"version":"2.0"}}"#,
        ).unwrap();
        // lakehouse.metadata.json with defaultSchema → should enable schemas
        fs::write(
            lh_dir.join("lakehouse.metadata.json"),
            r#"{"defaultSchema": "dbo"}"#,
        )
        .unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        let item = &workspace.items[0];

        // Should have creationPayload with enableSchemas
        assert!(item.creation_payload.is_some());
        assert_eq!(
            item.creation_payload.as_ref().unwrap()["enableSchemas"],
            true
        );
    }

    #[test]
    fn test_parse_reads_creation_payload_from_platform_metadata() {
        let dir = TempDir::new().unwrap();

        let wh_dir = dir.path().join("MyWH.Warehouse");
        fs::create_dir_all(&wh_dir).unwrap();
        // .platform with creationPayload in metadata (fabric-cicd format)
        fs::write(
            wh_dir.join(".platform"),
            r#"{"metadata":{"type":"Warehouse","displayName":"MyWH","creationPayload":{"collation":"Latin1_General_100_BIN2_UTF8"}},"config":{"version":"2.0"}}"#,
        ).unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        let item = &workspace.items[0];

        // Should read creationPayload from .platform metadata
        assert!(item.creation_payload.is_some());
        assert_eq!(
            item.creation_payload.as_ref().unwrap()["collation"],
            "Latin1_General_100_BIN2_UTF8"
        );
    }

    #[test]
    fn test_spark_job_definition_format_fallback() {
        let dir = TempDir::new().unwrap();

        let sjd_dir = dir.path().join("MySJD.SparkJobDefinition");
        fs::create_dir_all(&sjd_dir).unwrap();
        // .platform WITHOUT definitionFormat
        fs::write(
            sjd_dir.join(".platform"),
            r#"{"metadata":{"type":"SparkJobDefinition","displayName":"MySJD"},"config":{"version":"2.0"}}"#,
        ).unwrap();
        fs::write(sjd_dir.join("SparkJobDefinitionV1.json"), "{}").unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        let item = &workspace.items[0];

        // Should auto-detect format as SparkJobDefinitionV2
        assert_eq!(
            item.metadata.definition_format.as_deref(),
            Some("SparkJobDefinitionV2")
        );
    }

    #[test]
    fn test_parse_nested_folder_computes_folder_path() {
        let dir = TempDir::new().unwrap();

        // Create nested structure: ETL/Bronze/MyNb.Notebook
        let nb_dir = dir.path().join("ETL").join("Bronze").join("MyNb.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        fs::write(
            nb_dir.join(".platform"),
            r#"{"metadata":{"type":"Notebook","displayName":"MyNb"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(nb_dir.join("notebook-content.py"), "# code").unwrap();

        // Root-level item
        let root_nb = dir.path().join("RootNb.Notebook");
        fs::create_dir_all(&root_nb).unwrap();
        fs::write(
            root_nb.join(".platform"),
            r#"{"metadata":{"type":"Notebook","displayName":"RootNb"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(root_nb.join("notebook-content.py"), "# root").unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        assert_eq!(workspace.items.len(), 2);

        // Find the nested item and check its folder_path
        let nested = workspace
            .items
            .iter()
            .find(|i| i.metadata.display_name == "MyNb")
            .unwrap();
        assert_eq!(nested.folder_path, "/ETL/Bronze");

        // Root item should have empty folder_path
        let root = workspace
            .items
            .iter()
            .find(|i| i.metadata.display_name == "RootNb")
            .unwrap();
        assert_eq!(root.folder_path, "");
    }

    #[test]
    fn test_parse_source_directory_with_governance_metadata() {
        let dir = TempDir::new().unwrap();

        let nb_dir = dir.path().join("MyNb.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        fs::write(
            nb_dir.join(".platform"),
            r#"{"metadata":{"type":"Notebook","displayName":"MyNb"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(nb_dir.join("notebook-content.py"), "# code").unwrap();
        fs::write(
            nb_dir.join("governance.metadata.json"),
            r#"{
                "sensitivityLabel": {"id": "label-uuid-123"},
                "tags": [{"id": "tag-uuid-456", "displayName": "Production"}]
            }"#,
        )
        .unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        assert_eq!(workspace.items.len(), 1);

        let item = &workspace.items[0];
        assert!(item.governance.is_some());
        let gov = item.governance.as_ref().unwrap();
        assert!(gov.sensitivity_label.is_some());
        assert_eq!(gov.sensitivity_label.as_ref().unwrap().id, "label-uuid-123");
        assert_eq!(gov.tags.len(), 1);
        assert_eq!(gov.tags[0].id, "tag-uuid-456");
        assert_eq!(gov.tags[0].display_name, "Production");

        // governance.metadata.json should NOT be in definition parts
        assert!(
            !item
                .parts
                .iter()
                .any(|p| p.path == "governance.metadata.json")
        );
    }

    #[test]
    fn test_parse_governance_metadata_label_only() {
        let dir = TempDir::new().unwrap();

        let nb_dir = dir.path().join("Nb.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        fs::write(
            nb_dir.join(".platform"),
            r#"{"metadata":{"type":"Notebook","displayName":"Nb"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(nb_dir.join("notebook-content.py"), "# code").unwrap();
        fs::write(
            nb_dir.join("governance.metadata.json"),
            r#"{"sensitivityLabel": {"id": "label-only-uuid"}}"#,
        )
        .unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        let item = &workspace.items[0];
        assert!(item.governance.is_some());
        let gov = item.governance.as_ref().unwrap();
        assert_eq!(
            gov.sensitivity_label.as_ref().unwrap().id,
            "label-only-uuid"
        );
        assert!(gov.tags.is_empty());
    }

    #[test]
    fn test_parse_governance_metadata_tags_only() {
        let dir = TempDir::new().unwrap();

        let nb_dir = dir.path().join("Nb.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        fs::write(
            nb_dir.join(".platform"),
            r#"{"metadata":{"type":"Notebook","displayName":"Nb"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(nb_dir.join("notebook-content.py"), "# code").unwrap();
        fs::write(
            nb_dir.join("governance.metadata.json"),
            r#"{"tags": [{"id": "tag-1", "displayName": "Dev"}, {"id": "tag-2", "displayName": "ETL"}]}"#,
        )
        .unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        let item = &workspace.items[0];
        assert!(item.governance.is_some());
        let gov = item.governance.as_ref().unwrap();
        assert!(gov.sensitivity_label.is_none());
        assert_eq!(gov.tags.len(), 2);
    }

    #[test]
    fn test_parse_governance_metadata_empty_is_none() {
        let dir = TempDir::new().unwrap();

        let nb_dir = dir.path().join("Nb.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        fs::write(
            nb_dir.join(".platform"),
            r#"{"metadata":{"type":"Notebook","displayName":"Nb"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(nb_dir.join("notebook-content.py"), "# code").unwrap();
        // Empty governance file (no label, no tags)
        fs::write(nb_dir.join("governance.metadata.json"), r"{}").unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        let item = &workspace.items[0];
        // Should be None because there's nothing to apply
        assert!(item.governance.is_none());
    }

    #[test]
    fn test_parse_governance_metadata_absent_file() {
        let dir = TempDir::new().unwrap();

        let nb_dir = dir.path().join("Nb.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        fs::write(
            nb_dir.join(".platform"),
            r#"{"metadata":{"type":"Notebook","displayName":"Nb"},"config":{"version":"2.0"}}"#,
        )
        .unwrap();
        fs::write(nb_dir.join("notebook-content.py"), "# code").unwrap();
        // No governance.metadata.json file

        let workspace = parse_source_directory(dir.path()).unwrap();
        let item = &workspace.items[0];
        assert!(item.governance.is_none());
    }

    #[test]
    fn test_synthesize_platform_metadata_report_and_model() {
        let dir = TempDir::new().unwrap();

        // <name>.SemanticModel with the required entry file.
        let sm_dir = dir.path().join("Sales.SemanticModel");
        fs::create_dir_all(&sm_dir).unwrap();
        fs::write(sm_dir.join("definition.pbism"), "{}").unwrap();
        let sm = synthesize_platform_metadata(&sm_dir, "Sales.SemanticModel").unwrap();
        assert_eq!(sm.item_type, "SemanticModel");
        assert_eq!(sm.display_name, "Sales");
        assert!(sm.logical_id.is_none());

        // <name>.Report with the required entry file.
        let rp_dir = dir.path().join("Sales.Report");
        fs::create_dir_all(&rp_dir).unwrap();
        fs::write(rp_dir.join("definition.pbir"), "{}").unwrap();
        let rp = synthesize_platform_metadata(&rp_dir, "Sales.Report").unwrap();
        assert_eq!(rp.item_type, "Report");
        assert_eq!(rp.display_name, "Sales");
    }

    #[test]
    fn test_synthesize_platform_metadata_rejects_non_pbip() {
        let dir = TempDir::new().unwrap();

        // Unrecognized suffix.
        let nb_dir = dir.path().join("Foo.Notebook");
        fs::create_dir_all(&nb_dir).unwrap();
        assert!(synthesize_platform_metadata(&nb_dir, "Foo.Notebook").is_none());

        // Correct suffix but missing the required entry file → not synthesizable.
        let empty_dir = dir.path().join("Bar.Report");
        fs::create_dir_all(&empty_dir).unwrap();
        assert!(synthesize_platform_metadata(&empty_dir, "Bar.Report").is_none());

        // No dot / empty base.
        assert!(synthesize_platform_metadata(dir.path(), "Report").is_none());
        let dot_dir = dir.path().join(".SemanticModel");
        fs::create_dir_all(&dot_dir).unwrap();
        fs::write(dot_dir.join("definition.pbism"), "{}").unwrap();
        assert!(synthesize_platform_metadata(&dot_dir, ".SemanticModel").is_none());
    }

    #[test]
    fn test_parse_source_directory_synthesizes_pbip_without_platform() {
        // A raw Power BI Desktop PBIP root: two item folders WITHOUT any
        // `.platform` sidecar. Both must be discovered and typed correctly.
        let dir = TempDir::new().unwrap();

        let sm_dir = dir.path().join("Sales.SemanticModel");
        fs::create_dir_all(sm_dir.join("definition")).unwrap();
        fs::write(sm_dir.join("definition.pbism"), r#"{"version":"4.0"}"#).unwrap();
        fs::write(
            sm_dir.join("definition").join("model.tmdl"),
            "model Model\n",
        )
        .unwrap();

        let rp_dir = dir.path().join("Sales.Report");
        fs::create_dir_all(&rp_dir).unwrap();
        fs::write(rp_dir.join("definition.pbir"), r#"{"version":"4.0"}"#).unwrap();
        // Desktop user-state folder that must be excluded from parts.
        fs::create_dir_all(rp_dir.join(".pbi")).unwrap();
        fs::write(rp_dir.join(".pbi").join("localSettings.json"), "{}").unwrap();

        // A root-level pointer file + gitignore should be ignored (not dirs).
        fs::write(dir.path().join("Sales.pbip"), "{}").unwrap();

        let workspace = parse_source_directory(dir.path()).unwrap();
        assert_eq!(workspace.items.len(), 2);

        let model = workspace
            .items
            .iter()
            .find(|i| i.metadata.item_type == "SemanticModel")
            .expect("semantic model discovered");
        assert_eq!(model.metadata.display_name, "Sales");
        assert!(model.metadata.logical_id.is_none());

        let report = workspace
            .items
            .iter()
            .find(|i| i.metadata.item_type == "Report")
            .expect("report discovered");
        assert_eq!(report.metadata.display_name, "Sales");
        // `.pbi/` user state must not be sent as a definition part.
        assert!(
            report
                .parts
                .iter()
                .all(|p| !p.path.starts_with(".pbi/") && !p.path.starts_with(".pbi\\"))
        );
        assert!(report.parts.iter().any(|p| p.path == "definition.pbir"));

        // The (type, name) index resolves both — this is what report byPath
        // rebinding relies on when the model has no on-disk logicalId.
        assert!(
            workspace
                .type_name_index
                .contains_key(&("SemanticModel".to_owned(), "Sales".to_owned()))
        );
    }
}
