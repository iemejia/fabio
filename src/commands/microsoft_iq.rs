use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
pub enum MicrosoftIqCommand {
    /// Retrieve relevance-ranked organizational context
    Retrieve {
        /// Non-empty natural-language query
        #[arg(long)]
        text: String,
        /// Maximum total token count of summaries (1 to 2,147,483,647; default 2048)
        #[arg(long, default_value_t = 2048, value_parser = clap::value_parser!(i32).range(1..))]
        token_budget: i32,
    },
    /// Check whether the caller can see any Microsoft IQ context
    AmIInScope,
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &MicrosoftIqCommand) -> Result<()> {
    match command {
        MicrosoftIqCommand::Retrieve { text, token_budget } => {
            let body = retrieve_body(text, *token_budget)?;
            let data = client
                .post_readonly("/microsoftIQ/retrieve", &body)
                .await
                .map_err(|error| {
                    enrich_forbidden(error, "microsoft-iq retrieve", "MicrosoftIQ.Read.All")
                })?;
            output::render_list(
                cli,
                retrieve_results(&data)?,
                &["rank", "score", "type", "tokenCount", "data.summary"],
                &["RANK", "SCORE", "TYPE", "TOKENS", "SUMMARY"],
                "identifier",
            );
            Ok(())
        }
        MicrosoftIqCommand::AmIInScope => {
            let data = client
                .post_readonly("/microsoftIQ/amIInScope", &json!({}))
                .await
                .map_err(|error| {
                    enrich_forbidden(error, "microsoft-iq am-i-in-scope", "MicrosoftIQ.Read.All")
                })?;
            output::render_object(cli, scope_result(&data)?, "inScope");
            Ok(())
        }
    }
}

fn retrieve_body(text: &str, token_budget: i32) -> Result<Value> {
    if text.trim().is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Microsoft IQ query text cannot be empty",
            "Provide --text with a non-empty natural-language query.",
        )
        .into());
    }
    if token_budget <= 0 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Microsoft IQ token budget must be positive",
            "Use --token-budget from 1 to 2147483647 (default 2048).",
        )
        .into());
    }
    Ok(json!({ "query": { "text": text }, "tokenBudget": token_budget }))
}

fn retrieve_results(response: &Value) -> Result<&[Value]> {
    if crate::commands::diagnostics::has_error(response) {
        return Err(FabioError::api_error("Microsoft IQ returned an error-bearing retrieval response, not empty permitted context.").into());
    }
    response["results"].as_array().map(Vec::as_slice).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::ApiError,
            "Microsoft IQ retrieval response is missing its results array",
            "An empty permitted result must contain results: []; inspect the service response rather than treating an invalid response as no context.",
        ).into()
    })
}

fn scope_result(response: &Value) -> Result<&Value> {
    if crate::commands::diagnostics::has_error(response) || !response["inScope"].is_boolean() {
        return Err(FabioError::with_hint(ErrorCode::ApiError,
            "Microsoft IQ scope response is malformed or error-bearing",
            "A successful scope response contains boolean inScope. False means no permitted context, not an OAuth error; preserve real authorization/backend failures separately.").into());
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieve_request_matches_spec() {
        let body = retrieve_body("quarterly risks", 1024).unwrap();
        assert_eq!(body["query"]["text"], "quarterly risks");
        assert_eq!(body["tokenBudget"], 1024);
    }

    #[test]
    fn retrieve_rejects_blank_queries_without_changing_valid_text() {
        for text in ["", " ", "\r\n\t"] {
            assert!(retrieve_body(text, 64).is_err());
        }
        assert_eq!(
            retrieve_body("  risques trimestriels  ", 64).unwrap()["query"]["text"],
            "  risques trimestriels  "
        );
    }

    #[test]
    fn retrieve_results_distinguishes_empty_context_from_malformed_response() {
        assert_eq!(retrieve_results(&json!({"results": []})).unwrap().len(), 0);
        for response in [
            json!({}),
            json!({"results": null}),
            json!({"results": {}}),
            json!([]),
        ] {
            assert!(retrieve_results(&response).is_err());
        }
    }

    #[test]
    fn token_budget_uses_the_published_signed_int32_range() {
        assert!(retrieve_body("context", 0).is_err());
        assert!(retrieve_body("context", -1).is_err());
        assert_eq!(
            retrieve_body("context", i32::MAX).unwrap()["tokenBudget"],
            i32::MAX
        );
    }

    #[test]
    fn scope_false_is_success_but_missing_or_error_bearing_data_is_not() {
        let response = json!({"inScope":false,"futureField":{"retain":true}});
        assert_eq!(scope_result(&response).unwrap(), &response);
        for response in [
            json!({}),
            json!({"inScope":"false"}),
            json!({"inScope":false,"errorCode":"Denied"}),
        ] {
            assert!(scope_result(&response).is_err());
        }
        assert!(retrieve_results(&json!({"results":[],"error":{"message":"failed"}})).is_err());
    }

    #[test]
    fn retrieval_preserves_null_scores_and_open_typed_pointers_verbatim() {
        let response = json!({"results":[{"identifier":"synthetic","rank":0,"score":null,"lexicalMatch":null,
            "data":{"summary":"Synthetic context","uri":{"$type":"FutureReference","opaque":{"keep":true}}}}]});
        assert_eq!(
            retrieve_results(&response).unwrap(),
            response["results"].as_array().unwrap().as_slice()
        );
    }
}
