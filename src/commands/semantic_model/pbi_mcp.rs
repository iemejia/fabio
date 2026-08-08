//! `semantic-model` commands backed by the remote **Power BI MCP server** —
//! Copilot-powered capabilities with no direct REST equivalent:
//! - `generate-dax` — natural-language → DAX (Copilot's DAX-generation engine)
//! - `copilot-schema` — the Copilot-oriented model schema + author custom
//!   instructions / AI-optimized metadata (which `INFO.VIEW.*` cannot surface)
//!
//! fabio drives these via the generic `mcp_client` (see `commands::powerbi_mcp`).

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::powerbi_mcp::{call_powerbi_tool, tool_text_as_json};
use crate::output;

use super::operations::execute_dax_and_render;

const TOOL_GENERATE: &str = "GenerateQuery";
const TOOL_SCHEMA: &str = "GetSemanticModelSchema";

/// Extract the generated DAX from a `GenerateQuery` tool result (the tool returns
/// `{"daxQuery": "...", "semanticModel": {...}}`). Pure.
fn extract_generated_dax(parsed: &Value) -> Option<String> {
    parsed
        .get("daxQuery")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// `semantic-model generate-dax` — translate a natural-language prompt into DAX
/// using the remote Power BI MCP server's `GenerateQuery` tool (Copilot). With
/// `--execute`, the generated DAX is also run via `executeQueries` and the rows
/// returned. Read-only (`--execute` only runs a SELECT-style DAX query).
pub(super) async fn generate_dax(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    prompt: &str,
    execute: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "semantic-model generate-dax",
        &serde_json::json!({
            "id": id,
            "prompt": prompt,
            "tool": TOOL_GENERATE,
            "execute": execute,
        }),
    ) {
        return Ok(());
    }

    let result = call_powerbi_tool(
        client,
        TOOL_GENERATE,
        serde_json::json!({ "artifactId": id, "userInput": prompt }),
    )
    .await?;

    if result.is_error {
        anyhow::bail!(
            "Power BI MCP GenerateQuery returned an error: {}",
            result.text()
        );
    }

    let parsed = tool_text_as_json(&result);
    let dax = extract_generated_dax(&parsed);

    if execute {
        let Some(ref dax_query) = dax else {
            anyhow::bail!("GenerateQuery returned no daxQuery to execute: {parsed}");
        };
        // Reuse the shared executeQueries path (renders rows like `query`).
        return execute_dax_and_render(
            cli,
            client,
            workspace,
            id,
            dax_query,
            "semantic-model generate-dax",
        )
        .await;
    }

    let out = serde_json::json!({
        "prompt": prompt,
        "dax": dax,
        "model": parsed.get("semanticModel").cloned().unwrap_or(Value::Null),
        "note": "Copilot-generated DAX. Run it with: fabio semantic-model query --id <ID> --dax '<DAX>' (or re-run with --execute).",
    });
    output::render_object(cli, &out, "dax");
    Ok(())
}

/// `semantic-model copilot-schema` — fetch the Copilot-oriented schema for a
/// model from the Power BI MCP server's `GetSemanticModelSchema` tool. Unlike the
/// offline `list-tables`/`list-columns`/… (raw `INFO.VIEW.*`), this returns a
/// single serialized schema PLUS the author's custom instructions and any
/// AI-optimized "prepare data for AI" metadata — grounding for DAX generation.
/// Read-only.
pub(super) async fn copilot_schema(
    cli: &Cli,
    client: &FabricClient,
    _workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "semantic-model copilot-schema",
        &serde_json::json!({ "id": id, "tool": TOOL_SCHEMA }),
    ) {
        return Ok(());
    }

    let result =
        call_powerbi_tool(client, TOOL_SCHEMA, serde_json::json!({ "artifactId": id })).await?;

    if result.is_error {
        anyhow::bail!(
            "Power BI MCP GetSemanticModelSchema returned an error: {}",
            result.text()
        );
    }

    output::render_object(cli, &tool_text_as_json(&result), "schema");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_generated_dax_reads_dax_query_field() {
        let parsed = serde_json::json!({
            "daxQuery": "EVALUATE SUMMARIZECOLUMNS('dimstore'[Region])",
            "semanticModel": { "Name": "RetailSalesModel" }
        });
        assert_eq!(
            extract_generated_dax(&parsed).as_deref(),
            Some("EVALUATE SUMMARIZECOLUMNS('dimstore'[Region])")
        );
    }

    #[test]
    fn extract_generated_dax_none_when_absent() {
        assert!(extract_generated_dax(&serde_json::json!({ "text": "no dax" })).is_none());
    }
}
