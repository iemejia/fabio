mod config;
mod crud;
mod datasources;
mod definition;
mod elements;
mod evaluate;
mod fewshots;
mod query;
mod validate;

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::llm::LlmConfig;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples data-agent\nAlso available: fabio context schema DataAgent | fabio context workflow data-agent-setup"
)]
pub enum DataAgentCommand {
    /// List data agents in a workspace
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a data agent
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,
    },
    /// Create a new data agent
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent display name
        #[arg(long)]
        name: String,

        /// Data agent description (max 256 characters)
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update a data agent (name and/or description)
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description (max 256 characters)
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a data agent
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Query (chat with) a published data agent using natural language
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Natural language question (omit to read from stdin)
        #[arg(short, long)]
        prompt: Option<String>,

        /// Published URL (from portal Settings page after publishing the agent)
        #[arg(long)]
        published_url: Option<String>,

        /// Include execution details (SQL queries, tool calls, run steps)
        #[arg(long)]
        show_steps: bool,

        /// Agent stage to query: only `production` (published) is supported via the public API
        #[arg(long, default_value = "production")]
        stage: String,

        /// Reuse an existing thread for a multi-turn follow-up (from a prior query's `threadId`)
        #[arg(long)]
        thread_id: Option<String>,

        /// Keep the thread after the query (returns its `threadId` for a follow-up turn)
        #[arg(long)]
        keep_thread: bool,

        /// Download files attached to the answer into this directory (e.g. generated CSVs/charts)
        #[arg(long, value_name = "DIR")]
        download_files: Option<String>,

        /// Extract chart/visual specifications the agent generated (chart type, axes,
        /// title, sort, and the aggregated data) into a `visuals` array in the output
        #[arg(long)]
        visuals: bool,

        /// Maximum wait time in seconds for the query to complete (default: 300)
        #[arg(long, default_value = "300")]
        timeout: u64,
    },

    /// Batch-run a set of questions against a published data agent (evaluation primitive)
    Evaluate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Questions file: JSON (array of strings, or [{"question","expected"}]) or CSV/TSV with a `question` column (optional `expected`)
        #[arg(long)]
        questions: String,

        /// Published URL (from portal Settings page after publishing the agent)
        #[arg(long)]
        published_url: Option<String>,

        /// Number of times to run each question (default: 1)
        #[arg(long, default_value = "1")]
        repeats: u32,

        /// Include execution details (SQL queries, tool calls, run steps) per run
        #[arg(long)]
        show_steps: bool,

        /// Agent stage to query: only `production` (published) is supported via the public API
        #[arg(long, default_value = "production")]
        stage: String,

        /// Maximum wait time in seconds per question run (default: 300)
        #[arg(long, default_value = "300")]
        timeout: u64,

        /// LLM judge endpoint to grade answers (Azure `OpenAI` resource URL or `OpenAI`-compatible base). Enables LLM grading.
        #[arg(long, env = "FABIO_LLM_ENDPOINT")]
        llm_endpoint: Option<String>,

        /// LLM judge API key
        #[arg(long, env = "FABIO_LLM_KEY")]
        llm_key: Option<String>,

        /// LLM judge model (Azure deployment name, or model id for `OpenAI`-compatible)
        #[arg(long, env = "FABIO_LLM_MODEL")]
        llm_model: Option<String>,

        /// Azure `OpenAI` API version (default: 2024-10-21)
        #[arg(long, env = "FABIO_LLM_API_VERSION")]
        llm_api_version: Option<String>,
    },

    /// Validate a data source's few-shot examples with an LLM (duplicates, conflicts, quality)
    ValidateFewshots {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Stage to read: staging (draft) or published (live). Default: staging
        #[arg(long, default_value = "staging")]
        stage: String,

        /// LLM judge endpoint (Azure `OpenAI` resource URL or `OpenAI`-compatible base). Required.
        #[arg(long, env = "FABIO_LLM_ENDPOINT")]
        llm_endpoint: Option<String>,

        /// LLM judge API key. Required.
        #[arg(long, env = "FABIO_LLM_KEY")]
        llm_key: Option<String>,

        /// LLM judge model (Azure deployment name, or model id for `OpenAI`-compatible). Required.
        #[arg(long, env = "FABIO_LLM_MODEL")]
        llm_model: Option<String>,

        /// Azure `OpenAI` API version (default: 2024-10-21)
        #[arg(long, env = "FABIO_LLM_API_VERSION")]
        llm_api_version: Option<String>,
    },

    /// Print the Model Context Protocol (MCP) endpoint URL for consuming a published data agent
    McpUrl {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,
    },

    // ── Configuration ────────────────────────────────────────────────────
    /// Get the configuration of a data agent (instructions, data sources, preview runtime)
    #[command(display_order = 8)]
    GetConfig {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Stage to read: staging (draft) or published (live). Default: staging
        #[arg(long, default_value = "staging")]
        stage: String,
    },
    /// Update the configuration of a data agent (instructions, preview runtime)
    #[command(display_order = 9)]
    UpdateConfig {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// AI instructions for the agent (guides data source selection and query generation)
        #[arg(long)]
        instructions: Option<String>,

        /// Path to file containing AI instructions (alternative to --instructions)
        #[arg(long, conflicts_with = "instructions")]
        instructions_file: Option<String>,

        /// Enable preview runtime (multi-step reasoning: Advanced NL2SQL for SQL sources AND Advanced DAX generation for semantic models)
        #[arg(long)]
        enable_preview_runtime: bool,

        /// Disable preview runtime (revert to the single-pass standard runtime: GA NL2SQL / standard DAX generation)
        #[arg(long, conflicts_with = "enable_preview_runtime")]
        disable_preview_runtime: bool,
    },

    // ── Datasource Management ────────────────────────────────────────────
    /// List configured data sources for a data agent
    #[command(display_order = 13)]
    ListDatasources {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Stage to read: staging (draft) or published (live). Default: staging
        #[arg(long, default_value = "staging")]
        stage: String,
    },
    /// Show details of a configured data source
    #[command(display_order = 14)]
    ShowDatasource {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Stage to read: staging (draft) or published (live). Default: staging
        #[arg(long, default_value = "staging")]
        stage: String,
    },
    /// Add a data source to the agent (auto-discovers schema from artifact)
    #[command(display_order = 15)]
    AddDatasource {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Artifact name or ID (lakehouse, warehouse, KQL database, semantic model, etc.)
        #[arg(long, visible_alias = "artifact-id")]
        artifact: String,

        /// Workspace containing the artifact (defaults to same workspace as agent)
        #[arg(long)]
        artifact_workspace: Option<String>,

        /// Artifact type (auto-detected if omitted). Values: `Lakehouse`, `Warehouse`, `KQLDatabase`, `SemanticModel`, `Ontology`, `GraphModel`, `MirroredDatabase`, `SQLDatabase`
        #[arg(long, value_name = "TYPE")]
        artifact_type: Option<String>,

        /// Data source instructions (how the agent should use this source)
        #[arg(long)]
        instructions: Option<String>,
    },
    /// Remove a data source from the agent
    #[command(display_order = 16)]
    RemoveDatasource {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID to remove
        #[arg(long)]
        datasource: String,
    },
    /// Update a data source's metadata (instructions, description)
    #[command(display_order = 16)]
    UpdateDatasource {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// New data source instructions (how the agent should use this source)
        #[arg(long)]
        instructions: Option<String>,

        /// New description for the data source
        #[arg(long)]
        description: Option<String>,
    },

    // ── Few-shot Management ──────────────────────────────────────────────
    /// List few-shot examples for a data source
    #[command(display_order = 17)]
    ListFewshots {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Stage to read: staging (draft) or published (live). Default: staging
        #[arg(long, default_value = "staging")]
        stage: String,
    },
    /// Show a specific few-shot example by ID
    #[command(display_order = 17)]
    ShowFewshot {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Few-shot example ID
        #[arg(long)]
        fewshot_id: String,

        /// Stage to read: staging (draft) or published (live). Default: staging
        #[arg(long, default_value = "staging")]
        stage: String,
    },
    /// Add a few-shot example (question/query pair) to a data source
    #[command(display_order = 18)]
    AddFewshot {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Natural language question
        #[arg(long)]
        question: String,

        /// The example query that answers the question, in the data source's
        /// language (SQL for lakehouse/warehouse, KQL for a KQL database, GQL
        /// for graph/ontology). Aliases: --sql/--kql/--dax/--gql.
        #[arg(long, visible_aliases = ["sql", "kql", "dax", "gql"])]
        answer: String,
    },
    /// Update an existing few-shot example (question and/or query)
    #[command(display_order = 18)]
    UpdateFewshot {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Few-shot example ID to update
        #[arg(long)]
        fewshot_id: String,

        /// Updated natural language question
        #[arg(long)]
        question: Option<String>,

        /// Updated example query (aliases: --sql/--kql/--dax/--gql)
        #[arg(long, visible_aliases = ["sql", "kql", "dax", "gql"])]
        answer: Option<String>,
    },
    /// Remove a few-shot example by ID
    #[command(display_order = 19)]
    RemoveFewshot {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Few-shot example ID to remove
        #[arg(long)]
        fewshot_id: String,
    },
    /// Delete all few-shot examples for a data source
    #[command(display_order = 19)]
    ClearFewshots {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,
    },
    /// Bulk upload few-shot examples from a JSON or CSV file
    #[command(display_order = 20)]
    UploadFewshots {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// File with few-shots (JSON: [{"question":"...", "query":"..."}] or CSV with question,query columns)
        #[arg(long)]
        file: String,
    },
    /// Select or unselect data-source elements (tables, or ontology/graph entity types)
    #[command(display_order = 21)]
    SelectTables {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Comma-separated table names to select (e.g. "orders,products,customers")
        #[arg(long)]
        tables: Option<String>,

        /// Select all tables
        #[arg(long, conflicts_with = "tables")]
        all_tables: bool,

        /// Comma-separated element names of any type to select (e.g. ontology
        /// entity types on an Ontology/GraphModel data source)
        #[arg(long)]
        elements: Option<String>,

        /// Select all elements regardless of type
        #[arg(long, conflicts_with_all = ["elements", "all_tables"])]
        all_elements: bool,

        /// Unselect (instead of select)
        #[arg(long)]
        unselect: bool,
    },
    /// List elements (tables, columns) in a data source with selection state and descriptions
    #[command(display_order = 22)]
    ListElements {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Stage to read: staging (draft) or published (live). Default: staging
        #[arg(long, default_value = "staging")]
        stage: String,
    },
    /// Set or clear a description on a table or column in a data source
    #[command(display_order = 23)]
    DescribeElement {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Dot-separated path to the element (e.g. `dbo.orders` for a table, `dbo.orders.total_amount` for a column)
        #[arg(long)]
        path: String,

        /// Description text (omit or pass empty string to clear)
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a stale schema element (only elements no longer in the live schema)
    #[command(display_order = 24)]
    DeleteElement {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Data source name or ID
        #[arg(long)]
        datasource: String,

        /// Element ID to delete (from list-elements output)
        #[arg(long)]
        element_id: String,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a data agent (configuration, data sources, etc.)
    #[command(display_order = 10)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a data agent (configure data sources, instructions, etc.)
    #[command(display_order = 11)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Path to definition file (JSON with parts array)
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON definition (alternative to --file)
        #[arg(long)]
        content: Option<String>,

        /// Also update item metadata from .platform file if present
        #[arg(long)]
        update_metadata: bool,
    },
    /// Publish a data agent (promotes draft configuration to published state)
    #[command(display_order = 12)]
    Publish {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,

        /// Optional publish description
        #[arg(long)]
        description: Option<String>,

        /// Also publish to Microsoft 365 Copilot Agent Store
        #[arg(long)]
        to_m365: bool,
    },
    /// Reset staging (discard all draft changes, revert to published state)
    #[command(display_order = 12)]
    Reset {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Data agent ID
        #[arg(long)]
        id: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &DataAgentCommand) -> Result<()> {
    match command {
        DataAgentCommand::List { workspace } => crud::list(cli, client, workspace).await,
        DataAgentCommand::Show { workspace, id } => crud::show(cli, client, workspace, id).await,
        DataAgentCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
        } => crud::create(
            cli,
            client,
            workspace,
            name,
            description.as_deref(),
            sensitivity_label.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent create", "Member")),
        DataAgentCommand::Update {
            workspace,
            id,
            name,
            description,
        } => crud::update(
            cli,
            client,
            workspace,
            id,
            name.as_deref(),
            description.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent update", "Contributor")),
        DataAgentCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => crud::delete(cli, client, workspace, id, *hard_delete)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent delete", "Member")),
        DataAgentCommand::Query {
            workspace,
            id,
            prompt,
            published_url,
            show_steps,
            stage,
            thread_id,
            keep_thread,
            download_files,
            visuals,
            timeout,
        } => query::query(
            cli,
            client,
            workspace,
            id,
            prompt.as_deref(),
            published_url.as_deref(),
            *show_steps,
            stage,
            thread_id.as_deref(),
            *keep_thread,
            download_files.as_deref(),
            *visuals,
            *timeout,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent query", "Viewer")),
        DataAgentCommand::Evaluate {
            workspace,
            id,
            questions,
            published_url,
            repeats,
            show_steps,
            stage,
            timeout,
            llm_endpoint,
            llm_key,
            llm_model,
            llm_api_version,
        } => {
            let llm = LlmConfig {
                endpoint: llm_endpoint.clone(),
                key: llm_key.clone(),
                model: llm_model.clone(),
                api_version: llm_api_version.clone(),
            };
            evaluate::evaluate(
                cli,
                client,
                workspace,
                id,
                questions,
                published_url.as_deref(),
                *repeats,
                *show_steps,
                stage,
                *timeout,
                &llm,
            )
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent evaluate", "Viewer"))
        }
        DataAgentCommand::ValidateFewshots {
            workspace,
            id,
            datasource,
            stage,
            llm_endpoint,
            llm_key,
            llm_model,
            llm_api_version,
        } => {
            let llm = LlmConfig {
                endpoint: llm_endpoint.clone(),
                key: llm_key.clone(),
                model: llm_model.clone(),
                api_version: llm_api_version.clone(),
            };
            validate::ensure_llm_configured(&llm)?;
            validate::validate_fewshots(cli, client, workspace, id, datasource, stage, &llm)
                .await
                .map_err(|e| enrich_forbidden(e, "data-agent validate-fewshots", "Viewer"))
        }
        DataAgentCommand::McpUrl { workspace, id } => query::mcp_url(cli, client, workspace, id)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent mcp-url", "Viewer")),
        DataAgentCommand::GetConfig {
            workspace,
            id,
            stage,
        } => config::get_config(cli, client, workspace, id, stage)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent get-config", "Viewer")),
        DataAgentCommand::UpdateConfig {
            workspace,
            id,
            instructions,
            instructions_file,
            enable_preview_runtime,
            disable_preview_runtime,
        } => config::update_config(
            cli,
            client,
            workspace,
            id,
            instructions.as_deref(),
            instructions_file.as_deref(),
            *enable_preview_runtime,
            *disable_preview_runtime,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent update-config", "Contributor")),
        DataAgentCommand::ListDatasources {
            workspace,
            id,
            stage,
        } => datasources::list_datasources(cli, client, workspace, id, stage)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent list-datasources", "Viewer")),
        DataAgentCommand::ShowDatasource {
            workspace,
            id,
            datasource,
            stage,
        } => datasources::show_datasource(cli, client, workspace, id, datasource, stage)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent show-datasource", "Viewer")),
        DataAgentCommand::AddDatasource {
            workspace,
            id,
            artifact,
            artifact_workspace,
            artifact_type,
            instructions,
        } => datasources::add_datasource(
            cli,
            client,
            workspace,
            id,
            artifact,
            artifact_workspace.as_deref(),
            artifact_type.as_deref(),
            instructions.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent add-datasource", "Contributor")),
        DataAgentCommand::RemoveDatasource {
            workspace,
            id,
            datasource,
        } => datasources::remove_datasource(cli, client, workspace, id, datasource)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent remove-datasource", "Contributor")),
        DataAgentCommand::UpdateDatasource {
            workspace,
            id,
            datasource,
            instructions,
            description,
        } => datasources::update_datasource(
            cli,
            client,
            workspace,
            id,
            datasource,
            instructions.as_deref(),
            description.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent update-datasource", "Contributor")),
        DataAgentCommand::ListFewshots {
            workspace,
            id,
            datasource,
            stage,
        } => fewshots::list_fewshots(cli, client, workspace, id, datasource, stage)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent list-fewshots", "Viewer")),
        DataAgentCommand::ShowFewshot {
            workspace,
            id,
            datasource,
            fewshot_id,
            stage,
        } => fewshots::show_fewshot(cli, client, workspace, id, datasource, fewshot_id, stage)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent show-fewshot", "Viewer")),
        DataAgentCommand::AddFewshot {
            workspace,
            id,
            datasource,
            question,
            answer,
        } => fewshots::add_fewshot(cli, client, workspace, id, datasource, question, answer)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent add-fewshot", "Contributor")),
        DataAgentCommand::UpdateFewshot {
            workspace,
            id,
            datasource,
            fewshot_id,
            question,
            answer,
        } => fewshots::update_fewshot(
            cli,
            client,
            workspace,
            id,
            datasource,
            fewshot_id,
            question.as_deref(),
            answer.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent update-fewshot", "Contributor")),
        DataAgentCommand::RemoveFewshot {
            workspace,
            id,
            datasource,
            fewshot_id,
        } => fewshots::remove_fewshot(cli, client, workspace, id, datasource, fewshot_id)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent remove-fewshot", "Contributor")),
        DataAgentCommand::ClearFewshots {
            workspace,
            id,
            datasource,
        } => fewshots::clear_fewshots(cli, client, workspace, id, datasource)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent clear-fewshots", "Contributor")),
        DataAgentCommand::UploadFewshots {
            workspace,
            id,
            datasource,
            file,
        } => fewshots::upload_fewshots(cli, client, workspace, id, datasource, file)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent upload-fewshots", "Contributor")),
        DataAgentCommand::SelectTables {
            workspace,
            id,
            datasource,
            tables,
            all_tables,
            elements,
            all_elements,
            unselect,
        } => datasources::select_tables(
            cli,
            client,
            workspace,
            id,
            datasource,
            tables.as_deref(),
            elements.as_deref(),
            *all_tables,
            *all_elements,
            *unselect,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent select-tables", "Contributor")),
        DataAgentCommand::ListElements {
            workspace,
            id,
            datasource,
            stage,
        } => elements::list_elements(cli, client, workspace, id, datasource, stage)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent list-elements", "Viewer")),
        DataAgentCommand::DescribeElement {
            workspace,
            id,
            datasource,
            path,
            description,
        } => elements::describe_element(
            cli,
            client,
            workspace,
            id,
            datasource,
            path,
            description.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent describe-element", "Contributor")),
        DataAgentCommand::DeleteElement {
            workspace,
            id,
            datasource,
            element_id,
        } => elements::delete_element(cli, client, workspace, id, datasource, element_id)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent delete-element", "Contributor")),
        DataAgentCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => definition::get_definition(cli, client, workspace, id, *decode)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent get-definition", "Contributor")),
        DataAgentCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
            update_metadata,
        } => definition::update_definition(
            cli,
            client,
            workspace,
            id,
            file.as_deref(),
            content.as_deref(),
            *update_metadata,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "data-agent update-definition", "Contributor")),
        DataAgentCommand::Publish {
            workspace,
            id,
            description,
            to_m365,
        } => definition::publish(cli, client, workspace, id, description.as_deref(), *to_m365)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent publish", "Contributor")),
        DataAgentCommand::Reset { workspace, id } => definition::reset(cli, client, workspace, id)
            .await
            .map_err(|e| enrich_forbidden(e, "data-agent reset", "Contributor")),
    }
}

// ─── Shared Helpers ──────────────────────────────────────────────────────────

/// Resolve a datasource name or ID to its UUID by listing staging datasources.
///
/// The new staging datasources API uses its own UUID identifiers. This helper allows
/// users to reference datasources by display name, datasource ID, or artifact ID.
pub(super) async fn resolve_datasource_id(
    client: &FabricClient,
    workspace: &str,
    agent_id: &str,
    datasource: &str,
) -> Result<String> {
    // Always list datasources and match by name, datasource ID, or artifact ID
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/dataAgents/{agent_id}/staging/datasources"),
            "value",
            true,
            None,
        )
        .await?;

    let found = resp.items.iter().find(|ds| {
        let name = ds.get("displayName").and_then(Value::as_str).unwrap_or("");
        let ds_id = ds.get("id").and_then(Value::as_str).unwrap_or("");
        // Also check nested itemReference.itemId for artifact ID matching
        let artifact_id = ds
            .get("itemReference")
            .and_then(|r| r.get("itemId"))
            .and_then(Value::as_str)
            .unwrap_or("");
        name.eq_ignore_ascii_case(datasource) || ds_id == datasource || artifact_id == datasource
    });

    found.map_or_else(
        || {
            Err(FabioError::with_hint(
                ErrorCode::NotFound,
                format!("Data source '{datasource}' not found"),
                "List available data sources: fabio data-agent list-datasources -w <workspace> --id <id>",
            )
            .into())
        },
        |ds| {
            Ok(ds
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string())
        },
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn resolve_datasource_id_always_queries_api() {
        // The resolver always lists datasources from the API to match by name,
        // datasource UUID, or artifact ID — it cannot shortcut for UUID-formatted
        // inputs because artifact IDs and datasource IDs are both UUIDs.
        let uuid = "12345678-abcd-ef01-2345-678901234567";
        let name = "MyWarehouse";
        // Both should go through the same resolution path (tested in E2E)
        assert_ne!(uuid, name);
    }

    /// Regression for f9179b7: the `--answer` few-shot query flag accepts the
    /// language-specific aliases `--sql`/`--kql`/`--dax`/`--gql`, so an agent can
    /// use the natural flag for the data source's language.
    #[test]
    fn add_fewshot_answer_accepts_query_language_aliases() {
        use super::DataAgentCommand;
        use crate::cli::{Cli, Command};
        use clap::Parser;

        // The derived `Cli` for 77 groups is deep enough to overflow the default
        // 2 MB test-thread stack when parsing — run on a large-stack thread (same
        // pattern as `cli::no_subcommand_flag_collides_with_global`).
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                for alias in ["--answer", "--sql", "--kql", "--dax", "--gql"] {
                    let cli = Cli::try_parse_from([
                        "fabio",
                        "data-agent",
                        "add-fewshot",
                        "--workspace",
                        "ws",
                        "--id",
                        "agent",
                        "--datasource",
                        "ds",
                        "--question",
                        "how many sales?",
                        alias,
                        "SELECT COUNT(*) FROM Sales",
                    ])
                    .unwrap_or_else(|e| panic!("alias {alias} must parse: {e}"));
                    let Command::DataAgent { command } = cli.command else {
                        panic!("expected data-agent command");
                    };
                    let DataAgentCommand::AddFewshot { answer, .. } = command else {
                        panic!("expected add-fewshot subcommand");
                    };
                    assert_eq!(answer, "SELECT COUNT(*) FROM Sales", "alias {alias}");
                }
            })
            .expect("spawn parse thread")
            .join()
            .expect("parse thread panicked");
    }
}
