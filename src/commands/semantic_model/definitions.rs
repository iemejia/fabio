use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

pub(super) async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/semanticModels/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model get-definition", "Contributor"))?;
    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

pub(super) async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: &str,
    allow_purge_data: bool,
) -> Result<()> {
    let content = std::fs::read_to_string(file).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Failed to read file '{file}': {e}"),
            "Provide a model.bim (TMSL) file. A SemanticModel definition needs definition.pbism \
             plus a model body (model.bim, or a definition/ folder of *.tmdl). \
             See the canonical parts: fabio context schema SemanticModel. \
             Validate offline: fabio item validate-definition --type SemanticModel --dir <folder>."
                .to_string(),
        )
    })?;
    let body = build_update_definition_body(&content, allow_purge_data);

    // Surface the purge warning + destructive signal in the dry-run preview when the
    // irreversible --allow-purge-data bypass is active.
    if output::dry_run_guard_purge_aware(
        cli,
        "semantic-model update-definition",
        &body,
        allow_purge_data,
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/semanticModels/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model update-definition", "Contributor"))?;

    let obj = build_update_definition_result(id, workspace, allow_purge_data);
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Build the success payload for `update-definition`, attaching an irreversibility
/// warning whenever the `--allow-purge-data` safety bypass was active so the signal
/// is present in the success output, not only in the dry-run preview.
fn build_update_definition_result(
    id: &str,
    workspace: &str,
    allow_purge_data: bool,
) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "id": id,
        "workspace": workspace,
        "status": "definition_updated"
    });
    output::attach_purge_warning(&mut obj, allow_purge_data);
    obj
}

fn build_update_definition_body(content: &str, allow_purge_data: bool) -> serde_json::Value {
    let mut body = crate::definition_spec::build_update_definition_body(content, "model.bim");
    if allow_purge_data {
        body["options"] = serde_json::json!({
            "allowPurgeData": true
        });
    }
    body
}

#[cfg(test)]
mod tests {
    use super::{build_update_definition_body, build_update_definition_result};

    #[test]
    fn update_definition_result_warns_when_purge_enabled() {
        let obj = build_update_definition_result("id-1", "ws-1", true);
        assert_eq!(obj["status"], "definition_updated");
        let warning = obj["warning"].as_str().expect("warning present");
        assert!(warning.contains("allowPurgeData"));
        assert!(warning.contains("irreversible"));
        // Wording must be conditional: allowPurgeData only PERMITS a purge.
        assert!(warning.contains("may leave data intact"));
    }

    #[test]
    fn update_definition_result_has_no_warning_by_default() {
        let obj = build_update_definition_result("id-1", "ws-1", false);
        assert!(obj.get("warning").is_none());
    }

    #[test]
    fn update_definition_serializes_allow_purge_data_option() {
        let body = build_update_definition_body(r#"{"model":{}}"#, true);
        assert_eq!(body["options"]["allowPurgeData"], true);
        assert_eq!(body["definition"]["parts"][0]["path"], "model.bim");
    }

    #[test]
    fn update_definition_omits_options_by_default() {
        let body = build_update_definition_body(r#"{"model":{}}"#, false);
        assert!(body.get("options").is_none());
    }
}
