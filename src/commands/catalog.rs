use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
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
        /// Search text; supports quoted phrases, *, ?, &&, and underscores
        #[arg(short = 's', long = "search")]
        search_query: Option<String>,

        /// Filter by item type. Comma-separated; at most 500 values of 50 characters.
        #[arg(short = 't', long = "type")]
        item_type: Option<String>,

        /// Exclude item types. Comma-separated; shares the 500-value Type limit.
        #[arg(long)]
        exclude_type: Option<String>,

        /// Filter to workspace IDs. Comma-separated; at most 12.
        #[arg(long)]
        workspace_id: Option<String>,

        /// Results per page (1-1000; defaults to 50)
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=1000))]
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
            workspace_id,
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
                workspace_id.as_deref(),
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
    workspace_id: Option<&str>,
    top: Option<u32>,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    // --file and --content take full control of the body (raw passthrough).
    // A --continuation-token resumes a specific page: the token ENCODES the
    // original search/filter, so the request must contain ONLY the token
    // (repeating search/filter/pageSize → `ConflictingFilterParameters`).
    let mut body = if let Some(t) = cli.continuation_token.as_deref() {
        if query.is_some()
            || item_type.is_some()
            || exclude_type.is_some()
            || workspace_id.is_some()
            || top.is_some()
            || file.is_some()
            || content.is_some()
        {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--continuation-token cannot be combined with search, filter, page-size, file, or content options",
                "Resume with only: fabio catalog search --continuation-token <TOKEN>",
            )
            .into());
        }
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
                if query.is_none()
                    && item_type.is_none()
                    && exclude_type.is_none()
                    && workspace_id.is_none()
                {
                    return Err(FabioError::with_hint(
                        ErrorCode::InvalidInput,
                        "At least one of --search, --type, --exclude-type, --workspace-id, --file, or --content must be provided"
                            .to_string(),
                        "Example: fabio catalog search --search \"my lakehouse\" --type Notebook --workspace-id <WS> --top 10"
                            .to_string(),
                    )
                    .into());
                }
                build_search_body(query, item_type, exclude_type, workspace_id, top)?
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
        last_token = next_page_token(&data);

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
    workspace_id: Option<&str>,
    top: Option<u32>,
) -> Result<Value> {
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
    if let Some(filter) = build_filter(item_type, exclude_type, workspace_id)? {
        body.insert("filter".to_string(), Value::from(filter));
    }

    Ok(Value::Object(body))
}

/// Extract the continuation token for the NEXT page from a `/catalog/search`
/// response. The API returns an EMPTY-string token (not null/absent) on the last
/// page, so an empty token is normalized to `None` ("no more pages") — otherwise
/// a follow-up request with an empty token fails with `InvalidContinuationToken`.
fn next_page_token(data: &Value) -> Option<String> {
    data.get("continuationToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Build the catalog `filter` string from comma-separated include/exclude item
/// types. Include types are joined with `or` (`Type eq 'A' or Type eq 'B'`),
/// exclude types with `and` (`Type ne 'C' and Type ne 'D'`); when both are
/// present they are combined with `and`. Returns `None` when neither is given. Pure.
fn split_values(raw: Option<&str>) -> Vec<&str> {
    raw.into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect()
}

fn build_filter(
    item_type: Option<&str>,
    exclude_type: Option<&str>,
    workspace_id: Option<&str>,
) -> Result<Option<String>> {
    let included_types = split_values(item_type);
    let excluded_types = split_values(exclude_type);
    if included_types.len() + excluded_types.len() > 500 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Catalog filters support at most 500 Type values",
            "Reduce the comma-separated values passed to --type and --exclude-type.",
        )
        .into());
    }
    if let Some(value) = included_types
        .iter()
        .chain(excluded_types.iter())
        .find(|value| value.len() > 50)
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Catalog Type filter value exceeds 50 characters: '{value}'"),
            "Use a valid Fabric item type such as Report, Lakehouse, SemanticModel, or Workspace.",
        )
        .into());
    }

    let include: Vec<String> = included_types
        .into_iter()
        .map(|t| format!("Type eq '{t}'"))
        .collect();
    let exclude: Vec<String> = excluded_types
        .into_iter()
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
    let workspace_ids = split_values(workspace_id);
    if workspace_ids.len() > 12 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Catalog filters support at most 12 WorkspaceId values",
            "Reduce the comma-separated workspace IDs passed to --workspace-id.",
        )
        .into());
    }
    for id in &workspace_ids {
        validate_uuid(id, "--workspace-id")?;
    }
    if !workspace_ids.is_empty() {
        let workspace_filter = workspace_ids
            .into_iter()
            .map(|id| format!("WorkspaceId eq '{id}'"))
            .collect::<Vec<_>>();
        clauses.push(if workspace_filter.len() == 1 {
            workspace_filter.into_iter().next().unwrap_or_default()
        } else {
            format!("({})", workspace_filter.join(" or "))
        });
    }
    if clauses.is_empty() {
        Ok(None)
    } else {
        Ok(Some(clauses.join(" and ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_search_body_query_only() {
        let body = build_search_body(Some("lakehouse"), None, None, None, None).unwrap();
        assert_eq!(body["search"], "lakehouse");
        assert!(body.get("filter").is_none());
        assert!(body.get("pageSize").is_none());
    }

    #[test]
    fn build_search_body_with_type_filter() {
        let body = build_search_body(
            Some("test"),
            Some("Notebook,Lakehouse"),
            None,
            None,
            Some(5),
        )
        .unwrap();
        assert_eq!(body["search"], "test");
        assert_eq!(body["pageSize"], 5);
        assert_eq!(
            body["filter"],
            "(Type eq 'Notebook' or Type eq 'Lakehouse')"
        );
    }

    #[test]
    fn next_page_token_treats_empty_string_as_done() {
        // The last-page quirk: an empty-string token means "no more pages".
        assert_eq!(
            next_page_token(&serde_json::json!({ "continuationToken": "" })),
            None
        );
    }

    #[test]
    fn next_page_token_returns_present_token() {
        assert_eq!(
            next_page_token(&serde_json::json!({ "continuationToken": "abc123" })),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn next_page_token_none_when_absent() {
        assert_eq!(next_page_token(&serde_json::json!({ "value": [] })), None);
    }

    #[test]
    fn build_search_body_single_type_no_parens() {
        let body = build_search_body(None, Some("Lakehouse"), None, None, None).unwrap();
        assert_eq!(body["filter"], "Type eq 'Lakehouse'");
    }

    #[test]
    fn build_search_body_with_exclude_type() {
        let body = build_search_body(None, None, Some("Dashboard"), None, None).unwrap();
        assert_eq!(body["filter"], "Type ne 'Dashboard'");
    }

    #[test]
    fn build_search_body_both_filters() {
        let body = build_search_body(
            Some("sales"),
            Some("Notebook"),
            Some("Lakehouse"),
            None,
            Some(20),
        )
        .unwrap();
        assert_eq!(body["search"], "sales");
        assert_eq!(body["pageSize"], 20);
        assert_eq!(body["filter"], "Type eq 'Notebook' and Type ne 'Lakehouse'");
    }

    #[test]
    fn build_filter_none_when_empty() {
        assert!(build_filter(None, None, None).unwrap().is_none());
        assert!(build_filter(Some(""), Some("  "), None).unwrap().is_none());
    }

    #[test]
    fn build_search_body_with_workspace_filter() {
        let body = build_search_body(
            Some("revenue"),
            Some("Report,SemanticModel"),
            None,
            Some("7f2c8a91-3b4d-4e5f-a6b7-c8d9e0f1a2b3"),
            Some(2),
        )
        .unwrap();
        assert_eq!(
            body["filter"],
            "(Type eq 'Report' or Type eq 'SemanticModel') and WorkspaceId eq '7f2c8a91-3b4d-4e5f-a6b7-c8d9e0f1a2b3'"
        );
    }

    #[test]
    fn workspace_filter_rejects_invalid_uuid() {
        assert!(build_filter(None, None, Some("not-a-guid")).is_err());
    }
}
