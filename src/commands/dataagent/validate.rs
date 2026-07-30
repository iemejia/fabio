//! `data-agent validate-fewshots` — LLM-based semantic review of a data
//! source's few-shot examples.
//!
//! Mirrors the Python SDK's few-shot validation (`_few_shot_validation.py`),
//! which fabio previously listed as out of scope because it needs an external
//! judge model. The judge is now supplied by the caller via `--llm-*`
//! (`src/llm.rs`): fabio fetches the few-shots and asks the model to flag
//! duplicates, semantic conflicts, ambiguity, and low-quality or likely-wrong
//! queries, returning a structured report.

use anyhow::Result;
use serde_json::Value;

use super::resolve_datasource_id;
use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::llm::{LlmClient, LlmConfig};
use crate::output;

const SYSTEM_PROMPT: &str = "You are a meticulous reviewer of few-shot examples used to teach a \
Microsoft Fabric data agent how to translate natural-language questions into SQL/KQL/DAX queries. \
Each example is a {question, query} pair. Review the whole set and identify problems that would \
degrade the agent's answer quality. Consider these issue types: \
'duplicate' (two examples with essentially the same question), \
'conflict' (similar questions mapped to materially different queries, or vice versa), \
'ambiguous' (a question too vague to map to one query), \
'low_quality' (unclear, trivial, or unhelpful example), \
'incorrect_query' (the query looks syntactically wrong or does not answer the question). \
Respond with ONLY a JSON object, no prose, of the form: \
{\"issues\":[{\"fewshotIds\":[\"<id>\"...],\"type\":\"<type>\",\"severity\":\"high|medium|low\",\
\"explanation\":\"...\",\"suggestion\":\"...\"}],\"overallQuality\":\"good|fair|poor\",\
\"summary\":\"one-sentence overview\"}. \
If there are no issues, return an empty issues array and overallQuality 'good'.";

/// Run an LLM review over a data source's few-shot examples.
#[allow(clippy::too_many_arguments)]
pub(super) async fn validate_fewshots(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    stage: &str,
    llm: &LlmConfig,
) -> Result<()> {
    // Fail fast with a clear message if the judge model is not configured.
    let llm_client = LlmClient::from_config(llm)?;

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let prefix = if stage.eq_ignore_ascii_case("published") {
        ""
    } else {
        "/staging"
    };

    let resp = client
        .get_list(
            &format!(
                "/workspaces/{workspace}/dataAgents/{id}{prefix}/datasources/{ds_id}/fewshots"
            ),
            "value",
            true,
            None,
        )
        .await?;

    let fewshots: Vec<Value> = resp.items;
    if fewshots.is_empty() {
        let result = serde_json::json!({
            "datasource": datasource,
            "fewshotCount": 0,
            "overallQuality": "good",
            "issues": [],
            "summary": "No few-shot examples to validate.",
        });
        output::render_object(cli, &result, "summary");
        return Ok(());
    }

    let user_prompt = build_validation_prompt(&fewshots);
    let review = llm_client
        .complete_json(SYSTEM_PROMPT, &user_prompt)
        .await?;

    // Assemble the output around the model's review, adding provenance fields.
    let issues = review
        .get("issues")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let issue_count = issues.as_array().map_or(0, Vec::len);
    let result = serde_json::json!({
        "datasource": datasource,
        "fewshotCount": fewshots.len(),
        "issueCount": issue_count,
        "overallQuality": review.get("overallQuality").cloned().unwrap_or(Value::Null),
        "summary": review.get("summary").cloned().unwrap_or(Value::Null),
        "issues": issues,
        "model": llm.model,
    });
    output::render_object(cli, &result, "summary");
    Ok(())
}

/// Build the user prompt listing every few-shot example with its ID.
fn build_validation_prompt(fewshots: &[Value]) -> String {
    use std::fmt::Write as _;
    let mut prompt =
        String::from("Review these few-shot examples and report issues as specified.\n\n");
    for (i, fs) in fewshots.iter().enumerate() {
        let fid = fs.get("id").and_then(Value::as_str).unwrap_or("(unknown)");
        let question = fs
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let query = fs.get("query").and_then(Value::as_str).unwrap_or("").trim();
        let _ = write!(
            prompt,
            "{n}. id={fid}\n   question: {question}\n   query: {query}\n\n",
            n = i + 1
        );
    }
    prompt
}

/// Guard used by the dispatcher to produce a clear error when the LLM is not
/// configured before any network work happens.
pub(super) fn ensure_llm_configured(llm: &LlmConfig) -> Result<()> {
    if llm.is_configured() {
        Ok(())
    } else {
        Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "validate-fewshots requires an external judge model".to_string(),
            "Provide --llm-endpoint, --llm-key, and --llm-model (or FABIO_LLM_ENDPOINT/\
             FABIO_LLM_KEY/FABIO_LLM_MODEL). For Azure OpenAI, --llm-model is the deployment name.",
        )
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_validation_prompt_lists_all_with_ids() {
        let fewshots = vec![
            serde_json::json!({"id": "fs-1", "question": "total sales?", "query": "SELECT SUM(x)"}),
            serde_json::json!({"id": "fs-2", "question": "top product?", "query": "SELECT TOP 1 ..."}),
        ];
        let prompt = build_validation_prompt(&fewshots);
        assert!(prompt.contains("id=fs-1"));
        assert!(prompt.contains("total sales?"));
        assert!(prompt.contains("id=fs-2"));
        assert!(prompt.contains("SELECT TOP 1"));
    }

    #[test]
    fn build_validation_prompt_tolerates_missing_fields() {
        let fewshots = vec![serde_json::json!({"question": "q only"})];
        let prompt = build_validation_prompt(&fewshots);
        assert!(prompt.contains("id=(unknown)"));
        assert!(prompt.contains("q only"));
    }

    #[test]
    fn ensure_llm_configured_errors_when_missing() {
        assert!(ensure_llm_configured(&LlmConfig::default()).is_err());
        let cfg = LlmConfig {
            endpoint: Some("https://x.openai.azure.com".into()),
            key: Some("k".into()),
            model: Some("dep".into()),
            api_version: None,
        };
        assert!(ensure_llm_configured(&cfg).is_ok());
    }
}
