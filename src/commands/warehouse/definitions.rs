use std::fs;
use std::path::Path;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;

use super::WarehouseDefinitionFormat;
use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

fn definition_from_inputs(
    file: Option<&str>,
    dir: Option<&str>,
    format: Option<WarehouseDefinitionFormat>,
) -> Result<Value> {
    let mut definition = match (file, dir) {
        (Some(path), None) => {
            let path = Path::new(path);
            let bytes = fs::read(path).map_err(|error| {
                FabioError::new(
                    ErrorCode::InvalidInput,
                    format!("Failed to read '{}': {error}", path.display()),
                )
            })?;
            if path.extension().is_some_and(|value| value == "json")
                || bytes
                    .iter()
                    .find(|byte| !byte.is_ascii_whitespace())
                    .is_some_and(|byte| matches!(byte, b'{' | b'['))
            {
                let raw: Value = serde_json::from_slice(&bytes).map_err(|error| {
                    FabioError::invalid_input(format!("Invalid warehouse definition JSON: {error}"))
                })?;
                return validate_json_definition(raw, format);
            }
            let name = match format.unwrap_or(WarehouseDefinitionFormat::Sqlproj) {
                WarehouseDefinitionFormat::Sqlproj => "sqldw.sqlproj",
                WarehouseDefinitionFormat::Dacpac => "sqldw.dacpac",
            };
            serde_json::json!({
                "parts": [{
                    "path": name,
                    "payload": BASE64.encode(bytes),
                    "payloadType": "InlineBase64",
                }]
            })
        }
        (None, Some(path)) => definition_from_dir(path)?,
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Provide exactly one of --definition-file or --definition-dir",
                "Use --definition-file for a DACPAC or --definition-dir for SQL project parts.",
            )
            .into());
        }
    };
    if let Some(format) = format {
        definition["format"] = Value::from(format.as_str());
    }
    validate_json_definition(definition, format)
}

fn definition_from_dir(path: &str) -> Result<Value> {
    let root = Path::new(path);
    if !fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_dir()) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Definition directory does not exist or is not a regular directory: '{}'",
                root.display()
            ),
            "Point --definition-dir at a non-symlink root directory of the SQL project definition.",
        )
        .into());
    }
    let mut paths = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).map_err(|error| {
            FabioError::new(
                ErrorCode::InvalidInput,
                format!("Failed to read '{}': {error}", current.display()),
            )
        })? {
            let entry = entry.map_err(|error| {
                FabioError::new(
                    ErrorCode::InvalidInput,
                    format!("Failed to read directory entry: {error}"),
                )
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|error| {
                FabioError::new(
                    ErrorCode::InvalidInput,
                    format!("Failed to inspect '{}': {error}", path.display()),
                )
            })?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                paths.push(path);
            } else {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!(
                        "Warehouse definition contains a symlink or non-regular entry: '{}'",
                        path.display()
                    ),
                    "Remove symbolic links and special files from the definition directory.",
                )
                .into());
            }
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            "Warehouse definition directory contains no files",
        )
        .into());
    }
    let parts = paths
        .into_iter()
        .map(|path| {
            let relative = path.trim_prefix(root);
            let part_path = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let bytes = fs::read(&path).map_err(|error| {
                FabioError::new(
                    ErrorCode::InvalidInput,
                    format!("Failed to read '{}': {error}", path.display()),
                )
            })?;
            Ok(serde_json::json!({
                "path": part_path,
                "payload": BASE64.encode(bytes),
                "payloadType": "InlineBase64",
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(serde_json::json!({"parts": parts}))
}

pub(super) fn optional_definition(
    file: Option<&str>,
    dir: Option<&str>,
    content: Option<&str>,
    format: Option<WarehouseDefinitionFormat>,
) -> Result<Option<Value>> {
    if let Some(content) = content {
        if file.is_some() || dir.is_some() {
            return Err(FabioError::invalid_input("Provide exactly one definition input").into());
        }
        let raw = crate::commands::query_input::resolve_query_input(
            Some(content),
            "warehouse definition",
            "--content",
            "--content @warehouse.json",
        )?;
        let value = if raw.trim_start().starts_with(['{', '[']) {
            serde_json::from_str(&raw).map_err(|error| {
                FabioError::invalid_input(format!("Invalid warehouse definition JSON: {error}"))
            })?
        } else {
            if raw.trim().is_empty() || matches!(format, Some(WarehouseDefinitionFormat::Dacpac)) {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "Inline raw content must be a non-empty SQL project",
                    "Use --file for a DACPAC binary or provide a JSON definition envelope.",
                )
                .into());
            }
            serde_json::json!({"parts": [{"path": "sqldw.sqlproj", "payload": BASE64.encode(raw.as_bytes()), "payloadType": "InlineBase64"}]})
        };
        return validate_json_definition(value, format).map(Some);
    }
    if file.is_none() && dir.is_none() {
        if format.is_some() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--definition-format requires --definition-file or --definition-dir",
                "Provide definition content or remove --definition-format.",
            )
            .into());
        }
        return Ok(None);
    }
    definition_from_inputs(file, dir, format).map(Some)
}

fn validate_json_definition(
    value: Value,
    format: Option<WarehouseDefinitionFormat>,
) -> Result<Value> {
    let mut definition = value.get("definition").cloned().unwrap_or(value);
    let parts = definition
        .get("parts")
        .and_then(Value::as_array)
        .filter(|parts| !parts.is_empty())
        .ok_or_else(|| {
            FabioError::invalid_input("Warehouse definition requires a non-empty parts array")
        })?;
    let mut paths = std::collections::HashSet::new();
    for part in parts {
        let path = part
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .ok_or_else(|| {
                FabioError::invalid_input(
                    "Each warehouse definition part requires a non-empty path",
                )
            })?;
        if path.starts_with('/')
            || path.contains('\\')
            || path.contains(':')
            || path
                .split('/')
                .any(|segment| matches!(segment, "" | "." | ".."))
            || !paths.insert(path)
        {
            return Err(FabioError::invalid_input(
                "Warehouse part paths must be unique, relative paths without traversal",
            )
            .into());
        }
        if part.get("payloadType").and_then(Value::as_str) != Some("InlineBase64")
            || part
                .get("payload")
                .and_then(Value::as_str)
                .is_none_or(|payload| BASE64.decode(payload).is_err())
        {
            return Err(FabioError::invalid_input("Each warehouse definition part requires a valid base64 payload and payloadType InlineBase64").into());
        }
    }
    if let Some(format) = format {
        definition["format"] = Value::from(format.as_str());
    }
    if definition
        .get("format")
        .is_some_and(|value| !matches!(value.as_str(), Some("dacpac" | "sqlproj")))
    {
        return Err(FabioError::invalid_input(
            "Warehouse definition format must be sqlproj or dacpac",
        )
        .into());
    }
    Ok(definition)
}

pub(super) async fn get(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<WarehouseDefinitionFormat>,
    decode: bool,
) -> Result<()> {
    crate::client::validate_uuid(workspace, "--workspace")?;
    crate::client::validate_uuid(id, "--id")?;
    let path = format.map_or_else(
        || format!("/workspaces/{workspace}/warehouses/{id}/getDefinition"),
        |format| {
            format!(
                "/workspaces/{workspace}/warehouses/{id}/getDefinition?format={}",
                format.as_str()
            )
        },
    );
    let data = client
        .post_readonly_with_lro(&path, &serde_json::json!({}))
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse get-definition", "Contributor"))?;
    let data = if decode {
        output::decode_definition_parts(data)
    } else {
        data
    };
    output::render_object(cli, &data, "definition");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    dir: Option<&str>,
    content: Option<&str>,
    format: Option<WarehouseDefinitionFormat>,
    update_metadata: bool,
) -> Result<()> {
    crate::client::validate_uuid(workspace, "--workspace")?;
    crate::client::validate_uuid(id, "--id")?;
    let definition = optional_definition(file, dir, content, format)?.ok_or_else(|| {
        FabioError::invalid_input("Provide --file, --content, or --definition-dir")
    })?;
    let part_count = definition["parts"].as_array().map_or(0, Vec::len);
    if output::dry_run_guard(
        cli,
        "warehouse update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "format": definition.get("format").and_then(Value::as_str).unwrap_or("sqlproj"),
            "partCount": part_count,
            "updateMetadata": update_metadata,
        }),
    ) {
        return Ok(());
    }
    let path = if update_metadata {
        format!("/workspaces/{workspace}/warehouses/{id}/updateDefinition?updateMetadata=true")
    } else {
        format!("/workspaces/{workspace}/warehouses/{id}/updateDefinition")
    };
    let data = client
        .post(&path, &serde_json::json!({"definition": definition}), true)
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse update-definition", "Contributor"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        output::render_object(
            cli,
            &serde_json::json!({"id": id, "status": "definition_updated"}),
            "status",
        );
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_definition_rejects_format_without_content() {
        assert!(
            optional_definition(None, None, None, Some(WarehouseDefinitionFormat::Dacpac)).is_err()
        );
    }

    #[test]
    fn dacpac_file_serializes_required_part_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("definition.dacpac");
        fs::write(&path, b"dacpac").unwrap();
        let definition =
            definition_from_inputs(path.to_str(), None, Some(WarehouseDefinitionFormat::Dacpac))
                .unwrap();
        assert_eq!(definition["format"], "dacpac");
        assert_eq!(definition["parts"][0]["path"], "sqldw.dacpac");
        assert_eq!(definition["parts"][0]["payload"], "ZGFjcGFj");
        assert_eq!(definition["parts"][0]["payloadType"], "InlineBase64");
    }

    #[test]
    fn sqlproj_directory_has_deterministic_relative_parts() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("sqldw.sqlproj"), b"project").unwrap();
        let tables = directory.path().join("dbo").join("Tables");
        fs::create_dir_all(&tables).unwrap();
        fs::write(tables.join("Orders.sql"), b"table").unwrap();
        let definition = definition_from_inputs(None, directory.path().to_str(), None).unwrap();
        assert!(definition.get("format").is_none());
        assert_eq!(definition["parts"][0]["path"], "dbo/Tables/Orders.sql");
        assert_eq!(definition["parts"][1]["path"], "sqldw.sqlproj");
    }

    #[test]
    fn empty_definition_directory_is_rejected_before_update() {
        let directory = tempfile::tempdir().unwrap();
        assert!(definition_from_inputs(None, directory.path().to_str(), None).is_err());
    }

    #[test]
    fn definition_directory_must_be_a_directory() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("not-a-directory");
        fs::write(&file, b"content").unwrap();
        assert!(definition_from_inputs(None, file.to_str(), None).is_err());
    }

    #[test]
    fn definition_json_validates_envelopes_and_rejects_unsafe_parts() {
        let valid = serde_json::json!({"parts": [{"path": "sqldw.sqlproj", "payload": "cHJvamVjdA==", "payloadType": "InlineBase64"}]});
        assert_eq!(
            validate_json_definition(serde_json::json!({"definition": valid}), None).unwrap(),
            valid
        );
        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"parts": []}),
            serde_json::json!({"parts": [{"path": "sqldw.sqlproj"}]}),
        ] {
            assert!(validate_json_definition(invalid, None).is_err());
        }
        for path in [
            "../project.sqlproj",
            "/project.sqlproj",
            "C:/project.sqlproj",
            "a\\project.sqlproj",
        ] {
            let mut value = valid.clone();
            value["parts"][0]["path"] = Value::from(path);
            assert!(validate_json_definition(value, None).is_err());
        }
        let mut value = valid.clone();
        value["parts"][0]["payload"] = Value::from("not base64!");
        assert!(validate_json_definition(value, None).is_err());
        let mut value = valid.clone();
        value["parts"]
            .as_array_mut()
            .unwrap()
            .push(valid["parts"][0].clone());
        assert!(validate_json_definition(value, None).is_err());
    }

    #[test]
    fn inline_json_and_file_json_use_the_same_validation() {
        let raw = r#"{"parts":[{"path":"sqldw.dacpac","payload":"ZGFjcGFj","payloadType":"InlineBase64"}]}"#;
        let expected = optional_definition(
            None,
            None,
            Some(raw),
            Some(WarehouseDefinitionFormat::Dacpac),
        )
        .unwrap()
        .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("warehouse.json");
        fs::write(&file, raw).unwrap();
        assert_eq!(
            definition_from_inputs(file.to_str(), None, Some(WarehouseDefinitionFormat::Dacpac))
                .unwrap(),
            expected
        );
        assert!(optional_definition(None, None, Some("{invalid"), None).is_err());
        assert!(optional_definition(None, None, Some(""), None).is_err());
        assert!(
            optional_definition(
                None,
                None,
                Some("<Project/>"),
                Some(WarehouseDefinitionFormat::Dacpac)
            )
            .is_err()
        );
        assert!(optional_definition(file.to_str(), None, Some(raw), None).is_err());
    }
}
