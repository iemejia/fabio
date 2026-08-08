use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum CatalogCommand {
    /// Search the Fabric catalog
    #[command(display_order = 1)]
    Search {
        /// Search query string
        #[arg(short = 's', long = "search")]
        search_query: Option<String>,

        /// Filter by item type (e.g., Notebook, Lakehouse). Comma-separated for multiple.
        #[arg(short = 't', long = "type")]
        item_type: Option<String>,

        /// Exclude item types from results. Comma-separated for multiple.
        #[arg(long)]
        exclude_type: Option<String>,

        /// Maximum number of results to return
        #[arg(long)]
        top: Option<u32>,

        /// Path to JSON file with full search request body
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON search request body
        #[arg(long)]
        content: Option<String>,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &CatalogCommand) -> Result<()> {
    match command {
        CatalogCommand::Search {
            search_query,
            item_type,
            exclude_type,
            top,
            file,
            content,
        } => {
            search(
                cli,
                client,
                search_query.as_deref(),
                item_type.as_deref(),
                exclude_type.as_deref(),
                *top,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn search(
    cli: &Cli,
    client: &FabricClient,
    query: Option<&str>,
    item_type: Option<&str>,
    exclude_type: Option<&str>,
    top: Option<u32>,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    // --file and --content take full control of the body (raw passthrough).
    // A --continuation-token resumes a specific page: the token ENCODES the
    // original search/filter, so the request must contain ONLY the token
    // (repeating search/filter/pageSize → `ConflictingFilterParameters`).
    let mut body = if let Some(t) = cli.continuation_token.as_deref() {
        serde_json::json!({ "continuationToken": t })
    } else {
        match (file, content) {
            (Some(path), _) => {
                let raw = std::fs::read_to_string(path)
                    .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
                serde_json::from_str::<Value>(&raw)
                    .map_err(|e| anyhow::anyhow!("Invalid JSON: {e}"))?
            }
            (_, Some(c)) => serde_json::from_str::<Value>(c)
                .map_err(|e| anyhow::anyhow!("Invalid JSON: {e}"))?,
            _ => {
                // Build body from convenience flags
                if query.is_none() && item_type.is_none() && exclude_type.is_none() {
                    return Err(FabioError::with_hint(
                        ErrorCode::InvalidInput,
                        "At least one of --query, --type, --file, or --content must be provided"
                            .to_string(),
                        "Example: fabio catalog search --query \"my lakehouse\" --type Notebook --top 10"
                            .to_string(),
                    )
                    .into());
                }
                build_search_body(query, item_type, exclude_type, top)
            }
        }
    };

    if output::dry_run_guard(cli, "catalog search", &body) {
        return Ok(());
    }

    // Auto-paginate when --all: keep posting with the returned continuationToken
    // until exhausted (accumulating pages). Without --all, fetch a single page
    // and surface the token so the caller can resume with --continuation-token.
    let mut all_items: Vec<Value> = Vec::new();
    let mut last_token: Option<String>;
    loop {
        let data = client.post("/catalog/search", &body, false).await?;
        let Some(arr) = data.get("value").and_then(Value::as_array) else {
            output::render_object(cli, &data, "value");
            return Ok(());
        };
        all_items.extend(arr.iter().cloned());
        last_token = data
            .get("continuationToken")
            .and_then(Value::as_str)
            // The API returns an EMPTY-string token (not null/absent) on the last
            // page — treat that as "no more pages" so we don't post an empty token.
            .filter(|s| !s.is_empty())
            .map(str::to_owned);

        if !cli.all || last_token.is_none() {
            break;
        }
        // Next page: the token encodes the search/filter — send ONLY it.
        body = serde_json::json!({ "continuationToken": last_token.clone().unwrap_or_default() });
    }

    // Flatten the `{value:[...]}` search envelope to the standard list shape
    // (`{data:[...],count:N}`) so agents can iterate/filter/project `data`
    // consistently with every other list command.
    output::render_list_with_token(
        cli,
        &all_items,
        &[
            "displayName",
            "id",
            "type",
            "hierarchy.workspace.displayName",
            "description",
        ],
        &["NAME", "ID", "TYPE", "WORKSPACE", "DESCRIPTION"],
        "id",
        // When --all exhausted the pages, there's no further token to surface.
        if cli.all { None } else { last_token.as_deref() },
    );
    Ok(())
}

/// Build a catalog search request body from convenience flags.
fn build_search_body(
    query: Option<&str>,
    item_type: Option<&str>,
    exclude_type: Option<&str>,
    top: Option<u32>,
) -> Value {
    let mut body = serde_json::Map::new();

    // The `CatalogQueryRequest` fields are `search` / `pageSize` / `filter`
    // (NOT `searchString` / `top` / `itemTypes` — those are silently ignored by
    // the API, which then returns a default unfiltered listing).
    if let Some(q) = query {
        body.insert("search".to_string(), Value::from(q));
    }

    if let Some(t) = top {
        body.insert("pageSize".to_string(), Value::Number(t.into()));
    }

    // `filter` is an OData-style string over the `Type` property, e.g.
    // "Type eq 'Report' or Type eq 'Lakehouse'". `--exclude-type` becomes
    // "Type ne 'X'" clauses ANDed with the include clause.
    if let Some(filter) = build_type_filter(item_type, exclude_type) {
        body.insert("filter".to_string(), Value::from(filter));
    }

    Value::Object(body)
}

/// Build the catalog `filter` string from comma-separated include/exclude item
/// types. Include types are joined with `or` (`Type eq 'A' or Type eq 'B'`),
/// exclude types with `and` (`Type ne 'C' and Type ne 'D'`); when both are
/// present they are combined with `and`. Returns `None` when neither is given. Pure.
fn build_type_filter(item_type: Option<&str>, exclude_type: Option<&str>) -> Option<String> {
    let include: Vec<String> = item_type
        .into_iter()
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|t| format!("Type eq '{t}'"))
        .collect();
    let exclude: Vec<String> = exclude_type
        .into_iter()
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|t| format!("Type ne '{t}'"))
        .collect();

    let mut clauses: Vec<String> = Vec::new();
    if !include.is_empty() {
        clauses.push(if include.len() == 1 {
            include.into_iter().next().unwrap_or_default()
        } else {
            format!("({})", include.join(" or "))
        });
    }
    if !exclude.is_empty() {
        clauses.push(exclude.join(" and "));
    }
    if clauses.is_empty() {
        None
    } else {
        Some(clauses.join(" and "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_search_body_query_only() {
        let body = build_search_body(Some("lakehouse"), None, None, None);
        assert_eq!(body["search"], "lakehouse");
        assert!(body.get("filter").is_none());
        assert!(body.get("pageSize").is_none());
    }

    #[test]
    fn build_search_body_with_type_filter() {
        let body = build_search_body(Some("test"), Some("Notebook,Lakehouse"), None, Some(5));
        assert_eq!(body["search"], "test");
        assert_eq!(body["pageSize"], 5);
        assert_eq!(
            body["filter"],
            "(Type eq 'Notebook' or Type eq 'Lakehouse')"
        );
    }

    #[test]
    fn build_search_body_single_type_no_parens() {
        let body = build_search_body(None, Some("Lakehouse"), None, None);
        assert_eq!(body["filter"], "Type eq 'Lakehouse'");
    }

    #[test]
    fn build_search_body_with_exclude_type() {
        let body = build_search_body(None, None, Some("Dashboard"), None);
        assert_eq!(body["filter"], "Type ne 'Dashboard'");
    }

    #[test]
    fn build_search_body_both_filters() {
        let body = build_search_body(Some("sales"), Some("Notebook"), Some("Lakehouse"), Some(20));
        assert_eq!(body["search"], "sales");
        assert_eq!(body["pageSize"], 20);
        assert_eq!(body["filter"], "Type eq 'Notebook' and Type ne 'Lakehouse'");
    }

    #[test]
    fn build_type_filter_none_when_empty() {
        assert!(build_type_filter(None, None).is_none());
        assert!(build_type_filter(Some(""), Some("  ")).is_none());
    }
}
