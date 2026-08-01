use std::collections::HashMap;

use anyhow::{Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cli::Cli;
use crate::client::FabricClient;

use super::changeset::{Change, ChangeAction, Changeset};
use super::platform::SourceWorkspace;

/// Deployed item representation (from workspace API).
#[derive(Debug, Clone)]
pub struct DeployedItem {
    pub id: String,
    pub display_name: String,
    pub item_type: String,
    #[allow(dead_code)]
    pub definition_hash: Option<String>,
}

/// Fetch the list of deployed items in the target workspace.
pub async fn fetch_deployed_items(
    client: &FabricClient,
    workspace: &str,
    item_types: Option<&[String]>,
) -> Result<Vec<DeployedItem>> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/items"),
            "value",
            true,
            None,
        )
        .await?;

    let mut items: Vec<DeployedItem> = resp
        .items
        .iter()
        .filter_map(|v| {
            let id = v.get("id")?.as_str()?.to_owned();
            let display_name = v.get("displayName")?.as_str()?.to_owned();
            let item_type = v.get("type")?.as_str()?.to_owned();
            Some(DeployedItem {
                id,
                display_name,
                item_type,
                definition_hash: None,
            })
        })
        .collect();

    // Filter by item types if specified
    if let Some(types) = item_types {
        items.retain(|item| {
            types
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&item.item_type))
        });
    }

    Ok(items)
}

/// Fetch the definition of a deployed item and compute its content hash.
///
/// Returns `None` if the item doesn't support definitions (e.g., Dashboard).
pub async fn fetch_definition_hash(
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
) -> Result<Option<String>> {
    let path = format!("/workspaces/{workspace}/items/{item_id}/getDefinition");

    let result = client.post(&path, &serde_json::json!({}), true).await;

    match result {
        Ok(data) => {
            let parts = data
                .get("definition")
                .and_then(|d| d.get("parts"))
                .and_then(|p| p.as_array());

            parts.map_or(Ok(None), |parts| Ok(Some(hash_api_parts(parts))))
        }
        Err(e) => {
            let err_str = e.to_string();
            // Some item types don't support getDefinition (404 or specific error)
            if err_str.contains("NOT_FOUND")
                || err_str.contains("not supported")
                || err_str.contains("InvalidItemType")
            {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

/// Fetch a deployed item's logical ID from its `.platform` definition part.
///
/// Returns `None` if the item has no definition, no `.platform` part, or no `logicalId`.
async fn fetch_deployed_logical_id(
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
) -> Result<Option<String>> {
    let path = format!("/workspaces/{workspace}/items/{item_id}/getDefinition");
    let result = client.post(&path, &serde_json::json!({}), true).await;

    match result {
        Ok(data) => {
            let parts = data
                .get("definition")
                .and_then(|d| d.get("parts"))
                .and_then(|p| p.as_array());

            let Some(parts) = parts else {
                return Ok(None);
            };

            // Find the .platform part
            let platform_part = parts.iter().find(|p| {
                p.get("path")
                    .and_then(|v| v.as_str())
                    .is_some_and(|path| path == ".platform")
            });

            let Some(platform_part) = platform_part else {
                return Ok(None);
            };

            // Decode and extract logicalId
            let Some(payload_b64) = platform_part.get("payload").and_then(|v| v.as_str()) else {
                return Ok(None);
            };

            let Ok(bytes) = BASE64.decode(payload_b64) else {
                return Ok(None);
            };

            let Ok(content) = std::str::from_utf8(&bytes) else {
                return Ok(None);
            };

            let Ok(parsed) = serde_json::from_str::<Value>(content) else {
                return Ok(None);
            };

            Ok(parsed
                .get("config")
                .and_then(|c| c.get("logicalId"))
                .and_then(|v| v.as_str())
                .map(str::to_owned))
        }
        Err(_) => Ok(None),
    }
}

/// Compute content hash from API definition parts (same algorithm as local).
/// Excludes `.platform` from hash (API modifies logicalId in it).
fn hash_api_parts(parts: &[Value]) -> String {
    let mut hasher = Sha256::new();

    let mut sorted: Vec<(&str, &str)> = parts
        .iter()
        .filter_map(|p| {
            let path = p.get("path")?.as_str()?;
            // Exclude .platform from hash comparison (API rewrites logicalId)
            if path == ".platform" {
                return None;
            }
            let payload = p.get("payload")?.as_str()?;
            Some((path, payload))
        })
        .collect();
    sorted.sort_by_key(|(path, _)| *path);

    for (path, payload) in sorted {
        hasher.update(path.as_bytes());
        hasher.update(b"\x00");
        hasher.update(payload.as_bytes());
        hasher.update(b"\x00");
    }

    let hash = hasher.finalize();
    super::platform::sha256_hex(&hash)
}

/// Resolve a workspace identifier (ID or name) to a workspace ID.
pub async fn resolve_workspace(client: &FabricClient, workspace: &str) -> Result<String> {
    // If it looks like a GUID, use it directly
    if is_guid(workspace) {
        return Ok(workspace.to_owned());
    }

    // Otherwise, search by name
    let resp = client.get_list("/workspaces", "value", true, None).await?;

    for item in &resp.items {
        if let Some(name) = item.get("displayName").and_then(|v| v.as_str())
            && name.eq_ignore_ascii_case(workspace)
            && let Some(id) = item.get("id").and_then(|v| v.as_str())
        {
            return Ok(id.to_owned());
        }
    }

    bail!("Workspace not found: \"{workspace}\". Use a workspace ID or verify the name.")
}

fn is_guid(s: &str) -> bool {
    let s = s.trim();
    s.len() == 36
        && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && s.chars().filter(|&c| c == '-').count() == 4
}

/// Build a changeset by comparing source items against deployed items.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub async fn build_changeset(
    cli: &Cli,
    client: &FabricClient,
    workspace_id: &str,
    source: &SourceWorkspace,
    deployed_items: &[DeployedItem],
    item_types: Option<&[String]>,
    delete_orphans: bool,
    force_all: bool,
    allow_delete_types: Option<&[String]>,
) -> Result<Changeset> {
    let mut changeset = Changeset::new();

    if !cli.quiet {
        eprintln!(
            "[deploy] comparing {} source item(s) against {} deployed item(s)",
            source.items.len(),
            deployed_items.len()
        );
    }

    // Build deployed lookup: (type, name) → DeployedItem
    let mut deployed_map: HashMap<(String, String), DeployedItem> = HashMap::new();
    for item in deployed_items {
        deployed_map.insert(
            (item.item_type.clone(), item.display_name.clone()),
            item.clone(),
        );
    }

    // Track which deployed items are matched (for orphan detection)
    let mut matched_deployed: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    // First pass: match by (type, name) — the fast path
    let mut unmatched_source: Vec<usize> = Vec::new(); // indices into source.items

    for (idx, source_item) in source.items.iter().enumerate() {
        // Skip if filtered by item type
        if let Some(types) = item_types
            && !types
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&source_item.metadata.item_type))
        {
            continue;
        }

        let key = (
            source_item.metadata.item_type.clone(),
            source_item.metadata.display_name.clone(),
        );

        match deployed_map.get(&key) {
            Some(deployed) => {
                matched_deployed.insert(key);

                if force_all {
                    // Force mode: always update
                    changeset.changes.push(Change {
                        name: source_item.metadata.display_name.clone(),
                        item_type: source_item.metadata.item_type.clone(),
                        action: ChangeAction::Update,
                        reason: "forced update (--force-all)".to_owned(),
                        logical_id: source_item.metadata.logical_id.clone(),
                        deployed_id: Some(deployed.id.clone()),
                        source_hash: Some(source_item.content_hash.clone()),
                        previous_name: None,
                    });
                } else {
                    // Fetch deployed definition hash for comparison
                    let deployed_hash =
                        fetch_definition_hash(client, workspace_id, &deployed.id).await?;

                    match deployed_hash {
                        Some(ref dh) if *dh == source_item.content_hash => {
                            changeset.changes.push(Change {
                                name: source_item.metadata.display_name.clone(),
                                item_type: source_item.metadata.item_type.clone(),
                                action: ChangeAction::Skip,
                                reason: "unchanged".to_owned(),
                                logical_id: source_item.metadata.logical_id.clone(),
                                deployed_id: Some(deployed.id.clone()),
                                source_hash: Some(source_item.content_hash.clone()),
                                previous_name: None,
                            });
                        }
                        _ => {
                            changeset.changes.push(Change {
                                name: source_item.metadata.display_name.clone(),
                                item_type: source_item.metadata.item_type.clone(),
                                action: ChangeAction::Update,
                                reason: "definition changed".to_owned(),
                                logical_id: source_item.metadata.logical_id.clone(),
                                deployed_id: Some(deployed.id.clone()),
                                source_hash: Some(source_item.content_hash.clone()),
                                previous_name: None,
                            });
                        }
                    }
                }
            }
            None => {
                // Not matched by name — track for logical ID rename detection
                unmatched_source.push(idx);
            }
        }

        // Warn if item has no logical ID
        if source_item.metadata.logical_id.is_none() {
            changeset.warnings.push(format!(
                "{} \"{}\" has no logicalId in .platform — rename tracking won't work",
                source_item.metadata.item_type, source_item.metadata.display_name
            ));
        }
    }

    // Second pass: for unmatched source items WITH logical IDs, try to find
    // deployed items of the same type that have the same logical ID (rename detection).
    // Only fetch definitions for unmatched deployed items that could be candidates.
    let unmatched_deployed: Vec<&DeployedItem> = deployed_items
        .iter()
        .filter(|d| !matched_deployed.contains(&(d.item_type.clone(), d.display_name.clone())))
        .collect();

    for &src_idx in &unmatched_source {
        let source_item = &source.items[src_idx];
        let Some(ref source_lid) = source_item.metadata.logical_id else {
            // No logical ID — can't detect rename, treat as new
            changeset.changes.push(Change {
                name: source_item.metadata.display_name.clone(),
                item_type: source_item.metadata.item_type.clone(),
                action: ChangeAction::Create,
                reason: "new item".to_owned(),
                logical_id: source_item.metadata.logical_id.clone(),
                deployed_id: None,
                source_hash: Some(source_item.content_hash.clone()),
                previous_name: None,
            });
            continue;
        };

        // Find candidate deployed items of the same type
        let candidates: Vec<&&DeployedItem> = unmatched_deployed
            .iter()
            .filter(|d| {
                d.item_type
                    .eq_ignore_ascii_case(&source_item.metadata.item_type)
            })
            .collect();

        let mut rename_found = false;

        if !candidates.is_empty() {
            // Check each candidate's definition for a matching logical ID
            for candidate in &candidates {
                if matched_deployed
                    .contains(&(candidate.item_type.clone(), candidate.display_name.clone()))
                {
                    continue;
                }

                if let Ok(Some(lid)) =
                    fetch_deployed_logical_id(client, workspace_id, &candidate.id).await
                    && lid == *source_lid
                {
                    // Found a rename: same logical ID, different name
                    matched_deployed
                        .insert((candidate.item_type.clone(), candidate.display_name.clone()));

                    changeset.changes.push(Change {
                        name: source_item.metadata.display_name.clone(),
                        item_type: source_item.metadata.item_type.clone(),
                        action: ChangeAction::Rename,
                        reason: format!(
                            "renamed from \"{}\" (matched by logical ID)",
                            candidate.display_name
                        ),
                        logical_id: source_item.metadata.logical_id.clone(),
                        deployed_id: Some(candidate.id.clone()),
                        source_hash: Some(source_item.content_hash.clone()),
                        previous_name: Some(candidate.display_name.clone()),
                    });
                    rename_found = true;
                    break;
                }
            }
        }

        if !rename_found {
            changeset.changes.push(Change {
                name: source_item.metadata.display_name.clone(),
                item_type: source_item.metadata.item_type.clone(),
                action: ChangeAction::Create,
                reason: "new item".to_owned(),
                logical_id: source_item.metadata.logical_id.clone(),
                deployed_id: None,
                source_hash: Some(source_item.content_hash.clone()),
                previous_name: None,
            });
        }
    }

    // Detect orphans (deployed but not in source)
    if delete_orphans {
        for (key, deployed) in &deployed_map {
            if !matched_deployed.contains(key) {
                if super::is_delete_allowed(&key.0, allow_delete_types) {
                    changeset.changes.push(Change {
                        name: key.1.clone(),
                        item_type: key.0.clone(),
                        action: ChangeAction::Delete,
                        reason: "not in source".to_owned(),
                        logical_id: None,
                        deployed_id: Some(deployed.id.clone()),
                        source_hash: None,
                        previous_name: None,
                    });
                } else {
                    changeset.warnings.push(format!(
                        "Skipped deletion of {} \"{}\" (protected type). \
                         Pass --allow-delete-types {} to allow.",
                        key.0, key.1, key.0
                    ));
                }
            }
        }
    }

    // Check for unresolved logical ID references
    validate_references(source, &mut changeset);

    Ok(changeset)
}

/// Validate that cross-item references (logical IDs embedded in definitions) can be resolved.
///
/// Scans all source item definition payloads for occurrences of logical IDs from other
/// items in the source. If a referenced item is not being created/updated in the changeset
/// AND doesn't already exist in the workspace, emits a warning.
fn validate_references(source: &SourceWorkspace, changeset: &mut Changeset) {
    // Build set of logical IDs that WILL be available after this deployment:
    // 1. Items being created/updated in the changeset (their logical IDs will resolve)
    // 2. Items being skipped (they already exist with their deployed IDs)
    let resolvable_logical_ids: std::collections::HashSet<&str> = changeset
        .changes
        .iter()
        .filter(|c| {
            matches!(
                c.action,
                ChangeAction::Create | ChangeAction::Update | ChangeAction::Skip
            )
        })
        .filter_map(|c| c.logical_id.as_deref())
        .collect();

    // Scan each source item's definition payloads for references to other items' logical IDs
    for source_item in &source.items {
        let Some(ref _item_logical_id) = source_item.metadata.logical_id else {
            continue;
        };

        for part in &source_item.parts {
            // Decode payload to check for logical ID references
            let Ok(bytes) = BASE64.decode(&part.payload) else {
                continue;
            };
            let Ok(content) = std::str::from_utf8(&bytes) else {
                continue;
            };

            // Check if this payload references any logical ID from the source
            for other_item in &source.items {
                let Some(ref other_lid) = other_item.metadata.logical_id else {
                    continue;
                };

                // Skip self-references
                if std::ptr::eq(source_item, other_item) {
                    continue;
                }

                // If the payload contains this logical ID but it won't be resolvable
                if content.contains(other_lid.as_str())
                    && !resolvable_logical_ids.contains(other_lid.as_str())
                {
                    changeset.warnings.push(format!(
                        "{} \"{}\" references logical ID \"{}\" ({} \"{}\") which is not in the deployment scope",
                        source_item.metadata.item_type,
                        source_item.metadata.display_name,
                        other_lid,
                        other_item.metadata.item_type,
                        other_item.metadata.display_name,
                    ));
                }
            }
        }
    }
}

/// Compute a workspace state fingerprint from the deployed item list.
///
/// This is a lightweight hash over the sorted list of (id, type, name) tuples.
/// Used for plan staleness detection: if the fingerprint changes between plan and apply,
/// it means the workspace state has diverged.
pub fn compute_workspace_fingerprint(deployed_items: &[DeployedItem]) -> String {
    let mut hasher = Sha256::new();

    let mut sorted: Vec<(&str, &str, &str)> = deployed_items
        .iter()
        .map(|item| {
            (
                item.id.as_str(),
                item.item_type.as_str(),
                item.display_name.as_str(),
            )
        })
        .collect();
    sorted.sort_unstable();

    for (id, item_type, name) in sorted {
        hasher.update(id.as_bytes());
        hasher.update(b"\x00");
        hasher.update(item_type.as_bytes());
        hasher.update(b"\x00");
        hasher.update(name.as_bytes());
        hasher.update(b"\x00");
    }

    let hash = hasher.finalize();
    super::platform::sha256_hex(&hash)
}

/// Normalize a content hash from the deployed API response for comparison.
///
/// The Fabric API may return base64 payloads with different whitespace or key ordering.
/// This function re-encodes JSON parts with sorted keys before hashing.
#[allow(dead_code)]
pub fn normalize_and_hash_definition(definition: &Value) -> Option<String> {
    let parts = definition.get("definition")?.get("parts")?.as_array()?;

    let mut normalized_parts: Vec<(&str, String)> = Vec::new();

    for part in parts {
        let path = part.get("path")?.as_str()?;
        let payload = part.get("payload")?.as_str()?;

        // Try to decode, parse as JSON, re-serialize with sorted keys
        let normalized_payload = BASE64.decode(payload).map_or_else(
            |_| payload.to_owned(),
            |decoded| {
                serde_json::from_slice::<Value>(&decoded).map_or_else(
                    |_| payload.to_owned(),
                    |json| {
                        let re_encoded = serde_json::to_string(&json).unwrap_or_default();
                        BASE64.encode(re_encoded.as_bytes())
                    },
                )
            },
        );

        normalized_parts.push((path, normalized_payload));
    }

    normalized_parts.sort_by_key(|(path, _)| *path);

    let mut hasher = Sha256::new();
    for (path, payload) in &normalized_parts {
        hasher.update(path.as_bytes());
        hasher.update(b"\x00");
        hasher.update(payload.as_bytes());
        hasher.update(b"\x00");
    }

    let hash = hasher.finalize();
    Some(super::platform::sha256_hex(&hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    use super::super::platform::{DefinitionPart, PlatformMetadata, SourceItem};

    #[test]
    fn test_is_guid_valid() {
        assert!(is_guid("12345678-1234-1234-1234-123456789abc"));
        assert!(is_guid("ABCDEF00-0000-0000-0000-000000000000"));
        assert!(is_guid("abcdef00-1111-2222-3333-444444444444"));
    }

    #[test]
    fn test_is_guid_invalid() {
        assert!(!is_guid("not-a-guid"));
        assert!(!is_guid("12345678-1234-1234-1234"));
        assert!(!is_guid("12345678123412341234123456789abc")); // no dashes
        assert!(!is_guid("12345678-1234-1234-1234-123456789abcX")); // too long
        assert!(!is_guid("")); // empty
        assert!(!is_guid("My Workspace Name")); // workspace name
    }

    #[test]
    fn test_hash_api_parts_deterministic() {
        let parts = vec![
            serde_json::json!({"path": "b.json", "payload": "Y29udGVudC1i"}),
            serde_json::json!({"path": "a.json", "payload": "Y29udGVudC1h"}),
        ];

        let hash1 = hash_api_parts(&parts);
        assert!(hash1.starts_with("sha256:"));
        assert_eq!(hash1.len(), 7 + 64); // "sha256:" + 64 hex chars

        // Same parts in different order should produce same hash (sorted internally)
        let parts_reversed = vec![parts[1].clone(), parts[0].clone()];
        let hash2 = hash_api_parts(&parts_reversed);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_api_parts_different_content() {
        let parts_a = vec![serde_json::json!({"path": "a.json", "payload": "AAAA"})];
        let parts_b = vec![serde_json::json!({"path": "a.json", "payload": "BBBB"})];

        let hash_a = hash_api_parts(&parts_a);
        let hash_b = hash_api_parts(&parts_b);
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn test_hash_api_parts_different_paths() {
        let parts_a = vec![serde_json::json!({"path": "x.json", "payload": "AAAA"})];
        let parts_b = vec![serde_json::json!({"path": "y.json", "payload": "AAAA"})];

        let hash_a = hash_api_parts(&parts_a);
        let hash_b = hash_api_parts(&parts_b);
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn test_hash_api_parts_skips_malformed() {
        // Missing payload field
        let parts = vec![serde_json::json!({"path": "a.json"})];
        let hash = hash_api_parts(&parts);
        // Should still return a valid hash (of empty input)
        assert!(hash.starts_with("sha256:"));
    }

    #[test]
    fn test_normalize_and_hash_definition_basic() {
        let payload = BASE64.encode(br#"{"z":1,"a":2}"#);
        let definition = serde_json::json!({
            "definition": {
                "parts": [
                    {"path": "file.json", "payload": payload, "payloadType": "InlineBase64"}
                ]
            }
        });

        let hash = normalize_and_hash_definition(&definition);
        assert!(hash.is_some());
        assert!(hash.unwrap().starts_with("sha256:"));
    }

    #[test]
    fn test_normalize_and_hash_definition_key_order_with_preserve_order() {
        // With serde_json `preserve_order` feature enabled, JSON key order
        // is maintained during re-serialization. So different key orders
        // produce different hashes (this is expected behavior — the source
        // of truth is the exact byte sequence, not semantic equivalence).
        let payload_az = BASE64.encode(br#"{"a":1,"z":2}"#);
        let payload_za = BASE64.encode(br#"{"z":2,"a":1}"#);

        let def_az = serde_json::json!({
            "definition": {
                "parts": [{"path": "f.json", "payload": payload_az, "payloadType": "InlineBase64"}]
            }
        });
        let def_za = serde_json::json!({
            "definition": {
                "parts": [{"path": "f.json", "payload": payload_za, "payloadType": "InlineBase64"}]
            }
        });

        let hash_az = normalize_and_hash_definition(&def_az).unwrap();
        let hash_za = normalize_and_hash_definition(&def_za).unwrap();

        // With preserve_order, keys stay in insertion order — hashes differ
        // This is correct: the local source is the canonical representation,
        // and the comparison catches any byte-level change.
        assert_ne!(hash_az, hash_za);
    }

    #[test]
    fn test_normalize_and_hash_definition_same_content_same_hash() {
        // Same exact payload should produce identical hash
        let payload = BASE64.encode(br#"{"key":"value","num":42}"#);

        let def1 = serde_json::json!({
            "definition": {
                "parts": [{"path": "f.json", "payload": payload, "payloadType": "InlineBase64"}]
            }
        });
        let def2 = serde_json::json!({
            "definition": {
                "parts": [{"path": "f.json", "payload": payload, "payloadType": "InlineBase64"}]
            }
        });

        let hash1 = normalize_and_hash_definition(&def1).unwrap();
        let hash2 = normalize_and_hash_definition(&def2).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_normalize_and_hash_definition_non_json_payload() {
        // Binary/non-JSON payload should hash as-is
        let payload = BASE64.encode(b"\x00\x01\x02\x03");
        let definition = serde_json::json!({
            "definition": {
                "parts": [{"path": "binary.dat", "payload": payload, "payloadType": "InlineBase64"}]
            }
        });

        let hash = normalize_and_hash_definition(&definition);
        assert!(hash.is_some());
    }

    #[test]
    fn test_normalize_and_hash_definition_missing_parts() {
        let definition = serde_json::json!({"definition": {}});
        assert!(normalize_and_hash_definition(&definition).is_none());

        let definition2 = serde_json::json!({});
        assert!(normalize_and_hash_definition(&definition2).is_none());
    }

    #[test]
    fn test_compute_workspace_fingerprint_deterministic() {
        let items = vec![
            DeployedItem {
                id: "bbb".to_owned(),
                display_name: "ItemB".to_owned(),
                item_type: "Notebook".to_owned(),
                definition_hash: None,
            },
            DeployedItem {
                id: "aaa".to_owned(),
                display_name: "ItemA".to_owned(),
                item_type: "Lakehouse".to_owned(),
                definition_hash: None,
            },
        ];

        let fp1 = compute_workspace_fingerprint(&items);

        // Reversed order should produce same fingerprint (sorted internally)
        let items_rev = vec![items[1].clone(), items[0].clone()];
        let fp2 = compute_workspace_fingerprint(&items_rev);

        assert_eq!(fp1, fp2);
        assert!(fp1.starts_with("sha256:"));
    }

    #[test]
    fn test_compute_workspace_fingerprint_changes_on_modification() {
        let items1 = vec![DeployedItem {
            id: "aaa".to_owned(),
            display_name: "Item".to_owned(),
            item_type: "Notebook".to_owned(),
            definition_hash: None,
        }];
        let items2 = vec![DeployedItem {
            id: "bbb".to_owned(),
            display_name: "Item".to_owned(),
            item_type: "Notebook".to_owned(),
            definition_hash: None,
        }];

        let fp1 = compute_workspace_fingerprint(&items1);
        let fp2 = compute_workspace_fingerprint(&items2);
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_compute_workspace_fingerprint_empty() {
        let fp = compute_workspace_fingerprint(&[]);
        assert!(fp.starts_with("sha256:"));
    }

    #[test]
    fn test_compute_workspace_fingerprint_sensitive_to_name_change() {
        let items1 = vec![DeployedItem {
            id: "aaa".to_owned(),
            display_name: "ItemA".to_owned(),
            item_type: "Notebook".to_owned(),
            definition_hash: None,
        }];
        let items2 = vec![DeployedItem {
            id: "aaa".to_owned(),
            display_name: "ItemB".to_owned(), // only name changed
            item_type: "Notebook".to_owned(),
            definition_hash: None,
        }];

        let fp1 = compute_workspace_fingerprint(&items1);
        let fp2 = compute_workspace_fingerprint(&items2);
        assert_ne!(fp1, fp2, "Fingerprint should change when item name changes");
    }

    #[test]
    fn test_compute_workspace_fingerprint_sensitive_to_type_change() {
        let items1 = vec![DeployedItem {
            id: "aaa".to_owned(),
            display_name: "Item".to_owned(),
            item_type: "Notebook".to_owned(),
            definition_hash: None,
        }];
        let items2 = vec![DeployedItem {
            id: "aaa".to_owned(),
            display_name: "Item".to_owned(),
            item_type: "Lakehouse".to_owned(), // only type changed
            definition_hash: None,
        }];

        let fp1 = compute_workspace_fingerprint(&items1);
        let fp2 = compute_workspace_fingerprint(&items2);
        assert_ne!(fp1, fp2, "Fingerprint should change when item type changes");
    }

    #[test]
    fn test_compute_workspace_fingerprint_sensitive_to_added_item() {
        let items1 = vec![DeployedItem {
            id: "aaa".to_owned(),
            display_name: "Item".to_owned(),
            item_type: "Notebook".to_owned(),
            definition_hash: None,
        }];
        let items2 = vec![
            DeployedItem {
                id: "aaa".to_owned(),
                display_name: "Item".to_owned(),
                item_type: "Notebook".to_owned(),
                definition_hash: None,
            },
            DeployedItem {
                id: "bbb".to_owned(),
                display_name: "Item2".to_owned(),
                item_type: "Lakehouse".to_owned(),
                definition_hash: None,
            },
        ];

        let fp1 = compute_workspace_fingerprint(&items1);
        let fp2 = compute_workspace_fingerprint(&items2);
        assert_ne!(fp1, fp2, "Fingerprint should change when item is added");
    }

    #[test]
    fn test_compute_workspace_fingerprint_ignores_definition_hash() {
        // definition_hash is not part of fingerprint (it's a field on the struct
        // but fingerprint only uses id, type, name)
        let items1 = vec![DeployedItem {
            id: "aaa".to_owned(),
            display_name: "Item".to_owned(),
            item_type: "Notebook".to_owned(),
            definition_hash: None,
        }];
        let items2 = vec![DeployedItem {
            id: "aaa".to_owned(),
            display_name: "Item".to_owned(),
            item_type: "Notebook".to_owned(),
            definition_hash: Some("sha256:different".to_owned()),
        }];

        let fp1 = compute_workspace_fingerprint(&items1);
        let fp2 = compute_workspace_fingerprint(&items2);
        assert_eq!(
            fp1, fp2,
            "Fingerprint should not change when only definition_hash differs"
        );
    }

    #[test]
    fn test_hash_api_parts_empty_list() {
        let parts: Vec<Value> = vec![];
        let hash = hash_api_parts(&parts);
        assert!(hash.starts_with("sha256:"));
        // Empty input should still produce a consistent hash
        let hash2 = hash_api_parts(&parts);
        assert_eq!(hash, hash2);
    }

    // --- validate_references tests ---

    #[test]
    fn test_validate_references_no_warnings_when_all_resolved() {
        // Source has two items: Notebook references Lakehouse's logical ID
        // Both are in deployment scope (both will be created) → no warnings
        let lakehouse_lid = "lid-lakehouse-001";
        let notebook_payload = format!(r#"{{"defaultLakehouse":"{lakehouse_lid}"}}"#);

        let source = SourceWorkspace {
            items: vec![
                SourceItem {
                    metadata: PlatformMetadata {
                        item_type: "Lakehouse".to_owned(),
                        display_name: "SalesLH".to_owned(),
                        logical_id: Some(lakehouse_lid.to_owned()),
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![],
                    content_hash: "sha256:aaa".to_owned(),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                },
                SourceItem {
                    metadata: PlatformMetadata {
                        item_type: "Notebook".to_owned(),
                        display_name: "ETL".to_owned(),
                        logical_id: Some("lid-notebook-001".to_owned()),
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![DefinitionPart {
                        path: "notebook-content.py".to_owned(),
                        payload: BASE64.encode(notebook_payload.as_bytes()),
                        payload_type: "InlineBase64".to_owned(),
                    }],
                    content_hash: "sha256:bbb".to_owned(),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                },
            ],
            logical_id_index: HashMap::from([("lid-lakehouse-001".to_owned(), 0)]),
            type_name_index: HashMap::new(),
        };

        let mut changeset = Changeset::new();
        // Both items are being created (in scope)
        changeset.changes.push(Change {
            name: "SalesLH".to_owned(),
            item_type: "Lakehouse".to_owned(),
            action: ChangeAction::Create,
            reason: "new".to_owned(),
            logical_id: Some(lakehouse_lid.to_owned()),
            deployed_id: None,
            source_hash: None,
            previous_name: None,
        });
        changeset.changes.push(Change {
            name: "ETL".to_owned(),
            item_type: "Notebook".to_owned(),
            action: ChangeAction::Create,
            reason: "new".to_owned(),
            logical_id: Some("lid-notebook-001".to_owned()),
            deployed_id: None,
            source_hash: None,
            previous_name: None,
        });

        validate_references(&source, &mut changeset);

        assert!(
            changeset.warnings.is_empty(),
            "Expected no warnings, got: {:?}",
            changeset.warnings
        );
    }

    #[test]
    fn test_validate_references_warns_on_unresolvable_reference() {
        // Source has Notebook that references a logical ID of an item NOT in deployment scope
        let external_lid = "lid-external-lakehouse";
        let notebook_payload = format!(r#"{{"defaultLakehouse":"{external_lid}"}}"#);

        let source = SourceWorkspace {
            items: vec![
                SourceItem {
                    metadata: PlatformMetadata {
                        item_type: "Lakehouse".to_owned(),
                        display_name: "ExternalLH".to_owned(),
                        logical_id: Some(external_lid.to_owned()),
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![],
                    content_hash: "sha256:aaa".to_owned(),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                },
                SourceItem {
                    metadata: PlatformMetadata {
                        item_type: "Notebook".to_owned(),
                        display_name: "ETL".to_owned(),
                        logical_id: Some("lid-notebook-001".to_owned()),
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![DefinitionPart {
                        path: "notebook-content.py".to_owned(),
                        payload: BASE64.encode(notebook_payload.as_bytes()),
                        payload_type: "InlineBase64".to_owned(),
                    }],
                    content_hash: "sha256:bbb".to_owned(),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                },
            ],
            logical_id_index: HashMap::from([
                (external_lid.to_owned(), 0),
                ("lid-notebook-001".to_owned(), 1),
            ]),
            type_name_index: HashMap::new(),
        };

        let mut changeset = Changeset::new();
        // Only Notebook is in scope (Create), ExternalLH is marked as Delete (not in scope)
        changeset.changes.push(Change {
            name: "ETL".to_owned(),
            item_type: "Notebook".to_owned(),
            action: ChangeAction::Create,
            reason: "new".to_owned(),
            logical_id: Some("lid-notebook-001".to_owned()),
            deployed_id: None,
            source_hash: None,
            previous_name: None,
        });
        changeset.changes.push(Change {
            name: "ExternalLH".to_owned(),
            item_type: "Lakehouse".to_owned(),
            action: ChangeAction::Delete,
            reason: "orphan".to_owned(),
            logical_id: Some(external_lid.to_owned()),
            deployed_id: Some("deployed-id-123".to_owned()),
            source_hash: None,
            previous_name: None,
        });

        validate_references(&source, &mut changeset);

        assert_eq!(changeset.warnings.len(), 1);
        assert!(changeset.warnings[0].contains("lid-external-lakehouse"));
        assert!(changeset.warnings[0].contains("not in the deployment scope"));
    }

    #[test]
    fn test_validate_references_skip_items_are_resolvable() {
        // An item that is Skipped (already deployed, unchanged) should count as resolvable
        let lakehouse_lid = "lid-lakehouse-skip";
        let notebook_payload = format!(r#"{{"defaultLakehouse":"{lakehouse_lid}"}}"#);

        let source = SourceWorkspace {
            items: vec![
                SourceItem {
                    metadata: PlatformMetadata {
                        item_type: "Lakehouse".to_owned(),
                        display_name: "ExistingLH".to_owned(),
                        logical_id: Some(lakehouse_lid.to_owned()),
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![],
                    content_hash: "sha256:aaa".to_owned(),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                },
                SourceItem {
                    metadata: PlatformMetadata {
                        item_type: "Notebook".to_owned(),
                        display_name: "ETL".to_owned(),
                        logical_id: Some("lid-notebook-002".to_owned()),
                        description: None,
                        definition_format: None,
                        sensitivity_label_id: None,
                        platform_creation_payload: None,
                    },
                    parts: vec![DefinitionPart {
                        path: "notebook-content.py".to_owned(),
                        payload: BASE64.encode(notebook_payload.as_bytes()),
                        payload_type: "InlineBase64".to_owned(),
                    }],
                    content_hash: "sha256:bbb".to_owned(),
                    creation_payload: None,
                    shortcuts: None,
                    governance: None,
                    schedules: None,
                    folder_path: String::new(),
                    source_path: std::path::PathBuf::from("/tmp"),
                },
            ],
            logical_id_index: HashMap::from([
                (lakehouse_lid.to_owned(), 0),
                ("lid-notebook-002".to_owned(), 1),
            ]),
            type_name_index: HashMap::new(),
        };

        let mut changeset = Changeset::new();
        // Lakehouse is Skip (already deployed with same hash)
        changeset.changes.push(Change {
            name: "ExistingLH".to_owned(),
            item_type: "Lakehouse".to_owned(),
            action: ChangeAction::Skip,
            reason: "unchanged".to_owned(),
            logical_id: Some(lakehouse_lid.to_owned()),
            deployed_id: Some("deployed-lh-id".to_owned()),
            source_hash: None,
            previous_name: None,
        });
        changeset.changes.push(Change {
            name: "ETL".to_owned(),
            item_type: "Notebook".to_owned(),
            action: ChangeAction::Create,
            reason: "new".to_owned(),
            logical_id: Some("lid-notebook-002".to_owned()),
            deployed_id: None,
            source_hash: None,
            previous_name: None,
        });

        validate_references(&source, &mut changeset);

        assert!(
            changeset.warnings.is_empty(),
            "Skip items should be resolvable, got: {:?}",
            changeset.warnings
        );
    }

    #[test]
    fn test_validate_references_no_false_positives_for_items_without_logical_id() {
        // Items without logical IDs should not trigger validation warnings
        let source = SourceWorkspace {
            items: vec![SourceItem {
                metadata: PlatformMetadata {
                    item_type: "Notebook".to_owned(),
                    display_name: "Simple".to_owned(),
                    logical_id: None, // No logical ID → skip validation
                    description: None,
                    definition_format: None,
                    sensitivity_label_id: None,
                    platform_creation_payload: None,
                },
                parts: vec![DefinitionPart {
                    path: "notebook-content.py".to_owned(),
                    payload: BASE64.encode(b"some content with random-guid-looking-string"),
                    payload_type: "InlineBase64".to_owned(),
                }],
                content_hash: "sha256:ccc".to_owned(),
                creation_payload: None,
                shortcuts: None,
                governance: None,
                schedules: None,
                folder_path: String::new(),
                source_path: std::path::PathBuf::from("/tmp"),
            }],
            logical_id_index: HashMap::new(),
            type_name_index: HashMap::new(),
        };

        let mut changeset = Changeset::new();
        changeset.changes.push(Change {
            name: "Simple".to_owned(),
            item_type: "Notebook".to_owned(),
            action: ChangeAction::Create,
            reason: "new".to_owned(),
            logical_id: None,
            deployed_id: None,
            source_hash: None,
            previous_name: None,
        });

        validate_references(&source, &mut changeset);

        assert!(changeset.warnings.is_empty());
    }
}
