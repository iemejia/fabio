mod crud;
mod definitions;
mod operations;
mod powerbi;

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples semantic-model\nAlso available: fabio context schema SemanticModel | fabio context workflow direct-lake-report"
)]
pub enum SemanticModelCommand {
    /// List semantic models in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a semantic model
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Create a new semantic model from a definition file (model.bim)
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Path to model definition file (model.bim TMSL/TMDL format)
        #[arg(long)]
        file: String,

        /// SQL endpoint or lakehouse ID for live connection (generates definition.pbism)
        #[arg(long, visible_alias = "connection-id")]
        connection: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update semantic model properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a semantic model
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a semantic model
    #[command(name = "get-definition", display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a semantic model from a file
    #[command(name = "update-definition", display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Path to model definition file (model.bim TMSL/TMDL format)
        #[arg(long)]
        file: String,
    },
    /// Execute a DAX query against a semantic model
    #[command(display_order = 8)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// DAX query (e.g., "EVALUATE Sales"). If omitted, reads from stdin.
        #[arg(long)]
        dax: Option<String>,

        /// Read DAX query from a file
        #[arg(long, conflicts_with = "dax")]
        file: Option<String>,
    },
    /// Bind a semantic model to a connection
    #[command(name = "bind-connection", display_order = 10)]
    BindConnection {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Connection ID to bind
        #[arg(long)]
        connection_id: String,
    },
    /// Unbind a connection from a semantic model
    #[command(name = "unbind-connection", display_order = 10)]
    UnbindConnection {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Refresh a semantic model (required to frame Direct Lake models after creation)
    ///
    /// Basic refresh sends just --type. Passing --objects / --commit-mode /
    /// --max-parallelism / --retry-count triggers an ENHANCED refresh (the TMSL
    /// refresh command's granular options over the Power BI enhanced-refresh API).
    #[command(display_order = 11)]
    Refresh {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Refresh type
        #[arg(long, default_value = "Full")]
        r#type: String,

        /// Enhanced refresh: JSON array of tables/partitions to refresh, e.g.
        /// '[{"table":"Sales"},{"table":"Sales","partition":"2024"}]'
        #[arg(long)]
        objects: Option<String>,

        /// Enhanced refresh commit mode: transactional | partialBatch
        #[arg(long)]
        commit_mode: Option<String>,

        /// Enhanced refresh: maximum number of parallel processing threads
        #[arg(long)]
        max_parallelism: Option<u32>,

        /// Enhanced refresh: number of times to retry on transient failure
        #[arg(long)]
        retry_count: Option<u32>,
    },
    /// Take over a semantic model (converts definition-managed to service-managed for portal editing)
    #[command(display_order = 12)]
    Takeover {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List parameters of a semantic model
    #[command(name = "list-parameters", display_order = 13)]
    ListParameters {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List tables of a semantic model (via DAX INFO.VIEW.TABLES — no definition parsing)
    #[command(name = "list-tables", display_order = 13)]
    ListTables {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List columns of a semantic model (via DAX INFO.VIEW.COLUMNS)
    #[command(name = "list-columns", display_order = 13)]
    ListColumns {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List measures of a semantic model (via DAX INFO.VIEW.MEASURES)
    #[command(name = "list-measures", display_order = 13)]
    ListMeasures {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List relationships of a semantic model (via DAX INFO.VIEW.RELATIONSHIPS)
    #[command(name = "list-relationships", display_order = 13)]
    ListRelationships {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Update parameters of a semantic model
    #[command(name = "update-parameters", display_order = 14)]
    UpdateParameters {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// JSON content with parameter updates (inline or @file or @- for stdin)
        #[arg(long)]
        content: String,
    },
    /// List datasources of a semantic model
    #[command(name = "list-datasources", display_order = 15)]
    ListDatasources {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Update datasources of a semantic model
    #[command(name = "update-datasources", display_order = 16)]
    UpdateDatasources {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// JSON content with datasource updates (inline or @file or @- for stdin)
        #[arg(long)]
        content: String,
    },
    /// List users (permissions) of a semantic model
    #[command(name = "list-users", display_order = 17)]
    ListUsers {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Add a user to a semantic model
    #[command(name = "add-user", display_order = 18)]
    AddUser {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Principal identifier (email, OID, or group ID)
        #[arg(long)]
        principal: String,

        /// Principal type
        #[arg(long, value_parser = ["User", "Group", "App"])]
        principal_type: String,

        /// Access right for the dataset
        #[arg(long, value_parser = ["Read", "ReadExplore", "ReadReshare", "ReadReshareExplore"])]
        access_right: String,
    },
    /// Remove a user from a semantic model
    #[command(name = "delete-user", display_order = 19)]
    DeleteUser {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// User email or principal ID to remove
        #[arg(long)]
        user: String,
    },
    /// Get refresh history and status for a semantic model
    #[command(name = "refresh-status", display_order = 20)]
    RefreshStatus {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Maximum number of refresh entries to return (default: 10)
        #[arg(long, default_value = "10")]
        top: u32,
    },
    /// Get execution details of a specific (enhanced) refresh by its request id
    #[command(name = "refresh-details", display_order = 20)]
    RefreshDetails {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Refresh request id (the `requestId` from refresh-status)
        #[arg(long)]
        refresh_id: String,
    },
    /// Cancel an in-progress enhanced refresh by its request id
    #[command(name = "cancel-refresh", display_order = 20)]
    CancelRefresh {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Refresh request id (the `requestId` from refresh-status) to cancel
        #[arg(long)]
        refresh_id: String,
    },
    /// Get the scheduled (automatic) refresh configuration
    #[command(name = "get-refresh-schedule", display_order = 20)]
    GetRefreshSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Update the scheduled (automatic) refresh configuration
    ///
    /// Import / Direct Lake models. Times must be on the full or half hour
    /// (HH:00 / HH:30). To disable, pass ONLY --enabled false (the API rejects
    /// changing other settings while disabling).
    #[command(name = "update-refresh-schedule", display_order = 20)]
    UpdateRefreshSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Enable or disable the schedule
        #[arg(long)]
        enabled: Option<bool>,

        /// Comma-separated weekday names (e.g. "Monday,Thursday")
        #[arg(long)]
        days: Option<String>,

        /// Comma-separated times on the full/half hour (e.g. "07:00,13:30")
        #[arg(long)]
        times: Option<String>,

        /// Local time zone id (e.g. "UTC")
        #[arg(long)]
        local_time_zone_id: Option<String>,

        /// Failure/completion notification: NoNotification | MailOnFailure | MailOnCompletion
        #[allow(clippy::doc_markdown)]
        #[arg(long)]
        notify_option: Option<String>,
    },
    /// List upstream (lineage) datasets that this semantic model depends on
    #[command(name = "list-upstream", display_order = 21)]
    ListUpstream {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Clone a semantic model to the same or different workspace
    #[command(display_order = 22)]
    Clone {
        /// Source workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID to clone
        #[arg(long)]
        id: String,

        /// Display name for the cloned model
        #[arg(long)]
        name: String,

        /// Destination workspace ID (defaults to same workspace)
        #[arg(long, visible_alias = "dest-workspace")]
        target_workspace: Option<String>,
    },
    /// Export a semantic model as a .pbix file
    #[command(name = "export-pbix", display_order = 23)]
    ExportPbix {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Output file path (e.g., model.pbix)
        #[arg(long)]
        file: String,
    },
    /// Import a .pbix file as a new semantic model
    #[command(name = "import-pbix", display_order = 24)]
    ImportPbix {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name for the imported model
        #[arg(long)]
        name: String,

        /// Path to the .pbix file to import
        #[arg(long)]
        file: String,

        /// Conflict resolution: Abort, Overwrite, `CreateOrOverwrite`, `GenerateUniqueName`
        #[arg(long, default_value = "Abort")]
        name_conflict: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &SemanticModelCommand,
) -> Result<()> {
    match command {
        SemanticModelCommand::List { workspace } => crud::list(cli, client, workspace).await,
        SemanticModelCommand::Show { workspace, id } => {
            crud::show(cli, client, workspace, id).await
        }
        SemanticModelCommand::Create {
            workspace,
            name,
            description,
            file,
            connection,
            sensitivity_label,
        } => {
            crud::create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                file,
                connection.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        SemanticModelCommand::Update {
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
        SemanticModelCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => crud::delete(cli, client, workspace, id, *hard_delete).await,
        SemanticModelCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => definitions::get_definition(cli, client, workspace, id, *decode).await,
        SemanticModelCommand::UpdateDefinition {
            workspace,
            id,
            file,
        } => definitions::update_definition(cli, client, workspace, id, file).await,
        SemanticModelCommand::Query {
            workspace,
            id,
            dax,
            file,
        } => operations::query(cli, client, workspace, id, dax.as_deref(), file.as_deref()).await,
        SemanticModelCommand::BindConnection {
            workspace,
            id,
            connection_id,
        } => operations::bind_connection(cli, client, workspace, id, connection_id).await,
        SemanticModelCommand::UnbindConnection { workspace, id } => {
            operations::unbind_connection(cli, client, workspace, id).await
        }
        SemanticModelCommand::Refresh {
            workspace,
            id,
            r#type,
            objects,
            commit_mode,
            max_parallelism,
            retry_count,
        } => {
            operations::refresh(
                cli,
                client,
                workspace,
                id,
                r#type,
                objects.as_deref(),
                commit_mode.as_deref(),
                *max_parallelism,
                *retry_count,
            )
            .await
        }
        SemanticModelCommand::Takeover { workspace, id } => {
            operations::takeover(cli, client, workspace, id).await
        }
        SemanticModelCommand::ListParameters { workspace, id } => {
            powerbi::list_parameters(cli, client, workspace, id).await
        }
        SemanticModelCommand::ListTables { workspace, id } => {
            operations::list_tables(cli, client, workspace, id).await
        }
        SemanticModelCommand::ListColumns { workspace, id } => {
            operations::list_columns(cli, client, workspace, id).await
        }
        SemanticModelCommand::ListMeasures { workspace, id } => {
            operations::list_measures(cli, client, workspace, id).await
        }
        SemanticModelCommand::ListRelationships { workspace, id } => {
            operations::list_relationships(cli, client, workspace, id).await
        }
        SemanticModelCommand::UpdateParameters {
            workspace,
            id,
            content,
        } => powerbi::update_parameters(cli, client, workspace, id, content).await,
        SemanticModelCommand::ListDatasources { workspace, id } => {
            powerbi::list_datasources(cli, client, workspace, id).await
        }
        SemanticModelCommand::UpdateDatasources {
            workspace,
            id,
            content,
        } => powerbi::update_datasources(cli, client, workspace, id, content).await,
        SemanticModelCommand::ListUsers { workspace, id } => {
            powerbi::list_users(cli, client, workspace, id).await
        }
        SemanticModelCommand::AddUser {
            workspace,
            id,
            principal,
            principal_type,
            access_right,
        } => {
            powerbi::add_user(
                cli,
                client,
                workspace,
                id,
                principal,
                principal_type,
                access_right,
            )
            .await
        }
        SemanticModelCommand::DeleteUser {
            workspace,
            id,
            user,
        } => powerbi::delete_user(cli, client, workspace, id, user).await,
        SemanticModelCommand::RefreshStatus { workspace, id, top } => {
            powerbi::refresh_status(cli, client, workspace, id, *top).await
        }
        SemanticModelCommand::RefreshDetails {
            workspace,
            id,
            refresh_id,
        } => operations::refresh_details(cli, client, workspace, id, refresh_id).await,
        SemanticModelCommand::CancelRefresh {
            workspace,
            id,
            refresh_id,
        } => operations::cancel_refresh(cli, client, workspace, id, refresh_id).await,
        SemanticModelCommand::GetRefreshSchedule { workspace, id } => {
            operations::get_refresh_schedule(cli, client, workspace, id).await
        }
        SemanticModelCommand::UpdateRefreshSchedule {
            workspace,
            id,
            enabled,
            days,
            times,
            local_time_zone_id,
            notify_option,
        } => {
            operations::update_refresh_schedule(
                cli,
                client,
                workspace,
                id,
                *enabled,
                days.as_deref(),
                times.as_deref(),
                local_time_zone_id.as_deref(),
                notify_option.as_deref(),
            )
            .await
        }
        SemanticModelCommand::ListUpstream { workspace, id } => {
            powerbi::list_upstream(cli, client, workspace, id).await
        }
        SemanticModelCommand::Clone {
            workspace,
            id,
            name,
            target_workspace,
        } => {
            powerbi::clone_model(
                cli,
                client,
                workspace,
                id,
                name,
                target_workspace.as_deref(),
            )
            .await
        }
        SemanticModelCommand::ExportPbix {
            workspace,
            id,
            file,
        } => powerbi::export_pbix(cli, client, workspace, id, file).await,
        SemanticModelCommand::ImportPbix {
            workspace,
            name,
            file,
            name_conflict,
        } => powerbi::import_pbix(cli, client, workspace, name, file, name_conflict).await,
    }
}

pub(super) fn parse_json_content(content: &str, command: &str) -> Result<Value> {
    serde_json::from_str(content).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid JSON in --content: {e}"),
            format!(
                "Example: fabio semantic-model {command} --content '{{\"updateDetails\":[...]}}'"
            ),
        )
        .into()
    })
}
