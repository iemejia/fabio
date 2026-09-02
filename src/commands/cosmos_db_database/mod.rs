//! `cosmos-db-database` command group.
//!
//! Combines the control-plane item CRUD (`crud`) with data-plane operations
//! (`containers`, `documents`) that speak the Cosmos DB `NoSQL` REST API through
//! the shared `data_plane` transport.

mod containers;
mod crud;
mod data_plane;
mod documents;

use anyhow::Result;
use clap::Subcommand;

use crate::cli::Cli;
use crate::client::FabricClient;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum CosmosDbDatabaseCommand {
    /// List Cosmos DB databases in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a Cosmos DB database
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Cosmos DB database ID
        #[arg(long)]
        id: String,
    },
    /// Create a new Cosmos DB database
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
    /// Update Cosmos DB database properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Cosmos DB database ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a Cosmos DB database
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Cosmos DB database ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a Cosmos DB database
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Cosmos DB database ID
        #[arg(long)]
        id: String,
        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a Cosmos DB database
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Cosmos DB database ID
        #[arg(long)]
        id: String,
        /// Path to definition file
        #[arg(long)]
        file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
    /// List containers in a Cosmos DB database (data-plane)
    #[command(display_order = 8)]
    ListContainers {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Cosmos DB database ID
        #[arg(long)]
        id: String,
        /// Override the Cosmos DB data-plane endpoint (default: resolved from item properties)
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Create a container in a Cosmos DB database (data-plane)
    #[command(display_order = 9)]
    CreateContainer {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Cosmos DB database ID
        #[arg(long)]
        id: String,
        /// Container name
        #[arg(long)]
        container: String,
        /// Partition key path (e.g. /categoryId). A leading slash is added if omitted.
        #[arg(long)]
        partition_key: String,
        /// Autoscale maximum throughput (RU/s). Fabric Cosmos is autoscale-only.
        #[arg(long, default_value = "1000")]
        autoscale_max: u32,
        /// Default time-to-live for documents, in seconds (-1 = on, no expiry)
        #[arg(long)]
        ttl: Option<i64>,
        /// Override the Cosmos DB data-plane endpoint (default: resolved from item properties)
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Delete a container and all its documents (data-plane, irreversible)
    #[command(display_order = 10)]
    DeleteContainer {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Cosmos DB database ID
        #[arg(long)]
        id: String,
        /// Container name
        #[arg(long)]
        container: String,
        /// Override the Cosmos DB data-plane endpoint (default: resolved from item properties)
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Run a query against a container (Cosmos DB data-plane)
    #[command(display_order = 11)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Cosmos DB database ID
        #[arg(long)]
        id: String,
        /// Container name
        #[arg(long)]
        container: String,
        /// Query text (inline, @file, or piped via stdin)
        #[arg(long)]
        query_text: Option<String>,
        /// Bind a query parameter as name=value (repeatable). Values are typed
        /// when numeric/boolean, else string.
        #[arg(long = "parameter", value_name = "NAME=VALUE")]
        parameters: Vec<String>,
        /// Scope the query to a single partition-key value (else cross-partition)
        #[arg(long)]
        partition_key: Option<String>,
        /// Max documents per page request (x-ms-max-item-count)
        #[arg(long)]
        max_item_count: Option<u32>,
        /// Override the Cosmos DB data-plane endpoint (default: resolved from item properties)
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Bulk import documents from JSONL/JSON into a container (data-plane, upsert by default)
    #[command(display_order = 12)]
    Import {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Cosmos DB database ID
        #[arg(long)]
        id: String,
        /// Target container name
        #[arg(long)]
        container: String,
        /// Source file path (JSONL or JSON array). Reads stdin when omitted.
        #[arg(long)]
        source: Option<String>,
        /// Input format: jsonl, json-array, or auto
        #[arg(long, default_value = "auto")]
        format: String,
        /// Write mode: upsert (default, idempotent) or insert
        #[arg(long, default_value = "upsert")]
        mode: String,
        /// Max concurrent writes (default: auto)
        #[arg(long)]
        concurrency: Option<usize>,
        /// Skip invalid documents instead of aborting the import
        #[arg(long)]
        continue_on_error: bool,
        /// Override the Cosmos DB data-plane endpoint (default: resolved from item properties)
        #[arg(long)]
        endpoint: Option<String>,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &CosmosDbDatabaseCommand,
) -> Result<()> {
    match command {
        CosmosDbDatabaseCommand::List { workspace } => crud::list(cli, client, workspace).await,
        CosmosDbDatabaseCommand::Show { workspace, id } => {
            crud::show(cli, client, workspace, id).await
        }
        CosmosDbDatabaseCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
        } => {
            crud::create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        CosmosDbDatabaseCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            crud::update(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        CosmosDbDatabaseCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => crud::delete(cli, client, workspace, id, *hard_delete).await,
        CosmosDbDatabaseCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => crud::get_definition(cli, client, workspace, id, *decode).await,
        CosmosDbDatabaseCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
        } => {
            crud::update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        CosmosDbDatabaseCommand::ListContainers {
            workspace,
            id,
            endpoint,
        } => containers::list_containers(cli, client, workspace, id, endpoint.as_deref()).await,
        CosmosDbDatabaseCommand::CreateContainer {
            workspace,
            id,
            container,
            partition_key,
            autoscale_max,
            ttl,
            endpoint,
        } => {
            containers::create_container(
                cli,
                client,
                workspace,
                id,
                container,
                partition_key,
                *autoscale_max,
                *ttl,
                endpoint.as_deref(),
            )
            .await
        }
        CosmosDbDatabaseCommand::DeleteContainer {
            workspace,
            id,
            container,
            endpoint,
        } => {
            containers::delete_container(cli, client, workspace, id, container, endpoint.as_deref())
                .await
        }
        CosmosDbDatabaseCommand::Query {
            workspace,
            id,
            container,
            query_text,
            parameters,
            partition_key,
            max_item_count,
            endpoint,
        } => {
            documents::query(
                cli,
                client,
                workspace,
                id,
                container,
                query_text.as_deref(),
                parameters,
                partition_key.as_deref(),
                *max_item_count,
                endpoint.as_deref(),
            )
            .await
        }
        CosmosDbDatabaseCommand::Import {
            workspace,
            id,
            container,
            source,
            format,
            mode,
            concurrency,
            continue_on_error,
            endpoint,
        } => {
            documents::import(
                cli,
                client,
                workspace,
                id,
                container,
                source.as_deref(),
                format,
                mode,
                *concurrency,
                *continue_on_error,
                endpoint.as_deref(),
            )
            .await
        }
    }
}
