use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

pub(super) async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/semanticModels"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    let has_labels = resp
        .items
        .iter()
        .any(|item| item.get("sensitivityLabel").is_some_and(|v| !v.is_null()));
    let has_tags = output::has_tags(&resp.items);

    let display_items;
    let items_ref: &[Value] = if has_tags {
        display_items = output::enrich_with_tags_display(&resp.items);
        &display_items
    } else {
        &resp.items
    };

    match (has_labels, has_tags) {
        (true, true) => output::render_list_with_token(
            cli,
            items_ref,
            &[
                "displayName",
                "id",
                "description",
                "sensitivityLabel.id",
                "_tagsDisplay",
            ],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (true, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "sensitivityLabel.id"],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, true) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "_tagsDisplay"],
            &["NAME", "ID", "DESCRIPTION", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description"],
            &["NAME", "ID", "DESCRIPTION"],
            "id",
            resp.continuation_token.as_deref(),
        ),
    }
    Ok(())
}

pub(super) async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/semanticModels/{id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

/// Build the `definition.pbism` for a semantic model.
///
/// The MS schema (`semanticModel/definitionProperties/1.0.0`) marks `$schema`
/// and `version` as REQUIRED. Version "4.0" for TMDL, "3.0" for model.bim (v3
/// JSON); Fabric normalizes the stored version on ingest.
fn build_pbism(is_tmdl: bool) -> Value {
    serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/semanticModel/definitionProperties/1.0.0/schema.json",
        "version": if is_tmdl { "4.0" } else { "3.0" }
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    file: &str,
    connection: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let content = std::fs::read_to_string(file).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Failed to read file '{file}': {e}"),
            "Provide a valid model.bim or .tmdl file path.".to_string(),
        )
    })?;
    let encoded = BASE64.encode(content.as_bytes());

    // Detect format from file extension
    let is_tmdl = std::path::Path::new(file)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("tmdl"));

    let mut parts = vec![serde_json::json!({
        "path": if is_tmdl { "definition/model.tmdl" } else { "model.bim" },
        "payload": encoded,
        "payloadType": "InlineBase64"
    })];

    // Always include definition.pbism (required by Fabric API).
    let pbism = build_pbism(is_tmdl);
    let pbism_encoded = BASE64.encode(pbism.to_string().as_bytes());
    parts.push(serde_json::json!({
        "path": "definition.pbism",
        "payload": pbism_encoded,
        "payloadType": "InlineBase64"
    }));

    // For TMDL models with --connection, generate the expressions.tmdl
    if let Some(conn_id) = connection
        && is_tmdl
    {
        let expr = format!(
            "expression DatabaseQuery =\n\
                 \t\tlet\n\
                 \t\t\tdatabase = Sql.Database(\"placeholder\", \"{conn_id}\")\n\
                 \t\tin\n\
                 \t\t\tdatabase\n\
                 \tlineageTag: 00000000-0000-0000-0000-000000000001"
        );
        let expr_encoded = BASE64.encode(expr.as_bytes());
        parts.push(serde_json::json!({
            "path": "definition/expressions.tmdl",
            "payload": expr_encoded,
            "payloadType": "InlineBase64"
        }));
    }

    let mut body = serde_json::json!({
        "displayName": name,
        "definition": {
            "parts": parts
        }
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(
        cli,
        "semantic-model create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "file": file,
            "connection": connection,
            "sensitivityLabel": sensitivity_label
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/semanticModels"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_create_error(enrich_forbidden(e, "semantic-model create", "Member")))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio semantic-model update --workspace <WS> --id <ID> --name \"New Name\""
                .to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::from(n);
    }
    if let Some(d) = description {
        body["description"] = Value::from(d);
    }

    if output::dry_run_guard(cli, "semantic-model update", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/semanticModels/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "semantic-model delete",
        &serde_json::json!({
            "workspace": workspace,
            "id": id, "hardDelete": hard_delete
        }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/semanticModels/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/semanticModels/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Error Enrichment ────────────────────────────────────────────────────────

/// Enrich semantic model API errors with actionable hints for common failures.
///
/// Intercepts known error patterns and provides corrective guidance so that
/// agents (and users) can self-correct without searching documentation.
fn enrich_create_error(err: anyhow::Error) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };

    let msg = &fabio_err.message;
    let msg_lower = msg.to_lowercase();

    // Pattern: "Import from JSON supported for V3 models only"
    if msg_lower.contains("v3 models only") || msg_lower.contains("import from json") {
        return FabioError::with_hint(
            fabio_err.code,
            msg.clone(),
            "model.bim must use compatibilityLevel 1604 (not 1550) and include \
             \"defaultPowerBIDataSourceVersion\": \"powerBI_V3\" in the model object. \
             Example: {\"compatibilityLevel\": 1604, \"model\": {\"defaultPowerBIDataSourceVersion\": \"powerBI_V3\", ...}}"
        ).into();
    }

    // Pattern: TMDL "InvalidValueFormat" for PowerBIDataSourceVersion
    if msg_lower.contains("invalidvalueformat") && msg_lower.contains("powerbidatasourceversion") {
        return FabioError::with_hint(
            fabio_err.code,
            msg.clone(),
            "In TMDL, use 'defaultPowerBIDataSourceVersion: powerBI_V3' (with underscore). \
             The value 'powerBIDataSourceVersion3' is not valid. \
             Valid values: powerBI_V3.",
        )
        .into();
    }

    // Pattern: TMDL general parsing errors
    if msg_lower.contains("tmdl format error") {
        let hint = if msg_lower.contains("line number") {
            "Check TMDL syntax at the reported line. Common issues: \
             (1) Use tabs for indentation (not spaces). \
             (2) Enum values are case-sensitive (e.g., powerBI_V3, not powerbi_v3). \
             (3) Each table/column/partition needs a lineageTag GUID. \
             Reference: https://learn.microsoft.com/en-us/power-bi/developer/projects/projects-dataset#tmdl-format"
        } else {
            "TMDL parsing failed. Verify file uses tab indentation and valid enum values. \
             Reference: https://learn.microsoft.com/en-us/power-bi/developer/projects/projects-dataset#tmdl-format"
        };
        return FabioError::with_hint(fabio_err.code, msg.clone(), hint).into();
    }

    // Pattern: Definition parts missing or invalid
    if msg_lower.contains("definition") && msg_lower.contains("invalid") {
        return FabioError::with_hint(
            fabio_err.code,
            msg.clone(),
            "Semantic model creation requires: (1) a model definition file (model.bim or .tmdl), \
             (2) a definition.pbism entry. The CLI auto-generates definition.pbism. \
             For .bim files use compat 1604 + powerBI_V3. \
             For .tmdl files ensure 'defaultPowerBIDataSourceVersion: powerBI_V3'.",
        )
        .into();
    }

    // Pattern: DirectLake requires TMDL
    if msg_lower.contains("directlake") || msg_lower.contains("direct lake") {
        return FabioError::with_hint(
            fabio_err.code,
            msg.clone(),
            "Direct Lake semantic models require TMDL format (not model.bim). \
             Use a .tmdl file with partition mode: directLake and provide \
             --connection <sql-endpoint-id> to bind the lakehouse connection.",
        )
        .into();
    }

    // No known pattern matched — return original error
    err
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enrich_create_error_v3_models() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::ApiError,
            "Import from JSON supported for V3 models only".to_string(),
        )
        .into();

        let enriched = enrich_create_error(err);
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert!(fabio_err.hint.as_ref().unwrap().contains("1604"));
    }

    #[test]
    fn test_enrich_create_error_tmdl_format() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::ApiError,
            "TMDL Format Error: Parsing error at line number 5".to_string(),
        )
        .into();

        let enriched = enrich_create_error(err);
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert!(fabio_err.hint.as_ref().unwrap().contains("tab"));
    }

    #[test]
    fn pbism_conforms_to_ms_schema() {
        // MS semanticModel/definitionProperties/1.0.0 requires $schema + version.
        for (is_tmdl, want_version) in [(true, "4.0"), (false, "3.0")] {
            let pbism = build_pbism(is_tmdl);
            let schema = pbism["$schema"].as_str().unwrap();
            assert!(
                schema.contains("semanticModel/definitionProperties/1.")
                    && schema.ends_with("/schema.json"),
                "unexpected $schema: {schema}"
            );
            assert_eq!(pbism["version"], want_version);
        }
    }
}
