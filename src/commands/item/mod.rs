use std::fs;

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};

mod bulk;
mod copy_move;
mod crud;
mod definitions;
mod tags;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples item\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum ItemCommand {
    // ── Read ─────────────────────────────────────────────────────────────
    /// List items in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Filter by item type (e.g., Notebook, Lakehouse, Warehouse)
        #[arg(short = 't', long = "type", visible_alias = "item-type")]
        item_type: Option<String>,

        /// Filter by folder ID (server-side)
        #[arg(long)]
        folder: Option<String>,

        /// List items in nested subfolders (default: true when folder is specified)
        #[arg(long)]
        recursive: Option<bool>,

        /// Include additional properties in the response (e.g., description,folderId)
        #[arg(long)]
        include: Option<String>,
    },
    /// Show details of an item
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,
    },
    /// Get the definition (source code/content) of an item
    #[command(display_order = 3)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Definition format (optional, item-type dependent)
        #[arg(long)]
        format: Option<String>,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// List connections used by an item
    #[command(display_order = 4)]
    ListConnections {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,
    },
    /// Check if an item exists (returns {"exists": true/false})
    #[command(display_order = 5)]
    Exists {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,
    },
    /// Get the Fabric portal URL for an item
    #[command(display_order = 6)]
    Url {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Item type (e.g., Lakehouse, Notebook, Warehouse). Improves URL accuracy.
        #[arg(short = 't', long = "type")]
        item_type: Option<String>,
    },
    /// Aggregated item view: metadata + definition + connections
    #[command(display_order = 7)]
    Inspect {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,
    },
    /// Get downstream relations for an item (beta API)
    #[command(display_order = 8)]
    ListDownstreamRelations {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,
    },
    /// Get upstream relations for an item (beta API)
    #[command(display_order = 9)]
    ListUpstreamRelations {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,
    },

    // ── Create/Update/Delete ─────────────────────────────────────────────
    /// Create a new item
    #[command(display_order = 10)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item display name
        #[arg(long)]
        name: String,

        /// Item type (e.g., Lakehouse, Warehouse)
        #[arg(short = 't', long = "type")]
        item_type: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,
    },
    /// Update item properties (name and/or description)
    #[command(display_order = 11)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Update (override) item definition from file(s)
    #[command(display_order = 12)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Path to definition file (will be base64-encoded as a single part)
        #[arg(long)]
        file: Option<String>,

        /// Inline JSON definition (full definition payload with parts array)
        #[arg(long)]
        definition: Option<String>,

        /// When true, also update item metadata from .platform file
        #[arg(long)]
        update_metadata: bool,
    },
    /// Delete an item
    #[command(display_order = 13)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Copy/Move ────────────────────────────────────────────────────────
    /// Copy an item to another workspace
    #[command(display_order = 14)]
    Copy {
        /// Source workspace ID
        #[arg(long)]
        source_workspace: String,

        /// Item ID to copy
        #[arg(long)]
        id: String,

        /// Destination workspace ID
        #[arg(long)]
        dest_workspace: String,

        /// New name for the copy (optional, defaults to source name)
        #[arg(long)]
        name: Option<String>,
    },
    /// Move an item to another workspace (copy + delete source)
    #[command(display_order = 15)]
    Move {
        /// Source workspace ID
        #[arg(long)]
        source_workspace: String,

        /// Item ID to move
        #[arg(long)]
        id: String,

        /// Destination workspace ID
        #[arg(long)]
        dest_workspace: String,

        /// New name (optional, defaults to source name)
        #[arg(long)]
        name: Option<String>,
    },
    /// Move an item to a folder within the same workspace
    #[command(name = "move-to-folder", display_order = 16)]
    MoveToFolder {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Target folder ID (omit or pass empty string to move to workspace root)
        #[arg(long)]
        folder_id: Option<String>,
    },

    // ── Tags ─────────────────────────────────────────────────────────────
    /// Apply tags to an item
    #[command(display_order = 20)]
    ApplyTags {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Comma-separated tag IDs
        #[arg(long, value_delimiter = ',')]
        tag_ids: Vec<String>,
    },
    /// Remove tags from an item
    #[command(display_order = 21)]
    UnapplyTags {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Comma-separated tag IDs
        #[arg(long, value_delimiter = ',')]
        tag_ids: Vec<String>,
    },

    // ── Bulk Operations ──────────────────────────────────────────────────
    /// Bulk export item definitions (LRO)
    #[command(display_order = 30)]
    BulkExportDefinitions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Path to JSON file with request body
        #[arg(long, group = "input")]
        file: Option<String>,

        /// Inline JSON request body
        #[arg(long, group = "input")]
        content: Option<String>,
    },
    /// Bulk import item definitions (LRO)
    #[command(display_order = 31)]
    BulkImportDefinitions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Path to JSON file with request body
        #[arg(long, group = "input")]
        file: Option<String>,

        /// Inline JSON request body
        #[arg(long, group = "input")]
        content: Option<String>,
    },
    /// Bulk move items to another workspace (LRO)
    #[command(display_order = 32)]
    BulkMove {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Path to JSON file with request body
        #[arg(long, group = "input")]
        file: Option<String>,

        /// Inline JSON request body
        #[arg(long, group = "input")]
        content: Option<String>,
    },

    /// Bulk create items in parallel (client-side concurrency)
    #[command(display_order = 33)]
    BulkCreate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Path to JSON file with item array
        #[arg(long, group = "input")]
        file: Option<String>,

        /// Inline JSON array of items: [{"displayName":"...", "type":"..."}, ...]
        #[arg(long, group = "input")]
        content: Option<String>,
    },
    /// Bulk delete items in parallel (client-side concurrency)
    #[command(display_order = 34)]
    BulkDelete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Comma-separated item IDs to delete
        #[arg(long, value_delimiter = ',')]
        ids: Vec<String>,
    },

    // ── External Data Shares ─────────────────────────────────────────────
    /// List external data shares for an item
    #[command(display_order = 40)]
    ListExternalDataShares {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,
    },
    /// Create an external data share for an item
    #[command(display_order = 41)]
    CreateExternalDataShare {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Comma-separated paths to share
        #[arg(long, value_delimiter = ',')]
        paths: Vec<String>,

        /// Recipient tenant ID
        #[arg(long)]
        recipient_tenant_id: String,

        /// Recipient type (`User` or `ServicePrincipal`). If omitted, shares with entire tenant.
        #[arg(long)]
        recipient_type: Option<String>,

        /// Object ID of the recipient principal (required when --recipient-type is set)
        #[arg(long)]
        recipient_id: Option<String>,
    },
    /// Show details of an external data share
    #[command(display_order = 42)]
    ShowExternalDataShare {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// External data share ID
        #[arg(long)]
        share_id: String,
    },
    /// Revoke an external data share
    #[command(display_order = 43)]
    RevokeExternalDataShare {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// External data share ID
        #[arg(long)]
        share_id: String,
    },
    /// Delete an external data share
    #[command(display_order = 44)]
    DeleteExternalDataShare {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// External data share ID
        #[arg(long)]
        share_id: String,
    },

    // ── Identity ─────────────────────────────────────────────────────────
    /// Assign a managed identity to an item
    #[command(display_order = 50)]
    AssignIdentity {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,
    },

    // ── External Data Share Invitations ──────────────────────────────────
    /// Get an external data share invitation (platform-level)
    #[command(display_order = 55)]
    GetInvitation {
        /// Invitation ID
        #[arg(long)]
        invitation_id: String,
    },
    /// Accept an external data share invitation
    #[command(display_order = 56)]
    AcceptInvitation {
        /// Invitation ID
        #[arg(long)]
        invitation_id: String,

        /// Target workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name for the created item
        #[arg(long)]
        name: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &ItemCommand) -> Result<()> {
    match command {
        ItemCommand::List {
            workspace,
            item_type,
            folder,
            recursive,
            include,
        } => {
            crud::list(
                cli,
                client,
                workspace,
                item_type.as_deref(),
                folder.as_deref(),
                *recursive,
                include.as_deref(),
            )
            .await
        }
        ItemCommand::Show { workspace, id } => crud::show(cli, client, workspace, id).await,
        ItemCommand::GetDefinition {
            workspace,
            id,
            format,
            decode,
        } => {
            definitions::get_definition(cli, client, workspace, id, format.as_deref(), *decode)
                .await
        }
        ItemCommand::ListConnections { workspace, id } => {
            crud::list_connections(cli, client, workspace, id).await
        }
        ItemCommand::Exists { workspace, id } => crud::exists(cli, client, workspace, id).await,
        ItemCommand::Url {
            workspace,
            id,
            item_type,
        } => crud::url(cli, workspace, id, item_type.as_deref()),
        ItemCommand::Inspect { workspace, id } => crud::inspect(cli, client, workspace, id).await,
        ItemCommand::ListDownstreamRelations { workspace, id } => {
            crud::list_relations(cli, client, workspace, id, "downstream").await
        }
        ItemCommand::ListUpstreamRelations { workspace, id } => {
            crud::list_relations(cli, client, workspace, id, "upstream").await
        }
        ItemCommand::Create {
            workspace,
            name,
            item_type,
            description,
        } => {
            crud::create(
                cli,
                client,
                workspace,
                name,
                item_type,
                description.as_deref(),
            )
            .await
        }
        ItemCommand::Update {
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
        ItemCommand::UpdateDefinition {
            workspace,
            id,
            file,
            definition,
            update_metadata,
        } => {
            definitions::update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                definition.as_deref(),
                *update_metadata,
            )
            .await
        }
        ItemCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => crud::delete(cli, client, workspace, id, *hard_delete).await,
        ItemCommand::Copy {
            source_workspace,
            id,
            dest_workspace,
            name,
        } => {
            copy_move::copy(
                cli,
                client,
                source_workspace,
                id,
                dest_workspace,
                name.as_deref(),
            )
            .await
        }
        ItemCommand::Move {
            source_workspace,
            id,
            dest_workspace,
            name,
        } => {
            copy_move::move_item(
                cli,
                client,
                source_workspace,
                id,
                dest_workspace,
                name.as_deref(),
            )
            .await
        }
        ItemCommand::MoveToFolder {
            workspace,
            id,
            folder_id,
        } => copy_move::move_to_folder(cli, client, workspace, id, folder_id.as_deref()).await,
        ItemCommand::ApplyTags {
            workspace,
            id,
            tag_ids,
        } => tags::apply_tags(cli, client, workspace, id, tag_ids).await,
        ItemCommand::UnapplyTags {
            workspace,
            id,
            tag_ids,
        } => tags::unapply_tags(cli, client, workspace, id, tag_ids).await,
        ItemCommand::BulkExportDefinitions {
            workspace,
            file,
            content,
        } => {
            bulk::bulk_post(
                cli,
                client,
                workspace,
                "bulkExportDefinitions",
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        ItemCommand::BulkImportDefinitions {
            workspace,
            file,
            content,
        } => {
            bulk::bulk_post(
                cli,
                client,
                workspace,
                "bulkImportDefinitions",
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        ItemCommand::BulkMove {
            workspace,
            file,
            content,
        } => {
            bulk::bulk_post(
                cli,
                client,
                workspace,
                "bulkMove",
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        ItemCommand::BulkCreate {
            workspace,
            file,
            content,
        } => bulk::bulk_create(cli, client, workspace, file.as_deref(), content.as_deref()).await,
        ItemCommand::BulkDelete { workspace, ids } => {
            bulk::bulk_delete(cli, client, workspace, ids).await
        }
        ItemCommand::ListExternalDataShares { workspace, id } => {
            bulk::list_external_data_shares(cli, client, workspace, id).await
        }
        ItemCommand::CreateExternalDataShare {
            workspace,
            id,
            paths,
            recipient_tenant_id,
            recipient_type,
            recipient_id,
        } => {
            bulk::create_external_data_share(
                cli,
                client,
                workspace,
                id,
                paths,
                recipient_tenant_id,
                recipient_type.as_deref(),
                recipient_id.as_deref(),
            )
            .await
        }
        ItemCommand::ShowExternalDataShare {
            workspace,
            id,
            share_id,
        } => bulk::show_external_data_share(cli, client, workspace, id, share_id).await,
        ItemCommand::RevokeExternalDataShare {
            workspace,
            id,
            share_id,
        } => bulk::revoke_external_data_share(cli, client, workspace, id, share_id).await,
        ItemCommand::DeleteExternalDataShare {
            workspace,
            id,
            share_id,
        } => bulk::delete_external_data_share(cli, client, workspace, id, share_id).await,
        ItemCommand::AssignIdentity { workspace, id } => {
            crud::assign_identity(cli, client, workspace, id).await
        }
        ItemCommand::GetInvitation { invitation_id } => {
            crud::get_invitation(cli, client, invitation_id).await
        }
        ItemCommand::AcceptInvitation {
            invitation_id,
            workspace,
            name,
        } => crud::accept_invitation(cli, client, invitation_id, workspace, name).await,
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Read JSON body from --file or --content flag.
fn read_json_input(file: Option<&str>, content: Option<&str>, command: &str) -> Result<Value> {
    if let Some(c) = content {
        serde_json::from_str(c).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in --content: {e}"),
                format!("Provide valid JSON for {command}."),
            )
            .into()
        })
    } else if let Some(f) = file {
        let data = fs::read_to_string(f).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read file '{f}': {e}"),
                "Provide a valid file path.".to_string(),
            )
        })?;
        serde_json::from_str(&data).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in file '{f}': {e}"),
                format!("Provide valid JSON for {command}."),
            )
            .into()
        })
    } else {
        Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            format!("Example: fabio item {command} --workspace <WS> --file request.json"),
        )
        .into())
    }
}

// ─── Error Enrichment ────────────────────────────────────────────────────────

/// Known Fabric item types for error hints.
const KNOWN_ITEM_TYPES: &[&str] = &[
    "AzureDatabricksStorage",
    "CopyJob",
    "Dashboard",
    "DataAgent",
    "DataBuildToolJob",
    "DataPipeline",
    "Dataflow",
    "Datamart",
    "Environment",
    "Eventhouse",
    "Eventstream",
    "GraphQLApi",
    "KQLDashboard",
    "KQLDatabase",
    "KQLQueryset",
    "Lakehouse",
    "MLExperiment",
    "MLModel",
    "MirroredDatabase",
    "MirroredWarehouse",
    "MountedDataFactory",
    "Notebook",
    "Ontology",
    "PaginatedReport",
    "Reflex",
    "Report",
    "SQLEndpoint",
    "SemanticModel",
    "SparkJobDefinition",
    "Warehouse",
];

/// Enrich item create errors with valid type hints.
fn enrich_item_create_error(err: anyhow::Error, item_type: &str) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };

    if fabio_err.message.contains("invalid") && fabio_err.message.contains(item_type) {
        let valid_types = KNOWN_ITEM_TYPES.join(", ");
        let hint = format!(
            "'{item_type}' is not a valid Fabric item type. Valid types: {valid_types}. \
             List items to see types in your workspace: fabio item list --workspace <ID>"
        );
        return FabioError::with_hint(ErrorCode::InvalidInput, &fabio_err.message, hint).into();
    }

    err
}

/// Enrich item not-found errors with guidance.
fn enrich_item_not_found_error(err: anyhow::Error, workspace: &str, id: &str) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };

    if fabio_err.code == ErrorCode::NotFound {
        let hint = format!(
            "Item '{id}' not found in workspace '{workspace}'. \
             List available items: fabio item list --workspace {workspace}"
        );
        return FabioError::with_hint(ErrorCode::NotFound, &fabio_err.message, hint).into();
    }

    err
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_item_types_are_sorted() {
        let mut sorted = KNOWN_ITEM_TYPES.to_vec();
        sorted.sort_unstable();
        assert_eq!(KNOWN_ITEM_TYPES, sorted.as_slice());
    }

    #[test]
    fn known_item_types_are_pascal_case() {
        for t in KNOWN_ITEM_TYPES {
            let first = t.chars().next().unwrap();
            assert!(first.is_uppercase(), "Type '{t}' should be PascalCase");
        }
    }

    #[test]
    fn enrich_create_error_adds_hint_for_invalid_type() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::ApiError,
            "The request is invalid. Item type FakeType is not supported.".to_string(),
        )
        .into();
        let enriched = enrich_item_create_error(err, "FakeType");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::InvalidInput);
        assert!(fabio_err.hint.as_ref().unwrap().contains("Lakehouse"));
    }

    #[test]
    fn enrich_not_found_adds_hint() {
        let err: anyhow::Error =
            FabioError::new(ErrorCode::NotFound, "item not found".to_string()).into();
        let enriched = enrich_item_not_found_error(err, "ws-123", "item-456");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert!(fabio_err.hint.as_ref().unwrap().contains("item-456"));
        assert!(fabio_err.hint.as_ref().unwrap().contains("ws-123"));
    }

    #[test]
    fn enrich_preserves_non_matching_errors() {
        let err: anyhow::Error =
            FabioError::new(ErrorCode::ApiError, "something else".to_string()).into();
        let enriched = enrich_item_create_error(err, "Notebook");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        // Should NOT be INVALID_INPUT since message doesn't contain "invalid" + type
        assert_eq!(fabio_err.code, ErrorCode::ApiError);
    }
}
