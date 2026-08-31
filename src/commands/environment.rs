use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before creating items, run: fabio context schema Environment\nReturns the definition template with required fields and format."
)]
pub enum EnvironmentCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List environments in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of an environment
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,
    },
    /// Create a new environment
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update environment properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete an environment
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Publish ──────────────────────────────────────────────────────────
    /// Publish staged changes to an environment
    #[command(display_order = 10)]
    Publish {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,
    },
    /// Cancel a pending publish operation
    #[command(display_order = 11)]
    CancelPublish {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,
    },
    /// Get the published Spark settings (compute/pool/driver/executor)
    #[command(display_order = 12)]
    GetSparkSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,
    },
    /// Get the staging (draft) Spark settings
    #[command(display_order = 13)]
    GetStagingSparkSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of an environment
    #[command(display_order = 20)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of an environment
    #[command(display_order = 21)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,

        /// Path to definition file
        #[arg(long)]
        file: Option<String>,

        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },

    // ── Published Libraries ──────────────────────────────────────────────
    /// List published libraries of an environment
    #[command(display_order = 30)]
    ListLibraries {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,
    },
    /// Export external libraries configuration (published)
    #[command(display_order = 31)]
    ExportLibraries {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,
    },

    // ── Staging Libraries ────────────────────────────────────────────────
    /// List staging libraries of an environment
    #[command(display_order = 40)]
    ListStagingLibraries {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,
    },
    /// Delete a staging library by name
    #[command(display_order = 41)]
    DeleteStagingLibrary {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,

        /// Library filename to delete
        #[arg(long)]
        library_name: String,
    },
    /// Export external libraries configuration (staging)
    #[command(display_order = 42)]
    ExportStagingLibraries {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,
    },
    /// Import external libraries configuration into staging
    #[command(display_order = 43)]
    ImportStagingLibraries {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,

        /// Path to the external-libraries file (e.g. an `environment.yml` listing public/feed libraries). Sent verbatim as octet-stream.
        #[arg(long)]
        file: Option<String>,

        /// Inline external-libraries file content (e.g. environment.yml text). Sent verbatim as octet-stream.
        #[arg(long)]
        content: Option<String>,
    },
    /// Remove an external library from staging
    #[command(display_order = 44)]
    RemoveStagingLibrary {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,

        /// Library name to remove
        #[arg(long)]
        library_name: String,

        /// Library version to remove (the API requires the exact version).
        #[arg(long)]
        library_version: String,
    },
    /// Upload a custom library file into staging
    #[command(display_order = 45)]
    UploadStagingLibrary {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,

        /// Path to the library file to upload (.jar, .whl, .tar.gz, etc.)
        #[arg(long)]
        file: String,

        /// Library name (defaults to filename)
        #[arg(long)]
        library_name: Option<String>,
    },

    // ── Staging Spark Compute ────────────────────────────────────────────
    /// Update staging Spark compute configuration
    #[command(display_order = 50)]
    UpdateStagingSparkCompute {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Environment ID
        #[arg(long)]
        id: String,

        /// Path to JSON file with spark compute config (full body; conflicts with --runtime-version/--spark-property)
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content with spark compute config (full body; conflicts with --runtime-version/--spark-property)
        #[arg(long)]
        content: Option<String>,

        /// Spark runtime version to set (e.g. "1.3" for Spark 3.5 [current default], "2.0" for
        /// Spark 4.1 / Delta 4.2 — GA but opt-in until it becomes the default ~late Sep 2026).
        /// Merged into the existing staging compute (other fields preserved).
        #[arg(long, conflicts_with_all = ["file", "content"])]
        runtime_version: Option<String>,

        /// Spark configuration property to set as KEY=VALUE (repeatable). Merged into existing sparkProperties. E.g. --spark-property spark.native.enabled=true
        #[arg(long = "spark-property", value_name = "KEY=VALUE", conflicts_with_all = ["file", "content"])]
        spark_property: Vec<String>,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &EnvironmentCommand) -> Result<()> {
    match command {
        EnvironmentCommand::List { workspace } => list(cli, client, workspace).await,
        EnvironmentCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        EnvironmentCommand::Create {
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
        EnvironmentCommand::Update {
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
        EnvironmentCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        EnvironmentCommand::Publish { workspace, id } => publish(cli, client, workspace, id).await,
        EnvironmentCommand::CancelPublish { workspace, id } => {
            cancel_publish(cli, client, workspace, id).await
        }
        EnvironmentCommand::GetSparkSettings { workspace, id } => {
            get_spark_settings(cli, client, workspace, id).await
        }
        EnvironmentCommand::GetStagingSparkSettings { workspace, id } => {
            get_staging_spark_settings(cli, client, workspace, id).await
        }
        EnvironmentCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        EnvironmentCommand::UpdateDefinition {
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
        EnvironmentCommand::ListLibraries { workspace, id } => {
            list_libraries(cli, client, workspace, id).await
        }
        EnvironmentCommand::ExportLibraries { workspace, id } => {
            export_libraries(cli, client, workspace, id).await
        }
        EnvironmentCommand::ListStagingLibraries { workspace, id } => {
            list_staging_libraries(cli, client, workspace, id).await
        }
        EnvironmentCommand::DeleteStagingLibrary {
            workspace,
            id,
            library_name,
        } => delete_staging_library(cli, client, workspace, id, library_name).await,
        EnvironmentCommand::ExportStagingLibraries { workspace, id } => {
            export_staging_libraries(cli, client, workspace, id).await
        }
        EnvironmentCommand::ImportStagingLibraries {
            workspace,
            id,
            file,
            content,
        } => {
            import_staging_libraries(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        EnvironmentCommand::RemoveStagingLibrary {
            workspace,
            id,
            library_name,
            library_version,
        } => {
            remove_staging_library(cli, client, workspace, id, library_name, library_version).await
        }
        EnvironmentCommand::UploadStagingLibrary {
            workspace,
            id,
            file,
            library_name,
        } => {
            upload_staging_library(cli, client, workspace, id, file, library_name.as_deref()).await
        }
        EnvironmentCommand::UpdateStagingSparkCompute {
            workspace,
            id,
            file,
            content,
            runtime_version,
            spark_property,
        } => {
            update_staging_spark_compute(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
                runtime_version.as_deref(),
                spark_property,
            )
            .await
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "environments",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "environments", workspace, id).await
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
        "environment create",
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
        .post(
            &format!("/workspaces/{workspace}/environments"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "environment create", "Member"))?;
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
            "Example: fabio environment update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "environment update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/environments/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "environment update", "Contributor"))?;
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
        "environment delete",
        &serde_json::json!({
            "workspace": workspace,
            "id": id, "hardDelete": hard_delete
        }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/environments/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/environments/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "environment delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Publish ─────────────────────────────────────────────────────────────────

async fn publish(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "environment publish",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/environments/{id}/staging/publish"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "environment publish", "Contributor"))?;

    let obj = serde_json::json!({
        "id": id,
        "status": "publish_started"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn cancel_publish(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "environment cancel-publish",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }
    client
        .post(
            &format!("/workspaces/{workspace}/environments/{id}/staging/cancelPublish"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "environment cancel-publish", "Contributor"))?;

    let obj = serde_json::json!({
        "id": id,
        "status": "publish_cancelled"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Spark Settings ──────────────────────────────────────────────────────────

async fn get_spark_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/environments/{id}/sparkcompute"
        ))
        .await?;
    output::render_object(cli, &data, "instancePool");
    Ok(())
}

async fn get_staging_spark_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/environments/{id}/staging/sparkcompute"
        ))
        .await?;
    output::render_object(cli, &data, "instancePool");
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
            &format!("/workspaces/{workspace}/environments/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "environment get-definition", "Contributor"))?;
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
                "Example: fabio environment update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            ).into());
        }
    };

    let body =
        crate::definition_spec::build_update_definition_body(&script, "environment.metadata.json");

    if output::dry_run_guard(
        cli,
        "environment update-definition",
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
            &format!("/workspaces/{workspace}/environments/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "environment update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Published Libraries ─────────────────────────────────────────────────────

async fn list_libraries(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/environments/{id}/libraries"
        ))
        .await?;
    output::render_object(cli, &data, "customLibraries");
    Ok(())
}

async fn export_libraries(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    // The endpoint returns the raw external-libraries file (e.g. environment.yml
    // YAML), NOT JSON — fetch it as text and wrap it in the JSON envelope.
    let text = client
        .get_text(&format!(
            "/workspaces/{workspace}/environments/{id}/libraries/exportExternalLibraries"
        ))
        .await?;
    let obj = serde_json::json!({ "externalLibraries": text });
    output::render_object(cli, &obj, "externalLibraries");
    Ok(())
}

// ─── Staging Libraries ───────────────────────────────────────────────────────

async fn list_staging_libraries(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/environments/{id}/staging/libraries"
        ))
        .await?;
    output::render_object(cli, &data, "customLibraries");
    Ok(())
}

async fn delete_staging_library(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    library_name: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "environment delete-staging-library",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "libraryName": library_name
        }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/environments/{id}/staging/libraries?libraryToDelete={library_name}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "environment delete-staging-library", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "library": library_name, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn export_staging_libraries(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    // The endpoint returns the raw external-libraries file (e.g. environment.yml
    // YAML), NOT JSON — fetch it as text and wrap it in the JSON envelope.
    let text = client
        .get_text(&format!(
            "/workspaces/{workspace}/environments/{id}/staging/libraries/exportExternalLibraries"
        ))
        .await?;
    let obj = serde_json::json!({ "externalLibraries": text });
    output::render_object(cli, &obj, "externalLibraries");
    Ok(())
}

async fn import_staging_libraries(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    // The importExternalLibraries endpoint expects the RAW external-libraries file
    // (e.g. an `environment.yml`) as `application/octet-stream` — NOT JSON. Read the
    // bytes verbatim and post them unchanged (no JSON parsing/re-encoding).
    let bytes: Vec<u8> = match (file, content) {
        (Some(path), _) => std::fs::read(path).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read file '{path}': {e}"),
                "Verify the file path is correct and the file is readable.",
            )
        })?,
        (_, Some(c)) => c.as_bytes().to_vec(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Provide an environment.yml (public libraries / Azure Artifact Feed) file. Example: fabio environment import-staging-libraries --workspace <WS> --id <ID> --file environment.yml".to_string(),
            ).into());
        }
    };

    if output::dry_run_guard(
        cli,
        "environment import-staging-libraries",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": bytes.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post_octet_stream(
            &format!(
                "/workspaces/{workspace}/environments/{id}/staging/libraries/importExternalLibraries"
            ),
            bytes,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "environment import-staging-libraries", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "libraries_imported" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

async fn remove_staging_library(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    library_name: &str,
    library_version: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "environment remove-staging-library",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "name": library_name,
            "version": library_version
        }),
    ) {
        return Ok(());
    }

    // The removeExternalLibrary API requires BOTH the library name and its exact
    // version (fields `name`/`version`); the old `{libraryToRemove}` body was
    // rejected with "Provide the name and version of external library".
    let body = serde_json::json!({ "name": library_name, "version": library_version });

    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/environments/{id}/staging/libraries/removeExternalLibrary"
            ),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "environment remove-staging-library", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "library": library_name, "status": "removed" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Staging Spark Compute ───────────────────────────────────────────────────

/// Parse a `KEY=VALUE` Spark property argument.
fn parse_spark_property(s: &str) -> Result<(String, String)> {
    let (key, value) = s.split_once('=').ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --spark-property '{s}': expected KEY=VALUE"),
            "Example: --spark-property spark.native.enabled=true",
        )
    })?;
    let key = key.trim();
    if key.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --spark-property '{s}': key is empty"),
            "Example: --spark-property spark.native.enabled=true",
        )
        .into());
    }
    Ok((key.to_string(), value.to_string()))
}

/// Apply typed runtime-version / spark-property overrides onto an existing
/// staging sparkcompute object, preserving all other fields (and existing
/// `sparkProperties` keys that are not overridden). Pure function for testing.
fn apply_spark_compute_overrides(
    mut current: Value,
    runtime_version: Option<&str>,
    spark_properties: &[(String, String)],
) -> Value {
    if !current.is_object() {
        current = Value::Object(serde_json::Map::new());
    }
    let obj = current.as_object_mut().expect("ensured object above");
    if let Some(rv) = runtime_version {
        obj.insert("runtimeVersion".to_string(), Value::from(rv));
    }
    if !spark_properties.is_empty() {
        let props = obj
            .entry("sparkProperties".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !props.is_object() {
            *props = Value::Object(serde_json::Map::new());
        }
        let pobj = props.as_object_mut().expect("ensured object above");
        for (k, v) in spark_properties {
            pobj.insert(k.clone(), Value::from(v.as_str()));
        }
    }
    current
}

#[allow(clippy::too_many_arguments)]
async fn update_staging_spark_compute(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    runtime_version: Option<&str>,
    spark_property: &[String],
) -> Result<()> {
    let path = format!("/workspaces/{workspace}/environments/{id}/staging/sparkcompute");

    // Two mutually-exclusive modes:
    //   (a) raw JSON body via --file/--content (full replace of the compute body)
    //   (b) typed overrides via --runtime-version/--spark-property (read-merge-write,
    //       preserving all other fields and existing sparkProperties)
    let raw_body: Option<(Value, usize)> = match (file, content) {
        (Some(path), _) => {
            let s = std::fs::read_to_string(path).map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Failed to read file '{path}': {e}"),
                    "Verify the file path is correct and the file is readable.",
                )
            })?;
            let v: Value = serde_json::from_str(&s).map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Invalid JSON: {e}"),
                    "Provide valid JSON content via --file or --content.",
                )
            })?;
            Some((v, s.len()))
        }
        (_, Some(c)) => {
            let v: Value = serde_json::from_str(c).map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Invalid JSON: {e}"),
                    "Provide valid JSON content via --file or --content.",
                )
            })?;
            Some((v, c.len()))
        }
        (None, None) => None,
    };

    let typed_props: Option<Vec<(String, String)>> =
        if raw_body.is_none() && (runtime_version.is_some() || !spark_property.is_empty()) {
            Some(
                spark_property
                    .iter()
                    .map(|s| parse_spark_property(s))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        } else {
            None
        };

    if raw_body.is_none() && typed_props.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Provide either --file/--content or --runtime-version/--spark-property".to_string(),
            "Example: fabio environment update-staging-spark-compute --workspace <WS> --id <ID> --runtime-version 2.0 --spark-property spark.native.enabled=true".to_string(),
        ).into());
    }

    // Build a dry-run preview from the inputs (no network call for the typed path).
    let preview = if let Some((_, len)) = &raw_body {
        serde_json::json!({ "workspace": workspace, "id": id, "contentLength": len })
    } else {
        let props: serde_json::Map<String, Value> = typed_props
            .as_ref()
            .map(|p| {
                p.iter()
                    .map(|(k, v)| (k.clone(), Value::from(v.as_str())))
                    .collect()
            })
            .unwrap_or_default();
        serde_json::json!({
            "workspace": workspace,
            "id": id,
            "runtimeVersion": runtime_version,
            "sparkProperties": Value::Object(props),
        })
    };

    if output::dry_run_guard(cli, "environment update-staging-spark-compute", &preview) {
        return Ok(());
    }

    let body = if let Some((v, _)) = raw_body {
        v
    } else {
        // read-merge-write: fetch current staging compute, apply overrides.
        let current = client.get(&path).await.map_err(|e| {
            enrich_forbidden(e, "environment update-staging-spark-compute", "Contributor")
        })?;
        apply_spark_compute_overrides(current, runtime_version, &typed_props.unwrap_or_default())
    };

    let data = client.patch(&path, &body).await.map_err(|e| {
        enrich_forbidden(e, "environment update-staging-spark-compute", "Contributor")
    })?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "spark_compute_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Upload Staging Library ─────────────────────────────────────────────────

async fn upload_staging_library(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: &str,
    library_name: Option<&str>,
) -> Result<()> {
    let path = std::path::Path::new(file);
    let lib_name =
        library_name.unwrap_or_else(|| path.file_name().and_then(|n| n.to_str()).unwrap_or(file));

    let file_data =
        std::fs::read(file).map_err(|e| anyhow::anyhow!("Failed to read file '{file}': {e}"))?;

    if output::dry_run_guard(
        cli,
        "environment upload-staging-library",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "libraryName": lib_name,
            "sizeBytes": file_data.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post_octet_stream(
            &format!("/workspaces/{workspace}/environments/{id}/staging/libraries/{lib_name}"),
            file_data,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "environment upload-staging-library", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({
            "id": id,
            "libraryName": lib_name,
            "status": "uploaded"
        });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_spark_property_splits_key_value() {
        let (k, v) = parse_spark_property("spark.native.enabled=true").unwrap();
        assert_eq!(k, "spark.native.enabled");
        assert_eq!(v, "true");
    }

    #[test]
    fn parse_spark_property_trims_key_and_preserves_value() {
        // Value may itself contain '=' (only the first '=' splits).
        let (k, v) = parse_spark_property(" spark.conf.key = a=b=c ").unwrap();
        assert_eq!(k, "spark.conf.key");
        assert_eq!(v, " a=b=c ");
    }

    #[test]
    fn parse_spark_property_rejects_missing_equals() {
        assert!(parse_spark_property("spark.native.enabled").is_err());
    }

    #[test]
    fn parse_spark_property_rejects_empty_key() {
        assert!(parse_spark_property("=true").is_err());
        assert!(parse_spark_property("   =true").is_err());
    }

    #[test]
    fn apply_overrides_sets_runtime_version_only() {
        let current = json!({
            "runtimeVersion": "1.3",
            "driverCores": 8,
            "sparkProperties": { "existing.key": "keep" }
        });
        let out = apply_spark_compute_overrides(current, Some("2.0"), &[]);
        assert_eq!(out["runtimeVersion"], json!("2.0"));
        // Other fields preserved.
        assert_eq!(out["driverCores"], json!(8));
        // sparkProperties untouched when none supplied.
        assert_eq!(out["sparkProperties"]["existing.key"], json!("keep"));
    }

    #[test]
    fn apply_overrides_merges_spark_properties_preserving_existing() {
        let current = json!({
            "runtimeVersion": "1.3",
            "sparkProperties": { "existing.key": "keep", "override.me": "old" }
        });
        let props = vec![
            ("spark.native.enabled".to_string(), "true".to_string()),
            ("override.me".to_string(), "new".to_string()),
        ];
        let out = apply_spark_compute_overrides(current, None, &props);
        // runtimeVersion unchanged when not supplied.
        assert_eq!(out["runtimeVersion"], json!("1.3"));
        // existing preserved, new added, overridden replaced.
        assert_eq!(out["sparkProperties"]["existing.key"], json!("keep"));
        assert_eq!(
            out["sparkProperties"]["spark.native.enabled"],
            json!("true")
        );
        assert_eq!(out["sparkProperties"]["override.me"], json!("new"));
    }

    #[test]
    fn apply_overrides_creates_spark_properties_when_absent() {
        let current = json!({ "runtimeVersion": "2.0" });
        let props = vec![("k".to_string(), "v".to_string())];
        let out = apply_spark_compute_overrides(current, None, &props);
        assert_eq!(out["sparkProperties"]["k"], json!("v"));
    }

    #[test]
    fn apply_overrides_recovers_from_non_object_input() {
        let out = apply_spark_compute_overrides(json!("not-an-object"), Some("2.0"), &[]);
        assert_eq!(out["runtimeVersion"], json!("2.0"));
    }
}
