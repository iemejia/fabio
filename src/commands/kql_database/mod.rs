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
    /// Create a table shortcut in a KQL database (OneLake/S3/ADLS Gen2/GCS/S3-compatible/Azure Blob)
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

        /// Enable query acceleration on the shortcut (required field; default false)
        #[arg(long)]
        enable_query_acceleration: bool,

        /// Target type (typed path): `OneLake`, `AmazonS3`, `AdlsGen2`, `GoogleCloudStorage`,
        /// `S3Compatible`, `AzureBlobStorage`. When set, the target is built from the typed
        /// flags below instead of `--file`/`--content`.
        #[arg(long)]
        target_type: Option<String>,

        /// Connection ID for external (S3/ADLS/GCS/Blob) targets
        #[arg(long)]
        connection_id: Option<String>,

        /// Location URL for external targets (e.g. `https://acct.dfs.core.windows.net/container`)
        #[arg(long)]
        location: Option<String>,

        /// Subpath under the location (optional)
        #[arg(long)]
        subpath: Option<String>,

        /// Bucket name (`S3Compatible` only)
        #[arg(long)]
        bucket: Option<String>,

        /// Target workspace ID (`OneLake` target)
        #[arg(long)]
        target_workspace: Option<String>,

        /// Target item ID (`OneLake` target)
        #[arg(long)]
        target_item: Option<String>,

        /// Target path within the target item (`OneLake` target, e.g. `Tables/sales`)
        #[arg(long)]
        target_path: Option<String>,

        /// Raw target object as JSON (escape hatch; overrides typed target flags)
        #[arg(long)]
        target: Option<String>,

        /// JSON file with the full shortcut body (escape hatch; must include `target`)
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON with the full shortcut body (escape hatch; must include `target`)
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
            enable_query_acceleration,
            target_type,
            connection_id,
            location,
            subpath,
            bucket,
            target_workspace,
            target_item,
            target_path,
            target,
            file,
            content,
        } => {
            let flags = crate::commands::shortcut_target::ShortcutTargetFlags {
                connection_id: connection_id.as_deref(),
                location: location.as_deref(),
                subpath: subpath.as_deref(),
                bucket: bucket.as_deref(),
                target_workspace: target_workspace.as_deref(),
                target_item: target_item.as_deref(),
                target_path: target_path.as_deref(),
                ..Default::default()
            };
            create_shortcut(
                cli,
                client,
                workspace,
                id,
                name,
                *enable_query_acceleration,
                target_type.as_deref(),
                target.as_deref(),
                &flags,
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

/// The six target types a KQL-database table shortcut supports (a subset of the
/// nine Fabric shortcut targets — `Dataverse`, `ExternalDataShare`, and
/// `OneDriveSharePoint` are NOT valid for a KQL table shortcut).
const KQL_SHORTCUT_TARGET_TYPES: &str =
    "OneLake, AmazonS3, AdlsGen2, GoogleCloudStorage, S3Compatible, AzureBlobStorage";

/// Reject a resolved target discriminator that a KQL table shortcut cannot use.
fn ensure_kql_target_supported(disc: &str) -> Result<()> {
    if matches!(
        disc,
        "dataverse" | "externalDataShare" | "oneDriveSharePoint"
    ) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Target type '{disc}' is not supported for a KQL-database table shortcut"),
            format!("Supported target types: {KQL_SHORTCUT_TARGET_TYPES}."),
        )
        .into());
    }
    Ok(())
}

/// Build the typed `CreateTableShortcutRequest` body:
/// `{name, enableQueryAcceleration, target: {<disc>: {...}}}`. Pure.
fn build_kql_shortcut_body(
    name: &str,
    enable_query_acceleration: bool,
    target_type: &str,
    target_json: Option<&str>,
    flags: &crate::commands::shortcut_target::ShortcutTargetFlags<'_>,
) -> Result<Value> {
    // Reject a KQL-unsupported target type up front (before validating type-specific
    // flags) so the error names the real problem.
    if let Some(disc) = crate::commands::shortcut_target::normalize_target_type(target_type) {
        ensure_kql_target_supported(disc)?;
    }
    let (disc, target_body) =
        crate::commands::shortcut_target::build_shortcut_target(target_type, target_json, flags)?;
    ensure_kql_target_supported(&disc)?;
    Ok(serde_json::json!({
        "name": name,
        "enableQueryAcceleration": enable_query_acceleration,
        "target": { disc: target_body },
    }))
}

async fn list_shortcuts(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/kqlDatabases/{id}/shortcuts"
        ))
        .await?;

    // The TableShortcuts list response is `{ "value": [...] }`; fall back to a bare array.
    let items = data
        .get("value")
        .and_then(Value::as_array)
        .or_else(|| data.as_array());
    if let Some(arr) = items {
        output::render_list_with_token(
            cli,
            arr,
            &["name", "enableQueryAcceleration", "target"],
            &["NAME", "QUERY ACCEL", "TARGET"],
            "name",
            None,
        );
    } else {
        output::render_object(cli, &data, "shortcuts");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create_shortcut(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    enable_query_acceleration: bool,
    target_type: Option<&str>,
    target: Option<&str>,
    flags: &crate::commands::shortcut_target::ShortcutTargetFlags<'_>,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = if let Some(tt) = target_type {
        // Typed path: build `{name, enableQueryAcceleration, target: {<disc>: {...}}}`.
        build_kql_shortcut_body(name, enable_query_acceleration, tt, target, flags)?
    } else {
        // Escape hatch: a full shortcut body from --file/--content. fabio injects the
        // shortcut name and guarantees the required `enableQueryAcceleration` field.
        let mut config: Value = match (file, content) {
            (Some(path), _) => {
                let raw = std::fs::read_to_string(path)
                    .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
                serde_json::from_str(&raw)?
            }
            (_, Some(c)) => serde_json::from_str(c)?,
            (None, None) => {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "Provide a target: use --target-type with the typed flags, or --file/--content with a full shortcut body".to_string(),
                    "Example: fabio kql-database create-shortcut --workspace <WS> --id <ID> --name sales --target-type OneLake --target-workspace <WS> --target-item <LH> --target-path Tables/sales --enable-query-acceleration".to_string(),
                )
                .into());
            }
        };
        if let Some(obj) = config.as_object_mut() {
            obj.insert("name".to_string(), Value::from(name));
            obj.entry("enableQueryAcceleration")
                .or_insert_with(|| Value::Bool(enable_query_acceleration));
        }
        config
    };

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
    use super::{build_kql_shortcut_body, ensure_kql_target_supported};
    use crate::commands::kql_utils::parse_kusto_v2_response;
    use crate::commands::shortcut_target::ShortcutTargetFlags;
    use serde_json::json;

    #[test]
    fn kql_shortcut_body_typed_onelake() {
        let flags = ShortcutTargetFlags {
            target_workspace: Some("ws1"),
            target_item: Some("lh1"),
            target_path: Some("Tables/sales"),
            ..Default::default()
        };
        let body = build_kql_shortcut_body("sales", true, "OneLake", None, &flags).unwrap();
        assert_eq!(body["name"], "sales");
        assert_eq!(body["enableQueryAcceleration"], true);
        assert_eq!(body["target"]["oneLake"]["workspaceId"], "ws1");
        assert_eq!(body["target"]["oneLake"]["itemId"], "lh1");
        assert_eq!(body["target"]["oneLake"]["path"], "Tables/sales");
    }

    #[test]
    fn kql_shortcut_body_typed_s3() {
        let flags = ShortcutTargetFlags {
            location: Some("https://bucket.s3.amazonaws.com/data"),
            connection_id: Some("conn-1"),
            ..Default::default()
        };
        let body = build_kql_shortcut_body("ext", false, "AmazonS3", None, &flags).unwrap();
        assert_eq!(body["enableQueryAcceleration"], false);
        assert_eq!(
            body["target"]["amazonS3"]["location"],
            "https://bucket.s3.amazonaws.com/data"
        );
        assert_eq!(body["target"]["amazonS3"]["connectionId"], "conn-1");
    }

    #[test]
    fn kql_shortcut_rejects_unsupported_targets() {
        assert!(ensure_kql_target_supported("dataverse").is_err());
        assert!(ensure_kql_target_supported("externalDataShare").is_err());
        assert!(ensure_kql_target_supported("oneDriveSharePoint").is_err());
        assert!(ensure_kql_target_supported("oneLake").is_ok());
        assert!(ensure_kql_target_supported("amazonS3").is_ok());
        // A Dataverse target is rejected before the network call.
        let flags = ShortcutTargetFlags {
            connection_id: Some("c"),
            environment_domain: Some("https://org.crm.dynamics.com"),
            ..Default::default()
        };
        assert!(build_kql_shortcut_body("x", false, "dataverse", None, &flags).is_err());
    }

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
