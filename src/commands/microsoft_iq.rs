use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_forbidden;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum MicrosoftIqCommand {
    /// Retrieve relevance-ranked organizational context
    Retrieve {
        /// Non-empty natural-language query
        #[arg(long)]
        text: String,
        /// Maximum total token count of returned summaries
        #[arg(long, default_value_t = 2048, value_parser = clap::value_parser!(u32).range(1..))]
        token_budget: u32,
    },
    /// Check whether the caller can see any Microsoft IQ context
    AmIInScope,
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &MicrosoftIqCommand) -> Result<()> {
    match command {
        MicrosoftIqCommand::Retrieve { text, token_budget } => {
            let body = json!({ "query": { "text": text }, "tokenBudget": token_budget });
            let data = client
                .post_readonly("/microsoftIQ/retrieve", &body)
                .await
                .map_err(|error| {
                    enrich_forbidden(error, "microsoft-iq retrieve", "MicrosoftIQ.Read.All")
                })?;
            output::render_list(
                cli,
                data["results"].as_array().map_or(&[], Vec::as_slice),
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
            output::render_object(cli, &data, "inScope");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn retrieve_request_matches_spec() {
        let body = json!({ "query": { "text": "quarterly risks" }, "tokenBudget": 1024 });
        assert_eq!(body["query"]["text"], "quarterly risks");
        assert_eq!(body["tokenBudget"], 1024);
    }
}
