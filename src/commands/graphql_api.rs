use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before creating items, run: fabio context schema GraphQLApi\nReturns the definition template with required fields and format."
)]
pub enum GraphqlApiCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List GraphQL APIs in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a GraphQL API
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// GraphQL API ID
        #[arg(long)]
        id: String,
    },
    /// Create a new GraphQL API
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update GraphQL API properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// GraphQL API ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a GraphQL API
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// GraphQL API ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a GraphQL API
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// GraphQL API ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a GraphQL API
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// GraphQL API ID
        #[arg(long)]
        id: String,

        /// GraphQL schema file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// GraphQL schema content (inline)
        #[arg(long)]
        content: Option<String>,
    },

    // ── Query ────────────────────────────────────────────────────────────
    /// Execute a GraphQL query against a GraphQL API
    #[command(display_order = 8)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// GraphQL API ID
        #[arg(long)]
        id: String,

        /// GraphQL query text (use @file.graphql to read from file, or pipe via stdin)
        #[arg(long)]
        gql: Option<String>,

        /// JSON-encoded variables for the query
        #[arg(long)]
        variables: Option<String>,

        /// Operation name (for multi-operation documents)
        #[arg(long)]
        operation_name: Option<String>,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &GraphqlApiCommand) -> Result<()> {
    match command {
        GraphqlApiCommand::List { workspace } => list(cli, client, workspace).await,
        GraphqlApiCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        GraphqlApiCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        GraphqlApiCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            update(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        GraphqlApiCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        GraphqlApiCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        GraphqlApiCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        GraphqlApiCommand::Query {
            workspace,
            id,
            gql,
            variables,
            operation_name,
        } => {
            graphql_query(
                cli,
                client,
                workspace,
                id,
                gql.as_deref(),
                variables.as_deref(),
                operation_name.as_deref(),
            )
            .await
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "graphQLApis",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "graphQLApis", workspace, id).await
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(cli, "graphql-api create", &body) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/graphQLApis"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "graphql-api create", "Member"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio graphql-api update --workspace <WS> --id <ID> --name \"New Name\""
                .to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::from(n);
    }
    if let Some(d) = description {
        body["description"] = Value::from(d);
    }

    if output::dry_run_guard(cli, "graphql-api update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/graphQLApis/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "graphql-api update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    crate::commands::crud::delete(
        cli,
        client,
        "graphql-api",
        "graphQLApis",
        "Member",
        workspace,
        id,
        hard_delete,
    )
    .await
}

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    crate::commands::crud::get_definition(
        cli,
        client,
        "graphql-api",
        "graphQLApis",
        "Contributor",
        workspace,
        id,
        decode,
    )
    .await
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let script = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio graphql-api update-definition --workspace <WS> --id <ID> --file schema.graphql".to_string(),
            ).into());
        }
    };

    // Accept EITHER a full definition envelope (JSON with definition.parts —
    // e.g. a graphql-definition.json datasource binding, needed to wire the API
    // to a SQL source) OR a raw GraphQL SDL string (wrapped as schema.graphql).
    let body = crate::definition_spec::build_update_definition_body(&script, "schema.graphql");

    if output::dry_run_guard(
        cli,
        "graphql-api update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": script.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/graphQLApis/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "graphql-api update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Query ───────────────────────────────────────────────────────────────────

async fn graphql_query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    gql: Option<&str>,
    variables: Option<&str>,
    operation_name: Option<&str>,
) -> Result<()> {
    // Resolve GraphQL query text: --gql flag, @file prefix, or stdin.
    let query_text = crate::commands::query_input::resolve_query_input(
        gql,
        "GraphQL",
        "--gql",
        "Example: fabio graphql-api query --workspace <WS> --id <ID> --gql \"{ __schema { types { name } } }\"",
    )?;

    // Parse variables if provided
    let variables_value: Value = match variables {
        Some(v) => serde_json::from_str(v).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in --variables: {e}"),
                "Variables must be valid JSON, e.g.: --variables '{\"id\": 1}'".to_string(),
            )
        })?,
        None => Value::Null,
    };

    // Build standard GraphQL request body
    let mut body = serde_json::json!({
        "query": query_text,
    });
    if !variables_value.is_null() {
        body["variables"] = variables_value;
    }
    if let Some(op) = operation_name {
        body["operationName"] = Value::from(op);
    }

    // POST to the GraphQL execution endpoint (no LRO, synchronous)
    let data = client
        .post(
            &format!("/workspaces/{workspace}/graphQLApis/{id}/graphql"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "graphql-api query", "Viewer"))?;

    // Check for GraphQL-level errors (status 200 but errors in response)
    if let Some(errors) = data.get("errors") {
        if data.get("data").is_none_or(Value::is_null) {
            // Pure error response — render as error with the first error message
            let message = errors
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("GraphQL query returned errors");
            // Truncate full error details to avoid leaking server-side internals
            let errors_str = errors.to_string();
            let hint = if errors_str.len() > 500 {
                format!(
                    "Errors (truncated): {}...",
                    &errors_str[..errors_str.floor_char_boundary(500)]
                )
            } else {
                format!("Full errors: {errors_str}")
            };
            return Err(
                FabioError::with_hint(ErrorCode::ApiError, message.to_string(), hint).into(),
            );
        }
        // Partial response (data + errors) — render the full response including errors
        output::render_object(cli, &data, "data");
        return Ok(());
    }

    // Unwrap the GraphQL "data" envelope to avoid double-nesting in fabio's output
    let result = data.get("data").unwrap_or(&data);
    output::render_object(cli, result, "data");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_definition_wraps_raw_sdl_as_schema_graphql() {
        // A raw GraphQL SDL string is wrapped as a single schema.graphql part.
        let body = crate::definition_spec::build_update_definition_body(
            "type Query { hello: String }",
            "schema.graphql",
        );
        let parts = body["definition"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["path"], "schema.graphql");
    }

    #[test]
    fn update_definition_passes_through_datasource_binding_envelope() {
        // A full envelope (graphql-definition.json datasource binding, needed to
        // wire the API to a SQL source) is passed through verbatim.
        let env = r#"{"definition":{"parts":[{"path":"graphql-definition.json","payload":"e30=","payloadType":"InlineBase64"}]}}"#;
        let body = crate::definition_spec::build_update_definition_body(env, "schema.graphql");
        let parts = body["definition"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["path"], "graphql-definition.json");
    }

    #[test]
    fn test_graphql_body_construction_basic() {
        let body = serde_json::json!({
            "query": "{ users { id name } }",
        });
        assert_eq!(body["query"], "{ users { id name } }");
        assert!(body.get("variables").is_none());
        assert!(body.get("operationName").is_none());
    }

    #[test]
    fn test_graphql_body_construction_with_variables() {
        let mut body = serde_json::json!({
            "query": "query GetUser($id: ID!) { user(id: $id) { name } }",
        });
        let vars: Value = serde_json::from_str(r#"{"id": "123"}"#).unwrap();
        body["variables"] = vars;
        body["operationName"] = Value::from("GetUser");

        assert_eq!(
            body["query"],
            "query GetUser($id: ID!) { user(id: $id) { name } }"
        );
        assert_eq!(body["variables"]["id"], "123");
        assert_eq!(body["operationName"], "GetUser");
    }

    #[test]
    fn test_graphql_body_construction_variables_null_when_omitted() {
        // When variables is None, the body should NOT include variables key
        let mut body = serde_json::json!({"query": "{ users { id } }"});
        let variables_value: Value = Value::Null;
        if !variables_value.is_null() {
            body["variables"] = variables_value;
        }
        assert!(body.get("variables").is_none());
    }

    #[test]
    fn test_graphql_body_construction_variables_object() {
        let mut body =
            serde_json::json!({"query": "query($limit: Int) { users(first: $limit) { id } }"});
        let vars: Value = serde_json::from_str(r#"{"limit": 10}"#).unwrap();
        if !vars.is_null() {
            body["variables"] = vars;
        }
        assert_eq!(body["variables"]["limit"], 10);
    }

    #[test]
    fn test_graphql_body_construction_variables_nested() {
        let vars: Value =
            serde_json::from_str(r#"{"filter": {"category": {"eq": "Electronics"}}, "first": 5}"#)
                .unwrap();
        let mut body = serde_json::json!({"query": "query($filter: FilterInput, $first: Int) { products(filter: $filter, first: $first) { items { id } } }"});
        body["variables"] = vars;
        assert_eq!(body["variables"]["filter"]["category"]["eq"], "Electronics");
        assert_eq!(body["variables"]["first"], 5);
    }

    #[test]
    fn test_graphql_body_operation_name_omitted_when_none() {
        let body = serde_json::json!({"query": "{ users { id } }"});
        assert!(body.get("operationName").is_none());
    }

    #[test]
    fn test_graphql_error_response_pure_error() {
        let response = serde_json::json!({
            "errors": [
                {"message": "Cannot query field 'foo' on type 'Query'.", "locations": [{"line": 1, "column": 3}]}
            ]
        });
        let errors = response.get("errors").unwrap();
        let message = errors
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .unwrap();
        assert_eq!(message, "Cannot query field 'foo' on type 'Query'.");
        // No data field
        assert!(response.get("data").is_none());
    }

    #[test]
    fn test_graphql_error_response_pure_error_with_null_data() {
        // Some GraphQL servers return {"data": null, "errors": [...]}
        let response = serde_json::json!({
            "data": null,
            "errors": [{"message": "Internal server error"}]
        });
        assert!(response.get("data").is_some());
        assert!(response["data"].is_null());
        assert!(response.get("errors").is_some());
        // This should be treated as a pure error (data is null)
    }

    #[test]
    fn test_graphql_error_response_partial() {
        // GraphQL allows partial data with errors
        let response = serde_json::json!({
            "data": {"users": [{"id": "1"}]},
            "errors": [{"message": "Unauthorized field 'email'"}]
        });
        // Has both data and errors — should still render data
        assert!(response.get("data").is_some());
        assert!(!response["data"].is_null());
        assert!(response.get("errors").is_some());
    }

    #[test]
    fn test_graphql_error_response_multiple_errors() {
        let response = serde_json::json!({
            "errors": [
                {"message": "Field 'a' not found", "locations": [{"line": 1, "column": 3}]},
                {"message": "Field 'b' not found", "locations": [{"line": 1, "column": 10}]}
            ]
        });
        let errors = response["errors"].as_array().unwrap();
        assert_eq!(errors.len(), 2);
        // First error message is used for CLI error
        assert_eq!(errors[0]["message"], "Field 'a' not found");
    }

    #[test]
    fn test_graphql_response_data_unwrap() {
        // Normal success response — data should be unwrapped
        let response = serde_json::json!({
            "data": {"customers": {"items": [{"id": 1}, {"id": 2}]}}
        });
        let result = response.get("data").unwrap_or(&response);
        assert!(result.get("customers").is_some());
        assert_eq!(result["customers"]["items"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_graphql_response_no_data_key() {
        // Edge case: response doesn't have "data" key at all (unexpected)
        let response = serde_json::json!({"unexpected": "format"});
        let result = response.get("data").unwrap_or(&response);
        // Falls back to original response
        assert_eq!(result["unexpected"], "format");
    }

    #[test]
    fn test_graphql_introspection_query() {
        let query = "{ __schema { queryType { name } types { name kind } } }";
        let body = serde_json::json!({"query": query});
        assert!(body["query"].as_str().unwrap().contains("__schema"));
    }

    #[test]
    fn test_graphql_variables_invalid_json() {
        let bad_json = "not valid json";
        let result: Result<Value, _> = serde_json::from_str(bad_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_graphql_variables_empty_object() {
        let vars: Value = serde_json::from_str("{}").unwrap();
        assert!(vars.is_object());
        assert!(vars.as_object().unwrap().is_empty());
        // Empty object is NOT null, so it should be included
        assert!(!vars.is_null());
    }

    #[test]
    fn test_graphql_file_prefix_detection() {
        assert!("@query.graphql".starts_with('@'));
        assert!("@/tmp/my query.graphql".starts_with('@'));
        assert!(!"{ query }".starts_with('@'));
        assert!(!"query { users { id } }".starts_with('@'));
    }

    #[test]
    fn test_graphql_file_path_extraction() {
        let input = "@/tmp/my-query.graphql";
        let file_path = &input[1..];
        assert_eq!(file_path, "/tmp/my-query.graphql");
    }

    #[test]
    fn test_graphql_multiline_query() {
        let query = r"
            query GetProducts($category: String!) {
                products(filter: { category: { eq: $category } }) {
                    items {
                        product_id
                        category
                        price
                    }
                }
            }
        ";
        let body = serde_json::json!({"query": query});
        assert!(body["query"].as_str().unwrap().contains("GetProducts"));
        assert!(body["query"].as_str().unwrap().contains("$category"));
    }

    #[test]
    fn test_graphql_mutation_query_format() {
        // Mutations use the same body format as queries
        let mutation =
            r"mutation CreateUser($name: String!) { createUser(name: $name) { id name } }";
        let vars: Value = serde_json::from_str(r#"{"name": "Alice"}"#).unwrap();
        let mut body = serde_json::json!({"query": mutation});
        body["variables"] = vars;
        assert!(body["query"].as_str().unwrap().starts_with("mutation"));
        assert_eq!(body["variables"]["name"], "Alice");
    }

    #[test]
    fn test_graphql_error_with_extensions() {
        // Fabric returns errors with extensions containing code and other metadata
        let response = serde_json::json!({
            "errors": [{
                "message": "Introspection is not allowed for the current request.",
                "extensions": {"code": "HC0046", "field": "__schema"},
                "locations": [{"line": 1, "column": 3}]
            }]
        });
        let errors = response["errors"].as_array().unwrap();
        assert_eq!(errors[0]["extensions"]["code"], "HC0046");
        assert_eq!(
            errors[0]["message"],
            "Introspection is not allowed for the current request."
        );
    }
}
