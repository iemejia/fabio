use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before creating items, run: fabio context schema SparkJobDefinition\nReturns the definition template with required fields and format."
)]
pub enum SparkJobDefinitionCommand {
    /// List Spark job definitions in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a Spark job definition
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Spark Job Definition ID
        #[arg(long)]
        id: String,
    },
    /// Create a new Spark job definition
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Spark job definition display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update Spark job definition properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Spark Job Definition ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a Spark job definition
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Spark Job Definition ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a Spark job definition
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Spark Job Definition ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a Spark job definition
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Spark Job Definition ID
        #[arg(long)]
        id: String,

        /// Definition file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// Definition content (inline)
        #[arg(long)]
        content: Option<String>,
    },

    /// Run a Spark job definition
    #[command(display_order = 8)]
    Run {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Spark Job Definition ID
        #[arg(long)]
        id: String,

        /// Wait for the job to complete (polls until finished)
        #[arg(long)]
        wait: bool,

        /// Maximum time to wait in seconds
        #[arg(long, default_value = "600")]
        timeout: u64,

        /// Cancel the job if timeout is reached
        #[arg(long)]
        cancel_on_timeout: bool,
    },

    /// List Livy (Spark) sessions for a Spark job definition
    #[command(display_order = 9)]
    ListLivySessions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Spark Job Definition ID
        #[arg(long)]
        id: String,
    },

    /// Get details of a Livy (Spark) session for a Spark job definition
    #[command(display_order = 10)]
    GetLivySession {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Spark Job Definition ID
        #[arg(long)]
        id: String,

        /// Livy session ID
        #[arg(long)]
        livy_id: String,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &SparkJobDefinitionCommand,
) -> Result<()> {
    match command {
        SparkJobDefinitionCommand::List { workspace } => list(cli, client, workspace).await,
        SparkJobDefinitionCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        SparkJobDefinitionCommand::Create {
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
        SparkJobDefinitionCommand::Update {
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
        SparkJobDefinitionCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        SparkJobDefinitionCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        SparkJobDefinitionCommand::UpdateDefinition {
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
        SparkJobDefinitionCommand::Run {
            workspace,
            id,
            wait,
            timeout,
            cancel_on_timeout,
        } => {
            run(
                cli,
                client,
                workspace,
                id,
                *wait,
                *timeout,
                *cancel_on_timeout,
            )
            .await
        }
        SparkJobDefinitionCommand::ListLivySessions { workspace, id } => {
            list_livy_sessions(cli, client, workspace, id).await
        }
        SparkJobDefinitionCommand::GetLivySession {
            workspace,
            id,
            livy_id,
        } => get_livy_session(cli, client, workspace, id, livy_id).await,
    }
}

// ─── Livy sessions ───────────────────────────────────────────────────────────

async fn list_livy_sessions(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/sparkJobDefinitions/{id}/livySessions"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "spark-job-definition list-livy-sessions", "Viewer"))?;
    output::render_list_with_token(
        cli,
        &resp.items,
        &["livyId", "sparkApplicationId", "state", "jobType"],
        &["LIVY ID", "APP ID", "STATE", "JOB TYPE"],
        "livyId",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn get_livy_session(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    livy_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/sparkJobDefinitions/{id}/livySessions/{livy_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "spark-job-definition get-livy-session", "Viewer"))?;
    output::render_object(cli, &data, "livyId");
    Ok(())
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/sparkJobDefinitions"),
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

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/sparkJobDefinitions/{id}"))
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
        "spark-job-definition create",
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
            &format!("/workspaces/{workspace}/sparkJobDefinitions"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "spark-job-definition create", "Member"))?;
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
            "Example: fabio spark-job-definition update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
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

    if output::dry_run_guard(cli, "spark-job-definition update", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/sparkJobDefinitions/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "spark-job-definition update", "Contributor"))?;
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
        "spark-job-definition delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/sparkJobDefinitions/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/sparkJobDefinitions/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "spark-job-definition delete", "Member"))?;

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
            &format!("/workspaces/{workspace}/sparkJobDefinitions/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "spark-job-definition get-definition", "Contributor"))?;
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
    let definition_content = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                crate::definition_spec::definition_input_hint(
                    "SparkJobDefinition",
                    "spark-job-definition",
                    "update-definition",
                ),
            )
            .into());
        }
    };

    let body = crate::definition_spec::build_update_definition_body(
        &definition_content,
        "SparkJobDefinitionV1.json",
    );

    if output::dry_run_guard(
        cli,
        "spark-job-definition update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": definition_content.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/sparkJobDefinitions/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "spark-job-definition update-definition", "Contributor")
        })?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Run ─────────────────────────────────────────────────────────────────────

async fn run(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    wait: bool,
    timeout_secs: u64,
    cancel_on_timeout: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "spark-job-definition run",
        &serde_json::json!({ "workspace": workspace, "id": id, "wait": wait, "timeout": timeout_secs }),
    ) {
        return Ok(());
    }

    // The job-instance trigger returns 202 with an EMPTY body and the new
    // instance URL in the `Location` header, so the id must be read from that
    // header (via trigger_item_job), NOT from the response body.
    let job_id = client
        .trigger_item_job(workspace, id, "sparkjob", None)
        .await
        .map_err(|e| enrich_forbidden(e, "spark-job-definition run", "Contributor"))?;

    if !wait {
        let obj = serde_json::json!({ "itemId": id, "jobInstanceId": job_id, "status": "started" });
        output::render_object(cli, &obj, "status");
        return Ok(());
    }

    // Poll until completion
    let poll_interval = std::time::Duration::from_secs(5);
    let max_wait = std::time::Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > max_wait {
            if cancel_on_timeout && !job_id.is_empty() {
                let cancel_path =
                    format!("/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}/cancel");
                let _ = client
                    .post(&cancel_path, &serde_json::json!({}), false)
                    .await;
            }
            return Err(FabioError::with_hint(
                ErrorCode::Timeout,
                format!("Spark job timed out after {timeout_secs}s. Job ID: {job_id}"),
                format!("Increase --timeout (current: {timeout_secs}s) or use --cancel-on-timeout"),
            )
            .into());
        }

        tokio::time::sleep(poll_interval).await;

        let status_path = format!("/workspaces/{workspace}/items/{id}/jobs/instances/{job_id}");
        let status_resp = client.get(&status_path).await;

        if let Ok(ref status_data) = status_resp {
            let status = status_data
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            match status {
                "Completed" => {
                    output::render_object(cli, status_data, "status");
                    return Ok(());
                }
                "Failed" | "Cancelled" | "Deduped" => {
                    output::render_object(cli, status_data, "status");
                    return Err(FabioError::new(
                        ErrorCode::ApiError,
                        format!("Spark job {status}. Job ID: {job_id}"),
                    )
                    .into());
                }
                _ => {} // InProgress, NotStarted — keep polling
            }
        }
    }
}
