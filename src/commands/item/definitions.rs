use std::fmt::Write;
use std::fs;
use std::path::Path;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

// ─── Get Definition ──────────────────────────────────────────────────────────

pub(super) async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<&str>,
    decode: bool,
) -> Result<()> {
    let mut path = format!("/workspaces/{workspace}/items/{id}/getDefinition");
    if let Some(f) = format {
        let _ = write!(path, "?format={f}");
    }

    let data = client
        .post(&path, &serde_json::json!({}), true)
        .await
        .map_err(|e| enrich_forbidden(e, "item get-definition", "ReadWrite"))?;
    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

// ─── Update Definition ───────────────────────────────────────────────────────

pub(super) async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    definition: Option<&str>,
    update_metadata: bool,
) -> Result<()> {
    if file.is_none() && definition.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --definition must be provided".to_string(),
            "Pass --file <path> to wrap a single part (keyed by the file name), or --definition \
             with the full envelope {\"definition\":{\"parts\":[{\"path\":\"...\",\"payload\":\"base64\",\"payloadType\":\"InlineBase64\"}]}}. \
             Discover the required parts for an item type: fabio context schema <Type>. \
             Validate offline first: fabio item validate-definition --type <Type> --file <envelope.json>."
                .to_string(),
        )
        .into());
    }

    let body = if let Some(def_json) = definition {
        // Inline JSON definition payload
        serde_json::from_str::<Value>(def_json).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in --definition: {e}"),
                "Provide valid JSON: {\"definition\":{\"parts\":[{\"path\":\"...\",\"payload\":\"base64...\",\"payloadType\":\"InlineBase64\"}]}}"
                    .to_string(),
            )
        })?
    } else if let Some(file_path) = file {
        // Read file and encode as base64
        let path = Path::new(file_path);
        let content = fs::read(path).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read file '{file_path}': {e}"),
                "Provide a valid file path.".to_string(),
            )
        })?;

        let encoded = BASE64.encode(&content);
        let filename = path
            .file_name()
            .map_or("definition", |f| f.to_str().unwrap_or("definition"));

        serde_json::json!({
            "definition": {
                "parts": [{
                    "path": filename,
                    "payload": encoded,
                    "payloadType": "InlineBase64"
                }]
            }
        })
    } else {
        unreachable!()
    };

    if output::dry_run_guard(cli, "item update-definition", &body) {
        return Ok(());
    }

    let mut path = format!("/workspaces/{workspace}/items/{id}/updateDefinition");
    if update_metadata {
        path.push_str("?updateMetadata=true");
    }

    client
        .post(&path, &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "item update-definition", "ReadWrite"))?;

    let obj = serde_json::json!({
        "id": id,
        "workspace": workspace,
        "status": "definition_updated"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Validate Definition (offline) ───────────────────────────────────────────

/// Validate a definition envelope (or a folder of parts) offline, before any
/// create/update-definition API call. Emits machine-readable findings and exits
/// non-zero when the definition has errors (or, under `--strict`, warnings).
pub(super) fn validate_definition_offline(
    cli: &Cli,
    item_type: Option<&str>,
    file: Option<&str>,
    definition: Option<&str>,
    dir: Option<&str>,
    strict: bool,
) -> Result<()> {
    use anyhow::bail;

    use crate::definition_spec::{Severity, validate_definition};

    let envelope: Value = match (file, definition, dir) {
        (Some(path), None, None) => {
            let raw = fs::read_to_string(path).map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Failed to read file '{path}': {e}"),
                    "Point --file at a JSON file containing the definition envelope, \
                     or use --dir for a folder of parts."
                        .to_string(),
                )
            })?;
            serde_json::from_str(&raw).map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Invalid JSON in '{path}': {e}"),
                    "The file must contain {\"definition\":{\"parts\":[...]}} or {\"parts\":[...]}."
                        .to_string(),
                )
            })?
        }
        (None, Some(json), None) => serde_json::from_str(json).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in --definition: {e}"),
                "Provide {\"definition\":{\"parts\":[{\"path\":\"...\",\"payload\":\"base64...\",\"payloadType\":\"InlineBase64\"}]}}."
                    .to_string(),
            )
        })?,
        (None, None, Some(folder)) => assemble_parts_from_dir(folder)?,
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Provide exactly one of --file, --definition, or --dir".to_string(),
                "Example: fabio item validate-definition --type Notebook --dir ./MyNb.Notebook"
                    .to_string(),
            )
            .into());
        }
    };

    let findings = validate_definition(item_type, &envelope);
    let error_count = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warning_count = findings.len() - error_count;
    let valid = error_count == 0 && (!strict || warning_count == 0);
    let part_count = envelope
        .get("definition")
        .and_then(|d| d.get("parts"))
        .or_else(|| envelope.get("parts"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);

    let out = serde_json::json!({
        "valid": valid,
        "type": item_type,
        "partCount": part_count,
        "errorCount": error_count,
        "warningCount": warning_count,
        "strict": strict,
        "findings": findings,
    });
    output::render_object(cli, &out, "valid");

    if !valid {
        bail!(
            "Definition validation failed: {error_count} error(s), {warning_count} warning(s)\
             {}",
            if strict { " (--strict)" } else { "" }
        );
    }
    Ok(())
}

/// Walk a folder of definition parts and assemble a `{"definition":{"parts":[...]}}`
/// envelope (each file becomes an `InlineBase64` part keyed by its forward-slash
/// relative path). Mirrors how `deploy` reads an item folder.
fn assemble_parts_from_dir(folder: &str) -> Result<Value> {
    let root = Path::new(folder);
    if !root.is_dir() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Not a directory: '{folder}'"),
            "Point --dir at a folder containing definition part files (e.g. a deploy item folder)."
                .to_string(),
        )
        .into());
    }
    let mut parts = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).map_err(|e| {
            FabioError::new(
                ErrorCode::InvalidInput,
                format!("Cannot read '{}': {e}", current.display()),
            )
        })? {
            let entry = entry.map_err(|e| {
                FabioError::new(ErrorCode::InvalidInput, format!("Cannot read entry: {e}"))
            })?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                let rel_str = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                let bytes = fs::read(&path).map_err(|e| {
                    FabioError::new(
                        ErrorCode::InvalidInput,
                        format!("Cannot read '{}': {e}", path.display()),
                    )
                })?;
                parts.push(serde_json::json!({
                    "path": rel_str,
                    "payload": BASE64.encode(&bytes),
                    "payloadType": "InlineBase64",
                }));
            }
        }
    }
    Ok(serde_json::json!({ "definition": { "parts": parts } }))
}
