use std::collections::HashSet;

use anyhow::Result;
use serde_json::Value;

use crate::errors::{ErrorCode, FabioError};

pub fn parse_options_object(input: &str, flag: &str) -> Result<Value> {
    let raw = crate::commands::query_input::resolve_query_input(
        Some(input),
        "options JSON",
        flag,
        &format!("{flag} '{{\"validateOnly\":true}}'"),
    )?;
    let value: Value = serde_json::from_str(&raw).map_err(|error| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid {flag} JSON: {error}"),
            format!("Provide a JSON object, for example: {flag} '{{\"validateOnly\":true}}'"),
        )
    })?;
    if !value.is_object() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{flag} must be a JSON object"),
            format!("Example: {flag} '{{\"validateOnly\":true}}'"),
        )
        .into());
    }
    Ok(value)
}

pub fn parse_item_options(input: &str, identifier_field: &str, flag: &str) -> Result<Value> {
    let raw = crate::commands::query_input::resolve_query_input(
        Some(input),
        "per-item options JSON",
        flag,
        &format!(
            "{flag} '[{{\"{identifier_field}\":\"00000000-0000-0000-0000-000000000000\",\"options\":{{\"validateOnly\":true}}}}]'"
        ),
    )?;
    let entries: Value = serde_json::from_str(&raw).map_err(|error| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid {flag} JSON: {error}"),
            format!("Provide a JSON array of objects containing {identifier_field} and options."),
        )
    })?;
    let array = entries.as_array().ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{flag} must be a JSON array"),
            format!("Each entry must contain {identifier_field} and an options object."),
        )
    })?;

    let mut seen = HashSet::with_capacity(array.len());
    for (index, entry) in array.iter().enumerate() {
        let identifier = entry
            .get(identifier_field)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("{flag} entry {index} is missing string field '{identifier_field}'"),
                    format!("Each entry must contain {identifier_field} and an options object."),
                )
            })?;
        // Enforce the canonical 36-character hyphenated UUID form used for Fabric IDs
        // (client::validate_uuid), not the looser forms Uuid::parse_str accepts
        // (compact/URN/braced) — those are rejected by every other ID-taking command
        // and would only be rejected by the API after the request.
        crate::client::validate_uuid(identifier, identifier_field).map_err(|_| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!(
                    "{flag} entry {index} has invalid {identifier_field} '{identifier}': not a canonical UUID"
                ),
                format!(
                    "Use a canonical 36-character UUID (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx) for {identifier_field}."
                ),
            )
        })?;
        if !seen.insert(identifier.to_ascii_lowercase()) {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} targets {identifier_field} '{identifier}' more than once"),
                "Combine all options for an item into a single entry.",
            )
            .into());
        }
        if !entry.get("options").is_some_and(Value::is_object) {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} entry {index} must contain an options object"),
                format!("Each entry must contain {identifier_field} and an options object."),
            )
            .into());
        }
    }

    Ok(entries)
}

/// Whether any entry in a parsed per-item options array enables `allowPurgeData`.
///
/// Per-item options (`itemOptionsBySourceItemId` / `itemOptionsByLogicalId`, used by
/// `deployment-pipeline deploy`, `git pull`, `item bulk-import-definitions`, and
/// `workspace clone`) are item-type-specific, so a semantic-model entry can nest
/// `options.allowPurgeData=true`. Callers use this to surface the same purge
/// warning + conditional destructive dry-run signal as the top-level flag.
#[must_use]
pub fn entries_enable_purge(entries: &Value) -> bool {
    entries.as_array().is_some_and(|array| {
        array.iter().any(|entry| {
            entry
                .get("options")
                .and_then(|options| options.get("allowPurgeData"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_item_options, parse_options_object};

    #[test]
    fn parses_options_object() {
        let value = parse_options_object(r#"{"allowPurgeData":true}"#, "--options").unwrap();
        assert_eq!(value["allowPurgeData"], true);
    }

    #[test]
    fn rejects_non_object_options() {
        assert!(parse_options_object("[]", "--options").is_err());
    }

    #[test]
    fn parses_item_options_and_rejects_duplicate_targets() {
        let id = "88436e65-6ed1-8185-49ff-f61077fc73d4";
        let value = parse_item_options(
            &format!(r#"[{{"logicalId":"{id}","options":{{"validateOnly":true}}}}]"#),
            "logicalId",
            "--item-options",
        )
        .unwrap();
        assert_eq!(value[0]["logicalId"], id);

        let duplicate = format!(
            r#"[{{"logicalId":"{id}","options":{{}}}},{{"logicalId":"{id}","options":{{}}}}]"#
        );
        assert!(
            parse_item_options(&duplicate, "logicalId", "--item-options")
                .unwrap_err()
                .to_string()
                .contains("more than once")
        );
    }

    #[test]
    fn rejects_invalid_item_option_shape() {
        assert!(
            parse_item_options(
                r#"[{"sourceItemId":"not-a-uuid","options":[]}]"#,
                "sourceItemId",
                "--item-options",
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_non_object_options_with_valid_uuid() {
        // A valid UUID passes UUID validation, so this actually exercises the
        // options-object check (the invalid-UUID case fails earlier and never reaches it).
        let err = parse_item_options(
            r#"[{"sourceItemId":"88436e65-6ed1-8185-49ff-f61077fc73d4","options":[]}]"#,
            "sourceItemId",
            "--item-options",
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("must contain an options object"),
            "expected options-object error, got: {err}"
        );
    }

    #[test]
    fn detects_nested_allow_purge_data() {
        use super::entries_enable_purge;
        let with_purge = parse_item_options(
            r#"[{"logicalId":"88436e65-6ed1-8185-49ff-f61077fc73d4","options":{"allowPurgeData":true}}]"#,
            "logicalId",
            "--item-options",
        )
        .unwrap();
        assert!(entries_enable_purge(&with_purge));

        let without_purge = parse_item_options(
            r#"[{"logicalId":"88436e65-6ed1-8185-49ff-f61077fc73d4","options":{"validateOnly":true}}]"#,
            "logicalId",
            "--item-options",
        )
        .unwrap();
        assert!(!entries_enable_purge(&without_purge));
    }

    #[test]
    fn rejects_noncanonical_uuid_forms() {
        // Uuid::parse_str accepts these, but Fabric IDs must be canonical 36-char
        // hyphenated; validate_uuid (and every other ID command) rejects them.
        for id in [
            "88436e656ed1818549fff61077fc73d4",       // compact (no hyphens)
            "{88436e65-6ed1-8185-49ff-f61077fc73d4}", // braced
            "urn:uuid:88436e65-6ed1-8185-49ff-f61077fc73d4", // URN
        ] {
            let input = format!(r#"[{{"sourceItemId":"{id}","options":{{"validateOnly":true}}}}]"#);
            assert!(
                parse_item_options(&input, "sourceItemId", "--item-options").is_err(),
                "noncanonical UUID '{id}' must be rejected"
            );
        }
    }
}
