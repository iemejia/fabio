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
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    FabioError::new(
                        ErrorCode::InvalidInput,
                        "Definition file must have a valid UTF-8 file name",
                    )
                })?;
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
    Ok(definition)
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
            let relative = path.strip_prefix(root).unwrap_or(&path);
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
    format: Option<WarehouseDefinitionFormat>,
) -> Result<Option<Value>> {
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

pub(super) async fn get(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<WarehouseDefinitionFormat>,
    decode: bool,
) -> Result<()> {
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
        .post_readonly_lro(&path, &serde_json::json!({}))
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
    format: Option<WarehouseDefinitionFormat>,
    update_metadata: bool,
) -> Result<()> {
    let definition = definition_from_inputs(file, dir, format)?;
    let part_count = definition["parts"].as_array().map_or(0, Vec::len);
    if output::dry_run_guard(
        cli,
        "warehouse update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "format": format.map_or("sqlproj", WarehouseDefinitionFormat::as_str),
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
        assert!(optional_definition(None, None, Some(WarehouseDefinitionFormat::Dacpac)).is_err());
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
        assert_eq!(definition["parts"][0]["path"], "definition.dacpac");
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
}
