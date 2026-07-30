use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use tokio::time::sleep;

use crate::cli::Cli;
use crate::client::{self, FabricClient};
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// Polling interval for data agent query runs.
const QUERY_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// `OpenAI`-compatible API version used by the data agent published endpoint.
const OPENAI_API_VERSION: &str = "2024-05-01-preview";

/// Options controlling a single assistant query run.
///
/// Shared by `data-agent query` (single-turn or multi-turn) and
/// `data-agent evaluate` (batch), so both drive the identical Assistants flow.
pub(super) struct QueryOptions<'a> {
    /// Reuse an existing thread instead of creating one (enables multi-turn).
    pub thread_id: Option<&'a str>,
    /// Do not delete the thread after the run (so it can be reused).
    pub keep_thread: bool,
    /// Include run steps (SQL queries, tool calls) in the result.
    pub show_steps: bool,
    /// Directory to download answer-attached files into (`None` = skip).
    pub download_dir: Option<&'a Path>,
}

/// Validate the requested query stage.
///
/// Only a *published* data agent is reachable through the public Fabric API
/// (via its `publishedUrl`). The draft/staging ("sandbox") stage lives on the
/// internal workload host and has no public consumption endpoint, so a request
/// to query it must fail fast rather than silently querying production.
///
/// When an explicit `--published-url` is supplied the stage is irrelevant (the
/// caller pointed us at a concrete endpoint), so any value is accepted.
fn validate_query_stage(stage: &str, has_explicit_url: bool) -> Result<()> {
    if has_explicit_url {
        return Ok(());
    }
    match stage.trim().to_ascii_lowercase().as_str() {
        "production" | "published" | "prod" | "live" => Ok(()),
        "sandbox" | "staging" | "draft" => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Querying the '{stage}' (draft) stage is not supported via the public Fabric API"
            ),
            "Only a published data agent can be queried. Publish it first with: \
             fabio data-agent publish --workspace <WS> --id <ID>, then query the default \
             --stage production. To target a specific endpoint directly, pass --published-url.",
        )
        .into()),
        other => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --stage value '{other}'"),
            "Valid value: 'production' (the published agent). Draft/staging querying is not \
             available through the public API.",
        )
        .into()),
    }
}

/// Query a published data agent using the `OpenAI` Assistants protocol.
///
/// The data agent exposes an `OpenAI`-compatible endpoint at its published URL.
/// Flow: create assistant -> create thread -> post message -> create run -> poll -> read response.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    prompt: Option<&str>,
    published_url: Option<&str>,
    verbose: bool,
    stage: &str,
    thread_id: Option<&str>,
    keep_thread: bool,
    download_files: Option<&str>,
    timeout: u64,
) -> Result<()> {
    validate_query_stage(stage, published_url.is_some())?;

    // Resolve prompt text: --prompt flag or stdin
    let prompt_text = if let Some(p) = prompt {
        p.to_string()
    } else {
        let buf = io::read_to_string(io::stdin()).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read prompt from stdin: {e}"),
                "Use --prompt to provide the question directly, e.g.: fabio data-agent query --workspace <WS> --id <ID> --prompt \"What are the top 10 products?\"",
            )
        })?;
        if buf.trim().is_empty() {
            return Err(FabioError::invalid_input(
                "No prompt provided. Use --prompt or pipe text via stdin.",
            )
            .into());
        }
        buf
    };

    // Get the published URL: explicit flag, settings API, or constructed fallback.
    let resolved_url = if let Some(url) = published_url {
        client::validate_trusted_url(url, "--published-url")?;
        url.to_string()
    } else {
        let url = get_published_url(client, workspace, id).await?;
        // Validate API-returned URL to prevent token exfiltration via crafted settings
        client::validate_trusted_url(&url, "publishedUrl (from agent settings)")?;
        url
    };

    let download_dir = download_files.map(Path::new);

    // Use the OpenAI Assistants protocol against the published URL
    let token = client.require_auth().await?;
    let max_wait = Duration::from_secs(timeout);
    let opts = QueryOptions {
        thread_id,
        keep_thread,
        show_steps: verbose,
        download_dir,
    };
    let query_result =
        run_assistant_query(&resolved_url, &token, &prompt_text, &opts, max_wait).await?;

    let mut result = serde_json::json!({
        "question": prompt_text.trim(),
        "answer": query_result.answer,
        "threadId": query_result.thread_id,
    });
    if let Some(steps) = query_result.steps {
        result["steps"] = steps;
    }
    if download_dir.is_some() {
        result["files"] = Value::Array(query_result.files);
    }
    output::render_object(cli, &result, "answer");
    Ok(())
}

/// Build the Model Context Protocol (MCP) endpoint URL for a data agent.
///
/// This is the canonical runtime/consumption surface for a *published* data
/// agent: external MCP clients (Claude, Copilot Studio, Azure AI Foundry, custom
/// tools) connect to it to ask questions. Format (per the Fabric data agent SDK):
/// `{base}/mcp/workspaces/{workspace}/dataagents/{id}/agent`. Pure for testing.
fn build_mcp_url(base: &str, workspace: &str, id: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/mcp/workspaces/{workspace}/dataagents/{id}/agent")
}

/// Print the MCP endpoint URL used to consume a published data agent.
///
/// The URL is constructed deterministically; a best-effort published-state check
/// annotates whether the endpoint is live yet (it only works after publishing).
pub(super) async fn mcp_url(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let url = build_mcp_url(client::fabric_base_url(), workspace, id);
    let published = is_published(client, workspace, id).await;

    let mut result = serde_json::json!({
        "id": id,
        "mcpUrl": url,
        "published": published,
    });
    if !published {
        result["hint"] = Value::from(format!(
            "The MCP endpoint only works after the agent is published. Publish it with: fabio data-agent publish --workspace {workspace} --id {id}"
        ));
    }
    output::render_object(cli, &result, "mcpUrl");
    Ok(())
}

/// Best-effort check of whether a data agent is published.
///
/// The published-stage settings endpoint (`GET /dataAgents/{id}/settings`)
/// returns `200` for a published agent and `404 DataAgentNotPublished` for a
/// draft one, so a successful GET is a reliable "published" signal.
async fn is_published(client: &FabricClient, workspace: &str, id: &str) -> bool {
    client
        .get(&format!("/workspaces/{workspace}/dataAgents/{id}/settings"))
        .await
        .is_ok()
}

/// Build the canonical `OpenAI`-compatible consumption URL for a published agent.
///
/// The public `GET /dataAgents/{id}/settings` endpoint does not return a
/// `publishedUrl` field, so fabio constructs the well-known pattern
/// `{base}/workspaces/{ws}/dataagents/{id}/aiassistant/openai` (note the
/// lowercase `dataagents`), which serves the Assistants API for a *published*
/// agent. Verified live. Pure for unit testing.
fn build_published_url(base: &str, workspace: &str, id: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/workspaces/{workspace}/dataagents/{id}/aiassistant/openai")
}

/// Get the published URL of a data agent.
///
/// Strategy:
/// 1. Use the official published settings endpoint: `GET /dataAgents/{id}/settings`
/// 2. Fallback: check item properties for a `publishedUrl` field.
/// 3. Construct the standard URL pattern as last resort.
async fn get_published_url(client: &FabricClient, workspace: &str, id: &str) -> Result<String> {
    // The published settings endpoint is now part of the official Fabric REST API
    let settings_path = format!("/workspaces/{workspace}/dataAgents/{id}/settings");
    if let Ok(settings) = client.get(&settings_path).await
        && let Some(url) = settings
            .get("publishedUrl")
            .and_then(Value::as_str)
            .filter(|u| !u.is_empty())
    {
        return Ok(url.to_string());
    }

    // Fallback: Check item properties
    let data = client
        .get(&format!("/workspaces/{workspace}/dataAgents/{id}"))
        .await?;

    if let Some(url) = data
        .get("properties")
        .and_then(|p| p.get("publishedUrl"))
        .and_then(Value::as_str)
        .filter(|u| !u.is_empty())
    {
        return Ok(url.to_string());
    }

    // Last resort: construct the canonical consumption URL. The Fabric REST API
    // does not surface a `publishedUrl`, but the well-known
    // `.../dataagents/{id}/aiassistant/openai` endpoint serves the Assistants API
    // for a published agent (verified live). If the agent is not actually
    // published, the first Assistants call returns 404 with a publish hint via
    // `enrich_query_error`.
    Ok(build_published_url(
        client::fabric_base_url(),
        workspace,
        id,
    ))
}

/// Validate the query stage, then resolve the agent's published URL.
///
/// Shared by `data-agent evaluate` (and available to any consumer that must
/// reach the published endpoint without an explicit `--published-url`). The
/// returned URL is validated as a trusted Fabric host to prevent token
/// exfiltration via a crafted `publishedUrl` in the agent settings.
pub(super) async fn resolve_published_url(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    stage: &str,
) -> Result<String> {
    validate_query_stage(stage, false)?;
    let url = get_published_url(client, workspace, id).await?;
    client::validate_trusted_url(&url, "publishedUrl (from agent settings)")?;
    Ok(url)
}

/// Result of a data agent query, including the answer and optional execution steps.
pub(super) struct QueryResult {
    pub answer: String,
    /// The thread used for the run (reusable for multi-turn follow-ups).
    pub thread_id: String,
    pub steps: Option<Value>,
    /// Metadata for any files downloaded from the answer (empty unless requested).
    pub files: Vec<Value>,
}

/// Run a query against the data agent using the `OpenAI` Assistants API protocol.
pub(super) async fn run_assistant_query(
    base_url: &str,
    token: &str,
    question: &str,
    opts: &QueryOptions<'_>,
    max_wait: Duration,
) -> Result<QueryResult> {
    let http = crate::client::http_client_builder()
        .timeout(Duration::from_mins(6))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| FabioError::with_hint(ErrorCode::NetworkError, e.to_string(), "Verify the data agent is published. Check status: fabio data-agent show --workspace <WS> --id <ID>. Publish if needed: fabio data-agent publish --workspace <WS> --id <ID>"))?;

    let auth_header = token;

    // Step 1: Create assistant. Reuse the caller's thread if given (multi-turn),
    // otherwise create a fresh one.
    let assistant_id = create_assistant(&http, base_url, auth_header).await?;
    let (thread_id, created_thread) = match opts.thread_id {
        Some(t) => (t.to_string(), false),
        None => (create_thread(&http, base_url, auth_header).await?, true),
    };

    // Step 2: Post message and run
    post_message(&http, base_url, auth_header, &thread_id, question).await?;
    let run_id = create_run(&http, base_url, auth_header, &thread_id, &assistant_id).await?;

    // Step 3: Poll until complete
    poll_run_completion(&http, base_url, auth_header, &thread_id, &run_id, max_wait).await?;

    // Step 4 (optional): Retrieve run steps for verbose mode
    let steps = if opts.show_steps {
        Some(retrieve_run_steps(&http, base_url, auth_header, &thread_id, &run_id).await?)
    } else {
        None
    };

    // Step 5: Get the assistant messages, then extract the answer text.
    let messages = fetch_messages(&http, base_url, auth_header, &thread_id).await?;
    let response_text = extract_answer(&messages);

    // Step 5b (optional): Download any files the answer attached.
    let files = if let Some(dir) = opts.download_dir {
        download_answer_files(&http, base_url, auth_header, &messages, dir).await?
    } else {
        Vec::new()
    };

    // Step 6: Clean up the thread (best effort) — only if we created it and the
    // caller did not ask to keep it for a follow-up turn.
    if created_thread && !opts.keep_thread {
        let _ = http
            .delete(format!(
                "{base_url}/threads/{thread_id}?api-version={OPENAI_API_VERSION}"
            ))
            .header("Authorization", auth_header)
            .send()
            .await;
    }

    Ok(QueryResult {
        answer: response_text,
        thread_id,
        steps,
        files,
    })
}

/// Create an assistant on the data agent endpoint.
async fn create_assistant(
    http: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
) -> Result<String> {
    let resp = http
        .post(format!(
            "{base_url}/assistants?api-version=2024-05-01-preview"
        ))
        .header("Authorization", auth_header)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({"model": "not used"}))
        .send()
        .await
        .map_err(|e| FabioError::with_hint(ErrorCode::NetworkError, format!("Create assistant: {e}"), "Verify the data agent is published. Check status: fabio data-agent show --workspace <WS> --id <ID>"))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let retry_after = extract_retry_after(&resp);
        let text = resp.text().await.unwrap_or_default();
        return Err(enrich_query_error(
            status,
            &format!("Failed to create assistant: {text}"),
            base_url,
            retry_after.as_deref(),
        )
        .into());
    }
    let body: Value = resp.json().await.map_err(|e| {
        FabioError::with_hint(
            ErrorCode::ApiError,
            format!("Parse assistant response: {e}"),
            "Unexpected response format. This may indicate an API version mismatch.",
        )
    })?;
    Ok(body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

/// Create a thread on the data agent endpoint.
async fn create_thread(
    http: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
) -> Result<String> {
    let resp = http
        .post(format!("{base_url}/threads?api-version=2024-05-01-preview"))
        .header("Authorization", auth_header)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|e| FabioError::with_hint(ErrorCode::NetworkError, format!("Create thread: {e}"), "Verify the data agent is published. Check status: fabio data-agent show --workspace <WS> --id <ID>"))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let retry_after = extract_retry_after(&resp);
        let text = resp.text().await.unwrap_or_default();
        return Err(enrich_query_error(
            status,
            &format!("Failed to create thread: {text}"),
            base_url,
            retry_after.as_deref(),
        )
        .into());
    }
    let body: Value = resp.json().await.map_err(|e| {
        FabioError::with_hint(
            ErrorCode::ApiError,
            format!("Parse thread response: {e}"),
            "Unexpected response format. This may indicate an API version mismatch.",
        )
    })?;
    Ok(body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

/// Post a user message to an existing thread.
async fn post_message(
    http: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
    thread_id: &str,
    question: &str,
) -> Result<()> {
    let resp = http
        .post(format!(
            "{base_url}/threads/{thread_id}/messages?api-version=2024-05-01-preview"
        ))
        .header("Authorization", auth_header)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "role": "user",
            "content": question
        }))
        .send()
        .await
        .map_err(|e| FabioError::with_hint(ErrorCode::NetworkError, format!("Post message: {e}"), "Verify the data agent is published. Check status: fabio data-agent show --workspace <WS> --id <ID>"))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let retry_after = extract_retry_after(&resp);
        let text = resp.text().await.unwrap_or_default();
        return Err(enrich_query_error(
            status,
            &format!("Failed to post message: {text}"),
            base_url,
            retry_after.as_deref(),
        )
        .into());
    }
    Ok(())
}

/// Create a run on the thread and return the run ID.
async fn create_run(
    http: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
    thread_id: &str,
    assistant_id: &str,
) -> Result<String> {
    let resp = http
        .post(format!(
            "{base_url}/threads/{thread_id}/runs?api-version=2024-05-01-preview"
        ))
        .header("Authorization", auth_header)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "assistant_id": assistant_id
        }))
        .send()
        .await
        .map_err(|e| FabioError::with_hint(ErrorCode::NetworkError, format!("Create run: {e}"), "Verify the data agent is published. Check status: fabio data-agent show --workspace <WS> --id <ID>"))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let retry_after = extract_retry_after(&resp);
        let text = resp.text().await.unwrap_or_default();
        return Err(enrich_query_error(
            status,
            &format!("Failed to create run: {text}"),
            base_url,
            retry_after.as_deref(),
        )
        .into());
    }
    let body: Value = resp.json().await.map_err(|e| {
        FabioError::with_hint(
            ErrorCode::ApiError,
            format!("Parse run response: {e}"),
            "Unexpected response format. This may indicate an API version mismatch.",
        )
    })?;
    Ok(body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

/// Poll until the run reaches a terminal state.
async fn poll_run_completion(
    http: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
    thread_id: &str,
    run_id: &str,
    max_wait: Duration,
) -> Result<()> {
    let start = std::time::Instant::now();
    let terminal_states = ["completed", "failed", "cancelled", "requires_action"];

    loop {
        if start.elapsed() > max_wait {
            return Err(FabioError::with_hint(
                ErrorCode::Timeout,
                "Data agent query timed out waiting for response",
                "The query exceeded the maximum wait time. Possible causes: \
                 (1) Spark cold start on small capacities can take 2-5 minutes. \
                 (2) Complex queries over large datasets take longer. \
                 (3) The Fabric capacity may be overloaded. \
                 Retry the query, or check capacity status in the Azure portal.",
            )
            .into());
        }

        sleep(QUERY_POLL_INTERVAL).await;

        let poll_resp = http
            .get(format!(
                "{base_url}/threads/{thread_id}/runs/{run_id}?api-version=2024-05-01-preview"
            ))
            .header("Authorization", auth_header)
            .send()
            .await
            .map_err(|e| FabioError::with_hint(ErrorCode::NetworkError, format!("Poll run: {e}"), "Verify the data agent is published. Check status: fabio data-agent show --workspace <WS> --id <ID>"))?;

        if !poll_resp.status().is_success() {
            let status = poll_resp.status().as_u16();
            let retry_after = extract_retry_after(&poll_resp);
            let text = poll_resp.text().await.unwrap_or_default();
            return Err(enrich_query_error(
                status,
                &format!("Failed to poll run status: {text}"),
                base_url,
                retry_after.as_deref(),
            )
            .into());
        }

        let run_state: Value = poll_resp.json().await.map_err(|e| {
            FabioError::with_hint(
                ErrorCode::ApiError,
                format!("Parse run poll response: {e}"),
                "Unexpected response format. This may indicate an API version mismatch.",
            )
        })?;
        let status = run_state
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("");

        if terminal_states.contains(&status) {
            if status != "completed" {
                let err_msg = run_state
                    .get("last_error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Data agent run did not complete successfully");
                let hint = match status {
                    "failed" => {
                        "The data agent run failed. Check: \
                        (1) Is the Fabric capacity active? \
                        (2) Does the agent have access to its configured data sources? \
                        (3) Are the lakehouse tables loaded and accessible? \
                        Inspect the agent definition: fabio data-agent get-definition -w <workspace> --id <id>"
                    }
                    "cancelled" => {
                        "The run was cancelled. This may happen if the capacity \
                        is under pressure or the query was interrupted. Retry the query."
                    }
                    "requires_action" => {
                        "The run requires additional action (tool approval). \
                        This is unexpected for data agent queries — check the agent configuration."
                    }
                    _ => "The run ended in an unexpected state. Retry the query.",
                };
                return Err(FabioError::with_hint(
                    ErrorCode::ApiError,
                    format!("Run status '{status}': {err_msg}"),
                    hint,
                )
                .into());
            }
            return Ok(());
        }
    }
}

/// Fetch the thread messages (ascending order) from the data agent endpoint.
async fn fetch_messages(
    http: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
    thread_id: &str,
) -> Result<Value> {
    let resp = http
        .get(format!(
            "{base_url}/threads/{thread_id}/messages?api-version={OPENAI_API_VERSION}&order=asc"
        ))
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| FabioError::with_hint(ErrorCode::NetworkError, format!("Retrieve messages: {e}"), "Verify the data agent is published. Check status: fabio data-agent show --workspace <WS> --id <ID>"))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let retry_after = extract_retry_after(&resp);
        let text = resp.text().await.unwrap_or_default();
        return Err(enrich_query_error(
            status,
            &format!("Failed to retrieve messages: {text}"),
            base_url,
            retry_after.as_deref(),
        )
        .into());
    }

    resp.json().await.map_err(|e| {
        FabioError::with_hint(
            ErrorCode::ApiError,
            format!("Parse messages response: {e}"),
            "Unexpected response format. This may indicate an API version mismatch.",
        )
        .into()
    })
}

/// Extract the assistant's answer text from a thread messages payload.
///
/// Picks the most recent `role == "assistant"` message and returns its first
/// text content value. Pure so it can be unit-tested without a live endpoint.
fn extract_answer(messages: &Value) -> String {
    messages
        .get("data")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .rev()
                .find(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
        })
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .and_then(|content| {
            content.iter().find_map(|c| {
                c.get("text")
                    .and_then(|t| t.get("value"))
                    .and_then(Value::as_str)
            })
        })
        .unwrap_or("(No response from data agent)")
        .to_string()
}

/// Extract file IDs referenced by the assistant's answer.
///
/// Fabric data agents can attach generated files (CSVs, images) to an answer.
/// The `OpenAI` Assistants message shape exposes them in three places, all of
/// which are scanned across every assistant message:
/// - text-content annotations of type `file_path` / `file_citation`
/// - image-content items of type `image_file`
/// - message-level `attachments`
///
/// IDs are returned de-duplicated in first-seen order. Pure for unit testing.
fn extract_file_ids(messages: &Value) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut push = |id: &str| {
        if !id.is_empty() && !ids.iter().any(|existing| existing == id) {
            ids.push(id.to_string());
        }
    };

    let Some(data) = messages.get("data").and_then(Value::as_array) else {
        return ids;
    };
    for msg in data {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        // Content: text annotations + image_file items.
        if let Some(content) = msg.get("content").and_then(Value::as_array) {
            for item in content {
                if let Some(fid) = item
                    .get("image_file")
                    .and_then(|f| f.get("file_id"))
                    .and_then(Value::as_str)
                {
                    push(fid);
                }
                if let Some(annotations) = item
                    .get("text")
                    .and_then(|t| t.get("annotations"))
                    .and_then(Value::as_array)
                {
                    for ann in annotations {
                        for key in ["file_path", "file_citation"] {
                            if let Some(fid) = ann
                                .get(key)
                                .and_then(|f| f.get("file_id"))
                                .and_then(Value::as_str)
                            {
                                push(fid);
                            }
                        }
                    }
                }
            }
        }
        // Message-level attachments.
        if let Some(attachments) = msg.get("attachments").and_then(Value::as_array) {
            for att in attachments {
                if let Some(fid) = att.get("file_id").and_then(Value::as_str) {
                    push(fid);
                }
            }
        }
    }
    ids
}

/// Sanitize a server-provided filename to a safe basename for local writing.
///
/// Strips any directory components (defeating path traversal / absolute paths on
/// both Unix and Windows) and falls back to `fallback` when nothing usable
/// remains. Pure for unit testing.
fn sanitize_filename(name: &str, fallback: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('.');
    if base.is_empty() || base == "." || base == ".." {
        fallback.to_string()
    } else {
        base.to_string()
    }
}

/// Download the files attached to an answer into `dir`.
///
/// Best-effort per file: a failure to fetch one file is recorded in that file's
/// entry (as an `error` field) rather than aborting the whole query, so the
/// textual answer is always returned. Returns one metadata object per file.
async fn download_answer_files(
    http: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
    messages: &Value,
    dir: &Path,
) -> Result<Vec<Value>> {
    let file_ids = extract_file_ids(messages);
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }

    tokio::fs::create_dir_all(dir).await.map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Failed to create download directory '{}': {e}",
                dir.display()
            ),
            "Provide a writable path with --download-files, or create the directory first.",
        )
    })?;

    let mut out = Vec::with_capacity(file_ids.len());
    for fid in file_ids {
        out.push(download_one_file(http, base_url, auth_header, &fid, dir).await);
    }
    Ok(out)
}

/// Download a single file by ID; returns a metadata object (never errors).
async fn download_one_file(
    http: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
    file_id: &str,
    dir: &Path,
) -> Value {
    // Resolve a friendly filename from the file metadata (best effort).
    let filename = match http
        .get(format!(
            "{base_url}/files/{file_id}?api-version={OPENAI_API_VERSION}"
        ))
        .header("Authorization", auth_header)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r
            .json::<Value>()
            .await
            .ok()
            .and_then(|m| m.get("filename").and_then(Value::as_str).map(String::from))
            .map_or_else(|| file_id.to_string(), |f| sanitize_filename(&f, file_id)),
        _ => file_id.to_string(),
    };

    let content_resp = http
        .get(format!(
            "{base_url}/files/{file_id}/content?api-version={OPENAI_API_VERSION}"
        ))
        .header("Authorization", auth_header)
        .send()
        .await;

    let bytes = match content_resp {
        Ok(r) if r.status().is_success() => match r.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return serde_json::json!({"fileId": file_id, "error": format!("read body: {e}")});
            }
        },
        Ok(r) => {
            let status = r.status().as_u16();
            return serde_json::json!({"fileId": file_id, "error": format!("download failed with HTTP {status}")});
        }
        Err(e) => {
            return serde_json::json!({"fileId": file_id, "error": format!("request failed: {e}")});
        }
    };

    let path: PathBuf = dir.join(&filename);
    if let Err(e) = tokio::fs::write(&path, &bytes).await {
        return serde_json::json!({"fileId": file_id, "error": format!("write failed: {e}")});
    }
    serde_json::json!({
        "fileId": file_id,
        "filename": filename,
        "path": path.to_string_lossy(),
        "bytes": bytes.len(),
    })
}

/// Retrieve the run steps to show execution details (SQL queries, tool calls, etc.).
///
/// The `OpenAI` Assistants API exposes run steps at:
/// `GET /threads/{thread_id}/runs/{run_id}/steps`
///
/// Each step has a `step_details` field that may contain:
/// - `type: "tool_calls"` with tool call details (SQL queries, function calls)
/// - `type: "message_creation"` for the final response generation
async fn retrieve_run_steps(
    http: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
    thread_id: &str,
    run_id: &str,
) -> Result<Value> {
    let resp = http
        .get(format!(
            "{base_url}/threads/{thread_id}/runs/{run_id}/steps?api-version=2024-05-01-preview"
        ))
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| {
            FabioError::with_hint(ErrorCode::NetworkError, format!("Retrieve run steps: {e}"), "Verify the data agent is published. Check status: fabio data-agent show --workspace <WS> --id <ID>")
        })?;

    if !resp.status().is_success() {
        // Non-fatal: if steps endpoint is not available, return empty array
        return Ok(serde_json::json!([]));
    }

    let body: Value = resp.json().await.map_err(|e| {
        FabioError::with_hint(
            ErrorCode::ApiError,
            format!("Parse run steps response: {e}"),
            "Unexpected response format. This may indicate an API version mismatch.",
        )
    })?;

    // Extract meaningful step details
    let steps = body
        .get("data")
        .and_then(Value::as_array)
        .map(|steps_arr| {
            steps_arr
                .iter()
                .filter_map(|step| {
                    let step_type = step.get("type").and_then(Value::as_str)?;
                    let step_details = step.get("step_details")?;
                    let status = step
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");

                    match step_type {
                        "tool_calls" => {
                            let tool_calls = extract_tool_calls(step_details);
                            if tool_calls.is_empty() {
                                None
                            } else {
                                Some(serde_json::json!({
                                    "type": "tool_calls",
                                    "status": status,
                                    "tool_calls": tool_calls
                                }))
                            }
                        }
                        "message_creation" => Some(serde_json::json!({
                            "type": "message_creation",
                            "status": status,
                        })),
                        _ => Some(serde_json::json!({
                            "type": step_type,
                            "status": status,
                            "details": step_details
                        })),
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(Value::Array(steps))
}

/// Extract tool call details from a step's `step_details` field.
/// Returns a vec of structured objects with type, name, input, and output.
fn extract_tool_calls(step_details: &Value) -> Vec<Value> {
    let Some(tool_calls) = step_details.get("tool_calls").and_then(Value::as_array) else {
        return vec![];
    };

    tool_calls
        .iter()
        .map(|tc| {
            let tc_type = tc.get("type").and_then(Value::as_str).unwrap_or("unknown");
            match tc_type {
                "code_interpreter" => {
                    let input = tc
                        .get("code_interpreter")
                        .and_then(|ci| ci.get("input"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let outputs = tc
                        .get("code_interpreter")
                        .and_then(|ci| ci.get("outputs"))
                        .cloned()
                        .unwrap_or(Value::Array(vec![]));
                    serde_json::json!({
                        "type": "code_interpreter",
                        "input": input,
                        "outputs": outputs
                    })
                }
                "function" => {
                    let name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let arguments = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let output = tc
                        .get("function")
                        .and_then(|f| f.get("output"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    serde_json::json!({
                        "type": "function",
                        "name": name,
                        "arguments": arguments,
                        "output": output
                    })
                }
                // Fabric data agents may use custom tool types (e.g., SQL execution)
                _ => {
                    serde_json::json!({
                        "type": tc_type,
                        "details": tc
                    })
                }
            }
        })
        .collect()
}

// ─── Error Enrichment ────────────────────────────────────────────────────────

/// Extract the `Retry-After` header value from an HTTP response (seconds or date).
fn extract_retry_after(resp: &reqwest::Response) -> Option<String> {
    resp.headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

/// Enrich data agent query errors with actionable hints for common failures.
///
/// Intercepts HTTP status codes and known error patterns from the `OpenAI`
/// Assistants-compatible endpoint to guide agents toward self-correction.
fn enrich_query_error(
    status: u16,
    message: &str,
    base_url: &str,
    retry_after: Option<&str>,
) -> FabioError {
    let msg_lower = message.to_lowercase();

    // 404: The published URL is wrong or agent isn't published
    if status == 404 {
        return FabioError::with_hint(
            ErrorCode::NotFound,
            message.to_string(),
            format!(
                "The data agent endpoint returned 404. Possible causes: \
                 (1) The agent has not been published from the Fabric portal. \
                 (2) The --published-url is incorrect. \
                 Expected URL pattern: https://api.fabric.microsoft.com/v1/workspaces/{{workspace}}/dataagents/{{agentId}}/aiassistant/openai \
                 Current URL: {base_url}"
            ),
        );
    }

    // 401/403: Token or permission issue
    if status == 401 || status == 403 {
        return FabioError::with_hint(
            if status == 401 {
                ErrorCode::AuthRequired
            } else {
                ErrorCode::Forbidden
            },
            message.to_string(),
            "Authentication failed for the data agent endpoint. Ensure: \
             (1) You have at least Viewer role on the workspace. \
             (2) Your token is valid (re-run 'fabio auth login'). \
             (3) The data agent has been published and you have access to it."
                .to_string(),
        );
    }

    // 429: Rate limited — include Retry-After value
    if status == 429 {
        let hint = retry_after.map_or_else(
            || {
                "Rate-limited by the data agent endpoint. Wait at least 10 seconds \
                 before retrying. If this persists, the Fabric capacity may be under \
                 heavy load."
                    .to_string()
            },
            |seconds| {
                format!(
                    "Rate-limited by the data agent endpoint. Retry after {seconds} seconds. \
                     Do NOT retry before this time. If this persists, the Fabric capacity may \
                     be under heavy load."
                )
            },
        );
        return FabioError::with_hint(ErrorCode::RateLimited, message.to_string(), hint);
    }

    // Run failed or cancelled
    if msg_lower.contains("failed") || msg_lower.contains("cancelled") {
        return FabioError::with_hint(
            ErrorCode::ApiError,
            message.to_string(),
            "The data agent run failed. Possible causes: \
             (1) The data source (lakehouse/warehouse) is unavailable or the capacity is paused. \
             (2) The query references tables/columns not configured in the agent's data sources. \
             (3) The agent's AI instructions are misconfigured. \
             Check the agent definition with: fabio data-agent get-definition -w <workspace> --id <id>"
                .to_string(),
        );
    }

    // Timeout
    if msg_lower.contains("timeout") || msg_lower.contains("timed out") {
        return FabioError::with_hint(
            ErrorCode::Timeout,
            message.to_string(),
            "The data agent query timed out. This may happen on first use due to Spark cold start \
             (2-5 minutes on small capacities). Retry the query, or check if the Fabric capacity \
             is active and not overloaded."
                .to_string(),
        );
    }

    // Default: return error without hint
    FabioError::from_status(status, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_mcp_url_matches_documented_format() {
        let url = build_mcp_url("https://api.fabric.microsoft.com/v1", "ws-123", "agent-456");
        assert_eq!(
            url,
            "https://api.fabric.microsoft.com/v1/mcp/workspaces/ws-123/dataagents/agent-456/agent"
        );
    }

    #[test]
    fn build_mcp_url_trims_trailing_slash_on_base() {
        let url = build_mcp_url("https://api.fabric.microsoft.com/v1/", "w", "a");
        assert_eq!(
            url,
            "https://api.fabric.microsoft.com/v1/mcp/workspaces/w/dataagents/a/agent"
        );
    }

    #[test]
    fn build_mcp_url_honors_custom_base() {
        let url = build_mcp_url("https://example.test/v1", "w", "a");
        assert_eq!(
            url,
            "https://example.test/v1/mcp/workspaces/w/dataagents/a/agent"
        );
    }

    #[test]
    fn build_published_url_matches_canonical_pattern() {
        let url = build_published_url("https://api.fabric.microsoft.com/v1/", "ws-1", "agent-2");
        assert_eq!(
            url,
            "https://api.fabric.microsoft.com/v1/workspaces/ws-1/dataagents/agent-2/aiassistant/openai"
        );
    }

    #[test]
    fn validate_query_stage_accepts_production_aliases() {
        for s in ["production", "Published", "prod", "LIVE"] {
            assert!(
                validate_query_stage(s, false).is_ok(),
                "stage {s} should pass"
            );
        }
    }

    #[test]
    fn validate_query_stage_rejects_draft_stages() {
        for s in ["sandbox", "staging", "draft"] {
            let err = validate_query_stage(s, false).unwrap_err().to_string();
            assert!(
                err.contains("not supported") || err.contains("draft"),
                "stage {s} should be rejected, got: {err}"
            );
        }
    }

    #[test]
    fn validate_query_stage_rejects_unknown() {
        assert!(validate_query_stage("banana", false).is_err());
    }

    #[test]
    fn validate_query_stage_ignores_stage_with_explicit_url() {
        // An explicit --published-url overrides stage semantics entirely.
        assert!(validate_query_stage("sandbox", true).is_ok());
        assert!(validate_query_stage("banana", true).is_ok());
    }

    #[test]
    fn extract_answer_reads_last_assistant_text() {
        let messages = serde_json::json!({
            "data": [
                {"role": "user", "content": [{"type": "text", "text": {"value": "hi"}}]},
                {"role": "assistant", "content": [{"type": "text", "text": {"value": "first"}}]},
                {"role": "assistant", "content": [{"type": "text", "text": {"value": "final answer"}}]}
            ]
        });
        assert_eq!(extract_answer(&messages), "final answer");
    }

    #[test]
    fn extract_answer_skips_leading_non_text_content() {
        let messages = serde_json::json!({
            "data": [
                {"role": "assistant", "content": [
                    {"type": "image_file", "image_file": {"file_id": "f1"}},
                    {"type": "text", "text": {"value": "the chart shows growth"}}
                ]}
            ]
        });
        assert_eq!(extract_answer(&messages), "the chart shows growth");
    }

    #[test]
    fn extract_answer_defaults_when_empty() {
        assert_eq!(
            extract_answer(&serde_json::json!({"data": []})),
            "(No response from data agent)"
        );
    }

    #[test]
    fn extract_file_ids_collects_all_sources_deduped() {
        let messages = serde_json::json!({
            "data": [
                {"role": "user", "content": [{"type": "text", "text": {"value": "make a csv"}}]},
                {"role": "assistant",
                 "content": [
                    {"type": "image_file", "image_file": {"file_id": "img-1"}},
                    {"type": "text", "text": {"value": "see file",
                        "annotations": [
                            {"type": "file_path", "file_path": {"file_id": "csv-1"}},
                            {"type": "file_citation", "file_citation": {"file_id": "cite-1"}}
                        ]}}
                 ],
                 "attachments": [{"file_id": "att-1"}, {"file_id": "img-1"}]
                }
            ]
        });
        assert_eq!(
            extract_file_ids(&messages),
            vec!["img-1", "csv-1", "cite-1", "att-1"]
        );
    }

    #[test]
    fn extract_file_ids_empty_when_no_files() {
        let messages = serde_json::json!({
            "data": [{"role": "assistant", "content": [{"type": "text", "text": {"value": "no files"}}]}]
        });
        assert!(extract_file_ids(&messages).is_empty());
    }

    #[test]
    fn sanitize_filename_strips_paths_and_traversal() {
        assert_eq!(sanitize_filename("report.csv", "fb"), "report.csv");
        assert_eq!(sanitize_filename("../../etc/passwd", "fb"), "passwd");
        assert_eq!(sanitize_filename("dir\\sub\\chart.png", "fb"), "chart.png");
        assert_eq!(sanitize_filename("/abs/path/x.txt", "fb"), "x.txt");
        assert_eq!(sanitize_filename("", "fallback-id"), "fallback-id");
        assert_eq!(sanitize_filename("..", "fallback-id"), "fallback-id");
    }
}
