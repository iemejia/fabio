//! `data-agent query` / MCP-url helpers.
//!
//! A *published* Fabric data agent is consumed at runtime through its Model
//! Context Protocol (MCP) endpoint (the `OpenAI` Assistants API that previously
//! backed this path was retired by `OpenAI` on 2026-08-26). `query` sends a single
//! natural-language question to that endpoint and returns the answer; the
//! transport lives in [`super::mcp`].

use std::io;
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;

use super::mcp::run_mcp_query;
use crate::cli::Cli;
use crate::client::{self, FabricClient};
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// Validate the requested query stage.
///
/// Only a *published* data agent has an MCP endpoint, so a request to query a
/// draft/sandbox stage must fail fast rather than silently querying production.
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
                "Querying the '{stage}' (draft) stage is not supported — only a published agent has an MCP endpoint"
            ),
            "Publish the agent first: fabio data-agent publish --workspace <WS> --id <ID>, then \
             query the default --stage production. To target a specific endpoint directly, pass \
             --published-url with the MCP server URL.",
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

/// Query a published data agent through its MCP endpoint.
///
/// Flow: resolve the MCP URL (explicit `--published-url` or the well-known
/// pattern) -> run a single MCP `tools/call` -> render the answer.
#[allow(clippy::too_many_arguments)]
pub(super) async fn query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    prompt: Option<&str>,
    published_url: Option<&str>,
    raw: bool,
    stage: &str,
    timeout: u64,
) -> Result<()> {
    validate_query_stage(stage, published_url.is_some())?;

    // Resolve prompt text: --prompt flag or stdin.
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

    // Resolve the MCP endpoint: explicit flag or the well-known pattern.
    let resolved_url = if let Some(url) = published_url {
        client::validate_trusted_url(url, "--published-url")?;
        url.to_string()
    } else {
        let url = build_mcp_url(client::fabric_base_url(), workspace, id);
        client::validate_trusted_url(&url, "data agent MCP URL")?;
        url
    };

    let token = client.require_auth().await?;
    let max_wait = Duration::from_secs(timeout);
    let result = run_mcp_query(&resolved_url, &token, prompt_text.trim(), max_wait).await?;

    let mut out = serde_json::json!({
        "question": prompt_text.trim(),
        "answer": result.answer,
        "tool": result.tool,
    });
    if raw {
        out["raw"] = result.raw;
    }
    output::render_object(cli, &out, "answer");
    Ok(())
}

/// Build the Model Context Protocol (MCP) endpoint URL for a data agent.
///
/// This is the canonical runtime/consumption surface for a *published* data
/// agent. Format (per the Fabric data agent SDK):
/// `{base}/mcp/workspaces/{workspace}/dataagents/{id}/agent`. Pure for testing.
pub(super) fn build_mcp_url(base: &str, workspace: &str, id: &str) -> String {
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

/// Validate the query stage, then resolve the agent's MCP endpoint URL.
///
/// Shared by `data-agent evaluate` (and any consumer that must reach the MCP
/// endpoint without an explicit `--published-url`). The URL is validated as a
/// trusted Fabric host to prevent token exfiltration.
pub(super) fn resolve_mcp_url(workspace: &str, id: &str, stage: &str) -> Result<String> {
    validate_query_stage(stage, false)?;
    let url = build_mcp_url(client::fabric_base_url(), workspace, id);
    client::validate_trusted_url(&url, "data agent MCP URL")?;
    Ok(url)
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
    fn resolve_mcp_url_builds_and_validates() {
        let url = resolve_mcp_url("ws-1", "agent-2", "production").unwrap();
        assert!(url.ends_with("/mcp/workspaces/ws-1/dataagents/agent-2/agent"));
        // A draft stage is rejected before any URL is returned.
        assert!(resolve_mcp_url("ws-1", "agent-2", "sandbox").is_err());
    }
}
