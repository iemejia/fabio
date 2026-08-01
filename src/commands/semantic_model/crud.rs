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

/// Build definition parts from a SINGLE model file (model.bim TMSL or one .tmdl),
/// synthesizing the required definition.pbism (and an expressions.tmdl for a
/// TMDL model given a --connection).
fn build_single_file_parts(file: &str, connection: Option<&str>) -> Result<Vec<Value>> {
    let content = std::fs::read_to_string(file).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Failed to read file '{file}': {e}"),
            "Provide a valid model.bim or .tmdl file path.".to_string(),
        )
    })?;
    let encoded = BASE64.encode(content.as_bytes());
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
    parts.push(serde_json::json!({
        "path": "definition.pbism",
        "payload": BASE64.encode(pbism.to_string().as_bytes()),
        "payloadType": "InlineBase64"
    }));

    // For TMDL models with --connection, generate the expressions.tmdl.
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
        parts.push(serde_json::json!({
            "path": "definition/expressions.tmdl",
            "payload": BASE64.encode(expr.as_bytes()),
            "payloadType": "InlineBase64"
        }));
    }
    Ok(parts)
}

/// Validate a model definition folder before create: it must have `definition.pbism`
/// and a model body (`model.bim` for TMSL, or `definition/model.tmdl` for TMDL).
fn validate_model_folder(dir: &std::path::Path) -> Result<()> {
    if !dir.join("definition.pbism").exists() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("No definition.pbism in '{}'", dir.display()),
            "Point --definition at a .SemanticModel folder (containing definition.pbism + definition/ or model.bim)."
                .to_string(),
        )
        .into());
    }
    let has_bim = dir.join("model.bim").exists();
    let has_tmdl = dir.join("definition/model.tmdl").exists();
    if !has_bim && !has_tmdl {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Model folder has neither model.bim (TMSL) nor definition/model.tmdl (TMDL)"
                .to_string(),
            "A model folder needs a model body: model.bim or definition/model.tmdl.".to_string(),
        )
        .into());
    }
    Ok(())
}

/// Gather every file under a model definition folder into definition parts, with
/// paths relative to the folder root. Excludes `.platform`, `.pbi/`, and the
/// deploy sidecar metadata files.
fn gather_model_parts(dir: &std::path::Path) -> Result<Vec<Value>> {
    let mut parts = Vec::new();
    gather_recursive(dir, dir, &mut parts)?;
    if parts.is_empty() {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            format!("No model definition files found in '{}'", dir.display()),
        )
        .into());
    }
    Ok(parts)
}

fn gather_recursive(
    base: &std::path::Path,
    current: &std::path::Path,
    parts: &mut Vec<Value>,
) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if path.is_dir() {
            if name == ".pbi" || name == ".children" {
                continue;
            }
            gather_recursive(base, &path, parts)?;
        } else {
            if name == ".platform"
                || name == "creationPayload.json"
                || name == "shortcuts.metadata.json"
                || name == "governance.metadata.json"
                || name == "schedules.metadata.json"
            {
                continue;
            }
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let content = std::fs::read(&path)?;
            parts.push(serde_json::json!({
                "path": rel,
                "payload": BASE64.encode(&content),
                "payloadType": "InlineBase64"
            }));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    file: Option<&str>,
    definition: Option<&str>,
    connection: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let parts = if let Some(folder) = definition {
        // Gather a FULL model definition folder (definition.pbism + definition/
        // TMDL files, or model.bim). This is how a real multi-file TMDL model
        // ships — previously only `deploy` could push it.
        let dir = std::path::Path::new(folder);
        validate_model_folder(dir)?;
        gather_model_parts(dir)?
    } else if let Some(file) = file {
        build_single_file_parts(file, connection)?
    } else {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Provide --file (single model.bim/.tmdl) or --definition (a full model folder)"
                .to_string(),
            "e.g. --file model.bim  OR  --definition ./Sales.SemanticModel".to_string(),
        )
        .into());
    };

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

    fn write(path: &std::path::Path, content: &str) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn gather_model_parts_collects_tmdl_folder_excluding_platform() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("definition.pbism"), r#"{"version":"4.0"}"#);
        write(&root.join("definition/model.tmdl"), "model M");
        write(&root.join("definition/tables/Sales.tmdl"), "table Sales");
        write(&root.join(".platform"), r#"{"metadata":{}}"#);
        write(&root.join(".pbi/localSettings.json"), "{}");

        let parts = gather_model_parts(root).unwrap();
        let paths: Vec<&str> = parts.iter().map(|p| p["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"definition.pbism"));
        assert!(paths.contains(&"definition/model.tmdl"));
        assert!(paths.contains(&"definition/tables/Sales.tmdl"));
        assert!(!paths.contains(&".platform"));
        assert!(!paths.iter().any(|p| p.contains(".pbi/")));
    }

    #[test]
    fn validate_model_folder_requires_pbism_and_body() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Missing pbism.
        assert!(validate_model_folder(root).is_err());
        // pbism but no model body.
        write(&root.join("definition.pbism"), "{}");
        assert!(validate_model_folder(root).is_err());
        // pbism + TMDL body → ok.
        write(&root.join("definition/model.tmdl"), "model M");
        assert!(validate_model_folder(root).is_ok());
    }

    #[test]
    fn validate_model_folder_accepts_model_bim() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("definition.pbism"), "{}");
        write(&root.join("model.bim"), r#"{"model":{}}"#);
        assert!(validate_model_folder(root).is_ok());
    }

    #[test]
    fn build_single_file_parts_tmdl_synthesizes_pbism() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("model.tmdl");
        std::fs::write(&f, "model M").unwrap();
        let parts = build_single_file_parts(f.to_str().unwrap(), None).unwrap();
        let paths: Vec<&str> = parts.iter().map(|p| p["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"definition/model.tmdl"));
        assert!(paths.contains(&"definition.pbism"));
    }

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
