use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::FabioError;
use crate::output;

/// The staging-settings object that carries preview/experimental toggles.
///
/// The preview-runtime flag (Advanced NL2SQL and other preview built-in tools)
/// lives as a boolean inside this object. The Fabric data agent Python SDK
/// exposes it as `update_configuration(enable_preview_runtime=...)` and reads it
/// back as `config.enable_preview_runtime`; on the wire it is
/// `experimental.enableExperimentalFeatures` (live-confirmed via `staging/settings`).
const EXPERIMENTAL_FIELD: &str = "experimental";
/// The boolean flag inside the `experimental` object that selects the runtime.
const PREVIEW_RUNTIME_FLAG: &str = "enableExperimentalFeatures";

/// Get agent configuration via the settings API.
///
/// Uses: `GET /workspaces/{ws}/dataAgents/{id}/staging/settings` (staging)
///   or: `GET /workspaces/{ws}/dataAgents/{id}/settings` (published)
pub(super) async fn get_config(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    stage: &str,
) -> Result<()> {
    let prefix = stage_prefix(stage);
    let settings = client
        .get(&format!(
            "/workspaces/{workspace}/dataAgents/{id}{prefix}/settings"
        ))
        .await?;

    // Also fetch datasources list to include summary in config output
    let ds_resp = client
        .get_list(
            &format!("/workspaces/{workspace}/dataAgents/{id}{prefix}/datasources"),
            "value",
            true,
            None,
        )
        .await?;

    let ai_instructions = settings
        .get("aiInstructions")
        .cloned()
        .unwrap_or(Value::Null);

    let datasources: Vec<Value> = ds_resp
        .items
        .iter()
        .map(|ds| {
            serde_json::json!({
                "id": ds.get("id").and_then(Value::as_str),
                "displayName": ds.get("displayName").and_then(Value::as_str),
                "type": ds.get("type").and_then(Value::as_str),
            })
        })
        .collect();

    let config = serde_json::json!({
        "instructions": ai_instructions,
        "previewRuntime": preview_runtime_enabled(&settings),
        "dataSources": datasources,
    });

    output::render_object(cli, &config, "instructions");
    Ok(())
}

/// Read the preview-runtime toggle from a staging/published settings object.
///
/// Fabric nests the flag as `experimental.enableExperimentalFeatures`. A missing
/// `experimental` object, an empty one, or the flag absent all read back as
/// `false` (the standard runtime — the default for new agents).
fn preview_runtime_enabled(settings: &Value) -> bool {
    settings
        .get(EXPERIMENTAL_FIELD)
        .and_then(|e| e.get(PREVIEW_RUNTIME_FLAG))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Build the staging-settings PATCH body from the requested changes.
///
/// Pure so it can be unit-tested without an HTTP round-trip. Only fields the
/// caller actually requested are included (partial update).
///
/// When a runtime change is requested, the `experimental` object is rebuilt from
/// `existing_experimental` (read-modify-write) so sibling keys the server owns
/// (e.g. `mcpServers`) are preserved — only `enableExperimentalFeatures` flips.
/// `runtime_change` is `Some(true)` to enable, `Some(false)` to disable, `None`
/// to leave the runtime untouched.
fn build_settings_body(
    instructions: Option<&str>,
    runtime_change: Option<bool>,
    existing_experimental: Option<&Value>,
) -> serde_json::Map<String, Value> {
    let mut body = serde_json::Map::new();
    if let Some(instr) = instructions {
        body.insert("aiInstructions".to_string(), Value::from(instr));
    }
    if let Some(enabled) = runtime_change {
        // Preserve any sibling keys the server stores under `experimental`.
        let mut experimental = existing_experimental
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        experimental.insert(PREVIEW_RUNTIME_FLAG.to_string(), Value::Bool(enabled));
        body.insert(EXPERIMENTAL_FIELD.to_string(), Value::Object(experimental));
    }
    body
}

// ─── Private Helpers ─────────────────────────────────────────────────────────

const fn stage_prefix(stage: &str) -> &str {
    if stage.eq_ignore_ascii_case("published") {
        ""
    } else {
        "/staging"
    }
}

/// Update agent configuration via the staging settings API.
///
/// Uses: `PATCH /workspaces/{ws}/dataAgents/{id}/staging/settings`
#[allow(clippy::fn_params_excessive_bools, clippy::too_many_arguments)]
pub(super) async fn update_config(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    instructions: Option<&str>,
    instructions_file: Option<&str>,
    enable_preview_runtime: bool,
    disable_preview_runtime: bool,
) -> Result<()> {
    // Resolve instructions from --instructions or --instructions-file
    let resolved_instructions = match (instructions, instructions_file) {
        (Some(instr), _) => Some(instr.to_string()),
        (_, Some(path)) => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read instructions file '{path}': {e}"))?;
            Some(content)
        }
        _ => None,
    };

    // clap makes --enable/--disable-preview-runtime mutually exclusive; `None`
    // means "leave the runtime untouched".
    let runtime_change = if enable_preview_runtime {
        Some(true)
    } else if disable_preview_runtime {
        Some(false)
    } else {
        None
    };

    if resolved_instructions.is_none() && runtime_change.is_none() {
        return Err(FabioError::invalid_input(
            "At least one of --instructions, --instructions-file, --enable-preview-runtime, or --disable-preview-runtime must be provided",
        )
        .into());
    }

    if output::dry_run_guard(
        cli,
        "data-agent update-config",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "instructions": resolved_instructions.as_deref().map(|s| if s.len() > 100 { format!("{}...", &s[..s.floor_char_boundary(100)]) } else { s.to_string() }),
            "instructionsFile": instructions_file,
            "enablePreviewRuntime": enable_preview_runtime,
            "disablePreviewRuntime": disable_preview_runtime,
        }),
    ) {
        return Ok(());
    }

    // Read-modify-write: when toggling the runtime, fetch current settings first
    // so we preserve any sibling keys the server stores under `experimental`
    // (e.g. `mcpServers`). Skip the extra GET when only instructions change.
    let existing_experimental = if runtime_change.is_some() {
        client
            .get(&format!(
                "/workspaces/{workspace}/dataAgents/{id}/staging/settings"
            ))
            .await
            .ok()
            .and_then(|s| s.get(EXPERIMENTAL_FIELD).cloned())
    } else {
        None
    };

    let body = build_settings_body(
        resolved_instructions.as_deref(),
        runtime_change,
        existing_experimental.as_ref(),
    );

    let resp = client
        .patch(
            &format!("/workspaces/{workspace}/dataAgents/{id}/staging/settings"),
            &Value::Object(body),
        )
        .await?;

    // Report the effective preview-runtime state so agents can confirm which
    // runtime the agent will use for SQL sources (NL2SQL vs Advanced NL2SQL).
    // Prefer the server-echoed value; fall back to what we requested.
    let effective_preview = resp
        .get(EXPERIMENTAL_FIELD)
        .and_then(|e| e.get(PREVIEW_RUNTIME_FLAG))
        .and_then(Value::as_bool)
        .or(runtime_change);

    let mut result = if resp.is_null() || resp.as_object().is_some_and(serde_json::Map::is_empty) {
        serde_json::json!({
            "id": id,
            "status": "config_updated",
            "instructions": resolved_instructions.as_deref(),
        })
    } else {
        let mut r = serde_json::json!({
            "id": id,
            "status": "config_updated",
        });
        if let Some(instr) = resp.get("aiInstructions") {
            r["instructions"] = instr.clone();
        }
        r
    };
    if let Some(preview) = effective_preview {
        result["previewRuntime"] = Value::Bool(preview);
    }
    output::render_object(cli, &result, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn experimental_flag(body: &serde_json::Map<String, Value>) -> Option<bool> {
        body.get(EXPERIMENTAL_FIELD)
            .and_then(|e| e.get(PREVIEW_RUNTIME_FLAG))
            .and_then(Value::as_bool)
    }

    #[test]
    fn build_settings_body_instructions_only() {
        let body = build_settings_body(Some("guide the agent"), None, None);
        assert_eq!(
            body.get("aiInstructions").and_then(Value::as_str),
            Some("guide the agent")
        );
        // No runtime change requested → experimental object absent (partial update).
        assert!(!body.contains_key(EXPERIMENTAL_FIELD));
    }

    #[test]
    fn build_settings_body_enable_preview_runtime() {
        let body = build_settings_body(None, Some(true), None);
        assert_eq!(experimental_flag(&body), Some(true));
        assert!(!body.contains_key("aiInstructions"));
    }

    #[test]
    fn build_settings_body_disable_preview_runtime() {
        let body = build_settings_body(None, Some(false), None);
        assert_eq!(experimental_flag(&body), Some(false));
    }

    #[test]
    fn build_settings_body_instructions_and_preview_runtime() {
        let body = build_settings_body(Some("route SQL to lakehouse"), Some(true), None);
        assert_eq!(
            body.get("aiInstructions").and_then(Value::as_str),
            Some("route SQL to lakehouse")
        );
        assert_eq!(experimental_flag(&body), Some(true));
    }

    #[test]
    fn build_settings_body_empty_when_no_changes() {
        let body = build_settings_body(None, None, None);
        assert!(body.is_empty());
    }

    #[test]
    fn build_settings_body_preserves_sibling_experimental_keys() {
        // Read-modify-write must not clobber server-owned siblings (e.g. mcpServers).
        let existing = json!({
            "enableExperimentalFeatures": false,
            "mcpServers": [{"name": "keep-me"}]
        });
        let body = build_settings_body(None, Some(true), Some(&existing));
        assert_eq!(experimental_flag(&body), Some(true));
        assert_eq!(
            body[EXPERIMENTAL_FIELD]["mcpServers"][0]["name"],
            json!("keep-me"),
            "sibling experimental keys must be preserved on toggle"
        );
    }

    #[test]
    fn build_settings_body_ignores_non_object_existing_experimental() {
        // A malformed/null existing value must not break the toggle.
        let body = build_settings_body(None, Some(true), Some(&Value::Null));
        assert_eq!(experimental_flag(&body), Some(true));
    }

    #[test]
    fn preview_runtime_enabled_reads_nested_flag() {
        assert!(preview_runtime_enabled(
            &json!({ "experimental": { "enableExperimentalFeatures": true } })
        ));
        assert!(!preview_runtime_enabled(
            &json!({ "experimental": { "enableExperimentalFeatures": false } })
        ));
    }

    #[test]
    fn preview_runtime_enabled_defaults_false_when_absent() {
        assert!(!preview_runtime_enabled(&json!({ "aiInstructions": "x" })));
        assert!(!preview_runtime_enabled(&json!({ "experimental": {} })));
        assert!(!preview_runtime_enabled(&json!({ "experimental": null })));
        assert!(!preview_runtime_enabled(&json!({})));
    }
}
