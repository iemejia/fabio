use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

pub(super) fn from_input(
    file: Option<&str>,
    content: Option<&str>,
    format: Option<&str>,
) -> Result<(serde_json::Value, usize)> {
    let (bytes, source_path) = match (file, content) {
        (Some(path), None) => (
            std::fs::read(path).map_err(|error| {
                FabioError::invalid_input(format!("Cannot read {path}: {error}"))
            })?,
            Some(path),
        ),
        (None, Some(value)) => (value.as_bytes().to_vec(), None),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided",
                "Provide a .sqlproj/.dacpac with --file, or inline SQL project content with --content.",
            )
            .into());
        }
        (Some(_), Some(_)) => unreachable!("clap prevents conflicting definition inputs"),
    };
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
        let definition = value.get("definition").cloned().unwrap_or(value);
        if definition
            .get("parts")
            .and_then(serde_json::Value::as_array)
            .is_some()
        {
            return Ok((definition, bytes.len()));
        }
    }
    let format = format.unwrap_or("sqlproj");
    let default_path = format!("definition.{format}");
    let part_path = source_path
        .and_then(|path| std::path::Path::new(path).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or(&default_path);
    let definition = serde_json::json!({
        "format": format,
        "parts": [{
            "path": part_path,
            "payload": BASE64.encode(&bytes),
            "payloadType": "InlineBase64",
        }],
    });
    Ok((definition, bytes.len()))
}

pub(super) async fn get(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<&str>,
    decode: bool,
) -> Result<()> {
    let mut path = format!("/workspaces/{workspace}/warehouses/{id}/getDefinition");
    if let Some(value) = format {
        path.push_str("?format=");
        path.push_str(value);
    }
    let data = client
        .post_readonly_with_lro(&path, &serde_json::json!({}))
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse get-definition", "Contributor"))?;
    if decode {
        output::render_object(cli, &output::decode_definition_parts(data), "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    format: Option<&str>,
    update_metadata: bool,
) -> Result<()> {
    let (definition, content_length) = from_input(file, content, format)?;
    let body = serde_json::json!({"definition": definition});
    if output::dry_run_guard(
        cli,
        "warehouse update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "format": format.unwrap_or("sqlproj"),
            "contentLength": content_length,
            "updateMetadata": update_metadata,
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
    use super::from_input;

    #[test]
    fn definition_defaults_to_sqlproj() {
        let (definition, length) = from_input(None, Some("<Project />"), None).unwrap();
        assert_eq!(definition["format"], "sqlproj");
        assert_eq!(definition["parts"][0]["path"], "definition.sqlproj");
        assert_eq!(definition["parts"][0]["payloadType"], "InlineBase64");
        assert_eq!(length, 11);
    }
}
