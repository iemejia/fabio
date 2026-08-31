use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum MlModelCommand {
    /// List ML models in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of an ML model
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,
    },
    /// Create a new ML model
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update ML model properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete an ML model
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the ML model serving endpoint configuration
    #[command(display_order = 10)]
    GetEndpoint {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,
    },
    /// Update the ML model serving endpoint configuration
    #[command(display_order = 11)]
    UpdateEndpoint {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,

        /// Path to JSON file with endpoint config
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,

        /// Inline JSON content with endpoint config
        #[arg(long, conflicts_with = "file")]
        content: Option<String>,
    },
    /// Score against the ML model endpoint
    #[command(display_order = 12)]
    Score {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,

        /// Path to JSON file with input data
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,

        /// Inline JSON input data
        #[arg(long, conflicts_with = "file")]
        content: Option<String>,
    },
    /// List endpoint versions
    #[command(display_order = 20)]
    ListVersions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,
    },
    /// Get a specific endpoint version
    #[command(display_order = 21)]
    GetVersion {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,

        /// Version name
        #[arg(long)]
        version_name: String,
    },
    /// Update a specific endpoint version
    #[command(display_order = 22)]
    UpdateVersion {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,

        /// Version name
        #[arg(long)]
        version_name: String,

        /// Path to JSON file with version config
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,

        /// Inline JSON content with version config
        #[arg(long, conflicts_with = "file")]
        content: Option<String>,
    },
    /// Activate a specific endpoint version
    #[command(display_order = 23)]
    ActivateVersion {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,

        /// Version name
        #[arg(long)]
        version_name: String,
    },
    /// Deactivate a specific endpoint version
    #[command(display_order = 24)]
    DeactivateVersion {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,

        /// Version name
        #[arg(long)]
        version_name: String,
    },
    /// Score against a specific endpoint version
    #[command(display_order = 25)]
    ScoreVersion {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,

        /// Version name
        #[arg(long)]
        version_name: String,

        /// Path to JSON file with input data
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,

        /// Inline JSON input data
        #[arg(long, conflicts_with = "file")]
        content: Option<String>,
    },
    /// Deactivate all endpoint versions
    #[command(display_order = 26)]
    DeactivateAllVersions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,
    },

    /// List the `MLflow` model-registry versions of an ML model (the trained model versions)
    #[command(display_order = 30)]
    ListRegistryVersions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,
    },

    /// Get a specific `MLflow` model-registry version (source run, stage, status)
    #[command(display_order = 31, disable_version_flag = true)]
    GetRegistryVersion {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML model ID
        #[arg(long)]
        id: String,

        /// `MLflow` model-registry version number (e.g. 1, 2, 3)
        #[arg(long)]
        version: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &MlModelCommand) -> Result<()> {
    match command {
        MlModelCommand::List { workspace } => list(cli, client, workspace).await,
        MlModelCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        MlModelCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        MlModelCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            update(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        MlModelCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        MlModelCommand::GetEndpoint { workspace, id } => {
            get_endpoint(cli, client, workspace, id).await
        }
        MlModelCommand::UpdateEndpoint {
            workspace,
            id,
            file,
            content,
        } => {
            update_endpoint(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        MlModelCommand::Score {
            workspace,
            id,
            file,
            content,
        } => {
            score(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        MlModelCommand::ListVersions { workspace, id } => {
            list_versions(cli, client, workspace, id).await
        }
        MlModelCommand::GetVersion {
            workspace,
            id,
            version_name,
        } => get_version(cli, client, workspace, id, version_name).await,
        MlModelCommand::UpdateVersion {
            workspace,
            id,
            version_name,
            file,
            content,
        } => {
            update_version(
                cli,
                client,
                workspace,
                id,
                version_name,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        MlModelCommand::ActivateVersion {
            workspace,
            id,
            version_name,
        } => activate_version(cli, client, workspace, id, version_name).await,
        MlModelCommand::DeactivateVersion {
            workspace,
            id,
            version_name,
        } => deactivate_version(cli, client, workspace, id, version_name).await,
        MlModelCommand::ScoreVersion {
            workspace,
            id,
            version_name,
            file,
            content,
        } => {
            score_version(
                cli,
                client,
                workspace,
                id,
                version_name,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        MlModelCommand::DeactivateAllVersions { workspace, id } => {
            deactivate_all_versions(cli, client, workspace, id).await
        }
        MlModelCommand::ListRegistryVersions { workspace, id } => {
            list_registry_versions(cli, client, workspace, id).await
        }
        MlModelCommand::GetRegistryVersion {
            workspace,
            id,
            version,
        } => get_registry_version(cli, client, workspace, id, version).await,
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/mlModels"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_item_list(
        cli,
        &resp.items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/mlModels/{id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
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
        "ml-model create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "sensitivityLabel": sensitivity_label
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/mlModels"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model create", "Member"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update(
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
            "Example: fabio ml-model update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "ml-model update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/mlModels/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

fn read_json_body(file: Option<&str>, content: Option<&str>, command: &str) -> Result<Value> {
    match (file, content) {
        (Some(f), _) => {
            let text = std::fs::read_to_string(f)
                .map_err(|e| FabioError::not_found(format!("File not found: {f}: {e}")))?;
            Ok(serde_json::from_str(&text)?)
        }
        (_, Some(c)) => Ok(serde_json::from_str(c)?),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            format!(
                "Example: fabio ml-model {command} --workspace <WS> --id <ID> --content '{{...}}'"
            ),
        )
        .into()),
    }
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "ml-model delete",
        &serde_json::json!({
            "workspace": workspace,
            "id": id, "hardDelete": hard_delete
        }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/mlModels/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/mlModels/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn get_endpoint(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/mlModels/{id}/endpoint"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update_endpoint(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "update-endpoint")?;

    if output::dry_run_guard(cli, "ml-model update-endpoint", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/mlModels/{id}/endpoint"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model update-endpoint", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn score(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "score")?;

    let data = client
        .post(
            &format!("/workspaces/{workspace}/mlModels/{id}/endpoint/score"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model score", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn list_versions(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/mlModels/{id}/endpoint/versions"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

// ─── MLflow model registry ───────────────────────────────────────────────────

/// The per-workspace Fabric-hosted `MLflow` REST base. A Fabric ML Model item is
/// an `MLflow` *registered model* whose name equals the item's display name, so
/// the registry is queried by name (not by item GUID).
fn mlflow_base(workspace: &str) -> String {
    format!("/workspaces/{workspace}/mlflow/api/2.0/mlflow")
}

/// Build an `MLflow` `model-versions/search` filter for a registered model name,
/// doubling single quotes to keep the filter well-formed.
fn model_versions_filter(name: &str) -> String {
    let escaped = name.replace('\'', "''");
    format!("name='{escaped}'")
}

/// Resolve an ML Model item id to its display name (the `MLflow` registered-model name).
async fn resolve_model_name(client: &FabricClient, workspace: &str, id: &str) -> Result<String> {
    let item = client
        .get(&format!("/workspaces/{workspace}/mlModels/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model", "Viewer"))?;
    item.get("displayName")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "ML model has no displayName; cannot resolve the MLflow registered-model name."
                    .to_string(),
            )
            .into()
        })
}

async fn list_registry_versions(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let name = resolve_model_name(client, workspace, id).await?;
    let filter_expr = model_versions_filter(&name);
    let filter = urlencoding::encode(&filter_expr);
    let data = client
        .get(&format!(
            "{}/model-versions/search?filter={filter}",
            mlflow_base(workspace)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model list-registry-versions", "Viewer"))?;
    let versions = data
        .get("model_versions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    output::render_list(
        cli,
        &versions,
        &["name", "version", "current_stage", "status", "run_id"],
        &["NAME", "VERSION", "STAGE", "STATUS", "RUN ID"],
        "version",
    );
    Ok(())
}

async fn get_registry_version(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    version: &str,
) -> Result<()> {
    let name = resolve_model_name(client, workspace, id).await?;
    let name_q = urlencoding::encode(&name);
    let version_q = urlencoding::encode(version);
    let data = client
        .get(&format!(
            "{}/model-versions/get?name={name_q}&version={version_q}",
            mlflow_base(workspace)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model get-registry-version", "Viewer"))?;
    // Unwrap the `{ "model_version": {...} }` envelope so the version fields
    // (name, version, status, run_id, source, …) are at the top level — matching
    // the flattened rows from `list-registry-versions` and `ml-experiment get-run`.
    let mv = unwrap_model_version(data);
    output::render_object(cli, &mv, "version");
    Ok(())
}

/// Unwrap the `MLflow` model-version envelope (`{ "model_version": {...} }`) so the
/// version fields (`name`, `version`, `status`, `run_id`, `source`, …) are at the
/// top level. If the `model_version` key is absent (already-flat or an unexpected
/// shape), the input is returned unchanged.
fn unwrap_model_version(data: Value) -> Value {
    data.get("model_version").cloned().unwrap_or(data)
}

async fn get_version(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    version_name: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/mlModels/{id}/endpoint/versions/{version_name}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update_version(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    version_name: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "update-version")?;

    if output::dry_run_guard(cli, "ml-model update-version", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/mlModels/{id}/endpoint/versions/{version_name}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model update-version", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn activate_version(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    version_name: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "ml-model activate-version",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "versionName": version_name
        }),
    ) {
        return Ok(());
    }

    let body = serde_json::json!({});
    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/mlModels/{id}/endpoint/versions/{version_name}/activate"
            ),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model activate-version", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn deactivate_version(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    version_name: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "ml-model deactivate-version",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "versionName": version_name
        }),
    ) {
        return Ok(());
    }

    let body = serde_json::json!({});
    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/mlModels/{id}/endpoint/versions/{version_name}/deactivate"
            ),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model deactivate-version", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn score_version(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    version_name: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "score-version")?;

    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/mlModels/{id}/endpoint/versions/{version_name}/score"
            ),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model score-version", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn deactivate_all_versions(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "ml-model deactivate-all-versions",
        &serde_json::json!({
            "workspace": workspace,
            "id": id
        }),
    ) {
        return Ok(());
    }

    let body = serde_json::json!({});
    let data = client
        .post(
            &format!("/workspaces/{workspace}/mlModels/{id}/endpoint/versions/deactivateAll"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ml-model deactivate-all-versions", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mlflow_base_path() {
        assert_eq!(
            mlflow_base("ws-1"),
            "/workspaces/ws-1/mlflow/api/2.0/mlflow"
        );
    }

    #[test]
    fn model_versions_filter_builds_name_clause() {
        assert_eq!(model_versions_filter("MyModel"), "name='MyModel'");
    }

    #[test]
    fn model_versions_filter_escapes_single_quotes() {
        // Single quotes are doubled to keep the MLflow filter well-formed.
        assert_eq!(model_versions_filter("O'Brien"), "name='O''Brien'");
    }

    #[test]
    fn unwrap_model_version_flattens_envelope() {
        // The MLflow model-versions/get response nests the fields under
        // "model_version"; unwrap lifts them to the top level.
        let wrapped = serde_json::json!({
            "model_version": { "name": "M", "version": "3", "current_stage": "Production" }
        });
        let out = unwrap_model_version(wrapped);
        assert_eq!(out["version"], "3");
        assert_eq!(out["current_stage"], "Production");
        assert!(out.get("model_version").is_none());
    }

    #[test]
    fn unwrap_model_version_passes_through_flat_shape() {
        // Already-flat (or unexpected) shape is returned unchanged.
        let flat = serde_json::json!({ "version": "1", "status": "READY" });
        assert_eq!(unwrap_model_version(flat.clone()), flat);
    }
}
