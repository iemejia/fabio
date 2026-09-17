use anyhow::Result;
use serde_json::Value;

use crate::errors::{ErrorCode, FabioError};

pub fn parse_object(input: &str, flag: &str, example: &str) -> Result<Value> {
    parse_kind(input, flag, example, Value::is_object, "object")
}

pub fn parse_array(input: &str, flag: &str, example: &str) -> Result<Value> {
    parse_kind(input, flag, example, Value::is_array, "array")
}

pub fn parse_item_options(input: &str, flag: &str, id_field: &str, example: &str) -> Result<Value> {
    let value = parse_array(input, flag, example)?;
    let mut ids = std::collections::HashSet::new();
    for (index, entry) in value
        .as_array()
        .expect("parse_array returned an array")
        .iter()
        .enumerate()
    {
        let id = entry.get(id_field).and_then(Value::as_str).ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} entry {index} requires a string {id_field}."),
                format!("Example: {flag} '{example}'"),
            )
        })?;
        uuid::Uuid::parse_str(id).map_err(|error| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} entry {index} has an invalid {id_field}: {error}"),
                format!("{id_field} must be a UUID."),
            )
        })?;
        if !ids.insert(id) {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} targets {id_field} '{id}' more than once."),
                "Combine that item's settings into a single options object.",
            )
            .into());
        }
        if !entry.get("options").is_some_and(Value::is_object) {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{flag} entry {index} requires an options object."),
                format!("Example: {flag} '{example}'"),
            )
            .into());
        }
    }
    Ok(value)
}

fn parse_kind(
    input: &str,
    flag: &str,
    example: &str,
    expected: impl FnOnce(&Value) -> bool,
    kind: &str,
) -> Result<Value> {
    let resolved = super::query_input::resolve_query_input(
        Some(input),
        "JSON",
        flag,
        &format!("{flag} '{example}'"),
    )?;
    let value: Value = serde_json::from_str(&resolved).map_err(|error| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid {flag} JSON: {error}"),
            format!("Expected a JSON {kind}, for example: {flag} '{example}'"),
        )
    })?;
    if !expected(&value) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{flag} must be a JSON {kind}."),
            format!("Example: {flag} '{example}'"),
        )
        .into());
    }
    Ok(value)
}

pub fn insert_option(body: &mut Value, name: &str, value: Value) -> Result<()> {
    let body = body.as_object_mut().ok_or_else(|| {
        FabioError::new(
            ErrorCode::InvalidInput,
            "The request body must be a JSON object.",
        )
    })?;
    let options = body
        .entry("options")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "The request body's options property must be a JSON object.",
                "Remove the invalid options value or provide an object such as {\"allowPairingByName\":true}.",
            )
        })?;
    options.insert(name.to_string(), value);
    Ok(())
}

pub fn set_options(body: &mut Value, options: Value) -> Result<()> {
    let body = body.as_object_mut().ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "The request body must be a JSON object.",
            "Provide a definition envelope such as {\"definition\":{\"parts\":[...]}}.",
        )
    })?;
    body.insert("options".to_string(), options);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{insert_option, parse_array, parse_item_options, parse_object, set_options};

    #[test]
    fn parses_object_and_array_arguments() {
        assert_eq!(
            parse_object(r#"{"validateOnly":true}"#, "--options", "{}").unwrap(),
            serde_json::json!({"validateOnly": true})
        );
        assert_eq!(
            parse_array(
                r#"[{"logicalId":"id","options":{}}]"#,
                "--item-options",
                "[]"
            )
            .unwrap(),
            serde_json::json!([{"logicalId": "id", "options": {}}])
        );
    }

    #[test]
    fn rejects_wrong_json_kind() {
        let error = parse_object("[]", "--options", "{}").unwrap_err();
        assert!(error.to_string().contains("must be a JSON object"));
    }

    #[test]
    fn inserts_into_existing_options() {
        let mut body = serde_json::json!({"options": {"allowPairingByName": true}});
        insert_option(&mut body, "itemOptionsByLogicalId", serde_json::json!([])).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "options": {
                    "allowPairingByName": true,
                    "itemOptionsByLogicalId": []
                }
            })
        );
    }

    #[test]
    fn replaces_top_level_options() {
        let mut body = serde_json::json!({"definition": {"parts": []}});
        set_options(&mut body, serde_json::json!({"validateOnly": true})).unwrap();
        assert_eq!(body["options"]["validateOnly"], true);
    }

    #[test]
    fn validates_item_option_entries() {
        let id = "88436e65-6ed1-8185-49ff-f61077fc73d4";
        parse_item_options(
            &format!(r#"[{{"logicalId":"{id}","options":{{"validateOnly":true}}}}]"#),
            "--item-options",
            "logicalId",
            "[]",
        )
        .unwrap();

        let duplicate = format!(
            r#"[{{"logicalId":"{id}","options":{{}}}},{{"logicalId":"{id}","options":{{}}}}]"#
        );
        assert!(
            parse_item_options(&duplicate, "--item-options", "logicalId", "[]")
                .unwrap_err()
                .to_string()
                .contains("more than once")
        );
        assert!(
            parse_item_options(
                r#"[{"logicalId":"not-a-uuid","options":{}}]"#,
                "--item-options",
                "logicalId",
                "[]",
            )
            .is_err()
        );
    }
}
