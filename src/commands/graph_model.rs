use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum GraphModelCommand {
    /// List graph models in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a graph model
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Graph model ID
        #[arg(long)]
        id: String,
    },
    /// Create a new graph model
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

        /// Ontology ID to link the graph model to
        #[arg(long)]
        ontology: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update graph model properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Graph model ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a graph model
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Graph model ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a graph model
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Graph model ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a graph model
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Graph model ID
        #[arg(long)]
        id: String,

        /// Path to definition file
        #[arg(long)]
        file: Option<String>,

        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
    /// Trigger a graph refresh job
    #[command(display_order = 10)]
    RefreshGraph {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Graph model ID
        #[arg(long)]
        id: String,

        /// Wait for the refresh to complete
        #[arg(long)]
        wait: bool,

        /// Timeout in seconds when using --wait (default: 600)
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    /// Execute a GQL query against the graph
    #[command(visible_alias = "query", display_order = 11)]
    ExecuteQuery {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Graph model ID
        #[arg(long)]
        id: String,

        /// GQL query text (ISO/IEC 39075). Use `@file.gql` to read from a file,
        /// or omit to pipe via stdin. Named `--gql` to avoid clashing with the
        /// global `--query` `JMESPath` projection flag.
        #[arg(long)]
        gql: Option<String>,
    },
    /// Get the queryable graph type
    #[command(display_order = 12)]
    GetQueryableGraphType {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Graph model ID
        #[arg(long)]
        id: String,
    },
    /// Initialize a graph model for querying (portal-only operation)
    #[command(display_order = 20)]
    Initialize {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Graph model ID
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &GraphModelCommand) -> Result<()> {
    match command {
        GraphModelCommand::List { workspace } => list(cli, client, workspace).await,
        GraphModelCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        GraphModelCommand::Create {
            workspace,
            name,
            description,
            ontology,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                ontology.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        GraphModelCommand::Update {
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
        GraphModelCommand::Delete { workspace, id, hard_delete } => delete(cli, client, workspace, id, *hard_delete).await,
        GraphModelCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        GraphModelCommand::UpdateDefinition {
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
        GraphModelCommand::RefreshGraph {
            workspace,
            id,
            wait,
            timeout,
        } => refresh_graph(cli, client, workspace, id, *wait, *timeout).await,
        GraphModelCommand::ExecuteQuery {
            workspace,
            id,
            gql,
        } => execute_query(cli, client, workspace, id, gql.as_deref()).await,
        GraphModelCommand::GetQueryableGraphType { workspace, id } => {
            get_queryable_graph_type(cli, client, workspace, id).await
        }
        GraphModelCommand::Initialize { .. } => {
            Err(crate::errors::FabioError::with_hint(
                crate::errors::ErrorCode::InvalidInput,
                "Graph model initialization is a portal-only operation.",
                "Open the graph model in the Fabric portal to initialize it. \
                 The REST API refresh fails with 'VersionConfig does not exist' \
                 until the portal provisions the internal loading infrastructure. \
                 After portal initialization, use: fabio graph-model refresh-graph --workspace <WS> --id <ID>",
            ).into())
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/graphModels"),
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
        .get(&format!("/workspaces/{workspace}/graphModels/{id}"))
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
    ontology: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }

    // If an ontology ID is provided, include it in the definition
    if let Some(ont_id) = ontology {
        let ont_json = serde_json::json!({ "ontologyId": ont_id });
        let encoded = BASE64.encode(ont_json.to_string().as_bytes());
        body["definition"] = serde_json::json!({
            "parts": [{
                "path": "GraphModel.json",
                "payload": encoded,
                "payloadType": "InlineBase64"
            }]
        });
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(
        cli,
        "graph-model create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "ontology": ontology,
            "sensitivityLabel": sensitivity_label
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/graphModels"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "graph-model create", "Member"))?;
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
            "Example: fabio graph-model update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "graph-model update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/graphModels/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "graph-model update", "Contributor"))?;
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
        "graph-model delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/graphModels/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/graphModels/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "graph-model delete", "Member"))?;

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
            &format!("/workspaces/{workspace}/graphModels/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "graph-model get-definition", "Contributor"))?;
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
    let definition_json = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio graph-model update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            ).into());
        }
    };

    let body =
        crate::definition_spec::build_update_definition_body(&definition_json, "GraphModel.json");

    if output::dry_run_guard(
        cli,
        "graph-model update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": definition_json.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/graphModels/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "graph-model update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Extra operations ────────────────────────────────────────────────────────

async fn refresh_graph(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    wait: bool,
    timeout_secs: u64,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "graph-model refresh-graph",
        &serde_json::json!({ "workspace": workspace, "id": id, "wait": wait, "timeout": timeout_secs }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/graphModels/{id}/jobs/instances?jobType=RefreshGraph"
            ),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "graph-model refresh-graph", "Contributor"))?;

    if !wait {
        if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
            let obj = serde_json::json!({ "id": id, "status": "refresh_triggered" });
            output::render_object(cli, &obj, "status");
        } else {
            output::render_object(cli, &data, "id");
        }
        return Ok(());
    }

    // Poll graph model status until refresh completes
    let poll_interval = Duration::from_secs(5);
    let max_wait = Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > max_wait {
            return Err(FabioError::new(
                ErrorCode::Timeout,
                format!(
                    "Graph refresh timed out after {timeout_secs}s. Use 'graph-model show' to check status."
                ),
            )
            .into());
        }

        sleep(poll_interval).await;

        let model_data = client
            .get(&format!("/workspaces/{workspace}/graphModels/{id}"))
            .await?;

        let status_str = model_data
            .pointer("/properties/lastDataLoadingStatus/status")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");

        match status_str {
            "Completed" => {
                let obj = serde_json::json!({
                    "id": id,
                    "status": "Completed",
                    "queryReadiness": model_data.pointer("/properties/queryReadiness").and_then(|v| v.as_str()).unwrap_or("Unknown")
                });
                output::render_object(cli, &obj, "status");
                return Ok(());
            }
            "Failed" => {
                return Err(FabioError::new(
                    ErrorCode::ApiError,
                    format!("Graph refresh failed for model {id}"),
                )
                .into());
            }
            _ => {} // Continue polling (NotStarted, InProgress)
        }
    }
}

async fn execute_query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    gql: Option<&str>,
) -> Result<()> {
    let query = crate::commands::query_input::resolve_query_input(
        gql,
        "GQL",
        "--gql",
        "Example: fabio graph-model execute-query --workspace <WS> --id <ID> --gql \"MATCH (n) RETURN n LIMIT 10\"",
    )?;
    let body = serde_json::json!({ "query": query });

    let data = client
        .post(
            &format!("/workspaces/{workspace}/graphModels/{id}/executeQuery?preview=true"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "graph-model execute-query", "Contributor"))?;

    // The GQL Query API returns HTTP 200 even for failed queries, encoding the
    // outcome in the `status` object. Surface application-level errors as a
    // non-zero exit instead of silently succeeding.
    if let Some(message) = gql_status_error(&data) {
        return Err(FabioError::new(ErrorCode::ApiError, message).into());
    }

    output::render_object(cli, &data, "data");
    Ok(())
}

/// Inspect a GQL `executeQuery` response for an application-level error.
///
/// The GQL Query API always responds with HTTP 200; success or failure is
/// carried in `status.code` (a GQL status code per ISO/IEC 39075). Codes whose
/// first two characters are `00`/`01`/`02`/`03` are success/warning/no-data/info;
/// anything else (e.g. `42000` syntax error) is an error. Returns `Some(message)`
/// describing the failure (including the `cause` chain when present), or `None`
/// when the query completed successfully.
fn gql_status_error(data: &Value) -> Option<String> {
    let status = data.get("status")?;
    let code = status.get("code").and_then(Value::as_str)?;
    let is_success = code
        .get(..2)
        .is_some_and(|c| matches!(c, "00" | "01" | "02" | "03"));
    if is_success {
        return None;
    }
    let desc = status
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("query failed");
    let mut message = format!("GQL error {code}: {desc}");
    if let Some(cause_desc) = status
        .get("cause")
        .and_then(|c| c.get("description"))
        .and_then(Value::as_str)
    {
        use std::fmt::Write as _;
        let _ = write!(message, " (cause: {cause_desc})");
    }
    Some(message)
}

async fn get_queryable_graph_type(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/graphModels/{id}/getQueryableGraphType?preview=true"
        ))
        .await?;
    output::render_object(cli, &data, "data");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::gql_status_error;
    use serde_json::json;

    #[test]
    fn success_status_yields_no_error() {
        // A completed query returns code 00000 — must NOT be treated as an error.
        let data = json!({
            "status": {"code": "00000", "description": "note: successful completion"},
            "result": {"kind": "TABLE", "columns": [], "data": [{"n.StoreName": "Berlin"}]}
        });
        assert_eq!(gql_status_error(&data), None);
    }

    #[test]
    fn warning_no_data_and_info_prefixes_are_success() {
        for code in ["01001", "02000", "03000"] {
            let data = json!({ "status": {"code": code, "description": "ok"} });
            assert_eq!(
                gql_status_error(&data),
                None,
                "code {code} should be success"
            );
        }
    }

    #[test]
    fn syntax_error_status_is_reported_with_cause() {
        // The exact shape returned live for an invalid GQL query (HTTP 200).
        let data = json!({
            "status": {
                "code": "42000",
                "description": "error: syntax error or access rule violation",
                "cause": {
                    "code": "22000",
                    "description": "error: data exception; Syntax error at line 1:1"
                }
            }
        });
        let msg = gql_status_error(&data).expect("error must be surfaced");
        assert!(
            msg.contains("42000"),
            "message must include the status code: {msg}"
        );
        assert!(
            msg.contains("syntax error"),
            "message must include the description: {msg}"
        );
        assert!(
            msg.contains("cause:"),
            "message must include the cause chain: {msg}"
        );
    }

    #[test]
    fn missing_or_malformed_status_is_not_a_false_success() {
        // No status object at all -> treat as success (nothing to report).
        assert_eq!(
            gql_status_error(&json!({"result": {"kind": "NOTHING"}})),
            None
        );
        // A status with a non-string / too-short code is treated as an error (fail safe).
        let short = json!({ "status": {"code": "4"} });
        assert!(gql_status_error(&short).is_some());
    }
}
