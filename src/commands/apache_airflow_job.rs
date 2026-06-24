use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

const BASE: &str = "apacheAirflowJobs";

#[derive(Debug, Subcommand)]
pub enum ApacheAirflowJobCommand {
    /// List Apache Airflow jobs in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of an Apache Airflow job
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,
    },
    /// Create a new Apache Airflow job
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,
    },
    /// Update Apache Airflow job properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete an Apache Airflow job
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,

        /// Permanently delete (hard delete) instead of soft delete
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of an Apache Airflow job
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of an Apache Airflow job
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,

        /// Path to definition file
        #[arg(long)]
        file: Option<String>,

        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
    // ─── Environment Operations ──────────────────────────────────────────────
    /// Start the Airflow environment
    #[command(display_order = 10)]
    StartEnvironment {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,
    },
    /// Stop the Airflow environment
    #[command(display_order = 11)]
    StopEnvironment {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,
    },
    /// Get environment status
    #[command(display_order = 12)]
    GetEnvironment {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,
    },
    /// List installed libraries in the environment
    #[command(display_order = 13)]
    ListLibraries {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,
    },
    /// Deploy requirements.txt to the environment
    #[command(display_order = 14)]
    DeployRequirements {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,

        /// Path to requirements.txt file
        #[arg(long)]
        file: Option<String>,

        /// Inline requirements content
        #[arg(long)]
        content: Option<String>,
    },
    // ─── Settings Operations ─────────────────────────────────────────────────
    /// Get environment settings
    #[command(display_order = 20)]
    GetSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,
    },
    /// Update environment settings
    #[command(display_order = 21)]
    UpdateSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,

        /// Path to settings JSON file
        #[arg(long)]
        file: Option<String>,

        /// Inline settings JSON
        #[arg(long)]
        content: Option<String>,
    },
    // ─── Compute Operations ──────────────────────────────────────────────────
    /// Get environment compute information
    #[command(display_order = 25)]
    GetCompute {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,
    },
    /// Update the compute configuration for the Airflow job environment (pool template)
    #[command(display_order = 26)]
    UpdateCompute {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,

        /// Pool template ID to assign to this environment
        #[arg(long)]
        pool_template_id: String,
    },
    // ─── Files Operations ────────────────────────────────────────────────────
    /// List files (DAGs) in the Airflow job
    #[command(display_order = 30)]
    ListFiles {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,
    },
    /// Get (download) a file from the Airflow job
    #[command(display_order = 31)]
    GetFile {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,

        /// Remote file path
        #[arg(long)]
        path: String,
    },
    /// Upload a file to the Airflow job
    #[command(display_order = 32)]
    UploadFile {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,

        /// Remote file path (destination)
        #[arg(long)]
        path: String,

        /// Local file to upload
        #[arg(long)]
        file: String,
    },
    /// Delete a file from the Airflow job
    #[command(display_order = 33)]
    DeleteFile {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Apache Airflow job ID
        #[arg(long)]
        id: String,

        /// Remote file path
        #[arg(long)]
        path: String,
    },
    // ─── Workspace Settings ──────────────────────────────────────────────────
    /// Get workspace-level Airflow settings
    #[command(display_order = 40)]
    GetWorkspaceSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Update workspace-level Airflow settings
    #[command(display_order = 41)]
    UpdateWorkspaceSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Path to settings JSON file
        #[arg(long)]
        file: Option<String>,

        /// Inline settings JSON
        #[arg(long)]
        content: Option<String>,
    },
    // ─── Pool Templates ──────────────────────────────────────────────────────
    /// List pool templates
    #[command(display_order = 50)]
    ListPoolTemplates {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Create a pool template
    #[command(display_order = 51)]
    CreatePoolTemplate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Pool template name
        #[arg(long)]
        name: Option<String>,

        /// Path to pool template JSON file
        #[arg(long)]
        file: Option<String>,

        /// Inline pool template JSON
        #[arg(long)]
        content: Option<String>,
    },
    /// Get a pool template
    #[command(display_order = 52)]
    GetPoolTemplate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Pool template ID
        #[arg(long)]
        id: String,
    },
    /// Delete a pool template
    #[command(display_order = 53)]
    DeletePoolTemplate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Pool template ID
        #[arg(long)]
        id: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &ApacheAirflowJobCommand,
) -> Result<()> {
    match command {
        ApacheAirflowJobCommand::List { workspace } => list(cli, client, workspace).await,
        ApacheAirflowJobCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        ApacheAirflowJobCommand::Create {
            workspace,
            name,
            description,
        } => create(cli, client, workspace, name, description.as_deref()).await,
        ApacheAirflowJobCommand::Update {
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
        ApacheAirflowJobCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        ApacheAirflowJobCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        ApacheAirflowJobCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        // Environment
        ApacheAirflowJobCommand::StartEnvironment { workspace, id } => {
            start_environment(cli, client, workspace, id).await
        }
        ApacheAirflowJobCommand::StopEnvironment { workspace, id } => {
            stop_environment(cli, client, workspace, id).await
        }
        ApacheAirflowJobCommand::GetEnvironment { workspace, id } => {
            get_environment(cli, client, workspace, id).await
        }
        ApacheAirflowJobCommand::ListLibraries { workspace, id } => {
            list_libraries(cli, client, workspace, id).await
        }
        ApacheAirflowJobCommand::DeployRequirements {
            workspace,
            id,
            file,
            content,
        } => {
            deploy_requirements(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        // Settings
        ApacheAirflowJobCommand::GetSettings { workspace, id } => {
            get_settings(cli, client, workspace, id).await
        }
        ApacheAirflowJobCommand::UpdateSettings {
            workspace,
            id,
            file,
            content,
        } => {
            update_settings(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        // Compute
        ApacheAirflowJobCommand::GetCompute { workspace, id } => {
            get_compute(cli, client, workspace, id).await
        }
        ApacheAirflowJobCommand::UpdateCompute {
            workspace,
            id,
            pool_template_id,
        } => update_compute(cli, client, workspace, id, pool_template_id).await,
        // Files
        ApacheAirflowJobCommand::ListFiles { workspace, id } => {
            list_files(cli, client, workspace, id).await
        }
        ApacheAirflowJobCommand::GetFile {
            workspace,
            id,
            path,
        } => get_file(cli, client, workspace, id, path).await,
        ApacheAirflowJobCommand::UploadFile {
            workspace,
            id,
            path,
            file,
        } => upload_file(cli, client, workspace, id, path, file).await,
        ApacheAirflowJobCommand::DeleteFile {
            workspace,
            id,
            path,
        } => delete_file(cli, client, workspace, id, path).await,
        // Workspace Settings
        ApacheAirflowJobCommand::GetWorkspaceSettings { workspace } => {
            get_workspace_settings(cli, client, workspace).await
        }
        ApacheAirflowJobCommand::UpdateWorkspaceSettings {
            workspace,
            file,
            content,
        } => {
            update_workspace_settings(cli, client, workspace, file.as_deref(), content.as_deref())
                .await
        }
        // Pool Templates
        ApacheAirflowJobCommand::ListPoolTemplates { workspace } => {
            list_pool_templates(cli, client, workspace).await
        }
        ApacheAirflowJobCommand::CreatePoolTemplate {
            workspace,
            name,
            file,
            content,
        } => {
            create_pool_template(
                cli,
                client,
                workspace,
                name.as_deref(),
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        ApacheAirflowJobCommand::GetPoolTemplate { workspace, id } => {
            get_pool_template(cli, client, workspace, id).await
        }
        ApacheAirflowJobCommand::DeletePoolTemplate { workspace, id } => {
            delete_pool_template(cli, client, workspace, id).await
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/{BASE}"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
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
        .get(&format!("/workspaces/{workspace}/{BASE}/{id}"))
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
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }

    if output::dry_run_guard(
        cli,
        "apache-airflow-job create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/{BASE}"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job create", "Member"))?;
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
            "Example: fabio apache-airflow-job update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
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

    if output::dry_run_guard(cli, "apache-airflow-job update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/{BASE}/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
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
        "apache-airflow-job delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/{BASE}/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/{BASE}/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/{BASE}/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job get-definition", "Contributor"))?;
    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let script = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio apache-airflow-job update-definition --workspace <WS> --id <ID> --file dag.py".to_string(),
            ).into());
        }
    };

    let encoded = BASE64.encode(script.as_bytes());

    let body = serde_json::json!({
        "definition": {
            "parts": [
                {
                    "path": "dag.py",
                    "payload": encoded,
                    "payloadType": "InlineBase64"
                }
            ]
        }
    });

    if output::dry_run_guard(
        cli,
        "apache-airflow-job update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": script.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/{BASE}/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Environment Operations ──────────────────────────────────────────────────

async fn start_environment(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "apache-airflow-job start-environment",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/{BASE}/{id}/environment/start?beta=true"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job start-environment", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "environment_starting" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

async fn stop_environment(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "apache-airflow-job stop-environment",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/{BASE}/{id}/environment/stop?beta=true"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job stop-environment", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "environment_stopping" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

async fn get_environment(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/{BASE}/{id}/environment?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn list_libraries(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/{BASE}/{id}/environment/libraries?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "libraries");
    Ok(())
}

async fn deploy_requirements(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let requirements = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio apache-airflow-job deploy-requirements --workspace <WS> --id <ID> --file requirements.txt".to_string(),
            ).into());
        }
    };

    if output::dry_run_guard(
        cli,
        "apache-airflow-job deploy-requirements",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": requirements.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post_raw(
            &format!(
                "/workspaces/{workspace}/{BASE}/{id}/environment/deployRequirements?beta=true"
            ),
            &requirements,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "apache-airflow-job deploy-requirements", "Contributor")
        })?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "requirements_deploying" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Settings Operations ─────────────────────────────────────────────────────

async fn get_settings(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/{BASE}/{id}/environment/settings?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "settings");
    Ok(())
}

async fn update_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let payload = read_json_input(file, content, "apache-airflow-job update-settings")?;

    if output::dry_run_guard(cli, "apache-airflow-job update-settings", &payload) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/{BASE}/{id}/environment/updateSettings?beta=true"),
            &payload,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job update-settings", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "settings_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Compute Operations ──────────────────────────────────────────────────────

async fn get_compute(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/{BASE}/{id}/environment/compute?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "compute");
    Ok(())
}

async fn update_compute(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    pool_template_id: &str,
) -> Result<()> {
    let body = serde_json::json!({ "poolTemplateId": pool_template_id });

    if output::dry_run_guard(
        cli,
        "apache-airflow-job update-compute",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "poolTemplateId": pool_template_id
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/{BASE}/{id}/environment/updateCompute?beta=true"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job update-compute", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let result = serde_json::json!({ "id": id, "status": "compute_updated", "poolTemplateId": pool_template_id });
        output::render_object(cli, &result, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Files Operations ────────────────────────────────────────────────────────

async fn list_files(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/{BASE}/{id}/files?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "files");
    Ok(())
}

async fn get_file(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    path: &str,
) -> Result<()> {
    let content = client
        .get_text(&format!(
            "/workspaces/{workspace}/{BASE}/{id}/files/{path}?beta=true"
        ))
        .await?;
    let data = serde_json::json!({ "path": path, "content": content });
    output::render_object(cli, &data, "content");
    Ok(())
}

async fn upload_file(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    path: &str,
    file: &str,
) -> Result<()> {
    let file_content = std::fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("Failed to read file '{file}': {e}"))?;

    if output::dry_run_guard(
        cli,
        "apache-airflow-job upload-file",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "path": path,
            "contentLength": file_content.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .put_raw(
            &format!("/workspaces/{workspace}/{BASE}/{id}/files/{path}?beta=true"),
            &file_content,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job upload-file", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "path": path, "status": "uploaded" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

async fn delete_file(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    path: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "apache-airflow-job delete-file",
        &serde_json::json!({ "workspace": workspace, "id": id, "path": path }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/{BASE}/{id}/files/{path}?beta=true"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "apache-airflow-job delete-file", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "path": path, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Workspace Settings ──────────────────────────────────────────────────────

async fn get_workspace_settings(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/{BASE}/settings?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "settings");
    Ok(())
}

async fn update_workspace_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let payload = read_json_input(
        file,
        content,
        "apache-airflow-job update-workspace-settings",
    )?;

    if output::dry_run_guard(
        cli,
        "apache-airflow-job update-workspace-settings",
        &payload,
    ) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/{BASE}/settings?beta=true"),
            &payload,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "apache-airflow-job update-workspace-settings",
                "Contributor",
            )
        })?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "workspace": workspace, "status": "settings_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "settings");
    }
    Ok(())
}

// ─── Pool Templates ──────────────────────────────────────────────────────────

async fn list_pool_templates(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/{BASE}/poolTemplates?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "poolTemplates");
    Ok(())
}

async fn create_pool_template(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: Option<&str>,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let mut body = match (file, content) {
        (Some(path), _) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
            serde_json::from_str::<Value>(&raw)
                .map_err(|e| anyhow::anyhow!("Invalid JSON in '{path}': {e}"))?
        }
        (_, Some(c)) => serde_json::from_str::<Value>(c)
            .map_err(|e| anyhow::anyhow!("Invalid JSON content: {e}"))?,
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio apache-airflow-job create-pool-template --workspace <WS> --file pool.json".to_string(),
            ).into());
        }
    };

    if let Some(n) = name {
        body["name"] = Value::from(n);
    }

    if output::dry_run_guard(cli, "apache-airflow-job create-pool-template", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/{BASE}/poolTemplates?beta=true"),
            &body,
            false,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "apache-airflow-job create-pool-template", "Contributor")
        })?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn get_pool_template(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/{BASE}/poolTemplates/{id}?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete_pool_template(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "apache-airflow-job delete-pool-template",
        &serde_json::json!({ "workspace": workspace, "poolTemplateId": id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/{BASE}/poolTemplates/{id}?beta=true"
        ))
        .await
        .map_err(|e| {
            enrich_forbidden(e, "apache-airflow-job delete-pool-template", "Contributor")
        })?;

    let obj = serde_json::json!({ "poolTemplateId": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn read_json_input(file: Option<&str>, content: Option<&str>, cmd: &str) -> Result<Value> {
    match (file, content) {
        (Some(path), _) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
            serde_json::from_str::<Value>(&raw)
                .map_err(|e| anyhow::anyhow!("Invalid JSON in '{path}': {e}"))
        }
        (_, Some(c)) => serde_json::from_str::<Value>(c)
            .map_err(|e| anyhow::anyhow!("Invalid JSON content: {e}")),
        (None, None) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            format!("Example: fabio {cmd} --workspace <WS> --id <ID> --file settings.json"),
        )
        .into()),
    }
}
