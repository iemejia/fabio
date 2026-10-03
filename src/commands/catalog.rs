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

        /// Filter by containing workspace ID. Comma-separated for up to 12 workspaces.
        #[arg(long)]
        workspace_id: Option<String>,

        /// Server page size (1-1000; defaults to 50)
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=1000))]
        top: Option<u16>,

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
    top: Option<u16>,
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
                if query.is_none()
                    && item_type.is_none()
                    && exclude_type.is_none()
                    && workspace_id.is_none()
                {
                    return Err(FabioError::with_hint(
                        ErrorCode::InvalidInput,
                        "At least one of --query, --type, --exclude-type, --workspace-id, --file, or --content must be provided"
                            .to_string(),
                        "Example: fabio catalog search --query \"my lakehouse\" --workspace-id <WORKSPACE_ID> --top 10"
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
    top: Option<u16>,
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
    if let Some(filter) = build_catalog_filter(item_type, exclude_type, workspace_id)? {
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
fn build_catalog_filter(
    item_type: Option<&str>,
    exclude_type: Option<&str>,
    workspace_id: Option<&str>,
) -> Result<Option<String>> {
    let include: Vec<String> = item_type
        .into_iter()
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let exclude: Vec<String> = exclude_type
        .into_iter()
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if include.len() + exclude.len() > 500 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Catalog filters support at most 500 Type values",
            "Reduce the comma-separated values passed to --type and --exclude-type.",
        )
        .into());
    }
    if let Some(value) = include
        .iter()
        .chain(&exclude)
        .find(|value| value.len() > 50)
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Catalog Type filter value exceeds 50 characters: '{value}'"),
            "Use Fabric item type names of at most 50 characters.",
        )
        .into());
    }

    let include = include
        .into_iter()
        .map(|value| format!("Type eq '{value}'"))
        .collect::<Vec<_>>();
    let exclude = exclude
        .into_iter()
        .map(|value| format!("Type ne '{value}'"))
        .collect::<Vec<_>>();
    let workspaces = workspace_id
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if workspaces.len() > 12 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Catalog filters support at most 12 WorkspaceId values",
            "Reduce the comma-separated workspace IDs passed to --workspace-id.",
        )
        .into());
    }
    for id in &workspaces {
        uuid::Uuid::parse_str(id).map_err(|_| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid workspace ID in --workspace-id: '{id}'"),
                "Provide comma-separated workspace GUIDs.",
            )
        })?;
    }

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
    if !workspaces.is_empty() {
        let workspace_clauses = workspaces
            .into_iter()
            .map(|id| format!("WorkspaceId eq '{id}'"))
            .collect::<Vec<_>>();
        clauses.push(if workspace_clauses.len() == 1 {
            workspace_clauses.into_iter().next().unwrap_or_default()
        } else {
            format!("({})", workspace_clauses.join(" or "))
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
    fn build_catalog_filter_none_when_empty() {
        assert!(build_catalog_filter(None, None, None).unwrap().is_none());
        assert!(
            build_catalog_filter(Some(""), Some("  "), None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn build_catalog_filter_supports_workspace_entries() {
        let workspace = "7f2c8a91-3b4d-4e5f-a6b7-c8d9e0f1a2b3";
        let filter = build_catalog_filter(Some("Report,Workspace"), None, Some(workspace)).unwrap();
        assert_eq!(
            filter.as_deref(),
            Some(
                "(Type eq 'Report' or Type eq 'Workspace') and WorkspaceId eq '7f2c8a91-3b4d-4e5f-a6b7-c8d9e0f1a2b3'"
            )
        );
    }

    #[test]
    fn build_catalog_filter_rejects_invalid_workspace_id() {
        assert!(build_catalog_filter(None, None, Some("not-a-guid")).is_err());
    }
}
