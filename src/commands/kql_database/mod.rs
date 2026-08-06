use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples kql-database\nAlso available: fabio context schema KQLDatabase | fabio context workflow rti-pipeline"
)]
pub enum KqlDatabaseCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List KQL databases in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a KQL database
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,
    },
    /// Create a new KQL database
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Database display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Parent eventhouse item ID
        #[arg(long)]
        eventhouse_id: String,

        /// Database type: `ReadWrite` or `ReadOnlyFollowing`
        #[arg(long, default_value = "ReadWrite", value_parser = ["ReadWrite", "ReadOnlyFollowing"])]
        database_type: String,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update KQL database properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a KQL database
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    /// Execute a KQL query against a KQL database
    #[command(display_order = 6)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// KQL query text (use @file.kql to read from file, or pipe via stdin)
        #[arg(long)]
        kql: Option<String>,

        /// Override the Kusto query URI (auto-discovered from database properties if omitted)
        #[arg(long)]
        query_uri: Option<String>,
    },

    // ── Schema Discovery ─────────────────────────────────────────────────
    /// List entities (tables, materialized views, external tables, functions) in a database
    #[command(name = "list-entities", display_order = 7)]
    ListEntities {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Filter by entity type: table, materialized-view, external-table, function
        #[arg(long)]
        entity_type: Option<String>,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },
    /// Get schema for all entities in a database
    #[command(display_order = 8)]
    Describe {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },
    /// Get detailed schema for a specific entity (table, view, function)
    #[command(name = "describe-entity", display_order = 9)]
    DescribeEntity {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Entity name (table, materialized view, external table, or function)
        #[arg(long)]
        entity_name: String,

        /// Entity type: table (default), materialized-view, external-table, function
        #[arg(long, default_value = "table")]
        entity_type: String,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },
    /// Sample rows from a table, materialized view, external table, or function
    #[command(display_order = 10)]
    Sample {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Entity name to sample from
        #[arg(long)]
        entity_name: String,

        /// Number of rows to sample (default: 10)
        #[arg(long, default_value = "10")]
        count: u32,

        /// Entity type: table (default), materialized-view, external-table, function
        #[arg(long, default_value = "table")]
        entity_type: String,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },

    /// Ingest data into a KQL table — inline (small) or from OneLake/blob storage (large files)
    #[command(display_order = 11)]
    Ingest {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Target table name
        #[arg(long)]
        table: String,

        /// Inline CSV data to ingest (or use @file to read from file, or pipe via stdin).
        /// Limited to ~4 MB. For larger data use --source-path (OneLake/blob ingestion).
        #[arg(long, conflicts_with = "source_path")]
        data: Option<String>,

        /// Ingest from a storage source instead of inline. Either a full HTTPS URL to a trusted
        /// Microsoft endpoint (`OneLake` / ADLS Gen2) OR, together with --source-lakehouse, a
        /// lakehouse-relative path like `Files/raw/payments.json`. No ~4 MB size limit.
        #[arg(long)]
        source_path: Option<String>,

        /// Source lakehouse item ID. When set, --source-path is treated as a path within this
        /// lakehouse (`Files/...` or `Tables/...`) and resolved to a `OneLake` blob URL.
        #[arg(long, requires = "source_path")]
        source_lakehouse: Option<String>,

        /// Workspace hosting the source lakehouse (defaults to --workspace).
        #[arg(long, requires = "source_lakehouse")]
        source_workspace: Option<String>,

        /// Data format for storage ingestion: `Csv`, `Tsv`, `Json`, `MultiJson`, `Parquet` (default: `Csv`).
        /// Ignored for inline ingestion (always CSV).
        #[arg(long)]
        format: Option<String>,

        /// For Csv/Tsv storage ingestion: skip the first (header) row.
        #[arg(long)]
        ignore_first_record: bool,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },
    /// Show execution plan for a KQL query without running it
    #[command(name = "show-queryplan", display_order = 12)]
    ShowQueryplan {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// KQL query to analyze (use @file.kql to read from file, or pipe via stdin)
        #[arg(long)]
        kql: Option<String>,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },
    /// Run cluster diagnostics (capacity, health, ingestion failures)
    #[command(display_order = 13)]
    Diagnostics {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },
    /// Generate a deeplink URL for a KQL query in Fabric portal or ADX Web Explorer
    #[command(display_order = 14)]
    Deeplink {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// KQL query text to embed in the deeplink
        #[arg(long)]
        kql: String,

        /// Link style: auto (default), fabric, adx
        #[arg(long, default_value = "auto")]
        style: String,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },
    /// Print the remote MCP server URL for consuming this KQL database with AI agents
    #[command(display_order = 19)]
    McpUrl {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,
    },
    /// Retrieve KQL example pairs relevant to a natural-language prompt (via the eventhouse remote MCP server)
    #[command(display_order = 20)]
    Examples {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Natural-language description of the query you want to author
        #[arg(long)]
        prompt: String,

        /// Only fetch general (public, curated) KQL examples
        #[arg(long, conflicts_with = "specific_only")]
        general_only: bool,

        /// Only fetch database-specific (curated/learned) KQL examples
        #[arg(long)]
        specific_only: bool,
    },
    /// Retrieve relevant schema context (with column samples + stats) for a natural-language prompt (via the eventhouse remote MCP server)
    #[command(display_order = 21)]
    SchemaContext {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Natural-language description of the query you want to author
        #[arg(long)]
        prompt: String,
    },

    // ── Query Monitoring ─────────────────────────────────────────────────
    /// Show currently running queries on the KQL database
    #[command(display_order = 15)]
    QueriesRunning {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },
    /// Show the operations journal (completed operations history)
    #[command(display_order = 16)]
    Journal {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },
    /// Show recently completed queries
    #[command(display_order = 17)]
    QueriesCompleted {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Override the Kusto query URI
        #[arg(long)]
        query_uri: Option<String>,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a KQL database (KQL script)
    #[command(name = "get-definition", display_order = 18)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a KQL database
    #[command(name = "update-definition", display_order = 11)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// KQL script file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// KQL script content (inline)
        #[arg(long)]
        content: Option<String>,
    },

    // ── Shortcuts ────────────────────────────────────────────────────────
    /// List shortcuts in a KQL database
    #[command(name = "list-shortcuts", display_order = 10)]
    ListShortcuts {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,
    },
    /// Create a shortcut in a KQL database
    #[command(name = "create-shortcut", display_order = 11)]
    CreateShortcut {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Shortcut name
        #[arg(long)]
        name: String,

        /// JSON file with shortcut configuration
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON shortcut configuration
        #[arg(long)]
        content: Option<String>,
    },
    /// Get a shortcut in a KQL database
    #[command(name = "get-shortcut", display_order = 12)]
    GetShortcut {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Shortcut name
        #[arg(long)]
        shortcut_name: String,
    },
    /// Delete a shortcut in a KQL database
    #[command(name = "delete-shortcut", display_order = 13)]
    DeleteShortcut {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Shortcut name
        #[arg(long)]
        shortcut_name: String,
    },
    /// Bulk-create multiple shortcuts (LRO)
    #[command(name = "bulk-create-shortcuts", display_order = 14)]
    BulkCreateShortcuts {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL database ID
        #[arg(long)]
        id: String,

        /// Path to JSON file with array of shortcut requests
        #[arg(long, group = "input")]
        file: Option<String>,

        /// Inline JSON with array of shortcut requests
        #[arg(long, group = "input")]
        content: Option<String>,

        /// Conflict policy: `Abort`, `GenerateUniqueName`, `CreateOrOverwrite`, `OverwriteOnly`
        #[arg(long = "conflict-policy")]
        conflict_policy: Option<String>,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &KqlDatabaseCommand) -> Result<()> {
    match command {
        KqlDatabaseCommand::List { workspace } => list(cli, client, workspace).await,
        KqlDatabaseCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        KqlDatabaseCommand::Create {
            workspace,
            name,
            description,
            eventhouse_id,
            database_type,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                eventhouse_id,
                database_type,
                sensitivity_label.as_deref(),
            )
            .await
        }
        KqlDatabaseCommand::Update {
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
        KqlDatabaseCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        KqlDatabaseCommand::Query {
            workspace,
            id,
            kql,
            query_uri,
        } => {
            intelligence::query(
                cli,
                client,
                workspace,
                id,
                kql.as_deref(),
                query_uri.as_deref(),
            )
            .await
        }
        KqlDatabaseCommand::ListEntities {
            workspace,
            id,
            entity_type,
            query_uri,
        } => {
            intelligence::list_entities(
                cli,
                client,
                workspace,
                id,
                entity_type.as_deref(),
                query_uri.as_deref(),
            )
            .await
        }
        KqlDatabaseCommand::Describe {
            workspace,
            id,
            query_uri,
        } => intelligence::describe(cli, client, workspace, id, query_uri.as_deref()).await,
        KqlDatabaseCommand::DescribeEntity {
            workspace,
            id,
            entity_name,
            entity_type,
            query_uri,
        } => {
            intelligence::describe_entity(
                cli,
                client,
                workspace,
                id,
                entity_name,
                entity_type,
                query_uri.as_deref(),
            )
            .await
        }
        KqlDatabaseCommand::Sample {
            workspace,
            id,
            entity_name,
            count,
            entity_type,
            query_uri,
        } => {
            intelligence::sample(
                cli,
                client,
                workspace,
                id,
                entity_name,
                *count,
                entity_type,
                query_uri.as_deref(),
            )
            .await
        }
        KqlDatabaseCommand::Ingest {
            workspace,
            id,
            table,
            data,
            source_path,
            source_lakehouse,
            source_workspace,
            format,
            ignore_first_record,
            query_uri,
        } => {
            intelligence::ingest(
                cli,
                client,
                workspace,
                id,
                table,
                data.as_deref(),
                intelligence::IngestSource {
                    source_path: source_path.as_deref(),
                    source_lakehouse: source_lakehouse.as_deref(),
                    source_workspace: source_workspace.as_deref(),
                    format: format.as_deref(),
                    ignore_first_record: *ignore_first_record,
                },
                query_uri.as_deref(),
            )
            .await
        }
        KqlDatabaseCommand::ShowQueryplan {
            workspace,
            id,
            kql,
            query_uri,
        } => {
            intelligence::show_queryplan(
                cli,
                client,
                workspace,
                id,
                kql.as_deref(),
                query_uri.as_deref(),
            )
            .await
        }
        KqlDatabaseCommand::Diagnostics {
            workspace,
            id,
            query_uri,
        } => intelligence::diagnostics(cli, client, workspace, id, query_uri.as_deref()).await,
        KqlDatabaseCommand::Deeplink {
            workspace,
            id,
            kql,
            style,
            query_uri,
        } => {
            intelligence::deeplink(cli, client, workspace, id, kql, style, query_uri.as_deref())
                .await
        }
        KqlDatabaseCommand::McpUrl { workspace, id } => {
            mcp::mcp_url(cli, client, workspace, id).await
        }
        KqlDatabaseCommand::Examples {
            workspace,
            id,
            prompt,
            general_only,
            specific_only,
        } => {
            mcp::examples(
                cli,
                client,
                workspace,
                id,
                prompt,
                *general_only,
                *specific_only,
            )
            .await
        }
        KqlDatabaseCommand::SchemaContext {
            workspace,
            id,
            prompt,
        } => mcp::schema_context(cli, client, workspace, id, prompt).await,
        KqlDatabaseCommand::QueriesRunning {
            workspace,
            id,
            query_uri,
        } => intelligence::queries_running(cli, client, workspace, id, query_uri.as_deref()).await,
        KqlDatabaseCommand::Journal {
            workspace,
            id,
            query_uri,
        } => intelligence::journal(cli, client, workspace, id, query_uri.as_deref()).await,
        KqlDatabaseCommand::QueriesCompleted {
            workspace,
            id,
            query_uri,
        } => {
            intelligence::queries_completed(cli, client, workspace, id, query_uri.as_deref()).await
        }
        KqlDatabaseCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        KqlDatabaseCommand::UpdateDefinition {
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
        KqlDatabaseCommand::ListShortcuts { workspace, id } => {
            list_shortcuts(cli, client, workspace, id).await
        }
        KqlDatabaseCommand::CreateShortcut {
            workspace,
            id,
            name,
            file,
            content,
        } => {
            create_shortcut(
                cli,
                client,
                workspace,
                id,
                name,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        KqlDatabaseCommand::GetShortcut {
            workspace,
            id,
            shortcut_name,
        } => get_shortcut(cli, client, workspace, id, shortcut_name).await,
        KqlDatabaseCommand::DeleteShortcut {
            workspace,
            id,
            shortcut_name,
        } => delete_shortcut(cli, client, workspace, id, shortcut_name).await,
        KqlDatabaseCommand::BulkCreateShortcuts {
            workspace,
            id,
            file,
            content,
            conflict_policy,
        } => {
            bulk_create_shortcuts(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
                conflict_policy.as_deref(),
            )
            .await
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/kqlDatabases"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    let has_labels = resp
        .items
        .iter()
        .any(|item| item.get("sensitivityLabel").is_some_and(|v| !v.is_null()));
    let has_tags = output::has_tags(&resp.items);

    let display_items;
    let items_ref: &[Value] = if has_tags {
        display_items = output::enrich_with_tags_display(&resp.items);
        &display_items
    } else {
        &resp.items
    };

    match (has_labels, has_tags) {
        (true, true) => output::render_list_with_token(
            cli,
            items_ref,
            &[
                "displayName",
                "id",
                "description",
                "sensitivityLabel.id",
                "_tagsDisplay",
            ],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (true, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "sensitivityLabel.id"],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, true) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "_tagsDisplay"],
            &["NAME", "ID", "DESCRIPTION", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description"],
            &["NAME", "ID", "DESCRIPTION"],
            "id",
            resp.continuation_token.as_deref(),
        ),
    }
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/kqlDatabases/{id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    eventhouse_id: &str,
    database_type: &str,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
        "creationPayload": {
            "databaseType": database_type,
            "parentEventhouseItemId": eventhouse_id
        }
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(cli, "kql-database create", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/kqlDatabases"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "kql-database create", "Member"))?;
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
            "Example: fabio kql-database update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "kql-database update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/kqlDatabases/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "kql-database update", "Contributor"))?;
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
    if output::dry_run_guard(
        cli,
        "kql-database delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/kqlDatabases/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/kqlDatabases/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "kql-database delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

mod intelligence;
mod mcp;

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/kqlDatabases/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "kql-database get-definition", "Contributor"))?;
    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
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
                "Example: fabio kql-database update-definition --workspace <WS> --id <ID> --file schema.kql".to_string(),
            ).into());
        }
    };

    let body =
        crate::definition_spec::build_update_definition_body(&script, "DatabaseProperties.kql");

    if output::dry_run_guard(
        cli,
        "kql-database update-definition",
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
            &format!("/workspaces/{workspace}/kqlDatabases/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "kql-database update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Shortcuts ───────────────────────────────────────────────────────────────

async fn list_shortcuts(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/kqlDatabases/{id}/shortcuts"
        ))
        .await?;

    if let Some(arr) = data.as_array() {
        output::render_list_with_token(
            cli,
            arr,
            &["name", "target"],
            &["NAME", "TARGET"],
            "name",
            None,
        );
    } else {
        output::render_object(cli, &data, "shortcuts");
    }
    Ok(())
}

async fn create_shortcut(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let config: Value = match (file, content) {
        (Some(path), _) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
            serde_json::from_str(&raw)?
        }
        (_, Some(c)) => serde_json::from_str(c)?,
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio kql-database create-shortcut --workspace <WS> --id <ID> --name my-shortcut --content '{...}'"
                    .to_string(),
            )
            .into());
        }
    };

    let mut body = config;
    if let Some(obj) = body.as_object_mut() {
        obj.insert("name".to_string(), Value::from(name));
    }

    if output::dry_run_guard(cli, "kql-database create-shortcut", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/kqlDatabases/{id}/shortcuts"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "kql-database create-shortcut", "Contributor"))?;
    output::render_object(cli, &data, "name");
    Ok(())
}

async fn get_shortcut(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    shortcut_name: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/kqlDatabases/{id}/shortcuts/{shortcut_name}"
        ))
        .await?;
    output::render_object(cli, &data, "name");
    Ok(())
}

async fn delete_shortcut(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    shortcut_name: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "kql-database delete-shortcut",
        &serde_json::json!({ "workspace": workspace, "id": id, "shortcutName": shortcut_name }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/kqlDatabases/{id}/shortcuts/{shortcut_name}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "kql-database delete-shortcut", "Contributor"))?;

    let obj = serde_json::json!({ "shortcutName": shortcut_name, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn bulk_create_shortcuts(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    conflict_policy: Option<&str>,
) -> Result<()> {
    let input: Value = match (file, content) {
        (Some(path), _) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
            serde_json::from_str(&raw)?
        }
        (_, Some(c)) => serde_json::from_str(c)?,
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio kql-database bulk-create-shortcuts --workspace <WS> --id <ID> --file shortcuts.json"
                    .to_string(),
            )
            .into());
        }
    };

    // Wrap in the API envelope if user provided a raw array
    let body = if input.is_array() {
        serde_json::json!({ "createShortcutRequests": input })
    } else {
        input
    };

    if output::dry_run_guard(cli, "kql-database bulk-create-shortcuts", &body) {
        return Ok(());
    }

    let mut url = format!("/workspaces/{workspace}/items/{id}/shortcuts/bulkCreate");
    if let Some(policy) = conflict_policy {
        use std::fmt::Write;
        let _ = write!(url, "?shortcutConflictPolicy={policy}");
    }

    let data = client
        .post(&url, &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "kql-database bulk-create-shortcuts", "Contributor"))?;
    output::render_object(cli, &data, "value");
    Ok(())
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::commands::kql_utils::parse_kusto_v2_response;
    use serde_json::json;

    #[test]
    fn test_parse_kusto_v2_primary_result() {
        let frames = json!([
            {
                "FrameType": "DataSetHeader",
                "IsProgressive": false,
                "Version": "v2.0"
            },
            {
                "FrameType": "DataTable",
                "TableId": 0,
                "TableKind": "PrimaryResult",
                "TableName": "PrimaryResult",
                "Columns": [
                    {"ColumnName": "Name", "ColumnType": "string"},
                    {"ColumnName": "Age", "ColumnType": "int"},
                    {"ColumnName": "Score", "ColumnType": "real"}
                ],
                "Rows": [
                    ["Alice", 30, 95.5],
                    ["Bob", 25, 87.3]
                ]
            },
            {
                "FrameType": "DataSetCompletion",
                "HasErrors": false,
                "Cancelled": false
            }
        ]);

        let (rows, columns) = parse_kusto_v2_response(&frames).unwrap();
        assert_eq!(columns, vec!["Name", "Age", "Score"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["Name"], "Alice");
        assert_eq!(rows[0]["Age"], 30);
        assert_eq!(rows[1]["Name"], "Bob");
        assert_eq!(rows[1]["Score"], 87.3);
    }

    #[test]
    fn test_parse_kusto_v2_empty_result() {
        let frames = json!([
            {
                "FrameType": "DataSetHeader",
                "IsProgressive": false,
                "Version": "v2.0"
            },
            {
                "FrameType": "DataTable",
                "TableId": 0,
                "TableKind": "PrimaryResult",
                "TableName": "PrimaryResult",
                "Columns": [
                    {"ColumnName": "Count", "ColumnType": "long"}
                ],
                "Rows": []
            },
            {
                "FrameType": "DataSetCompletion",
                "HasErrors": false,
                "Cancelled": false
            }
        ]);

        let (rows, columns) = parse_kusto_v2_response(&frames).unwrap();
        assert_eq!(columns, vec!["Count"]);
        assert!(rows.is_empty());
    }

    #[test]
    fn test_parse_kusto_v2_no_primary_falls_back_to_first_datatable() {
        let frames = json!([
            {
                "FrameType": "DataSetHeader",
                "IsProgressive": false,
                "Version": "v2.0"
            },
            {
                "FrameType": "DataTable",
                "TableId": 0,
                "TableKind": "QueryCompletionInformation",
                "TableName": "@ExtendedProperties",
                "Columns": [
                    {"ColumnName": "Key", "ColumnType": "string"},
                    {"ColumnName": "Value", "ColumnType": "string"}
                ],
                "Rows": [
                    ["ServerExecutionTime", "00:00:00.001"]
                ]
            },
            {
                "FrameType": "DataSetCompletion",
                "HasErrors": false,
                "Cancelled": false
            }
        ]);

        let (rows, columns) = parse_kusto_v2_response(&frames).unwrap();
        assert_eq!(columns, vec!["Key", "Value"]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["Key"], "ServerExecutionTime");
    }

    #[test]
    fn test_parse_kusto_v2_null_values() {
        let frames = json!([
            {
                "FrameType": "DataTable",
                "TableId": 0,
                "TableKind": "PrimaryResult",
                "TableName": "PrimaryResult",
                "Columns": [
                    {"ColumnName": "Id", "ColumnType": "int"},
                    {"ColumnName": "Label", "ColumnType": "string"}
                ],
                "Rows": [
                    [1, null],
                    [2, "active"]
                ]
            }
        ]);

        let (rows, columns) = parse_kusto_v2_response(&frames).unwrap();
        assert_eq!(columns, vec!["Id", "Label"]);
        assert_eq!(rows.len(), 2);
        assert!(rows[0]["Label"].is_null());
        assert_eq!(rows[1]["Label"], "active");
    }

    #[test]
    fn test_parse_kusto_v2_no_frames_returns_empty() {
        let frames = json!([
            {
                "FrameType": "DataSetHeader",
                "IsProgressive": false,
                "Version": "v2.0"
            },
            {
                "FrameType": "DataSetCompletion",
                "HasErrors": false,
                "Cancelled": false
            }
        ]);

        let (rows, columns) = parse_kusto_v2_response(&frames).unwrap();
        assert!(rows.is_empty());
        assert!(columns.is_empty());
    }

    #[test]
    fn test_parse_kusto_v2_error_in_completion() {
        let frames = json!([
            {
                "FrameType": "DataSetHeader",
                "IsProgressive": false,
                "Version": "v2.0"
            },
            {
                "FrameType": "DataSetCompletion",
                "HasErrors": true,
                "OneApiErrors": "Syntax error in query"
            }
        ]);

        let result = parse_kusto_v2_response(&frames);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Syntax error in query"));
    }

    #[test]
    fn test_parse_kusto_v2_not_array_returns_error() {
        let frames = json!({"error": "unexpected"});

        let result = parse_kusto_v2_response(&frames);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("expected JSON array"));
    }
}
