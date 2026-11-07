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

fn validate_parts(definition: &Value) -> Result<()> {
    let parts = definition
        .get("parts")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Warehouse definition requires a parts array",
                "Provide {\"parts\":[{\"path\":\"...\",\"payload\":\"base64\",\"payloadType\":\"InlineBase64\"}]}",
            )
        })?;
    if parts.is_empty()
        || parts.iter().any(|part| {
            ["path", "payload", "payloadType"]
                .iter()
                .any(|field| part.get(*field).and_then(Value::as_str).is_none())
        })
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Each warehouse definition part requires string path, payload, and payloadType fields",
            "Use payloadType InlineBase64 and base64-encode each part payload.",
        )
        .into());
    }
    Ok(())
}

fn definition_from_json(input: &str, format: Option<WarehouseDefinitionFormat>) -> Result<Value> {
    let raw = crate::commands::query_input::resolve_query_input(
        Some(input),
        "warehouse definition JSON",
        "--definition",
        "--definition @warehouse-definition.json",
    )?;
    let value: Value = serde_json::from_str(&raw).map_err(|error| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid warehouse definition JSON: {error}"),
            "Provide a definition object or {\"definition\":{...}} envelope.",
        )
    })?;
    let mut definition = value.get("definition").cloned().unwrap_or(value);
    if !definition.is_object() {
        return Err(FabioError::invalid_input("Warehouse definition must be a JSON object").into());
    }
    if let Some(format) = format {
        definition["format"] = Value::from(format.as_str());
    }
    validate_parts(&definition)?;
    Ok(definition)
}

fn definition_from_dir(
    directory: &str,
    format: Option<WarehouseDefinitionFormat>,
) -> Result<Value> {
    let root = Path::new(directory);
    if !root.is_dir() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Not a directory: '{directory}'"),
            "Point --definition-dir at a folder containing warehouse definition parts.",
        )
        .into());
    }
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(FabioError::invalid_input("Warehouse definition directory is empty").into());
    }
    let parts: Result<Vec<Value>> = files
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let api_path = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let payload = BASE64.encode(fs::read(&path)?);
            Ok(serde_json::json!({
                "path": api_path,
                "payload": payload,
                "payloadType": "InlineBase64"
            }))
        })
        .collect();
    let mut definition = serde_json::json!({"parts": parts?});
    if let Some(format) = format {
        definition["format"] = Value::from(format.as_str());
    }
    Ok(definition)
}

pub(super) fn load(
    definition: Option<&str>,
    directory: Option<&str>,
    format: Option<WarehouseDefinitionFormat>,
) -> Result<Option<Value>> {
    match (definition, directory) {
        (Some(input), None) => definition_from_json(input, format).map(Some),
        (None, Some(path)) => definition_from_dir(path, format).map(Some),
        (None, None) if format.is_none() => Ok(None),
        (None, None) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--definition-format requires --definition or --definition-dir",
            "Provide the public definition input or remove --definition-format.",
        )
        .into()),
        (Some(_), Some(_)) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Use only one of --definition or --definition-dir",
            "Use --definition for JSON or --definition-dir for a folder of parts.",
        )
        .into()),
    }
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
        |value| {
            format!(
                "/workspaces/{workspace}/warehouses/{id}/getDefinition?format={}",
                value.as_str()
            )
        },
    );
    let data = client
        .post_readonly_lro(&path, &serde_json::json!({}))
        .await
        .map_err(|error| enrich_forbidden(error, "warehouse get-definition", "Contributor"))?;
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
    definition: Option<&str>,
    directory: Option<&str>,
    format: Option<WarehouseDefinitionFormat>,
    update_metadata: bool,
) -> Result<()> {
    let definition = load(definition, directory, format)?.ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Provide --definition or --definition-dir",
            "Example: --definition-dir ./warehouse --definition-format sqlproj",
        )
    })?;
    let body = serde_json::json!({"definition": definition});
    if output::dry_run_guard(
        cli,
        "warehouse update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "updateMetadata": update_metadata,
            "definition": body["definition"]
        }),
    ) {
        return Ok(());
    }
    let mut path = format!("/workspaces/{workspace}/warehouses/{id}/updateDefinition");
    if update_metadata {
        path.push_str("?updateMetadata=true");
    }
    let data = client
        .post(&path, &body, true)
        .await
        .map_err(|error| enrich_forbidden(error, "warehouse update-definition", "Contributor"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        output::render_object(
            cli,
            &serde_json::json!({"status": "definition_updated", "id": id}),
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
    fn definition_json_accepts_spec_shape_and_format() {
        let definition = definition_from_json(
            r#"{"parts":[{"path":"definition.dacpac","payload":"UEs=","payloadType":"InlineBase64"}]}"#,
            Some(WarehouseDefinitionFormat::Dacpac),
        )
        .unwrap();
        assert_eq!(definition["format"], "dacpac");
        assert_eq!(definition["parts"][0]["path"], "definition.dacpac");
    }

    #[test]
    fn definition_json_requires_all_part_fields() {
        assert!(definition_from_json(r#"{"parts":[{"path":"definition.dacpac"}]}"#, None).is_err());
    }
}
