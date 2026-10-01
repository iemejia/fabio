//! Scaffolds a `parameters.json` file by scanning or diffing exported workspace definitions.
//!
//! Two modes:
//! - **Scan mode** (`--source` only): Finds GUIDs and common environment-specific patterns
//! - **Diff mode** (`--source` + `--compare`): Compares two exports and generates rules for differing values

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use regex::Regex;
use serde_json::json;

use super::platform::{self, SourceItem};

/// GUID regex pattern for scanning definitions.
const GUID_PATTERN: &str =
    r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}";

/// Result of scanning/diffing for parameterization candidates.
#[derive(Debug)]
pub struct InitParamsResult {
    /// Generated parameters JSON content.
    pub parameters_json: serde_json::Value,
    /// Human-readable summary of what was found.
    pub summary: InitSummary,
}

#[derive(Debug)]
pub struct InitSummary {
    pub mode: &'static str,
    pub source_items: usize,
    pub compare_items: usize,
    pub rules_generated: usize,
    pub guids_found: usize,
}

/// Scan a single source directory for GUID patterns that likely need parameterization.
pub fn scan_for_candidates(source: &Path) -> Result<InitParamsResult> {
    let workspace = platform::parse_source_directory(source)?;

    let mut guid_occurrences: BTreeMap<String, Vec<GuidLocation>> = BTreeMap::new();
    let guid_re = Regex::new(GUID_PATTERN).expect("valid regex");

    for item in &workspace.items {
        for part in &item.parts {
            let Some(decoded) = decode_payload(&part.payload) else {
                continue; // Skip binary (non-UTF-8) payloads
            };
            for mat in guid_re.find_iter(&decoded) {
                let guid = mat.as_str().to_lowercase();
                guid_occurrences
                    .entry(guid)
                    .or_default()
                    .push(GuidLocation {
                        item_type: item.metadata.item_type.clone(),
                        item_name: item.metadata.display_name.clone(),
                        file_path: part.path.clone(),
                    });
            }
        }
    }

    // Filter: only GUIDs that appear in definition payloads (not just metadata)
    // and exclude common well-known GUIDs (all zeros, etc.)
    let candidates: BTreeMap<String, Vec<GuidLocation>> = guid_occurrences
        .into_iter()
        .filter(|(guid, _)| !is_well_known_guid(guid))
        .collect();

    // Build rules
    let rules: Vec<serde_json::Value> = candidates
        .iter()
        .map(|(guid, locations)| {
            let mut rule = json!({
                "find_value": guid,
                "replace_value": {
                    "_ALL_": format!("TODO_REPLACE_{}", &guid[..8])
                }
            });

            // If all occurrences are in one item type, scope it
            let types: BTreeSet<&str> = locations.iter().map(|l| l.item_type.as_str()).collect();
            if types.len() == 1 {
                rule["item_type"] = json!(types.into_iter().next().unwrap());
            }

            // If all occurrences are in one item, scope by name too
            let names: BTreeSet<&str> = locations.iter().map(|l| l.item_name.as_str()).collect();
            if names.len() == 1 {
                rule["item_name"] = json!(names.into_iter().next().unwrap());
            }

            rule
        })
        .collect();

    let guids_found = candidates.len();
    let rules_count = rules.len();

    let parameters_json = json!({
        "find_replace": rules
    });

    Ok(InitParamsResult {
        parameters_json,
        summary: InitSummary {
            mode: "scan",
            source_items: workspace.items.len(),
            compare_items: 0,
            rules_generated: rules_count,
            guids_found,
        },
    })
}

/// Compare two exported directories and generate rules for values that differ.
#[allow(clippy::too_many_lines)]
pub fn diff_for_parameters(
    source: &Path,
    compare: &Path,
    source_env: &str,
    compare_env: &str,
) -> Result<InitParamsResult> {
    let source_ws = platform::parse_source_directory(source)?;
    let compare_ws = platform::parse_source_directory(compare)?;

    // Build index for compare workspace by (type, name)
    let compare_index: HashMap<(&str, &str), &SourceItem> = compare_ws
        .items
        .iter()
        .map(|item| {
            (
                item.metadata.item_type.as_str(),
                item.metadata.display_name.as_str(),
            )
        })
        .zip(compare_ws.items.iter())
        .collect();

    let guid_re = Regex::new(GUID_PATTERN).expect("valid regex");
    let mut rules: Vec<serde_json::Value> = Vec::new();
    let mut seen_pairs: BTreeSet<(String, String)> = BTreeSet::new();

    for source_item in &source_ws.items {
        let key = (
            source_item.metadata.item_type.as_str(),
            source_item.metadata.display_name.as_str(),
        );

        let Some(compare_item) = compare_index.get(&key) else {
            continue; // Item only exists in source
        };

        // Compare each definition part
        for source_part in &source_item.parts {
            let Some(compare_part) = compare_item
                .parts
                .iter()
                .find(|p| p.path == source_part.path)
            else {
                continue; // Part only exists in source
            };

            let source_decoded = decode_payload(&source_part.payload);
            let compare_decoded = decode_payload(&compare_part.payload);

            let (Some(source_decoded), Some(compare_decoded)) = (source_decoded, compare_decoded)
            else {
                continue; // Skip binary (non-UTF-8) payloads
            };

            if source_decoded == compare_decoded {
                continue; // Identical
            }

            // Find GUIDs in source that don't appear in compare (and vice versa)
            let source_guids: BTreeSet<String> = guid_re
                .find_iter(&source_decoded)
                .map(|m| m.as_str().to_lowercase())
                .filter(|g| !is_well_known_guid(g))
                .collect();

            let compare_guids: BTreeSet<String> = guid_re
                .find_iter(&compare_decoded)
                .map(|m| m.as_str().to_lowercase())
                .filter(|g| !is_well_known_guid(g))
                .collect();

            // GUIDs unique to source — likely env-specific values
            let source_only: BTreeSet<&String> = source_guids.difference(&compare_guids).collect();
            let compare_only: Vec<&String> = compare_guids.difference(&source_guids).collect();

            // Try to pair them positionally (often GUIDs are replaced 1:1 in same position)
            if source_only.len() == compare_only.len() && !source_only.is_empty() {
                // Positional matching: find them in order of appearance in source text
                let source_ordered = find_ordered_guids(&source_decoded, &source_only);
                let compare_ordered =
                    find_ordered_guids(&compare_decoded, &compare_only.iter().copied().collect());

                for (src_guid, cmp_guid) in source_ordered.iter().zip(compare_ordered.iter()) {
                    let pair = (src_guid.clone(), cmp_guid.clone());
                    if seen_pairs.contains(&pair) {
                        continue;
                    }
                    seen_pairs.insert(pair);

                    let mut rule = json!({
                        "find_value": src_guid,
                        "replace_value": {
                            source_env: src_guid,
                            compare_env: cmp_guid
                        }
                    });

                    // Scope to item type
                    rule["item_type"] = json!(source_item.metadata.item_type);

                    // Scope to item if specific
                    rule["item_name"] = json!(source_item.metadata.display_name);

                    rules.push(rule);
                }
            } else {
                // Can't pair — generate individual rules for each unique source GUID
                for guid in &source_only {
                    let guid_str = (*guid).clone();
                    if seen_pairs.contains(&(guid_str.clone(), String::new())) {
                        continue;
                    }
                    seen_pairs.insert((guid_str.clone(), String::new()));

                    let mut rule = json!({
                        "find_value": guid_str,
                        "replace_value": {
                            source_env: guid_str,
                            compare_env: format!("TODO_REPLACE_{}", &guid_str[..8])
                        }
                    });
                    rule["item_type"] = json!(source_item.metadata.item_type);
                    rule["item_name"] = json!(source_item.metadata.display_name);
                    rules.push(rule);
                }
            }

            // Also find non-GUID string differences (connection strings, server names)
            find_string_differences(
                &source_decoded,
                &compare_decoded,
                source_item,
                &source_part.path,
                source_env,
                compare_env,
                &mut rules,
                &mut seen_pairs,
            );
        }
    }

    let rules_count = rules.len();

    let parameters_json = json!({
        "find_replace": rules
    });

    Ok(InitParamsResult {
        parameters_json,
        summary: InitSummary {
            mode: "diff",
            source_items: source_ws.items.len(),
            compare_items: compare_ws.items.len(),
            rules_generated: rules_count,
            guids_found: 0, // Not relevant in diff mode
        },
    })
}

/// Find non-GUID string differences between two JSON payloads.
/// Identifies connection strings, server names, and other environment-specific values.
#[allow(clippy::too_many_arguments)]
fn find_string_differences(
    source_text: &str,
    compare_text: &str,
    source_item: &SourceItem,
    file_path: &str,
    source_env: &str,
    compare_env: &str,
    rules: &mut Vec<serde_json::Value>,
    seen_pairs: &mut BTreeSet<(String, String)>,
) {
    // Try to parse both as JSON and do key-by-key comparison
    let Ok(source_json) = serde_json::from_str::<serde_json::Value>(source_text) else {
        return;
    };
    let Ok(compare_json) = serde_json::from_str::<serde_json::Value>(compare_text) else {
        return;
    };

    let mut diffs: Vec<(String, String)> = Vec::new();
    collect_json_string_diffs(&source_json, &compare_json, "", &mut diffs);

    let guid_re = Regex::new(GUID_PATTERN).expect("valid regex");

    for (src_val, cmp_val) in diffs {
        // Skip if it's purely a GUID difference (already handled)
        if guid_re.is_match(&src_val)
            && src_val.len() == 36
            && guid_re.is_match(&cmp_val)
            && cmp_val.len() == 36
        {
            continue;
        }

        // Skip very short or very long values
        if src_val.len() < 5 || src_val.len() > 500 {
            continue;
        }

        let pair = (src_val.clone(), cmp_val.clone());
        if seen_pairs.contains(&pair) {
            continue;
        }
        seen_pairs.insert(pair);

        let mut rule = json!({
            "find_value": src_val,
            "replace_value": {
                source_env: src_val,
                compare_env: cmp_val
            }
        });
        rule["item_type"] = json!(source_item.metadata.item_type);
        rule["item_name"] = json!(source_item.metadata.display_name);
        rule["file_path"] = json!(file_path);
        rules.push(rule);
    }
}

/// Recursively collect string value differences between two JSON values.
fn collect_json_string_diffs(
    a: &serde_json::Value,
    b: &serde_json::Value,
    path: &str,
    diffs: &mut Vec<(String, String)>,
) {
    match (a, b) {
        (serde_json::Value::Object(ma), serde_json::Value::Object(mb)) => {
            for (key, va) in ma {
                if let Some(vb) = mb.get(key) {
                    let child_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    collect_json_string_diffs(va, vb, &child_path, diffs);
                }
            }
        }
        (serde_json::Value::Array(arr_a), serde_json::Value::Array(arr_b)) => {
            for (i, (va, vb)) in arr_a.iter().zip(arr_b.iter()).enumerate() {
                let child_path = format!("{path}[{i}]");
                collect_json_string_diffs(va, vb, &child_path, diffs);
            }
        }
        (serde_json::Value::String(sa), serde_json::Value::String(sb)) if sa != sb => {
            diffs.push((sa.clone(), sb.clone()));
        }
        _ => {}
    }
}

/// Find GUIDs in text in order of first appearance.
fn find_ordered_guids(text: &str, guids: &BTreeSet<&String>) -> Vec<String> {
    let guid_re = Regex::new(GUID_PATTERN).expect("valid regex");
    let lowercase_guids: BTreeSet<String> = guids.iter().map(|g| g.to_lowercase()).collect();
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();

    for mat in guid_re.find_iter(text) {
        let g = mat.as_str().to_lowercase();
        if lowercase_guids.contains(&g) && !seen.contains(&g) {
            seen.insert(g.clone());
            ordered.push(g);
        }
    }

    ordered
}

/// Decode a base64 payload to a UTF-8 string, or None for binary content.
fn decode_payload(payload: &str) -> Option<String> {
    let bytes = BASE64.decode(payload).ok()?;
    String::from_utf8(bytes).ok()
}

/// Check if a GUID is a well-known placeholder that shouldn't be parameterized.
fn is_well_known_guid(guid: &str) -> bool {
    let normalized = guid.to_lowercase();
    // All-zeros
    normalized == "00000000-0000-0000-0000-000000000000"
        // All-ones (max UUID)
        || normalized == "ffffffff-ffff-ffff-ffff-ffffffffffff"
        // Common test GUIDs
        || normalized.starts_with("00000000-0000-0000-0000-00000000000")
}

#[derive(Debug, Clone)]
struct GuidLocation {
    item_type: String,
    item_name: String,
    #[allow(dead_code)]
    file_path: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_item(
        dir: &Path,
        folder_name: &str,
        item_type: &str,
        display_name: &str,
        content: &str,
        file_name: &str,
    ) {
        let item_dir = dir.join(folder_name);
        fs::create_dir_all(&item_dir).unwrap();

        let platform = json!({
            "metadata": {
                "type": item_type,
                "displayName": display_name
            },
            "config": {}
        });
        fs::write(
            item_dir.join(".platform"),
            serde_json::to_string_pretty(&platform).unwrap(),
        )
        .unwrap();

        let payload = BASE64.encode(content.as_bytes());
        // Write the raw file (platform parser reads files and base64-encodes them)
        fs::write(item_dir.join(file_name), content).unwrap();

        // The parser reads raw files, but for testing we need to verify the scanning
        // works on the parsed SourceWorkspace. Let's just write raw files.
        let _ = payload; // suppress unused warning
    }

    #[test]
    fn test_scan_finds_guids() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        let content = r#"{"workspaceId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890", "connectionId": "12345678-aaaa-bbbb-cccc-ddddeeeeaaaa"}"#;
        create_test_item(
            dir,
            "MyNotebook.Notebook",
            "Notebook",
            "MyNotebook",
            content,
            "notebook-content.py",
        );

        let result = scan_for_candidates(dir).unwrap();
        assert_eq!(result.summary.mode, "scan");
        assert_eq!(result.summary.source_items, 1);
        assert!(result.summary.guids_found >= 2);
        assert!(result.summary.rules_generated >= 2);

        let rules = result.parameters_json["find_replace"].as_array().unwrap();
        assert!(rules.len() >= 2);

        // Each rule should have find_value and replace_value
        for rule in rules {
            assert!(rule.get("find_value").is_some());
            assert!(rule.get("replace_value").is_some());
        }
    }

    #[test]
    fn test_scan_skips_well_known_guids() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        let content = r#"{"nullId": "00000000-0000-0000-0000-000000000000", "realId": "abcdef12-3456-7890-abcd-ef1234567890"}"#;
        create_test_item(
            dir,
            "MyItem.Lakehouse",
            "Lakehouse",
            "MyItem",
            content,
            "definition.json",
        );

        let result = scan_for_candidates(dir).unwrap();
        // Should find realId but skip the null GUID
        assert_eq!(result.summary.guids_found, 1);
    }

    #[test]
    fn test_diff_finds_guid_differences() {
        let tmp_source = TempDir::new().unwrap();
        let tmp_compare = TempDir::new().unwrap();

        let source_content = r#"{"connectionId": "aaaaaaaa-1111-2222-3333-444444444444"}"#;
        let compare_content = r#"{"connectionId": "bbbbbbbb-5555-6666-7777-888888888888"}"#;

        create_test_item(
            tmp_source.path(),
            "MyPipeline.DataPipeline",
            "DataPipeline",
            "MyPipeline",
            source_content,
            "pipeline-content.json",
        );
        create_test_item(
            tmp_compare.path(),
            "MyPipeline.DataPipeline",
            "DataPipeline",
            "MyPipeline",
            compare_content,
            "pipeline-content.json",
        );

        let result =
            diff_for_parameters(tmp_source.path(), tmp_compare.path(), "dev", "prod").unwrap();

        assert_eq!(result.summary.mode, "diff");
        assert!(result.summary.rules_generated >= 1);

        let rules = result.parameters_json["find_replace"].as_array().unwrap();
        assert_ne!(rules.len(), 0);

        // The first rule should map the source GUID to the compare GUID
        let rule = &rules[0];
        let find_val = rule["find_value"].as_str().unwrap();
        assert_eq!(find_val, "aaaaaaaa-1111-2222-3333-444444444444");

        let replace = &rule["replace_value"];
        assert_eq!(
            replace["dev"].as_str().unwrap(),
            "aaaaaaaa-1111-2222-3333-444444444444"
        );
        assert_eq!(
            replace["prod"].as_str().unwrap(),
            "bbbbbbbb-5555-6666-7777-888888888888"
        );
    }

    #[test]
    fn test_diff_finds_string_differences() {
        let tmp_source = TempDir::new().unwrap();
        let tmp_compare = TempDir::new().unwrap();

        let source_content = r#"{"server": "contoso-dev.database.windows.net", "port": 1433}"#;
        let compare_content = r#"{"server": "contoso-prod.database.windows.net", "port": 1433}"#;

        create_test_item(
            tmp_source.path(),
            "Config.DataPipeline",
            "DataPipeline",
            "Config",
            source_content,
            "pipeline-content.json",
        );
        create_test_item(
            tmp_compare.path(),
            "Config.DataPipeline",
            "DataPipeline",
            "Config",
            compare_content,
            "pipeline-content.json",
        );

        let result =
            diff_for_parameters(tmp_source.path(), tmp_compare.path(), "dev", "prod").unwrap();

        assert!(result.summary.rules_generated >= 1);

        let rules = result.parameters_json["find_replace"].as_array().unwrap();
        // Should find the server name difference
        let server_rule = rules.iter().find(|r| {
            r["find_value"]
                .as_str()
                .is_some_and(|v| v.contains("contoso-dev"))
        });
        assert!(server_rule.is_some(), "Should find server name difference");

        let sr = server_rule.unwrap();
        assert_eq!(
            sr["replace_value"]["prod"].as_str().unwrap(),
            "contoso-prod.database.windows.net"
        );
    }

    #[test]
    fn test_diff_no_common_items() {
        let tmp_source = TempDir::new().unwrap();
        let tmp_compare = TempDir::new().unwrap();

        create_test_item(
            tmp_source.path(),
            "A.Notebook",
            "Notebook",
            "A",
            "content",
            "notebook-content.py",
        );
        create_test_item(
            tmp_compare.path(),
            "B.Notebook",
            "Notebook",
            "B",
            "content",
            "notebook-content.py",
        );

        let result =
            diff_for_parameters(tmp_source.path(), tmp_compare.path(), "dev", "prod").unwrap();
        assert_eq!(result.summary.rules_generated, 0);
    }

    #[test]
    fn test_is_well_known_guid() {
        assert!(is_well_known_guid("00000000-0000-0000-0000-000000000000"));
        assert!(is_well_known_guid("00000000-0000-0000-0000-000000000001"));
        assert!(is_well_known_guid("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"));
        assert!(!is_well_known_guid("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
    }
}
