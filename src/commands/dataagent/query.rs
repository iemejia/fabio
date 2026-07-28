use std::io;
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
    _stage: &str,
    timeout: u64,
) -> Result<()> {
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

    // Use the OpenAI Assistants protocol against the published URL
    let token = client.require_auth().await?;
    let max_wait = Duration::from_secs(timeout);
    let query_result =
        run_assistant_query(&resolved_url, &token, &prompt_text, verbose, max_wait).await?;

    let mut result = serde_json::json!({
        "question": prompt_text.trim(),
        "answer": query_result.answer,
    });
    if let Some(steps) = query_result.steps {
        result["steps"] = steps;
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

    // No published URL found — the agent may not be published yet.
    Err(FabioError::with_hint(
        ErrorCode::ApiError,
        "Published URL not found. The data agent may not be published yet.",
        format!(
            "Publish the agent with 'fabio data-agent publish', then provide the URL with --published-url. \
             The URL pattern is: https://api.fabric.microsoft.com/v1/workspaces/{workspace}/dataagents/{id}/aiassistant/openai"
        ),
    )
    .into())
}

/// Result of a data agent query, including the answer and optional execution steps.
struct QueryResult {
    answer: String,
    steps: Option<Value>,
}

/// Run a query against the data agent using the `OpenAI` Assistants API protocol.
async fn run_assistant_query(
    base_url: &str,
    token: &str,
    question: &str,
    verbose: bool,
    max_wait: Duration,
) -> Result<QueryResult> {
    let http = crate::client::http_client_builder()
        .timeout(Duration::from_mins(6))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| FabioError::with_hint(ErrorCode::NetworkError, e.to_string(), "Verify the data agent is published. Check status: fabio data-agent show --workspace <WS> --id <ID>. Publish if needed: fabio data-agent publish --workspace <WS> --id <ID>"))?;

    let auth_header = token;

    // Step 1: Create assistant + thread
    let assistant_id = create_assistant(&http, base_url, auth_header).await?;
    let thread_id = create_thread(&http, base_url, auth_header).await?;

    // Step 2: Post message and run
    post_message(&http, base_url, auth_header, &thread_id, question).await?;
    let run_id = create_run(&http, base_url, auth_header, &thread_id, &assistant_id).await?;

    // Step 3: Poll until complete
    poll_run_completion(&http, base_url, auth_header, &thread_id, &run_id, max_wait).await?;

    // Step 4 (optional): Retrieve run steps for verbose mode
    let steps = if verbose {
        Some(retrieve_run_steps(&http, base_url, auth_header, &thread_id, &run_id).await?)
    } else {
        None
    };

    // Step 5: Get response
    let response_text = retrieve_response(&http, base_url, auth_header, &thread_id).await?;

    // Step 6: Clean up thread (best effort)
    let _ = http
        .delete(format!(
            "{base_url}/threads/{thread_id}?api-version=2024-05-01-preview"
        ))
        .header("Authorization", auth_header)
        .send()
        .await;

    Ok(QueryResult {
        answer: response_text,
        steps,
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

/// Retrieve the assistant's response from the thread messages.
async fn retrieve_response(
    http: &reqwest::Client,
    base_url: &str,
    auth_header: &str,
    thread_id: &str,
) -> Result<String> {
    let resp = http
        .get(format!(
            "{base_url}/threads/{thread_id}/messages?api-version=2024-05-01-preview&order=asc"
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

    let messages: Value = resp.json().await.map_err(|e| {
        FabioError::with_hint(
            ErrorCode::ApiError,
            format!("Parse messages response: {e}"),
            "Unexpected response format. This may indicate an API version mismatch.",
        )
    })?;

    // Extract the assistant's response (last message with role=assistant)
    let text = messages
        .get("data")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .rev()
                .find(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
        })
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|c| c.get("text"))
        .and_then(|t| t.get("value"))
        .and_then(Value::as_str)
        .unwrap_or("(No response from data agent)");

    Ok(text.to_string())
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
}
