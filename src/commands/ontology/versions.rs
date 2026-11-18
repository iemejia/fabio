use anyhow::Result;
use clap::ValueEnum;
use serde_json::{Map, Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CreationSource {
    #[value(name = "Manual")]
    Manual,
    #[value(name = "Automatic")]
    Automatic,
}

impl CreationSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Automatic => "Automatic",
        }
    }
}

fn versions_path(workspace: &str, ontology: &str) -> String {
    format!("/workspaces/{workspace}/ontologies/{ontology}/versions?beta=true")
}

fn version_path(workspace: &str, ontology: &str, version: &str) -> String {
    format!("/workspaces/{workspace}/ontologies/{ontology}/versions/{version}?beta=true")
}

fn restore_path(workspace: &str, ontology: &str, version: &str) -> String {
    format!("/workspaces/{workspace}/ontologies/{ontology}/versions/{version}/restore?beta=true")
}

fn create_body(
    name: &str,
    description: Option<&str>,
    creation_source: Option<CreationSource>,
) -> Result<Value> {
    validate_metadata(name, description)?;
    let mut body = Map::new();
    body.insert("displayName".to_string(), Value::from(name));
    if let Some(description) = description {
        body.insert("description".to_string(), Value::from(description));
    }
    if let Some(source) = creation_source {
        body.insert("creationSource".to_string(), Value::from(source.as_str()));
    }
    Ok(Value::Object(body))
}

fn update_body(name: Option<&str>, description: Option<&str>) -> Result<Value> {
    let mut body = Map::new();
    if let Some(name) = name {
        body.insert("displayName".to_string(), Value::from(name));
    }
    if let Some(description) = description {
        body.insert("description".to_string(), Value::from(description));
    }
    if body.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "No version fields to update",
            "Provide --name and/or --description.",
        )
        .into());
    }
    if let Some(name) = name {
        validate_metadata(name, None)?;
    }
    if let Some(description) = description {
        validate_metadata("", Some(description))?;
    }
    Ok(Value::Object(body))
}

fn validate_metadata(name: &str, description: Option<&str>) -> Result<()> {
    if name.chars().count() > 100 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Version display name exceeds 100 characters",
            "Shorten --name to 100 characters or fewer.",
        )
        .into());
    }
    if description.is_some_and(|value| value.chars().count() > 500) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Version description exceeds 500 characters",
            "Shorten --description to 500 characters or fewer.",
        )
        .into());
    }
    Ok(())
}

fn restore_body(file: Option<&str>, content: Option<&str>) -> Result<Value> {
    let options = match (file, content) {
        (Some(path), None) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| anyhow::anyhow!("Failed to read file '{path}': {error}"))?;
            Some(serde_json::from_str::<Value>(&text)?)
        }
        (None, Some(value)) => Some(serde_json::from_str::<Value>(value)?),
        (None, None) => None,
        (Some(_), Some(_)) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--file and --content cannot be used together",
                "Provide only one restore-options input.",
            )
            .into());
        }
    };
    if options.as_ref().is_some_and(|value| !value.is_object()) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Restore options must be a JSON object",
            r#"Example: --content '{"allowOverrideItems":true}'"#,
        )
        .into());
    }
    Ok(options.map_or_else(|| json!({}), |options| json!({"options": options})))
}

pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    ontology: &str,
) -> Result<()> {
    let response = client
        .get_list(
            &versions_path(workspace, ontology),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|error| enrich_forbidden(error, "ontology list-versions", "Contributor"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        &[
            "displayName",
            "id",
            "description",
            "createdDateTime",
            "creationSource",
        ],
        &["NAME", "ID", "DESCRIPTION", "CREATED", "SOURCE"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    ontology: &str,
    version: &str,
) -> Result<()> {
    let data = client
        .get(&version_path(workspace, ontology, version))
        .await
        .map_err(|error| enrich_forbidden(error, "ontology show-version", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    ontology: &str,
    name: &str,
    description: Option<&str>,
    creation_source: Option<CreationSource>,
) -> Result<()> {
    let body = create_body(name, description, creation_source)?;
    let preview = json!({"workspaceId": workspace, "ontologyId": ontology, "request": body});
    if output::dry_run_guard(cli, "ontology create-version", &preview) {
        return Ok(());
    }

    let data = client
        .post(&versions_path(workspace, ontology), &body, true)
        .await
        .map_err(|error| enrich_forbidden(error, "ontology create-version", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    ontology: &str,
    version: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    let body = update_body(name, description)?;
    let preview = json!({
        "workspaceId": workspace,
        "ontologyId": ontology,
        "versionId": version,
        "request": body
    });
    if output::dry_run_guard(cli, "ontology update-version", &preview) {
        return Ok(());
    }

    let data = client
        .patch(&version_path(workspace, ontology, version), &body)
        .await
        .map_err(|error| enrich_forbidden(error, "ontology update-version", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    ontology: &str,
    version: &str,
) -> Result<()> {
    let preview = json!({"workspaceId": workspace, "ontologyId": ontology, "versionId": version});
    if output::dry_run_guard_maybe_destructive(cli, "ontology delete-version", &preview, true) {
        return Ok(());
    }

    client
        .delete(&version_path(workspace, ontology, version))
        .await
        .map_err(|error| enrich_forbidden(error, "ontology delete-version", "Contributor"))?;
    output::render_object(cli, &json!({"status": "deleted", "id": version}), "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn restore(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    ontology: &str,
    version: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = restore_body(file, content)?;
    let preview = json!({
        "workspaceId": workspace,
        "ontologyId": ontology,
        "versionId": version,
        "request": body
    });
    if output::dry_run_guard_maybe_destructive(cli, "ontology restore-version", &preview, true) {
        return Ok(());
    }

    let data = client
        .post(&restore_path(workspace, ontology, version), &body, true)
        .await
        .map_err(|error| enrich_forbidden(error, "ontology restore-version", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_paths_require_beta() {
        assert_eq!(
            version_path("workspace", "ontology", "version"),
            "/workspaces/workspace/ontologies/ontology/versions/version?beta=true"
        );
        assert_eq!(
            restore_path("workspace", "ontology", "version"),
            "/workspaces/workspace/ontologies/ontology/versions/version/restore?beta=true"
        );
    }

    #[test]
    fn create_body_maps_all_version_fields() {
        let body =
            create_body("v1", Some("Initial snapshot"), Some(CreationSource::Manual)).unwrap();
        assert_eq!(body["displayName"], "v1");
        assert_eq!(body["description"], "Initial snapshot");
        assert_eq!(body["creationSource"], "Manual");
    }

    #[test]
    fn update_body_rejects_empty_request() {
        assert!(update_body(None, None).is_err());
    }

    #[test]
    fn version_metadata_enforces_spec_lengths() {
        assert!(create_body(&"x".repeat(101), None, None).is_err());
        assert!(update_body(None, Some(&"x".repeat(501))).is_err());
    }

    #[test]
    fn restore_body_wraps_options() {
        let body = restore_body(None, Some(r#"{"allowOverrideItems":true}"#)).unwrap();
        assert_eq!(body["options"]["allowOverrideItems"], true);
        assert_eq!(restore_body(None, None).unwrap(), json!({}));
        assert!(restore_body(None, Some("[]")).is_err());
    }

    #[test]
    fn version_metadata_accepts_exact_limits_and_description_clear() {
        assert!(create_body(&"x".repeat(100), Some(&"x".repeat(500)), None).is_ok());
        assert_eq!(
            update_body(None, Some("")).unwrap(),
            json!({"description": ""})
        );
        assert_eq!(
            create_body("Snapshot", None, None).unwrap(),
            json!({"displayName": "Snapshot"})
        );
    }

    #[test]
    fn restore_file_matches_inline_and_rejects_conflicting_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("restore.json");
        let content = r#"{"allowOverrideItems":false}"#;
        std::fs::write(&path, content).unwrap();
        assert_eq!(
            restore_body(Some(path.to_str().unwrap()), None).unwrap(),
            restore_body(None, Some(content)).unwrap()
        );
        assert!(restore_body(Some(path.to_str().unwrap()), Some(content)).is_err());
        assert!(restore_body(None, Some("{")).is_err());
        assert!(restore_body(None, Some("null")).is_err());
    }
}
