use std::collections::HashSet;

use anyhow::Result;
use serde_json::Value;

use crate::errors::{ErrorCode, FabioError};

pub fn parse_object(input: &str, flag: &str) -> Result<Value> {
    let resolved = super::query_input::resolve_query_input(
        Some(input),
        "JSON object",
        flag,
        r#"{"key":true}"#,
    )?;
    let value: Value = serde_json::from_str(&resolved).map_err(|error| {
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
            format!("Provide an object, for example: {flag} '{{\"validateOnly\":true}}'"),
        )
        .into());
    }
    Ok(value)
}

pub fn parse_item_options(input: &str, flag: &str, target_field: &str) -> Result<Value> {
    let example = format!(
        r#"[{{"{target_field}":"00000000-0000-0000-0000-000000000000","options":{{"validateOnly":true}}}}]"#
    );
    let resolved = super::query_input::resolve_query_input(
        Some(input),
        "per-item options JSON",
        flag,
        &example,
    )?;
    let value: Value = serde_json::from_str(&resolved).map_err(|error| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid {flag} JSON: {error}"),
            format!("Provide a JSON array such as: {example}"),
        )
    })?;
    let entries = value.as_array().ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{flag} must be a JSON array"),
            format!("Provide an array such as: {example}"),
        )
    })?;

    let mut targets = HashSet::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let Some(object) = entry.as_object() else {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} entry {index} must be an object"),
                format!("Each entry must contain {target_field} and options."),
            )
            .into());
        };
        let target = object
            .get(target_field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("{flag} entry {index} requires a non-empty {target_field}"),
                    format!("Each entry must contain {target_field} and options."),
                )
            })?;
        if uuid::Uuid::parse_str(target).is_err() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} entry {index} has an invalid {target_field} UUID"),
                format!("Use a valid UUID for {target_field}, for example: {example}"),
            )
            .into());
        }
        if !object.get("options").is_some_and(Value::is_object) {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} entry {index} requires an options object"),
                format!("Each entry must contain {target_field} and an options object."),
            )
            .into());
        }
        if !targets.insert(target) {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} targets {target_field} '{target}' more than once"),
                "Combine all options for an item into a single array entry.",
            )
            .into());
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{parse_item_options, parse_object};

    #[test]
    fn parses_options_object() {
        let value = parse_object(r#"{"allowPurgeData":true}"#, "--options").unwrap();
        assert_eq!(value["allowPurgeData"], true);
    }

    #[test]
    fn rejects_non_object_options() {
        assert!(parse_object("[]", "--options").is_err());
    }

    #[test]
    fn validates_unique_item_option_targets() {
        let input = r#"[
            {"logicalId":"00000000-0000-0000-0000-000000000001","options":{"validateOnly":true}},
            {"logicalId":"00000000-0000-0000-0000-000000000001","options":{"allowPurgeData":true}}
        ]"#;
        assert!(parse_item_options(input, "--item-options", "logicalId").is_err());
    }

    #[test]
    fn rejects_missing_options_object() {
        let input = r#"[{"sourceItemId":"00000000-0000-0000-0000-000000000001","options":true}]"#;
        assert!(parse_item_options(input, "--item-options", "sourceItemId").is_err());
    }

    #[test]
    fn accepts_options_loaded_from_file() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut file, br#"{"validateOnly":true}"#).unwrap();
        let input = format!("@{}", file.path().display());
        let value = parse_object(&input, "--options").unwrap();
        assert_eq!(value["validateOnly"], true);
    }
}
