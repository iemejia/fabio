mod audit;
mod crud;
mod definitions;
mod import;
mod insights;
mod mirroring;
mod query;
mod statistics;

use anyhow::Result;
use clap::Subcommand;

use crate::cli::Cli;
use crate::client::FabricClient;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples sql-database\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum SqlDatabaseCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List SQL databases in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a SQL database
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,
    },
    /// Create a new SQL database
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

        /// Creation mode: `New`, `Restore`, or `RestoreDeletedDatabase`
        #[arg(long, default_value = "New")]
        creation_mode: Option<String>,

        /// Backup retention period in days (1-35, for mode=New)
        #[arg(long)]
        backup_retention_days: Option<i32>,

        /// Database collation (for mode=New)
        #[arg(long)]
        collation: Option<String>,

        /// Source database workspace ID (for mode=Restore)
        #[arg(long)]
        source_workspace: Option<String>,

        /// Source database item ID (for mode=Restore)
        #[arg(long)]
        source_database: Option<String>,

        /// Point-in-time to restore (ISO 8601, for mode=Restore or `RestoreDeletedDatabase`)
        #[arg(long)]
        restore_point: Option<String>,

        /// Name of the restorable deleted database (for mode=RestoreDeletedDatabase)
        #[arg(long)]
        restorable_deleted_database_name: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update SQL database properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a SQL database
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a SQL database (dacpac or sqlproj format)
    #[command(display_order = 10)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Definition format: dacpac or sqlproj (default: dacpac)
        #[arg(long)]
        format: Option<String>,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a SQL database
    #[command(display_order = 11)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Definition file path (.dacpac or .sqlproj)
        #[arg(long)]
        file: Option<String>,

        /// Definition as inline base64 content
        #[arg(long)]
        content: Option<String>,

        /// Definition format: dacpac or sqlproj (default: dacpac)
        #[arg(long)]
        format: Option<String>,

        /// Also update item metadata from the definition
        #[arg(long)]
        update_metadata: bool,
    },

    // ── Mirroring ────────────────────────────────────────────────────────
    /// Start mirroring for the SQL database
    #[command(display_order = 20)]
    StartMirroring {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,
    },
    /// Stop mirroring for the SQL database
    #[command(display_order = 21)]
    StopMirroring {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,
    },

    // ── CMK ──────────────────────────────────────────────────────────────
    /// Revalidate Customer-Managed Key (CMK) for the SQL database
    #[command(display_order = 30)]
    RevalidateCmk {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,
    },

    // ── Audit settings ───────────────────────────────────────────────────
    /// Get SQL audit settings for the database
    #[command(display_order = 40)]
    GetAuditSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,
    },
    /// Update SQL audit settings for the database
    #[command(display_order = 41)]
    UpdateAuditSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Audit state: Enabled or Disabled
        #[arg(long)]
        state: Option<String>,

        /// Retention days (0 = indefinite)
        #[arg(long)]
        retention_days: Option<i64>,

        /// Audit actions and groups (comma-separated)
        #[arg(long, value_delimiter = ',')]
        audit_actions: Option<Vec<String>>,

        /// Predicate expression for filtering audit logs
        #[arg(long)]
        predicate_expression: Option<String>,
    },

    // ── Restorable deleted databases ─────────────────────────────────────
    /// List restorable deleted SQL databases in a workspace
    #[command(display_order = 50)]
    ListDeleted {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },

    // ── Query & connectivity ─────────────────────────────────────────────
    /// Execute a SQL query against a SQL database via TDS
    #[command(display_order = 60)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// SQL query to execute (prefix with @ to read from file, omit to read from stdin)
        #[arg(long)]
        sql: Option<String>,
    },
    /// Capture the estimated execution plan (`SHOWPLAN_XML`) without executing the query
    #[command(display_order = 61)]
    Plan {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// SQL query to plan (prefix with @ to read from file, omit to read from stdin)
        #[arg(long)]
        sql: Option<String>,
    },
    /// Show the TDS connection string for a SQL database
    #[command(display_order = 62)]
    ConnectionString {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,
    },

    /// Import data from a CSV or JSON file into a SQL database table
    ///
    /// Reads the file, infers column types, creates the table (unless --no-create-table),
    /// and inserts all rows via batched INSERT statements over TDS.
    #[command(display_order = 62)]
    Import {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Path to the CSV or JSON file to import
        #[arg(long)]
        file: String,

        /// Target table name (default: inferred from filename)
        #[arg(long)]
        table: Option<String>,

        /// Skip automatic CREATE TABLE (table must already exist)
        #[arg(long)]
        no_create_table: bool,

        /// Drop and recreate the table if it already exists
        #[arg(long)]
        drop_if_exists: bool,

        /// Batch size for INSERT statements (default: 100)
        #[arg(long, default_value = "100")]
        batch_size: usize,
    },

    // ── Query Insights ───────────────────────────────────────────────────
    /// List currently running queries on a SQL database
    #[command(display_order = 70)]
    QueriesRunning {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,
    },
    /// List completed query history
    #[command(display_order = 71)]
    QueriesHistory {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Number of rows to return
        #[arg(long, default_value = "100")]
        top: u32,

        /// Filter to queries tagged with this OPTION (LABEL = '...') value
        #[arg(long)]
        label: Option<String>,
    },
    /// Kill a running query session by session ID
    #[command(display_order = 72)]
    QueriesKill {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Session ID to kill
        #[arg(long)]
        session_id: i32,
    },

    // ── Statistics ────────────────────────────────────────────────────────
    /// List statistics on a SQL database
    #[command(display_order = 80)]
    StatisticsList {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Filter by table (schema.table or just table name)
        #[arg(long)]
        table: Option<String>,
    },
    /// Show details of a statistic
    #[command(display_order = 81)]
    StatisticsShow {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Statistic name
        #[arg(long)]
        name: String,
    },
    /// Create a user-defined statistic on a column
    #[command(display_order = 82)]
    StatisticsCreate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Table name (schema.table or just table name)
        #[arg(long)]
        table: String,

        /// Column name
        #[arg(long)]
        column: String,

        /// Statistic name
        #[arg(long)]
        name: String,
    },
    /// Update (refresh) an existing statistic
    #[command(display_order = 83)]
    StatisticsUpdate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Statistic name
        #[arg(long)]
        name: String,
    },
    /// Delete a user-defined statistic
    #[command(display_order = 84)]
    StatisticsDelete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Statistic name
        #[arg(long)]
        name: String,
    },
}

#[allow(
    clippy::too_many_lines,
    clippy::large_futures,
    clippy::large_stack_frames
)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &SqlDatabaseCommand) -> Result<()> {
    match command {
        SqlDatabaseCommand::List { workspace } => crud::list(cli, client, workspace).await,
        SqlDatabaseCommand::Show { workspace, id } => crud::show(cli, client, workspace, id).await,
        SqlDatabaseCommand::Create {
            workspace,
            name,
            description,
            creation_mode,
            backup_retention_days,
            collation,
            source_workspace,
            source_database,
            restore_point,
            restorable_deleted_database_name,
            sensitivity_label,
        } => {
            crud::create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                creation_mode.as_deref(),
                *backup_retention_days,
                collation.as_deref(),
                source_workspace.as_deref(),
                source_database.as_deref(),
                restore_point.as_deref(),
                restorable_deleted_database_name.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        SqlDatabaseCommand::Update {
            workspace,
            id,
            description,
        } => crud::update(cli, client, workspace, id, description.as_deref()).await,
        SqlDatabaseCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => crud::delete(cli, client, workspace, id, *hard_delete).await,
        SqlDatabaseCommand::GetDefinition {
            workspace,
            id,
            format,
            decode,
        } => {
            definitions::get_definition(cli, client, workspace, id, format.as_deref(), *decode)
                .await
        }
        SqlDatabaseCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
            format,
            update_metadata,
        } => {
            definitions::update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
                format.as_deref(),
                *update_metadata,
            )
            .await
        }
        SqlDatabaseCommand::StartMirroring { workspace, id } => {
            mirroring::start_mirroring(cli, client, workspace, id).await
        }
        SqlDatabaseCommand::StopMirroring { workspace, id } => {
            mirroring::stop_mirroring(cli, client, workspace, id).await
        }
        SqlDatabaseCommand::RevalidateCmk { workspace, id } => {
            audit::revalidate_cmk(cli, client, workspace, id).await
        }
        SqlDatabaseCommand::GetAuditSettings { workspace, id } => {
            audit::get_audit_settings(cli, client, workspace, id).await
        }
        SqlDatabaseCommand::UpdateAuditSettings {
            workspace,
            id,
            state,
            retention_days,
            audit_actions,
            predicate_expression,
        } => {
            audit::update_audit_settings(
                cli,
                client,
                workspace,
                id,
                state.as_deref(),
                *retention_days,
                audit_actions.as_deref(),
                predicate_expression.as_deref(),
            )
            .await
        }
        SqlDatabaseCommand::ListDeleted { workspace } => {
            audit::list_deleted(cli, client, workspace).await
        }
        SqlDatabaseCommand::Query { workspace, id, sql } => {
            query::query(cli, client, workspace, id, sql.as_deref()).await
        }
        SqlDatabaseCommand::Plan { workspace, id, sql } => {
            query::plan(cli, client, workspace, id, sql.as_deref()).await
        }
        SqlDatabaseCommand::ConnectionString { workspace, id } => {
            query::connection_string(cli, client, workspace, id).await
        }
        SqlDatabaseCommand::Import {
            workspace,
            id,
            file,
            table,
            no_create_table,
            drop_if_exists,
            batch_size,
        } => {
            import::import(
                cli,
                client,
                workspace,
                id,
                file,
                table.as_deref(),
                *no_create_table,
                *drop_if_exists,
                *batch_size,
            )
            .await
        }

        // ── Query Insights ───────────────────────────────────────────────
        SqlDatabaseCommand::QueriesRunning { workspace, id } => {
            insights::queries_running(cli, client, workspace, id).await
        }
        SqlDatabaseCommand::QueriesHistory {
            workspace,
            id,
            top,
            label,
        } => insights::queries_history(cli, client, workspace, id, *top, label.as_deref()).await,
        SqlDatabaseCommand::QueriesKill {
            workspace,
            id,
            session_id,
        } => insights::queries_kill(cli, client, workspace, id, *session_id).await,

        // ── Statistics ────────────────────────────────────────────────────
        SqlDatabaseCommand::StatisticsList {
            workspace,
            id,
            table,
        } => statistics::list(cli, client, workspace, id, table.as_deref()).await,
        SqlDatabaseCommand::StatisticsShow {
            workspace,
            id,
            name,
        } => statistics::show(cli, client, workspace, id, name).await,
        SqlDatabaseCommand::StatisticsCreate {
            workspace,
            id,
            table,
            column,
            name,
        } => statistics::create(cli, client, workspace, id, table, column, name).await,
        SqlDatabaseCommand::StatisticsUpdate {
            workspace,
            id,
            name,
        } => statistics::update(cli, client, workspace, id, name).await,
        SqlDatabaseCommand::StatisticsDelete {
            workspace,
            id,
            name,
        } => statistics::delete(cli, client, workspace, id, name).await,
    }
}
