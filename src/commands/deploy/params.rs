//! Parameter substitution for environment-aware deployments.
//!
//! Supports a JSON parameter file (`parameters.json`) with:
//! - `find_replace`: Literal or regex-based string replacement in definition payloads
//! - `key_value_replace`: JSONPath-based value replacement at specific JSON keys
//! - `spark_pool`: Spark pool instance ID to environment-specific pool configuration mapping
//! - `semantic_model_binding`: Semantic model connection ID promotion across environments
//! - `label_replace`: Sensitivity label ID mapping for cross-tenant deployments
//! - `tag_replace`: Governance tag ID mapping for cross-tenant deployments
//! - Dynamic variables: `$workspace.id`, `$workspace.name`, `$items.Type.Name.id`, `$ENV:VAR`
//!
//! The parameter file format is a superset of fabric-cicd's YAML `parameter.yml`,
//! expressed in JSON for agent-native tooling consistency.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use jsonpath_rust::JsonPath;
use jsonpath_rust::query::queryable::Queryable;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::platform::{DefinitionPart, SourceItem, SourceWorkspace};
use crate::errors::{ErrorCode, FabioError};

/// Parsed parameter file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameters {
    /// String find-and-replace rules.
    #[serde(default)]
    pub find_replace: Vec<FindReplaceRule>,

    /// JSONPath-based key-value replacement rules.
    #[serde(default)]
    pub key_value_replace: Vec<KeyValueReplaceRule>,

    /// Spark pool instance mapping rules.
    #[serde(default)]
    pub spark_pool: Vec<SparkPoolRule>,

    /// Semantic model connection binding rules.
    #[serde(default)]
    pub semantic_model_binding: Option<SemanticModelBinding>,

    /// Map source sensitivity label ID → target label ID (or null to skip).
    ///
    /// Enables cross-tenant deployments where governance label IDs differ between
    /// source and target tenants. A `null` value explicitly skips the label (don't
    /// apply in the target). IDs not in the map pass through unchanged.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub label_replace: HashMap<String, Option<String>>,

    /// Map source tag ID → target tag ID (or null to skip).
    ///
    /// Enables cross-tenant deployments where governance tag IDs differ between
    /// source and target tenants. A `null` value explicitly skips the tag (don't
    /// apply in the target). IDs not in the map pass through unchanged.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tag_replace: HashMap<String, Option<String>>,
}

/// A single find-and-replace rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindReplaceRule {
    /// The string or regex pattern to find.
    pub find_value: String,

    /// Environment-keyed replacement values.
    /// Key is the environment name (e.g., "dev", "prod") or "_ALL_" for all environments.
    pub replace_value: HashMap<String, String>,

    /// Enable regex mode. In regex mode, only capture group 1 is replaced.
    #[serde(default)]
    pub is_regex: bool,

    /// Restrict to specific item type(s).
    #[serde(default)]
    pub item_type: Option<StringOrVec>,

    /// Restrict to specific item name(s).
    #[serde(default)]
    pub item_name: Option<StringOrVec>,

    /// Restrict to specific file path(s) within definitions.
    #[serde(default)]
    pub file_path: Option<StringOrVec>,
}

/// A value that can be either a single string or a list of strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    Single(String),
    Multiple(Vec<String>),
}

impl StringOrVec {
    pub fn contains(&self, value: &str) -> bool {
        match self {
            Self::Single(s) => s.eq_ignore_ascii_case(value),
            Self::Multiple(v) => v.iter().any(|s| s.eq_ignore_ascii_case(value)),
        }
    }
}

/// A JSONPath-based key-value replacement rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyValueReplaceRule {
    /// `JSONPath` expression identifying the key(s) whose value should be replaced.
    pub find_key: String,

    /// Environment-keyed replacement values (can be any JSON value type).
    pub replace_value: HashMap<String, serde_json::Value>,

    /// Restrict to specific item type(s).
    #[serde(default)]
    pub item_type: Option<StringOrVec>,

    /// Restrict to specific item name(s).
    #[serde(default)]
    pub item_name: Option<StringOrVec>,

    /// Restrict to specific file path(s) within definitions.
    #[serde(default)]
    pub file_path: Option<StringOrVec>,
}

/// A Spark pool instance mapping rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SparkPoolRule {
    /// The source Spark pool instance ID to match.
    pub instance_pool_id: String,

    /// Environment-keyed pool configuration objects.
    pub replace_value: HashMap<String, SparkPoolConfig>,

    /// Restrict to specific item name(s).
    #[serde(default)]
    pub item_name: Option<StringOrVec>,
}

/// Spark pool configuration for a target environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SparkPoolConfig {
    /// Pool type: "Capacity" or "Workspace".
    #[serde(rename = "type")]
    pub pool_type: String,

    /// Pool display name.
    pub name: String,
}

/// Semantic model connection binding configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticModelBinding {
    /// Default connection binding applied to all semantic models.
    #[serde(default)]
    pub default: Option<ConnectionBinding>,

    /// Per-model connection binding overrides.
    #[serde(default)]
    pub models: Vec<ModelBinding>,
}

/// A connection binding with environment-keyed connection IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionBinding {
    /// Environment-keyed connection GUID values.
    pub connection_id: HashMap<String, String>,
}

/// A per-model connection binding override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelBinding {
    /// Semantic model name(s) this binding applies to.
    pub semantic_model_name: StringOrVec,

    /// Environment-keyed connection GUID values.
    pub connection_id: HashMap<String, String>,
}

/// Context for resolving dynamic variables during substitution.
pub struct SubstitutionContext<'a> {
    /// Target workspace ID.
    pub workspace_id: &'a str,
    /// Target workspace name (if resolved).
    pub workspace_name: Option<&'a str>,
    /// Map of (Type, Name) → deployed GUID for `$items.Type.Name.id` resolution.
    pub deployed_items: &'a HashMap<(String, String), String>,
}

/// Parse a parameter file from disk.
pub fn parse_parameters(path: &Path) -> Result<Parameters> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read parameters file: {}", path.display()))?;

    // Auto-detect format by extension: .yml/.yaml → YAML, otherwise JSON
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let params: Parameters = match ext.as_str() {
        "yml" | "yaml" => serde_yaml::from_str(&content)
            .with_context(|| format!("Invalid YAML in parameters file: {}", path.display()))?,
        _ => serde_json::from_str(&content).or_else(|_| {
            serde_yaml::from_str(&content).with_context(|| {
                format!("Invalid JSON/YAML in parameters file: {}", path.display())
            })
        })?,
    };

    // Validate rules
    for (i, rule) in params.find_replace.iter().enumerate() {
        if rule.find_value.is_empty() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("parameters file rule #{}: find_value cannot be empty", i + 1),
                "Provide a non-empty find_value string or regex pattern to match against definition payloads.",
            ).into());
        }
        if rule.replace_value.is_empty() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("parameters file rule #{}: replace_value must have at least one environment entry", i + 1),
                "Add at least one environment key, or use '_ALL_' as a universal fallback.",
            ).into());
        }
        if rule.is_regex {
            Regex::new(&rule.find_value).with_context(|| {
                format!(
                    "parameters file rule #{}: invalid regex pattern: {}",
                    i + 1,
                    rule.find_value
                )
            })?;
        }
    }

    // Validate key_value_replace rules
    for (i, rule) in params.key_value_replace.iter().enumerate() {
        if rule.find_key.is_empty() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!(
                    "key_value_replace rule #{}: find_key cannot be empty",
                    i + 1
                ),
                "Provide a JSONPath expression, e.g.: \"$.parentEventhouseItemId\"",
            )
            .into());
        }
        if rule.replace_value.is_empty() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("key_value_replace rule #{}: replace_value must have at least one environment entry", i + 1),
                "Add at least one environment key, or use '_ALL_' as a universal fallback.",
            ).into());
        }
        // Validate JSONPath syntax
        jsonpath_rust::parser::parse_json_path(&rule.find_key).with_context(|| {
            format!(
                "key_value_replace rule #{}: invalid JSONPath expression: {}",
                i + 1,
                rule.find_key
            )
        })?;
    }

    // Validate spark_pool rules
    for (i, rule) in params.spark_pool.iter().enumerate() {
        if rule.instance_pool_id.is_empty() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!(
                    "spark_pool rule #{}: instance_pool_id cannot be empty",
                    i + 1
                ),
                "Provide the current Spark pool instance ID to match against.",
            )
            .into());
        }
        if rule.replace_value.is_empty() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!(
                    "spark_pool rule #{}: replace_value must have at least one environment entry",
                    i + 1
                ),
                "Add at least one environment key, or use '_ALL_' as a universal fallback.",
            )
            .into());
        }
    }

    Ok(params)
}

/// Resolve a replacement value, expanding dynamic variables.
///
/// Dynamic variables:
/// - `$workspace.id` → target workspace GUID
/// - `$workspace.name` → target workspace display name
/// - `$items.Type.Name.id` → deployed GUID of the named item
/// - `$ENV:VAR_NAME` → value of environment variable
fn resolve_value(raw: &str, ctx: &SubstitutionContext<'_>) -> Result<String> {
    if !raw.starts_with('$') {
        return Ok(raw.to_owned());
    }

    if raw == "$workspace.id" {
        return Ok(ctx.workspace_id.to_owned());
    }

    if raw == "$workspace.name" {
        return ctx.workspace_name.map(str::to_owned).ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "$workspace.name not available (workspace resolved by ID, not name)",
                "Use $workspace.id instead, or pass --workspace by display name to enable $workspace.name resolution.",
            ).into()
        });
    }

    if let Some(var_name) = raw.strip_prefix("$ENV:") {
        return std::env::var(var_name).map_err(|_| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Environment variable '{var_name}' referenced in parameters is not set"),
                format!(
                    "Set the environment variable before running deploy: export {var_name}=<value>"
                ),
            )
            .into()
        });
    }

    if let Some(item_ref) = raw.strip_prefix("$items.") {
        // Format: $items.Type.Name.property
        // Supported properties: id, sqlendpoint, sqlendpointid, queryserviceuri
        let parts: Vec<&str> = item_ref.splitn(3, '.').collect();
        if parts.len() == 3 {
            let item_type = parts[0];
            let item_name = parts[1];
            let property = parts[2];

            match property {
                "id" => {
                    return ctx
                        .deployed_items
                        .get(&(item_type.to_owned(), item_name.to_owned()))
                        .cloned()
                        .ok_or_else(|| {
                            FabioError::with_hint(
                                ErrorCode::InvalidInput,
                                format!("Cannot resolve $items.{item_type}.{item_name}.id: item not found in deployed workspace or source"),
                                "Ensure the item exists in the workspace or source directory. Available variables: $workspace.id, $workspace.name, $items.Type.Name.id, $ENV:VAR_NAME",
                            ).into()
                        });
                }
                "sqlendpoint" | "sqlendpointid" | "queryserviceuri" => {
                    // Extended properties require pre-fetched data in deployed_items map
                    // The key format for extended props: (Type, Name.property) → value
                    let extended_key = (item_type.to_owned(), format!("{item_name}.{property}"));
                    return ctx
                        .deployed_items
                        .get(&extended_key)
                        .cloned()
                        .ok_or_else(|| {
                            FabioError::with_hint(
                                ErrorCode::InvalidInput,
                                format!("Cannot resolve $items.{item_type}.{item_name}.{property}: property not available"),
                                "Extended properties (sqlendpoint, sqlendpointid, queryserviceuri) are resolved from live workspace items.",
                            ).into()
                        });
                }
                _ => {
                    return Err(FabioError::with_hint(
                        ErrorCode::InvalidInput,
                        format!("Invalid $items property: '{property}'"),
                        "Supported: id, sqlendpoint, sqlendpointid, queryserviceuri",
                    )
                    .into());
                }
            }
        }
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Invalid $items reference: '{raw}'. Expected format: $items.Type.Name.property"
            ),
            "Supported properties: id, sqlendpoint, sqlendpointid, queryserviceuri",
        )
        .into());
    }

    // Unknown variable reference
    Err(FabioError::with_hint(
        ErrorCode::InvalidInput,
        format!("Unknown dynamic variable: '{raw}'"),
        "Supported: $workspace.id, $workspace.name, $items.Type.Name.<property>, $ENV:VAR_NAME",
    )
    .into())
}

/// Get the replacement value for a specific environment from a rule.
fn get_env_value<'a>(rule: &'a FindReplaceRule, env: &str) -> Option<&'a str> {
    // Check for exact environment match (case-insensitive)
    for (key, value) in &rule.replace_value {
        if key.eq_ignore_ascii_case(env) {
            return Some(value.as_str());
        }
    }
    // Check for _ALL_ wildcard
    for (key, value) in &rule.replace_value {
        if key.eq_ignore_ascii_case("_ALL_") {
            return Some(value.as_str());
        }
    }
    None
}

/// Apply parameter substitution to a source workspace's definition parts.
///
/// This modifies the definition payloads in-place (decoded from base64, substituted,
/// re-encoded to base64). The content hashes are recomputed after substitution.
pub fn apply_parameters(
    source: &mut SourceWorkspace,
    params: &Parameters,
    env: &str,
    ctx: &SubstitutionContext<'_>,
) -> Result<Vec<String>> {
    let mut warnings: Vec<String> = Vec::new();

    // Apply find_replace rules
    if !params.find_replace.is_empty() {
        apply_find_replace(source, &params.find_replace, env, ctx, &mut warnings)?;
    }

    // Apply key_value_replace rules
    if !params.key_value_replace.is_empty() {
        apply_key_value_replace(source, &params.key_value_replace, env, ctx, &mut warnings)?;
    }

    // Apply spark_pool rules
    if !params.spark_pool.is_empty() {
        apply_spark_pool_rules(source, &params.spark_pool, env, &mut warnings)?;
    }

    // Apply semantic_model_binding
    if let Some(ref binding) = params.semantic_model_binding {
        apply_semantic_model_binding(source, binding, env, &mut warnings)?;
    }

    // Apply label_replace to governance metadata (sensitivity labels)
    if !params.label_replace.is_empty() {
        apply_label_replace(source, &params.label_replace);
    }

    // Apply tag_replace to governance metadata (tags)
    if !params.tag_replace.is_empty() {
        apply_tag_replace(source, &params.tag_replace);
    }

    Ok(warnings)
}

/// Default workspace GUID placeholder used in source definitions.
/// This is the standard Fabric git integration placeholder for the "current workspace".
const DEFAULT_WORKSPACE_GUID: &str = "00000000-0000-0000-0000-000000000000";

/// Replace default workspace ID placeholders (`00000000-...`) with the target workspace ID.
///
/// Uses a regex to match workspace-reference keys (`default_lakehouse_workspace_id`,
/// `workspaceId`, `workspace`) where the value is the default GUID placeholder.
/// This matches fabric-cicd's `WORKSPACE_ID_REFERENCE_REGEX` behavior — only replacing
/// in known workspace-reference contexts, not blanket-replacing all occurrences.
///
/// Does NOT apply to shortcuts (handled separately in shortcut reconciliation where
/// `itemId` fields need the lakehouse's own GUID, not the workspace ID).
pub fn replace_default_workspace_id(source: &mut SourceWorkspace, workspace_id: &str) {
    // Regex matches: "key": "00000000-..." or key = "00000000-..."
    // where key is one of the known workspace-reference field names
    let pattern = regex::Regex::new(
        r#"(?i)"?(default_lakehouse_workspace_id|workspaceId|workspace)"?\s*[:=]\s*"00000000-0000-0000-0000-000000000000""#,
    )
    .expect("valid workspace ID regex");

    for item in &mut source.items {
        // Apply to definition parts (regex-based replacement)
        for part in &mut item.parts {
            if let Ok(decoded) = decode_part_payload(&part.payload)
                && decoded.contains(DEFAULT_WORKSPACE_GUID)
            {
                let replaced = pattern.replace_all(&decoded, |caps: &regex::Captures<'_>| {
                    // Preserve the key and format, replace only the GUID value
                    caps[0].replace(DEFAULT_WORKSPACE_GUID, workspace_id)
                });
                if replaced != decoded {
                    part.payload = BASE64.encode(replaced.as_bytes());
                }
            }
        }

        // Apply to creationPayload (also regex-based)
        if let Some(ref mut payload) = item.creation_payload {
            let s = payload.to_string();
            if s.contains(DEFAULT_WORKSPACE_GUID) {
                let replaced = pattern.replace_all(&s, |caps: &regex::Captures<'_>| {
                    caps[0].replace(DEFAULT_WORKSPACE_GUID, workspace_id)
                });
                if replaced != s
                    && let Ok(v) = serde_json::from_str(&replaced)
                {
                    *payload = v;
                }
            }
        }

        // Do NOT apply to shortcuts — they use itemId (lakehouse GUID, not workspace ID).
        // Shortcuts are resolved separately in execute_shortcut_hooks().

        // Recompute content hash
        item.content_hash = recompute_content_hash(&item.parts);
    }
}

/// Replace ALL non-target workspace IDs found in definition payloads with the target workspace.
///
/// This extends `replace_default_workspace_id` to also handle cases where the source
/// definitions contain the ORIGINAL workspace's real GUID (not just `00000000-...`).
/// This is common in repos like microsoft/fabric-toolbox that were exported without
/// Fabric Git Integration's workspace ID normalization.
///
/// Only replaces values in known workspace-reference fields (`workspaceId`,
/// `default_lakehouse_workspace_id`, `workspace`). Safe for first-time deployments.
pub fn replace_all_workspace_ids(source: &mut SourceWorkspace, target_workspace_id: &str) {
    // Regex matches: "key": "<any-guid>" where key is a known workspace-reference field
    let pattern = regex::Regex::new(
        r#"(?i)"?(default_lakehouse_workspace_id|workspaceId|workspace)"?\s*[:=]\s*"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})""#,
    )
    .expect("valid workspace ID regex");

    for item in &mut source.items {
        for part in &mut item.parts {
            if let Ok(decoded) = decode_part_payload(&part.payload) {
                let replaced = pattern.replace_all(&decoded, |caps: &regex::Captures<'_>| {
                    let existing_guid = &caps[2];
                    if existing_guid == target_workspace_id {
                        // Already correct, don't replace
                        caps[0].to_string()
                    } else {
                        // Replace the old workspace GUID with target
                        caps[0].replace(existing_guid, target_workspace_id)
                    }
                });
                if replaced != decoded {
                    part.payload = BASE64.encode(replaced.as_bytes());
                }
            }
        }

        // Recompute content hash
        item.content_hash = recompute_content_hash(&item.parts);
    }
}

/// Decode a base64 part payload to a string (best effort).
fn decode_part_payload(payload: &str) -> Result<String> {
    let bytes = BASE64.decode(payload)?;
    Ok(String::from_utf8(bytes)?)
}

/// Recompute the content hash for a set of definition parts.
/// Excludes `.platform` from hash (API modifies logicalId, breaking idempotency).
fn recompute_content_hash(parts: &[DefinitionPart]) -> String {
    super::platform::compute_content_hash_excluding_platform(parts)
}

/// Apply `find_replace` rules to a source workspace.
fn apply_find_replace(
    source: &mut SourceWorkspace,
    rules: &[FindReplaceRule],
    env: &str,
    ctx: &SubstitutionContext<'_>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    // Compile regex patterns once
    let compiled_rules: Vec<CompiledRule<'_>> = rules
        .iter()
        .filter_map(|rule| {
            let raw_value = get_env_value(rule, env)?;
            let resolved = resolve_value(raw_value, ctx);
            match resolved {
                Ok(replacement) => {
                    let pattern = if rule.is_regex {
                        match Regex::new(&rule.find_value) {
                            Ok(re) => RulePattern::Regex(re),
                            Err(e) => {
                                warnings.push(format!(
                                    "Skipping rule (invalid regex '{}'): {e}",
                                    rule.find_value
                                ));
                                return None;
                            }
                        }
                    } else {
                        RulePattern::Literal(rule.find_value.clone())
                    };

                    Some(CompiledRule {
                        pattern,
                        replacement,
                        rule,
                    })
                }
                Err(e) => {
                    warnings.push(format!("Skipping rule (cannot resolve '{raw_value}'): {e}"));
                    None
                }
            }
        })
        .collect();

    if compiled_rules.is_empty() {
        return Ok(());
    }

    // Apply rules to each item's definition parts and creationPayload
    for item in &mut source.items {
        apply_find_replace_to_item(item, &compiled_rules)?;
    }

    Ok(())
}

/// Apply compiled find/replace rules to a single source item (parts + creationPayload).
fn apply_find_replace_to_item(
    item: &mut SourceItem,
    compiled_rules: &[CompiledRule<'_>],
) -> Result<()> {
    for part in &mut item.parts {
        let should_process = compiled_rules.iter().any(|cr| {
            rule_applies_to_item(
                cr.rule,
                &item.metadata.item_type,
                &item.metadata.display_name,
                &part.path,
            )
        });

        if !should_process {
            continue;
        }

        // Decode the payload
        let decoded = BASE64
            .decode(&part.payload)
            .with_context(|| format!("Failed to decode base64 payload for {}", part.path))?;

        // Skip binary (non-UTF-8) files — they can't have text substitution applied.
        // This matches fabric-cicd's behavior of skipping binary/image files.
        let Ok(mut content) = String::from_utf8(decoded) else {
            continue;
        };

        let mut modified = false;

        for cr in compiled_rules {
            if !rule_applies_to_item(
                cr.rule,
                &item.metadata.item_type,
                &item.metadata.display_name,
                &part.path,
            ) {
                continue;
            }
            apply_rule_to_content(cr, &mut content, &mut modified);
        }

        if modified {
            part.payload = BASE64.encode(content.as_bytes());
        }
    }

    // Apply find_replace to creationPayload if present
    if let Some(ref mut payload) = item.creation_payload {
        let mut content = serde_json::to_string(payload).unwrap_or_default();
        let mut modified = false;

        for cr in compiled_rules {
            if !rule_applies_to_item(
                cr.rule,
                &item.metadata.item_type,
                &item.metadata.display_name,
                "creationPayload.json",
            ) {
                continue;
            }
            apply_rule_to_content(cr, &mut content, &mut modified);
        }

        if modified && let Ok(new_val) = serde_json::from_str(&content) {
            *payload = new_val;
        }
    }

    // Recompute content hash after substitution
    item.content_hash = compute_content_hash(&item.parts);
    Ok(())
}

/// Apply a single compiled rule to a content string, mutating in place.
fn apply_rule_to_content(cr: &CompiledRule<'_>, content: &mut String, modified: &mut bool) {
    match &cr.pattern {
        RulePattern::Literal(find) => {
            if content.contains(find.as_str()) {
                *content = content.replace(find.as_str(), &cr.replacement);
                *modified = true;
            }
        }
        RulePattern::Regex(re) => {
            let new_content = replace_capture_group(re, content, &cr.replacement);
            if new_content != *content {
                *content = new_content;
                *modified = true;
            }
        }
    }
}

/// Check if a `find_replace` rule applies to a specific item and file path.
fn rule_applies_to_item(
    rule: &FindReplaceRule,
    item_type: &str,
    item_name: &str,
    file_path: &str,
) -> bool {
    kv_rule_applies_to_item(
        rule.item_type.as_ref(),
        rule.item_name.as_ref(),
        rule.file_path.as_ref(),
        item_type,
        item_name,
        file_path,
    )
}

/// Check if a rule applies to a specific item and file path (generic version for `KeyValueReplaceRule`).
fn kv_rule_applies_to_item(
    item_type_filter: Option<&StringOrVec>,
    item_name_filter: Option<&StringOrVec>,
    file_path_filter: Option<&StringOrVec>,
    item_type: &str,
    item_name: &str,
    file_path: &str,
) -> bool {
    if let Some(types) = item_type_filter
        && !types.contains(item_type)
    {
        return false;
    }
    if let Some(names) = item_name_filter
        && !names.contains(item_name)
    {
        return false;
    }
    if let Some(paths) = file_path_filter
        && !paths.contains(file_path)
    {
        return false;
    }
    true
}

/// Apply `key_value_replace` rules to a source workspace.
fn apply_key_value_replace(
    source: &mut SourceWorkspace,
    rules: &[KeyValueReplaceRule],
    env: &str,
    ctx: &SubstitutionContext<'_>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    for rule in rules {
        // Get replacement value for this environment
        let replacement = get_env_value_json(&rule.replace_value, env);
        let Some(replacement) = replacement else {
            warnings.push(format!(
                "key_value_replace: no value for env '{env}' in rule with find_key '{}'",
                rule.find_key
            ));
            continue;
        };

        // Resolve dynamic variables if the value is a string
        let resolved_replacement = if let serde_json::Value::String(s) = replacement {
            match resolve_value(s, ctx) {
                Ok(resolved) => serde_json::Value::from(resolved),
                Err(e) => {
                    warnings.push(format!(
                        "key_value_replace: cannot resolve '{}': {e}",
                        rule.find_key
                    ));
                    continue;
                }
            }
        } else {
            replacement.clone()
        };

        // Validate JSONPath syntax upfront (already validated in parse_parameters, but guard here)
        if jsonpath_rust::parser::parse_json_path(&rule.find_key).is_err() {
            warnings.push(format!(
                "key_value_replace: invalid JSONPath '{}': parse error",
                rule.find_key
            ));
            continue;
        }

        for item in &mut source.items {
            for part in &mut item.parts {
                if !kv_rule_applies_to_item(
                    rule.item_type.as_ref(),
                    rule.item_name.as_ref(),
                    rule.file_path.as_ref(),
                    &item.metadata.item_type,
                    &item.metadata.display_name,
                    &part.path,
                ) {
                    continue;
                }

                // Decode payload
                let Ok(decoded) = BASE64.decode(&part.payload) else {
                    continue;
                };
                let Ok(content) = String::from_utf8(decoded) else {
                    continue;
                };

                // Parse as JSON
                let Ok(mut json_value) = serde_json::from_str::<serde_json::Value>(&content) else {
                    continue; // Not JSON, skip
                };

                // Apply JSONPath replacement
                let modified =
                    apply_jsonpath_replace(&mut json_value, &rule.find_key, &resolved_replacement);

                if modified {
                    let new_content = serde_json::to_string(&json_value)
                        .context("Failed to serialize JSON after key_value_replace")?;
                    part.payload = BASE64.encode(new_content.as_bytes());
                }
            }

            // Apply key_value_replace to creationPayload if present
            if let Some(ref mut payload) = item.creation_payload
                && kv_rule_applies_to_item(
                    rule.item_type.as_ref(),
                    rule.item_name.as_ref(),
                    rule.file_path.as_ref(),
                    &item.metadata.item_type,
                    &item.metadata.display_name,
                    "creationPayload.json",
                )
            {
                apply_jsonpath_replace(payload, &rule.find_key, &resolved_replacement);
            }

            // Recompute hash
            item.content_hash = compute_content_hash(&item.parts);
        }
    }

    Ok(())
}

/// Apply a `JSONPath` replacement to a JSON value. Returns true if modified.
fn apply_jsonpath_replace(
    value: &mut serde_json::Value,
    path_expr: &str,
    replacement: &serde_json::Value,
) -> bool {
    // Get all matching paths
    let Ok(paths) = value.query_only_path(path_expr) else {
        return false;
    };

    if paths.is_empty() {
        return false;
    }

    let mut modified = false;
    for path in paths {
        if let Some(target) = value.reference_mut(path) {
            *target = replacement.clone();
            modified = true;
        }
    }
    modified
}

/// Apply `spark_pool` rules to a source workspace.
fn apply_spark_pool_rules(
    source: &mut SourceWorkspace,
    rules: &[SparkPoolRule],
    env: &str,
    warnings: &mut Vec<String>,
) -> Result<()> {
    for rule in rules {
        // Get pool config for this environment
        let config = get_spark_pool_config(&rule.replace_value, env);
        let Some(config) = config else {
            warnings.push(format!(
                "spark_pool: no value for env '{env}' in rule with instance_pool_id '{}'",
                rule.instance_pool_id
            ));
            continue;
        };

        for item in &mut source.items {
            // Spark pool rules typically apply to Environment items
            if let Some(ref names) = rule.item_name
                && !names.contains(&item.metadata.display_name)
            {
                continue;
            }

            for part in &mut item.parts {
                // Decode payload
                let Ok(decoded) = BASE64.decode(&part.payload) else {
                    continue;
                };
                let Ok(content) = String::from_utf8(decoded) else {
                    continue;
                };

                // Check if this payload contains the instance_pool_id
                if !content.contains(&rule.instance_pool_id) {
                    continue;
                }

                // Parse as JSON and find/replace the pool configuration
                let Ok(mut json_value) = serde_json::from_str::<serde_json::Value>(&content) else {
                    continue;
                };

                let modified =
                    replace_spark_pool_in_json(&mut json_value, &rule.instance_pool_id, config);

                if modified {
                    let new_content = serde_json::to_string(&json_value)
                        .context("Failed to serialize JSON after spark_pool replace")?;
                    part.payload = BASE64.encode(new_content.as_bytes());
                }
            }

            item.content_hash = compute_content_hash(&item.parts);
        }
    }

    Ok(())
}

/// Replace Spark pool configuration in a JSON value tree.
/// Searches for objects containing `instancePoolId` matching the target,
/// then replaces the pool type and name fields.
fn replace_spark_pool_in_json(
    value: &mut serde_json::Value,
    instance_pool_id: &str,
    config: &SparkPoolConfig,
) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            // Check if this object has a matching instancePoolId
            let has_match = map
                .get("instancePoolId")
                .or_else(|| map.get("instance_pool_id"))
                .and_then(|v| v.as_str())
                .is_some_and(|v| v == instance_pool_id);

            if has_match {
                // Replace the pool type and name
                if let Some(t) = map.get_mut("type") {
                    *t = serde_json::Value::from(config.pool_type.clone());
                }
                if let Some(n) = map.get_mut("name") {
                    *n = serde_json::Value::from(config.name.clone());
                }
                return true;
            }

            // Recurse into child values
            let mut modified = false;
            for v in map.values_mut() {
                if replace_spark_pool_in_json(v, instance_pool_id, config) {
                    modified = true;
                }
            }
            modified
        }
        serde_json::Value::Array(arr) => {
            let mut modified = false;
            for v in arr {
                if replace_spark_pool_in_json(v, instance_pool_id, config) {
                    modified = true;
                }
            }
            modified
        }
        _ => false,
    }
}

/// Apply `semantic_model_binding` rules to a source workspace.
fn apply_semantic_model_binding(
    source: &mut SourceWorkspace,
    binding: &SemanticModelBinding,
    env: &str,
    warnings: &mut Vec<String>,
) -> Result<()> {
    for item in &mut source.items {
        // Only apply to SemanticModel items
        if !item
            .metadata
            .item_type
            .eq_ignore_ascii_case("SemanticModel")
        {
            continue;
        }

        // Find the connection ID for this model
        let connection_id = find_model_connection_id(binding, &item.metadata.display_name, env);

        let Some(connection_id) = connection_id else {
            continue; // No binding for this model + env
        };

        // Find and replace in definition.pbism or connection-related parts
        for part in &mut item.parts {
            let Ok(decoded) = BASE64.decode(&part.payload) else {
                continue;
            };
            let Ok(content) = String::from_utf8(decoded) else {
                continue;
            };

            let Ok(mut json_value) = serde_json::from_str::<serde_json::Value>(&content) else {
                continue;
            };

            let modified = replace_connection_id_in_json(&mut json_value, &connection_id);

            if modified {
                let new_content = serde_json::to_string(&json_value)
                    .context("Failed to serialize JSON after semantic_model_binding")?;
                part.payload = BASE64.encode(new_content.as_bytes());
            }
        }

        item.content_hash = compute_content_hash(&item.parts);
    }

    if binding.default.is_none() && binding.models.is_empty() {
        warnings.push("semantic_model_binding: no default or models defined".to_owned());
    }

    Ok(())
}

/// Apply `label_replace` to governance metadata on source items.
///
/// For each item with a sensitivity label, resolves the label ID through the map:
/// - `Some(target_id)` → substitute to the target ID
/// - `None` → explicitly skip (remove the label)
/// - Absent from map → pass through unchanged
fn apply_label_replace(source: &mut SourceWorkspace, label_map: &HashMap<String, Option<String>>) {
    for item in &mut source.items {
        let Some(ref mut governance) = item.governance else {
            continue;
        };
        let Some(ref label) = governance.sensitivity_label else {
            continue;
        };

        if let Some(replacement) = label_map.get(&label.id) {
            match replacement {
                Some(target_id) => {
                    // Mapped to a different label ID in the target tenant
                    governance.sensitivity_label = Some(super::platform::SensitivityLabelRef {
                        id: target_id.clone(),
                    });
                }
                None => {
                    // Explicitly skipped — do not apply this label in target
                    governance.sensitivity_label = None;
                }
            }
        }
        // Absent from map → pass through unchanged (no-op)

        // If governance has no label and no tags left, clear it entirely
        if governance.sensitivity_label.is_none() && governance.tags.is_empty() {
            item.governance = None;
        }
    }
}

/// Apply `tag_replace` to governance metadata on source items.
///
/// For each item with tags, resolves each tag ID through the map:
/// - `Some(target_id)` → substitute to the target ID
/// - `None` → explicitly skip (remove the tag)
/// - Absent from map → pass through unchanged
fn apply_tag_replace(source: &mut SourceWorkspace, tag_map: &HashMap<String, Option<String>>) {
    for item in &mut source.items {
        let Some(ref mut governance) = item.governance else {
            continue;
        };
        if governance.tags.is_empty() {
            continue;
        }

        let resolved_tags: Vec<super::platform::TagRef> = governance
            .tags
            .iter()
            .filter_map(|tag| {
                tag_map.get(&tag.id).map_or_else(
                    // Not in map → pass through unchanged
                    || Some(tag.clone()),
                    // Tag is in the map: Some(id) = mapped, None = skip
                    |replacement| {
                        replacement
                            .as_ref()
                            .map(|target_id| super::platform::TagRef {
                                id: target_id.clone(),
                                display_name: tag.display_name.clone(),
                            })
                    },
                )
            })
            .collect();

        governance.tags = resolved_tags;

        // If governance has no label and no tags left, clear it entirely
        if governance.sensitivity_label.is_none() && governance.tags.is_empty() {
            item.governance = None;
        }
    }
}

/// Find the connection ID for a specific model in the binding config.
fn find_model_connection_id(
    binding: &SemanticModelBinding,
    model_name: &str,
    env: &str,
) -> Option<String> {
    // Check model-specific overrides first
    for model in &binding.models {
        if model.semantic_model_name.contains(model_name) {
            return get_env_value_str(&model.connection_id, env).map(str::to_owned);
        }
    }

    // Fall back to default
    if let Some(ref default) = binding.default {
        return get_env_value_str(&default.connection_id, env).map(str::to_owned);
    }

    None
}

/// Replace connection IDs in semantic model JSON structures.
/// Looks for `connectionId`, `connection_id`, or connection string patterns.
fn replace_connection_id_in_json(value: &mut serde_json::Value, new_connection_id: &str) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            let mut modified = false;

            // Look for connection ID fields
            for key in &["connectionId", "connection_id", "pbiModelDatabaseName"] {
                if let Some(v) = map.get_mut(*key)
                    && v.is_string()
                {
                    let old = v.as_str().unwrap_or_default();
                    // Only replace if it looks like a GUID
                    if old.len() == 36 && old.contains('-') {
                        *v = serde_json::Value::from(new_connection_id);
                        modified = true;
                    }
                }
            }

            // Also handle connectionString containing semanticmodelid=<UUID>
            if let Some(v) = map.get_mut("connectionString")
                && let Some(cs) = v.as_str()
                && cs.contains("semanticmodelid=")
            {
                let re = Regex::new(r"semanticmodelid=([0-9a-fA-F-]{36})").expect("valid regex");
                let replacement = format!("semanticmodelid={new_connection_id}");
                let new_cs = re.replace(cs, replacement.as_str());
                if new_cs != cs {
                    *v = serde_json::Value::from(new_cs.into_owned());
                    modified = true;
                }
            }

            // Recurse
            for v in map.values_mut() {
                if replace_connection_id_in_json(v, new_connection_id) {
                    modified = true;
                }
            }
            modified
        }
        serde_json::Value::Array(arr) => {
            let mut modified = false;
            for v in arr {
                if replace_connection_id_in_json(v, new_connection_id) {
                    modified = true;
                }
            }
            modified
        }
        _ => false,
    }
}

/// Get an environment value from a string `HashMap` (for `connection_id` maps).
fn get_env_value_str<'a>(map: &'a HashMap<String, String>, env: &str) -> Option<&'a str> {
    for (key, value) in map {
        if key.eq_ignore_ascii_case(env) {
            return Some(value.as_str());
        }
    }
    for (key, value) in map {
        if key.eq_ignore_ascii_case("_ALL_") {
            return Some(value.as_str());
        }
    }
    None
}

/// Get an environment value from a JSON value `HashMap`.
fn get_env_value_json<'a>(
    map: &'a HashMap<String, serde_json::Value>,
    env: &str,
) -> Option<&'a serde_json::Value> {
    for (key, value) in map {
        if key.eq_ignore_ascii_case(env) {
            return Some(value);
        }
    }
    for (key, value) in map {
        if key.eq_ignore_ascii_case("_ALL_") {
            return Some(value);
        }
    }
    None
}

/// Get spark pool config for a specific environment.
fn get_spark_pool_config<'a>(
    map: &'a HashMap<String, SparkPoolConfig>,
    env: &str,
) -> Option<&'a SparkPoolConfig> {
    for (key, value) in map {
        if key.eq_ignore_ascii_case(env) {
            return Some(value);
        }
    }
    for (key, value) in map {
        if key.eq_ignore_ascii_case("_ALL_") {
            return Some(value);
        }
    }
    None
}

/// Replace the content matched by capture group 1 in a regex match.
///
/// fabric-cicd semantics: the full match is preserved, but the content of
/// group 1 is replaced with the new value.
pub(super) fn replace_capture_group(re: &Regex, text: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;

    for caps in re.captures_iter(text) {
        let full_match = caps.get(0).unwrap();

        // Append text before this match
        result.push_str(&text[last_end..full_match.start()]);

        if let Some(group1) = caps.get(1) {
            // Replace group 1 within the full match
            let before_group = &text[full_match.start()..group1.start()];
            let after_group = &text[group1.end()..full_match.end()];
            result.push_str(before_group);
            result.push_str(replacement);
            result.push_str(after_group);
        } else {
            // No group 1 — keep original match
            result.push_str(full_match.as_str());
        }

        last_end = full_match.end();
    }

    // Append remaining text
    result.push_str(&text[last_end..]);
    result
}

/// Compute content hash — delegates to shared implementation in platform.rs.
fn compute_content_hash(parts: &[DefinitionPart]) -> String {
    super::platform::compute_content_hash(parts)
}

/// Internal: compiled rule for efficient application.
struct CompiledRule<'a> {
    pattern: RulePattern,
    replacement: String,
    rule: &'a FindReplaceRule,
}

/// Internal: a pattern that has been validated/compiled.
enum RulePattern {
    Literal(String),
    Regex(Regex),
}

#[cfg(test)]
mod tests {
    use super::super::platform::{PlatformMetadata, SourceItem};
    use super::*;

    #[test]
    fn test_parse_parameters_basic() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("params.json");
        std::fs::write(
            &path,
            r#"{
                "find_replace": [
                    {
                        "find_value": "dev-server.database.windows.net",
                        "replace_value": {
                            "prod": "prod-server.database.windows.net"
                        }
                    }
                ]
            }"#,
        )
        .unwrap();

        let params = parse_parameters(&path).unwrap();
        assert_eq!(params.find_replace.len(), 1);
        assert_eq!(
            params.find_replace[0].find_value,
            "dev-server.database.windows.net"
        );
        assert!(!params.find_replace[0].is_regex);
    }

    #[test]
    fn test_parse_parameters_regex_rule() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("params.json");
        std::fs::write(
            &path,
            r#"{
                "find_replace": [
                    {
                        "find_value": "\"lakehouseId\":\\s*\"([0-9a-f-]{36})\"",
                        "replace_value": { "_ALL_": "new-lakehouse-id" },
                        "is_regex": true
                    }
                ]
            }"#,
        )
        .unwrap();

        let params = parse_parameters(&path).unwrap();
        assert_eq!(params.find_replace.len(), 1);
        assert!(params.find_replace[0].is_regex);
    }

    #[test]
    fn test_parse_parameters_invalid_regex() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("params.json");
        std::fs::write(
            &path,
            r#"{
                "find_replace": [
                    {
                        "find_value": "[invalid",
                        "replace_value": { "prod": "x" },
                        "is_regex": true
                    }
                ]
            }"#,
        )
        .unwrap();

        let result = parse_parameters(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid regex"));
    }

    #[test]
    fn test_resolve_value_workspace_id() {
        let ctx = SubstitutionContext {
            workspace_id: "ws-123",
            workspace_name: Some("MyWorkspace"),
            deployed_items: &HashMap::new(),
        };
        assert_eq!(resolve_value("$workspace.id", &ctx).unwrap(), "ws-123");
        assert_eq!(
            resolve_value("$workspace.name", &ctx).unwrap(),
            "MyWorkspace"
        );
    }

    #[test]
    fn test_resolve_value_items_ref() {
        let mut items = HashMap::new();
        items.insert(
            ("Lakehouse".to_owned(), "SalesLH".to_owned()),
            "guid-123".to_owned(),
        );

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &items,
        };

        assert_eq!(
            resolve_value("$items.Lakehouse.SalesLH.id", &ctx).unwrap(),
            "guid-123"
        );
    }

    #[test]
    fn test_resolve_value_env_var() {
        // Use a standard env var that's always present on all platforms
        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        // PATH exists on all CI/dev systems
        let result = resolve_value("$ENV:PATH", &ctx);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());

        // Non-existent env var should error
        let result = resolve_value("$ENV:FABIO_NONEXISTENT_VAR_12345", &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not set"));
    }

    #[test]
    fn test_resolve_value_literal() {
        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };
        assert_eq!(resolve_value("plain-string", &ctx).unwrap(), "plain-string");
    }

    #[test]
    fn test_get_env_value_exact_match() {
        let rule = FindReplaceRule {
            find_value: "x".to_owned(),
            replace_value: HashMap::from([
                ("dev".to_owned(), "dev-val".to_owned()),
                ("prod".to_owned(), "prod-val".to_owned()),
            ]),
            is_regex: false,
            item_type: None,
            item_name: None,
            file_path: None,
        };

        assert_eq!(get_env_value(&rule, "prod"), Some("prod-val"));
        assert_eq!(get_env_value(&rule, "staging"), None);
    }

    #[test]
    fn test_get_env_value_all_wildcard() {
        let rule = FindReplaceRule {
            find_value: "x".to_owned(),
            replace_value: HashMap::from([("_ALL_".to_owned(), "universal".to_owned())]),
            is_regex: false,
            item_type: None,
            item_name: None,
            file_path: None,
        };

        assert_eq!(get_env_value(&rule, "anything"), Some("universal"));
    }

    #[test]
    fn test_replace_capture_group() {
        let re = Regex::new(r#""lakehouseId":\s*"([^"]+)""#).unwrap();
        let text = r#"{"lakehouseId": "old-guid-123"}"#;
        let result = replace_capture_group(&re, text, "new-guid-456");
        assert_eq!(result, r#"{"lakehouseId": "new-guid-456"}"#);
    }

    #[test]
    fn test_replace_capture_group_multiple_matches() {
        let re = Regex::new(r#"id="([^"]+)""#).unwrap();
        let text = r#"id="aaa" and id="bbb""#;
        let result = replace_capture_group(&re, text, "xxx");
        assert_eq!(result, r#"id="xxx" and id="xxx""#);
    }

    #[test]
    fn test_literal_substitution() {
        let params = Parameters {
            find_replace: vec![FindReplaceRule {
                find_value: "old-server.database.windows.net".to_owned(),
                replace_value: HashMap::from([(
                    "prod".to_owned(),
                    "prod-server.database.windows.net".to_owned(),
                )]),
                is_regex: false,
                item_type: None,
                item_name: None,
                file_path: None,
            }],
            key_value_replace: Vec::new(),
            spark_pool: Vec::new(),
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let content = r#"{"server": "old-server.database.windows.net"}"#;
        let encoded = BASE64.encode(content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "DataPipeline".to_owned(),
                    display_name: "ETL".to_owned(),
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
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        let warnings = apply_parameters(&mut source, &params, "prod", &ctx).unwrap();
        assert!(warnings.is_empty());

        // Verify the payload was substituted
        let new_payload = &source.items[0].parts[0].payload;
        let decoded = BASE64.decode(new_payload).unwrap();
        let text = String::from_utf8(decoded).unwrap();
        assert!(text.contains("prod-server.database.windows.net"));
        assert!(!text.contains("old-server.database.windows.net"));

        // Hash should have changed
        assert_ne!(source.items[0].content_hash, "sha256:old");
    }

    #[test]
    fn test_substitution_scoped_by_item_type() {
        let params = Parameters {
            find_replace: vec![FindReplaceRule {
                find_value: "REPLACE_ME".to_owned(),
                replace_value: HashMap::from([("prod".to_owned(), "REPLACED".to_owned())]),
                is_regex: false,
                item_type: Some(StringOrVec::Single("Notebook".to_owned())),
                item_name: None,
                file_path: None,
            }],
            key_value_replace: Vec::new(),
            spark_pool: Vec::new(),
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let content = "REPLACE_ME";
        let encoded = BASE64.encode(content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![
                SourceItem {
                    metadata: super::super::platform::PlatformMetadata {
                        item_type: "Notebook".to_owned(),
                        display_name: "NB1".to_owned(),
                        logical_id: None,
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![DefinitionPart {
                        path: "notebook-content.py".to_owned(),
                        payload: encoded.clone(),
                        payload_type: "InlineBase64".to_owned(),
                    }],
                    content_hash: "sha256:a".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
                SourceItem {
                    metadata: super::super::platform::PlatformMetadata {
                        item_type: "DataPipeline".to_owned(),
                        display_name: "PL1".to_owned(),
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
                    content_hash: "sha256:b".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
            ],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        // Notebook should be substituted
        let nb_payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        assert_eq!(String::from_utf8(nb_payload).unwrap(), "REPLACED");

        // Pipeline should NOT be substituted (wrong type)
        let pl_payload = BASE64.decode(&source.items[1].parts[0].payload).unwrap();
        assert_eq!(String::from_utf8(pl_payload).unwrap(), "REPLACE_ME");
    }

    #[test]
    fn test_key_value_replace_basic() {
        let json_content = r#"{"server": "dev-server.database.windows.net", "port": 1433}"#;
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "DataPipeline".to_owned(),
                    display_name: "MyPipeline".to_owned(),
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
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: vec![KeyValueReplaceRule {
                find_key: "$.server".to_owned(),
                replace_value: HashMap::from([(
                    "prod".to_owned(),
                    serde_json::Value::from("prod-server.database.windows.net"),
                )]),
                item_type: None,
                item_name: None,
                file_path: None,
            }],
            spark_pool: Vec::new(),
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        let warnings = apply_parameters(&mut source, &params, "prod", &ctx).unwrap();
        assert!(warnings.is_empty());

        let payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let result: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(
            result["server"],
            serde_json::Value::from("prod-server.database.windows.net")
        );
        // port should be unchanged
        assert_eq!(result["port"], serde_json::json!(1433));
        // content hash should be recomputed
        assert_ne!(source.items[0].content_hash, "sha256:old");
    }

    #[test]
    fn test_key_value_replace_nested_path() {
        let json_content = r#"{"config": {"database": {"host": "dev.example.com"}}}"#;
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "Notebook".to_owned(),
                    display_name: "NB".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "config.json".to_owned(),
                    payload: encoded,
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: vec![KeyValueReplaceRule {
                find_key: "$.config.database.host".to_owned(),
                replace_value: HashMap::from([(
                    "prod".to_owned(),
                    serde_json::Value::from("prod.example.com"),
                )]),
                item_type: None,
                item_name: None,
                file_path: None,
            }],
            spark_pool: Vec::new(),
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        let payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let result: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(
            result["config"]["database"]["host"],
            serde_json::Value::from("prod.example.com")
        );
    }

    #[test]
    fn test_key_value_replace_with_numeric_value() {
        let json_content = r#"{"retryCount": 3, "timeout": 30}"#;
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "DataPipeline".to_owned(),
                    display_name: "PL".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "pipeline.json".to_owned(),
                    payload: encoded,
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: vec![KeyValueReplaceRule {
                find_key: "$.timeout".to_owned(),
                replace_value: HashMap::from([("prod".to_owned(), serde_json::json!(120))]),
                item_type: None,
                item_name: None,
                file_path: None,
            }],
            spark_pool: Vec::new(),
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        let payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let result: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(result["timeout"], serde_json::json!(120));
        assert_eq!(result["retryCount"], serde_json::json!(3)); // unchanged
    }

    #[test]
    fn test_key_value_replace_scoped_by_item_type() {
        let json_content = r#"{"server": "dev.example.com"}"#;
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![
                SourceItem {
                    metadata: super::super::platform::PlatformMetadata {
                        item_type: "DataPipeline".to_owned(),
                        display_name: "PL".to_owned(),
                        logical_id: None,
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![DefinitionPart {
                        path: "pipeline.json".to_owned(),
                        payload: encoded.clone(),
                        payload_type: "InlineBase64".to_owned(),
                    }],
                    content_hash: "sha256:a".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
                SourceItem {
                    metadata: super::super::platform::PlatformMetadata {
                        item_type: "Notebook".to_owned(),
                        display_name: "NB".to_owned(),
                        logical_id: None,
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![DefinitionPart {
                        path: "config.json".to_owned(),
                        payload: encoded,
                        payload_type: "InlineBase64".to_owned(),
                    }],
                    content_hash: "sha256:b".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
            ],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: vec![KeyValueReplaceRule {
                find_key: "$.server".to_owned(),
                replace_value: HashMap::from([(
                    "prod".to_owned(),
                    serde_json::Value::from("prod.example.com"),
                )]),
                item_type: Some(StringOrVec::Single("DataPipeline".to_owned())),
                item_name: None,
                file_path: None,
            }],
            spark_pool: Vec::new(),
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        // Pipeline should be modified
        let pl_payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let pl_result: serde_json::Value = serde_json::from_slice(&pl_payload).unwrap();
        assert_eq!(pl_result["server"], serde_json::json!("prod.example.com"));

        // Notebook should NOT be modified (wrong type)
        let nb_payload = BASE64.decode(&source.items[1].parts[0].payload).unwrap();
        let nb_result: serde_json::Value = serde_json::from_slice(&nb_payload).unwrap();
        assert_eq!(nb_result["server"], serde_json::json!("dev.example.com"));
    }

    #[test]
    fn test_key_value_replace_no_match_no_modification() {
        let json_content = r#"{"server": "dev.example.com"}"#;
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "DataPipeline".to_owned(),
                    display_name: "PL".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "pipeline.json".to_owned(),
                    payload: encoded.clone(),
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: vec![KeyValueReplaceRule {
                find_key: "$.nonexistent_key".to_owned(),
                replace_value: HashMap::from([("prod".to_owned(), serde_json::json!("new_value"))]),
                item_type: None,
                item_name: None,
                file_path: None,
            }],
            spark_pool: Vec::new(),
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        // Payload should be unchanged since jsonpath didn't match
        assert_eq!(source.items[0].parts[0].payload, encoded);
    }

    #[test]
    fn test_key_value_replace_missing_env_warns() {
        let json_content = r#"{"server": "dev.example.com"}"#;
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "DataPipeline".to_owned(),
                    display_name: "PL".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "pipeline.json".to_owned(),
                    payload: encoded.clone(),
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: vec![KeyValueReplaceRule {
                find_key: "$.server".to_owned(),
                replace_value: HashMap::from([(
                    "staging".to_owned(),
                    serde_json::json!("staging.example.com"),
                )]),
                item_type: None,
                item_name: None,
                file_path: None,
            }],
            spark_pool: Vec::new(),
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        let warnings = apply_parameters(&mut source, &params, "prod", &ctx).unwrap();
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("no value for env 'prod'"));
        // Payload should remain unchanged
        assert_eq!(source.items[0].parts[0].payload, encoded);
    }

    #[test]
    fn test_spark_pool_replacement() {
        let json_content = serde_json::json!({
            "sparkPoolConfig": {
                "instancePoolId": "aaaaaaaa-1111-2222-3333-444444444444",
                "type": "Workspace",
                "name": "dev-pool"
            }
        })
        .to_string();
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "Environment".to_owned(),
                    display_name: "MyEnv".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "environment.metadata.json".to_owned(),
                    payload: encoded,
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: Vec::new(),
            spark_pool: vec![SparkPoolRule {
                instance_pool_id: "aaaaaaaa-1111-2222-3333-444444444444".to_owned(),
                replace_value: HashMap::from([(
                    "prod".to_owned(),
                    SparkPoolConfig {
                        pool_type: "Capacity".to_owned(),
                        name: "prod-pool".to_owned(),
                    },
                )]),
                item_name: None,
            }],
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        let warnings = apply_parameters(&mut source, &params, "prod", &ctx).unwrap();
        assert!(warnings.is_empty());

        let payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let result: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(
            result["sparkPoolConfig"]["type"],
            serde_json::json!("Capacity")
        );
        assert_eq!(
            result["sparkPoolConfig"]["name"],
            serde_json::json!("prod-pool")
        );
        // instancePoolId should be unchanged (only type/name are replaced)
        assert_eq!(
            result["sparkPoolConfig"]["instancePoolId"],
            serde_json::json!("aaaaaaaa-1111-2222-3333-444444444444")
        );
    }

    #[test]
    fn test_spark_pool_nested_replacement() {
        // Pool config can be nested deeper in the tree
        let json_content = serde_json::json!({
            "compute": {
                "pools": [
                    {
                        "instancePoolId": "bbbbbbbb-1111-2222-3333-444444444444",
                        "type": "Workspace",
                        "name": "dev-spark"
                    }
                ]
            }
        })
        .to_string();
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "SparkJobDefinition".to_owned(),
                    display_name: "SJD".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "spark-job.json".to_owned(),
                    payload: encoded,
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: Vec::new(),
            spark_pool: vec![SparkPoolRule {
                instance_pool_id: "bbbbbbbb-1111-2222-3333-444444444444".to_owned(),
                replace_value: HashMap::from([(
                    "prod".to_owned(),
                    SparkPoolConfig {
                        pool_type: "Capacity".to_owned(),
                        name: "prod-spark".to_owned(),
                    },
                )]),
                item_name: None,
            }],
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        let payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let result: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(
            result["compute"]["pools"][0]["type"],
            serde_json::json!("Capacity")
        );
        assert_eq!(
            result["compute"]["pools"][0]["name"],
            serde_json::json!("prod-spark")
        );
    }

    #[test]
    fn test_spark_pool_no_match_leaves_unchanged() {
        let json_content = r#"{"sparkPoolConfig": {"instancePoolId": "other-id", "type": "Workspace", "name": "x"}}"#;
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "Environment".to_owned(),
                    display_name: "MyEnv".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "env.json".to_owned(),
                    payload: encoded.clone(),
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: Vec::new(),
            spark_pool: vec![SparkPoolRule {
                instance_pool_id: "aaaaaaaa-1111-2222-3333-444444444444".to_owned(),
                replace_value: HashMap::from([(
                    "prod".to_owned(),
                    SparkPoolConfig {
                        pool_type: "Capacity".to_owned(),
                        name: "prod-pool".to_owned(),
                    },
                )]),
                item_name: None,
            }],
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        // Payload unchanged — instance_pool_id string not found in content
        assert_eq!(source.items[0].parts[0].payload, encoded);
    }

    #[test]
    fn test_semantic_model_binding_default() {
        let json_content = serde_json::json!({
            "connectionId": "11111111-aaaa-bbbb-cccc-dddddddddddd",
            "pbiModelDatabaseName": "22222222-aaaa-bbbb-cccc-dddddddddddd"
        })
        .to_string();
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "SemanticModel".to_owned(),
                    display_name: "SalesModel".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "definition.pbism".to_owned(),
                    payload: encoded,
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: Vec::new(),
            spark_pool: Vec::new(),
            semantic_model_binding: Some(SemanticModelBinding {
                default: Some(ConnectionBinding {
                    connection_id: HashMap::from([(
                        "prod".to_owned(),
                        "99999999-aaaa-bbbb-cccc-dddddddddddd".to_owned(),
                    )]),
                }),
                models: Vec::new(),
            }),
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        let warnings = apply_parameters(&mut source, &params, "prod", &ctx).unwrap();
        assert!(warnings.is_empty());

        let payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let result: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(
            result["connectionId"],
            serde_json::json!("99999999-aaaa-bbbb-cccc-dddddddddddd")
        );
        assert_eq!(
            result["pbiModelDatabaseName"],
            serde_json::json!("99999999-aaaa-bbbb-cccc-dddddddddddd")
        );
    }

    #[test]
    fn test_semantic_model_binding_model_override() {
        let json_content = serde_json::json!({
            "connectionId": "11111111-aaaa-bbbb-cccc-dddddddddddd"
        })
        .to_string();
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![
                SourceItem {
                    metadata: super::super::platform::PlatformMetadata {
                        item_type: "SemanticModel".to_owned(),
                        display_name: "SalesModel".to_owned(),
                        logical_id: None,
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![DefinitionPart {
                        path: "definition.pbism".to_owned(),
                        payload: encoded.clone(),
                        payload_type: "InlineBase64".to_owned(),
                    }],
                    content_hash: "sha256:a".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
                SourceItem {
                    metadata: super::super::platform::PlatformMetadata {
                        item_type: "SemanticModel".to_owned(),
                        display_name: "HRModel".to_owned(),
                        logical_id: None,
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![DefinitionPart {
                        path: "definition.pbism".to_owned(),
                        payload: encoded,
                        payload_type: "InlineBase64".to_owned(),
                    }],
                    content_hash: "sha256:b".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
            ],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: Vec::new(),
            spark_pool: Vec::new(),
            semantic_model_binding: Some(SemanticModelBinding {
                default: Some(ConnectionBinding {
                    connection_id: HashMap::from([(
                        "prod".to_owned(),
                        "default-prod-conn-id-aaaa-bbbbbbbbbbbb".to_owned(),
                    )]),
                }),
                models: vec![ModelBinding {
                    semantic_model_name: StringOrVec::Single("SalesModel".to_owned()),
                    connection_id: HashMap::from([(
                        "prod".to_owned(),
                        "sales-prod-conn-id-aaaa-bbbbbbbbbbbb".to_owned(),
                    )]),
                }],
            }),
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        // SalesModel should use model-specific override
        let sales_payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let sales_result: serde_json::Value = serde_json::from_slice(&sales_payload).unwrap();
        assert_eq!(
            sales_result["connectionId"],
            serde_json::json!("sales-prod-conn-id-aaaa-bbbbbbbbbbbb")
        );

        // HRModel should use default binding
        let hr_payload = BASE64.decode(&source.items[1].parts[0].payload).unwrap();
        let hr_result: serde_json::Value = serde_json::from_slice(&hr_payload).unwrap();
        assert_eq!(
            hr_result["connectionId"],
            serde_json::json!("default-prod-conn-id-aaaa-bbbbbbbbbbbb")
        );
    }

    #[test]
    fn test_semantic_model_binding_connection_string_replacement() {
        let json_content = serde_json::json!({
            "connectionString": "Data Source=server;semanticmodelid=11111111-aaaa-bbbb-cccc-dddddddddddd;timeout=30"
        })
        .to_string();
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "SemanticModel".to_owned(),
                    display_name: "Model".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "definition.pbism".to_owned(),
                    payload: encoded,
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: Vec::new(),
            spark_pool: Vec::new(),
            semantic_model_binding: Some(SemanticModelBinding {
                default: Some(ConnectionBinding {
                    connection_id: HashMap::from([(
                        "prod".to_owned(),
                        "99999999-aaaa-bbbb-cccc-dddddddddddd".to_owned(),
                    )]),
                }),
                models: Vec::new(),
            }),
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        let payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let result: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        let conn_str = result["connectionString"].as_str().unwrap();
        assert!(conn_str.contains("semanticmodelid=99999999-aaaa-bbbb-cccc-dddddddddddd"));
        assert!(conn_str.contains("Data Source=server")); // Rest unchanged
    }

    #[test]
    fn test_semantic_model_binding_skips_non_semantic_models() {
        let json_content = serde_json::json!({
            "connectionId": "11111111-aaaa-bbbb-cccc-dddddddddddd"
        })
        .to_string();
        let encoded = BASE64.encode(json_content.as_bytes());

        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "Notebook".to_owned(),
                    display_name: "NB".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "config.json".to_owned(),
                    payload: encoded.clone(),
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:old".to_owned(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: None,
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: Vec::new(),
            key_value_replace: Vec::new(),
            spark_pool: Vec::new(),
            semantic_model_binding: Some(SemanticModelBinding {
                default: Some(ConnectionBinding {
                    connection_id: HashMap::from([(
                        "prod".to_owned(),
                        "99999999-aaaa-bbbb-cccc-dddddddddddd".to_owned(),
                    )]),
                }),
                models: Vec::new(),
            }),
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        // Notebook's connectionId should NOT be modified
        assert_eq!(source.items[0].parts[0].payload, encoded);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn test_all_parameter_types_combined() {
        // Test that all parameter types can run together without interference
        let pipeline_content = serde_json::json!({
            "server": "dev.example.com",
            "instancePoolId": "aaaaaaaa-1111-2222-3333-444444444444",
            "type": "Workspace",
            "name": "dev-pool"
        })
        .to_string();
        let model_content = serde_json::json!({
            "connectionId": "11111111-aaaa-bbbb-cccc-dddddddddddd"
        })
        .to_string();

        let mut source = SourceWorkspace {
            items: vec![
                SourceItem {
                    metadata: super::super::platform::PlatformMetadata {
                        item_type: "DataPipeline".to_owned(),
                        display_name: "PL".to_owned(),
                        logical_id: None,
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![DefinitionPart {
                        path: "pipeline.json".to_owned(),
                        payload: BASE64.encode(pipeline_content.as_bytes()),
                        payload_type: "InlineBase64".to_owned(),
                    }],
                    content_hash: "sha256:a".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
                SourceItem {
                    metadata: super::super::platform::PlatformMetadata {
                        item_type: "SemanticModel".to_owned(),
                        display_name: "SM".to_owned(),
                        logical_id: None,
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![DefinitionPart {
                        path: "definition.pbism".to_owned(),
                        payload: BASE64.encode(model_content.as_bytes()),
                        payload_type: "InlineBase64".to_owned(),
                    }],
                    content_hash: "sha256:b".to_owned(),
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                },
            ],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: vec![FindReplaceRule {
                find_value: "dev.example.com".to_owned(),
                replace_value: HashMap::from([("prod".to_owned(), "prod.example.com".to_owned())]),
                is_regex: false,
                item_type: None,
                item_name: None,
                file_path: None,
            }],
            key_value_replace: vec![KeyValueReplaceRule {
                find_key: "$.server".to_owned(),
                replace_value: HashMap::from([(
                    "prod".to_owned(),
                    serde_json::json!("OVERRIDDEN"),
                )]),
                item_type: Some(StringOrVec::Single("DataPipeline".to_owned())),
                item_name: None,
                file_path: None,
            }],
            spark_pool: vec![SparkPoolRule {
                instance_pool_id: "aaaaaaaa-1111-2222-3333-444444444444".to_owned(),
                replace_value: HashMap::from([(
                    "prod".to_owned(),
                    SparkPoolConfig {
                        pool_type: "Capacity".to_owned(),
                        name: "prod-pool".to_owned(),
                    },
                )]),
                item_name: None,
            }],
            semantic_model_binding: Some(SemanticModelBinding {
                default: Some(ConnectionBinding {
                    connection_id: HashMap::from([(
                        "prod".to_owned(),
                        "99999999-aaaa-bbbb-cccc-dddddddddddd".to_owned(),
                    )]),
                }),
                models: Vec::new(),
            }),
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        let warnings = apply_parameters(&mut source, &params, "prod", &ctx).unwrap();
        assert!(warnings.is_empty());

        // Check pipeline: find_replace runs first (dev→prod), then key_value_replace overrides server
        let pl_payload = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let pl_result: serde_json::Value = serde_json::from_slice(&pl_payload).unwrap();
        // key_value_replace runs AFTER find_replace, so server = "OVERRIDDEN"
        assert_eq!(pl_result["server"], serde_json::json!("OVERRIDDEN"));
        // spark_pool should have replaced type and name
        assert_eq!(pl_result["type"], serde_json::json!("Capacity"));
        assert_eq!(pl_result["name"], serde_json::json!("prod-pool"));

        // Check semantic model binding
        let sm_payload = BASE64.decode(&source.items[1].parts[0].payload).unwrap();
        let sm_result: serde_json::Value = serde_json::from_slice(&sm_payload).unwrap();
        assert_eq!(
            sm_result["connectionId"],
            serde_json::json!("99999999-aaaa-bbbb-cccc-dddddddddddd")
        );
    }

    #[test]
    fn test_parse_parameters_with_all_types() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parameters.json");
        let content = serde_json::json!({
            "find_replace": [{
                "find_value": "old",
                "replace_value": {"prod": "new"}
            }],
            "key_value_replace": [{
                "find_key": "$.config.server",
                "replace_value": {"prod": "new-server"}
            }],
            "spark_pool": [{
                "instance_pool_id": "aaaaaaaa-1111-2222-3333-444444444444",
                "replace_value": {"prod": {"type": "Capacity", "name": "prod-pool"}}
            }],
            "semantic_model_binding": {
                "default": {
                    "connection_id": {"prod": "99999999-prod-conn-id-dddddddddddd"}
                },
                "models": [{
                    "semantic_model_name": "SalesModel",
                    "connection_id": {"prod": "sales-conn-id-aaaa-bbbbbbbbbbbb"}
                }]
            }
        })
        .to_string();
        std::fs::write(&path, &content).unwrap();

        let params = parse_parameters(&path).unwrap();
        assert_eq!(params.find_replace.len(), 1);
        assert_eq!(params.key_value_replace.len(), 1);
        assert_eq!(params.key_value_replace[0].find_key, "$.config.server");
        assert_eq!(params.spark_pool.len(), 1);
        assert_eq!(
            params.spark_pool[0].instance_pool_id,
            "aaaaaaaa-1111-2222-3333-444444444444"
        );
        assert!(params.semantic_model_binding.is_some());
        let binding = params.semantic_model_binding.unwrap();
        assert!(binding.default.is_some());
        assert_eq!(binding.models.len(), 1);
    }

    #[test]
    fn test_parse_parameters_invalid_jsonpath() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parameters.json");
        let content = serde_json::json!({
            "key_value_replace": [{
                "find_key": "$[invalid[[[",
                "replace_value": {"prod": "new"}
            }]
        })
        .to_string();
        std::fs::write(&path, &content).unwrap();

        let result = parse_parameters(&path);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("invalid JSONPath"));
    }

    #[test]
    fn test_find_replace_applies_to_creation_payload() {
        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "KQLDatabase".to_owned(),
                    display_name: "MyDB".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: "sha256:empty".to_owned(),
                creation_payload: Some(serde_json::json!({
                    "databaseType": "ReadWrite",
                    "parentEventhouseItemId": "SOURCE_EH_ID"
                })),
                shortcuts: None,
                governance: None,
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
            }],
            logical_id_index: std::collections::HashMap::new(),
            type_name_index: std::collections::HashMap::new(),
        };

        let params = Parameters {
            find_replace: vec![FindReplaceRule {
                find_value: "SOURCE_EH_ID".to_owned(),
                replace_value: HashMap::from([("prod".to_owned(), "PROD_EH_ID".to_owned())]),
                item_type: None,
                item_name: None,
                file_path: None,
                is_regex: false,
            }],
            key_value_replace: vec![],
            spark_pool: vec![],
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-123",
            workspace_name: Some("TestWS"),
            deployed_items: &std::collections::HashMap::new(),
        };

        let warnings = apply_parameters(&mut source, &params, "prod", &ctx).unwrap();
        assert!(warnings.is_empty());

        let payload = source.items[0].creation_payload.as_ref().unwrap();
        assert_eq!(payload["parentEventhouseItemId"], "PROD_EH_ID");
    }

    #[test]
    fn test_key_value_replace_applies_to_creation_payload() {
        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "KQLDatabase".to_owned(),
                    display_name: "MyDB".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: "sha256:empty".to_owned(),
                creation_payload: Some(serde_json::json!({
                    "databaseType": "ReadWrite",
                    "parentEventhouseItemId": "old-id"
                })),
                shortcuts: None,
                governance: None,
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
            }],
            logical_id_index: std::collections::HashMap::new(),
            type_name_index: std::collections::HashMap::new(),
        };

        let params = Parameters {
            find_replace: vec![],
            key_value_replace: vec![KeyValueReplaceRule {
                find_key: "$.parentEventhouseItemId".to_owned(),
                replace_value: HashMap::from([(
                    "prod".to_owned(),
                    serde_json::json!("new-eh-id-456"),
                )]),
                item_type: None,
                item_name: None,
                file_path: None,
            }],
            spark_pool: vec![],
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-123",
            workspace_name: Some("TestWS"),
            deployed_items: &std::collections::HashMap::new(),
        };

        let warnings = apply_parameters(&mut source, &params, "prod", &ctx).unwrap();
        assert!(warnings.is_empty());

        let payload = source.items[0].creation_payload.as_ref().unwrap();
        assert_eq!(payload["parentEventhouseItemId"], "new-eh-id-456");
    }

    #[test]
    fn test_workspace_id_replace_only_known_keys() {
        // Workspace ID replacement should only affect known keys
        // (workspaceId, default_lakehouse_workspace_id, workspace)
        // NOT itemId or other fields with the default GUID.
        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "Notebook".to_owned(),
                    display_name: "Test".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "notebook-content.py".to_owned(),
                    payload: BASE64.encode(
                        br#"{"workspaceId": "00000000-0000-0000-0000-000000000000", "itemId": "00000000-0000-0000-0000-000000000000", "default_lakehouse_workspace_id": "00000000-0000-0000-0000-000000000000"}"#,
                    ),
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: String::new(),
                creation_payload: None,
                shortcuts: None,
                    governance: None,
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
            }],
            logical_id_index: std::collections::HashMap::new(),
            type_name_index: std::collections::HashMap::new(),
        };

        replace_default_workspace_id(&mut source, "real-workspace-id-123");

        let decoded = BASE64.decode(&source.items[0].parts[0].payload).unwrap();
        let content = String::from_utf8(decoded).unwrap();

        // workspaceId and default_lakehouse_workspace_id should be replaced
        assert!(content.contains("\"workspaceId\": \"real-workspace-id-123\""));
        assert!(content.contains("\"default_lakehouse_workspace_id\": \"real-workspace-id-123\""));
        // itemId should NOT be replaced (not a workspace reference key)
        assert!(content.contains("\"itemId\": \"00000000-0000-0000-0000-000000000000\""));
    }

    #[test]
    fn test_workspace_id_replace_skips_shortcuts() {
        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "Lakehouse".to_owned(),
                    display_name: "LH".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: String::new(),
                creation_payload: None,
                shortcuts: Some(vec![serde_json::json!({
                    "name": "myshortcut",
                    "path": "Tables",
                    "target": {
                        "oneLake": {
                            "workspaceId": "00000000-0000-0000-0000-000000000000",
                            "itemId": "00000000-0000-0000-0000-000000000000"
                        }
                    }
                })]),
                governance: None,
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
            }],
            logical_id_index: std::collections::HashMap::new(),
            type_name_index: std::collections::HashMap::new(),
        };

        replace_default_workspace_id(&mut source, "ws-id-999");

        // Shortcuts should NOT be modified (handled separately in shortcut hooks)
        let shortcut = &source.items[0].shortcuts.as_ref().unwrap()[0];
        assert_eq!(
            shortcut["target"]["oneLake"]["itemId"],
            "00000000-0000-0000-0000-000000000000"
        );
        assert_eq!(
            shortcut["target"]["oneLake"]["workspaceId"],
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn test_find_replace_skips_binary_payloads() {
        // Binary (non-UTF-8) payloads should be skipped without error
        let binary_content: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]; // JPEG magic bytes
        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: super::super::platform::PlatformMetadata {
                    item_type: "Report".to_owned(),
                    display_name: "MyReport".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "image.png".to_owned(),
                    payload: BASE64.encode(&binary_content),
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: String::new(),
                creation_payload: None,
                shortcuts: None,
                governance: None,
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
            }],
            logical_id_index: std::collections::HashMap::new(),
            type_name_index: std::collections::HashMap::new(),
        };

        let params = Parameters {
            find_replace: vec![FindReplaceRule {
                find_value: "anything".to_owned(),
                replace_value: std::iter::once(("dev".to_owned(), "replaced".to_owned())).collect(),
                is_regex: false,
                item_type: None,
                item_name: None,
                file_path: None,
            }],
            key_value_replace: vec![],
            spark_pool: vec![],
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-id",
            workspace_name: None,
            deployed_items: &std::collections::HashMap::new(),
        };

        // Should NOT error — binary payload is skipped gracefully
        let result = apply_parameters(&mut source, &params, "dev", &ctx);
        assert!(result.is_ok());
        // Payload should be unchanged
        assert_eq!(
            source.items[0].parts[0].payload,
            BASE64.encode(&binary_content)
        );
    }

    #[test]
    fn test_label_replace_substitutes_id() {
        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "Notebook".to_owned(),
                    display_name: "NB1".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: String::new(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: Some(super::super::platform::GovernanceMetadata {
                    sensitivity_label: Some(super::super::platform::SensitivityLabelRef {
                        id: "source-label-aaa".to_owned(),
                    }),
                    tags: vec![],
                }),
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: vec![],
            key_value_replace: vec![],
            spark_pool: vec![],
            semantic_model_binding: None,
            label_replace: HashMap::from([(
                "source-label-aaa".to_owned(),
                Some("target-label-bbb".to_owned()),
            )]),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        let gov = source.items[0].governance.as_ref().unwrap();
        assert_eq!(
            gov.sensitivity_label.as_ref().unwrap().id,
            "target-label-bbb"
        );
    }

    #[test]
    fn test_label_replace_null_skips_label() {
        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "Notebook".to_owned(),
                    display_name: "NB1".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: String::new(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: Some(super::super::platform::GovernanceMetadata {
                    sensitivity_label: Some(super::super::platform::SensitivityLabelRef {
                        id: "dev-only-label".to_owned(),
                    }),
                    tags: vec![],
                }),
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: vec![],
            key_value_replace: vec![],
            spark_pool: vec![],
            semantic_model_binding: None,
            label_replace: HashMap::from([("dev-only-label".to_owned(), None)]),
            tag_replace: HashMap::new(),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        // Label should be removed, and since no tags either, governance cleared entirely
        assert!(source.items[0].governance.is_none());
    }

    #[test]
    fn test_tag_replace_filters_and_maps() {
        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "DataPipeline".to_owned(),
                    display_name: "PL1".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: String::new(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: Some(super::super::platform::GovernanceMetadata {
                    sensitivity_label: None,
                    tags: vec![
                        super::super::platform::TagRef {
                            id: "tag-aaa".to_owned(),
                            display_name: "Tag A".to_owned(),
                        },
                        super::super::platform::TagRef {
                            id: "tag-bbb".to_owned(),
                            display_name: "Tag B".to_owned(),
                        },
                        super::super::platform::TagRef {
                            id: "tag-ccc".to_owned(),
                            display_name: "Tag C".to_owned(),
                        },
                    ],
                }),
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: vec![],
            key_value_replace: vec![],
            spark_pool: vec![],
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::from([
                ("tag-aaa".to_owned(), Some("tag-xxx".to_owned())), // map to different ID
                ("tag-bbb".to_owned(), None),                       // skip this tag
                                                                    // tag-ccc not in map → pass through
            ]),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        let gov = source.items[0].governance.as_ref().unwrap();
        assert_eq!(gov.tags.len(), 2);
        assert_eq!(gov.tags[0].id, "tag-xxx"); // mapped
        assert_eq!(gov.tags[0].display_name, "Tag A"); // display name preserved
        assert_eq!(gov.tags[1].id, "tag-ccc"); // passed through
    }

    #[test]
    fn test_unmapped_ids_pass_through() {
        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "Notebook".to_owned(),
                    display_name: "NB".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: String::new(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: Some(super::super::platform::GovernanceMetadata {
                    sensitivity_label: Some(super::super::platform::SensitivityLabelRef {
                        id: "unmapped-label-id".to_owned(),
                    }),
                    tags: vec![super::super::platform::TagRef {
                        id: "unmapped-tag-id".to_owned(),
                        display_name: "Unmapped Tag".to_owned(),
                    }],
                }),
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: vec![],
            key_value_replace: vec![],
            spark_pool: vec![],
            semantic_model_binding: None,
            label_replace: HashMap::from([(
                "some-other-label".to_owned(),
                Some("target".to_owned()),
            )]),
            tag_replace: HashMap::from([("some-other-tag".to_owned(), None)]),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        // Both should be unchanged since their IDs are not in the maps
        let gov = source.items[0].governance.as_ref().unwrap();
        assert_eq!(
            gov.sensitivity_label.as_ref().unwrap().id,
            "unmapped-label-id"
        );
        assert_eq!(gov.tags.len(), 1);
        assert_eq!(gov.tags[0].id, "unmapped-tag-id");
    }

    #[test]
    fn test_label_replace_parsed_from_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("params.json");
        std::fs::write(
            &path,
            r#"{
                "label_replace": {
                    "source-label-uuid": "target-label-uuid",
                    "dev-only-label-uuid": null
                },
                "tag_replace": {
                    "source-tag-uuid": "target-tag-uuid",
                    "dev-only-tag-uuid": null
                }
            }"#,
        )
        .unwrap();

        let params = parse_parameters(&path).unwrap();
        assert_eq!(params.label_replace.len(), 2);
        assert_eq!(
            params.label_replace.get("source-label-uuid"),
            Some(&Some("target-label-uuid".to_owned()))
        );
        assert_eq!(params.label_replace.get("dev-only-label-uuid"), Some(&None));
        assert_eq!(params.tag_replace.len(), 2);
        assert_eq!(
            params.tag_replace.get("source-tag-uuid"),
            Some(&Some("target-tag-uuid".to_owned()))
        );
        assert_eq!(params.tag_replace.get("dev-only-tag-uuid"), Some(&None));
    }

    #[test]
    fn test_all_tags_skipped_clears_governance() {
        let mut source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "Report".to_owned(),
                    display_name: "R1".to_owned(),
                    logical_id: None,
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![],
                content_hash: String::new(),
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
                creation_payload: None,
                shortcuts: None,
                governance: Some(super::super::platform::GovernanceMetadata {
                    sensitivity_label: None,
                    tags: vec![
                        super::super::platform::TagRef {
                            id: "tag-1".to_owned(),
                            display_name: "T1".to_owned(),
                        },
                        super::super::platform::TagRef {
                            id: "tag-2".to_owned(),
                            display_name: "T2".to_owned(),
                        },
                    ],
                }),
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let params = Parameters {
            find_replace: vec![],
            key_value_replace: vec![],
            spark_pool: vec![],
            semantic_model_binding: None,
            label_replace: HashMap::new(),
            tag_replace: HashMap::from([("tag-1".to_owned(), None), ("tag-2".to_owned(), None)]),
        };

        let ctx = SubstitutionContext {
            workspace_id: "ws-1",
            workspace_name: None,
            deployed_items: &HashMap::new(),
        };

        apply_parameters(&mut source, &params, "prod", &ctx).unwrap();

        // All tags skipped + no label → governance cleared entirely
        assert!(source.items[0].governance.is_none());
    }
}
