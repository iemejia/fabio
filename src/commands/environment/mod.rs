use anyhow::Result;
use clap::{Subcommand, ValueEnum};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

mod spark_compute;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CustomLivePoolSupport {
    #[value(name = "Enabled")]
    Enabled,
    #[value(name = "Disabled")]
    Disabled,
}

impl CustomLivePoolSupport {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "Enabled",
            Self::Disabled => "Disabled",
        }
    }
}

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

        /// Path to JSON file with Spark compute config (full body; conflicts with typed compute flags)
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON content with Spark compute config (full body; conflicts with typed compute flags)
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

        /// Enable or disable custom live-pool support. Disabling retains hydration settings.
        #[arg(long, value_enum, conflicts_with_all = ["file", "content"])]
        custom_live_pool_support: Option<CustomLivePoolSupport>,

        /// Maximum clusters to pre-hydrate per activation cycle (minimum 1).
        #[arg(
            long,
            value_parser = clap::value_parser!(i32).range(1..),
            conflicts_with_all = ["file", "content", "clear_custom_live_pool_settings"]
        )]
        max_clusters_to_hydrate: Option<i32>,

        /// Hydrated-cluster idle timeout as an ISO 8601 duration (PT20M through PT24H).
        #[arg(
            long,
            value_parser = spark_compute::parse_cluster_idle_timeout,
            conflicts_with_all = ["file", "content", "clear_custom_live_pool_settings"]
        )]
        cluster_idle_timeout: Option<String>,

        /// Live-pool activation lifespan as an ISO 8601 duration (PT30M through PT24H).
        #[arg(
            long,
            value_parser = spark_compute::parse_custom_live_pool_lifespan,
            conflicts_with_all = ["file", "content", "clear_custom_live_pool_settings"]
        )]
        custom_live_pool_lifespan: Option<String>,

        /// Remove custom live-pool hydration settings. Live-pool support is unchanged unless explicitly set.
        #[arg(long, conflicts_with_all = ["file", "content"])]
        clear_custom_live_pool_settings: bool,
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
            custom_live_pool_support,
            max_clusters_to_hydrate,
            cluster_idle_timeout,
            custom_live_pool_lifespan,
            clear_custom_live_pool_settings,
        } => {
            spark_compute::update_staging_spark_compute(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
                runtime_version.as_deref(),
                spark_property,
                *custom_live_pool_support,
                *max_clusters_to_hydrate,
                cluster_idle_timeout.as_deref(),
                custom_live_pool_lifespan.as_deref(),
                *clear_custom_live_pool_settings,
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
    crate::commands::crud::create(
        cli,
        client,
        "environment",
        "environments",
        "Member",
        workspace,
        name,
        description,
        sensitivity_label,
    )
    .await
}

async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    crate::commands::crud::update(
        cli,
        client,
        "environment",
        "environments",
        "Contributor",
        workspace,
        id,
        name,
        description,
    )
    .await
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    crate::commands::crud::delete(
        cli,
        client,
        "environment",
        "environments",
        "Member",
        workspace,
        id,
        hard_delete,
    )
    .await
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
    crate::commands::crud::get_definition(
        cli,
        client,
        "environment",
        "environments",
        "Contributor",
        workspace,
        id,
        decode,
    )
    .await
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
