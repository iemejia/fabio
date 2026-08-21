use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{execute_and_render_sql, parse_connection_string};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};

mod admin;
mod authoring;
mod crud;
mod insights;
mod mcp;
mod query;
mod restore_points;
mod retention;
mod schema;
mod statistics;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples warehouse\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum WarehouseCommand {
    /// List warehouses in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a warehouse
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,
    },
    /// Create a new warehouse
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,

        /// Collation for the warehouse (create-time only). Values:
        /// `Latin1_General_100_BIN2_UTF8` (case-sensitive, default) or
        /// `Latin1_General_100_CI_AS_KS_WS_SC_UTF8` (case-insensitive).
        #[arg(long)]
        collation: Option<String>,
    },
    /// Update warehouse properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a warehouse
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Execute a SQL query against a warehouse or SQL endpoint
    #[command(display_order = 10)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// SQL query to execute (prefix with @ to read from file, omit to read from stdin)
        #[arg(long)]
        sql: Option<String>,

        /// Execute via the remote Fabric DW MCP server (Fabric token, no direct TDS/1433)
        #[arg(long)]
        via_mcp: bool,
    },
    /// Capture the estimated execution plan (`SHOWPLAN_XML`) without executing the query
    #[command(display_order = 11)]
    Plan {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// SQL query to plan (prefix with @ to read from file, omit to read from stdin)
        #[arg(long)]
        sql: Option<String>,
    },
    /// Print the remote Fabric Data Warehouse MCP server URLs (item-scoped + global) for agent consumption
    #[command(display_order = 12)]
    McpUrl {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,
    },
    /// List tables and views in a warehouse (from `INFORMATION_SCHEMA.TABLES`)
    #[command(display_order = 13)]
    ListTables {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Only list tables in this schema (e.g. dbo)
        #[arg(long)]
        schema: Option<String>,
    },
    /// Describe the columns of a table (from `INFORMATION_SCHEMA.COLUMNS`)
    #[command(display_order = 14)]
    DescribeTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Table name, optionally schema-qualified (e.g. dbo.Customers or Customers)
        #[arg(long)]
        table: String,
    },
    /// Bulk-load files from Azure storage / `OneLake` into a table with `COPY INTO`
    #[command(display_order = 16)]
    CopyInto {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Target table, optionally schema-qualified (e.g. dbo.Orders). Must already exist.
        #[arg(long)]
        table: String,

        /// Source location: HTTPS Azure storage / `OneLake` URL (file or folder/wildcard)
        #[arg(long)]
        source: String,

        /// File format of the source data
        #[arg(long)]
        file_type: String,

        /// Optional target column list (comma-separated), matching the source order
        #[arg(long)]
        columns: Option<String>,

        /// CSV field terminator (e.g. ,)
        #[arg(long)]
        field_terminator: Option<String>,

        /// CSV row terminator (e.g. \n)
        #[arg(long)]
        row_terminator: Option<String>,

        /// CSV first data row (e.g. 2 to skip a header row)
        #[arg(long)]
        first_row: Option<u32>,

        /// CSV encoding (UTF8 or UTF16)
        #[arg(long)]
        encoding: Option<String>,

        /// SAS token for the source storage (omit to use the caller's Entra ID)
        #[arg(long)]
        sas_token: Option<String>,
    },
    /// Get the connection string for a warehouse
    #[command(display_order = 15)]
    ConnectionString {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Guest tenant ID (for cross-tenant access)
        #[arg(long)]
        guest_tenant_id: Option<String>,

        /// Private link type (for private endpoint access)
        #[arg(long)]
        private_link_type: Option<String>,
    },
    /// Get SQL pools configuration for a workspace
    #[command(display_order = 20)]
    GetSqlPoolsConfig {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Update SQL pools configuration for a workspace
    #[command(display_order = 21)]
    UpdateSqlPoolsConfig {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Path to JSON file with configuration (prefix with @)
        #[arg(long, group = "input")]
        file: Option<String>,

        /// Inline JSON content
        #[arg(long, group = "input")]
        content: Option<String>,
    },
    /// Get SQL audit settings for a warehouse
    #[command(display_order = 25)]
    GetAuditSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,
    },
    /// Update SQL audit settings for a warehouse
    #[command(display_order = 26)]
    UpdateAuditSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Audit state (e.g. Enabled, Disabled)
        #[arg(long)]
        state: Option<String>,

        /// Retention period in days
        #[arg(long)]
        retention_days: Option<u32>,

        /// Comma-separated list of audit actions
        #[arg(long)]
        audit_actions: Option<String>,

        /// Predicate expression for filtering audit logs (identity-based / column filters)
        #[arg(long)]
        predicate_expression: Option<String>,
    },
    /// Set audit actions and groups for a warehouse
    #[command(display_order = 27)]
    SetAuditActions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Comma-separated list of audit actions and groups
        #[arg(long, value_delimiter = ',')]
        actions: Vec<String>,
    },
    /// List restore points for a warehouse
    #[command(display_order = 30)]
    ListRestorePoints {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,
    },
    /// Create a restore point for a warehouse
    #[command(display_order = 31)]
    CreateRestorePoint {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Display name for the restore point
        #[arg(long)]
        name: Option<String>,

        /// Optional description for the restore point
        #[arg(long)]
        description: Option<String>,
    },
    /// Show details of a restore point
    #[command(display_order = 32)]
    ShowRestorePoint {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Restore point ID
        #[arg(long)]
        restore_point_id: String,
    },
    /// Update a restore point
    #[command(display_order = 33)]
    UpdateRestorePoint {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Restore point ID
        #[arg(long)]
        restore_point_id: String,

        /// New display name for the restore point
        #[arg(long)]
        name: Option<String>,

        /// New description for the restore point
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a restore point
    #[command(display_order = 34)]
    DeleteRestorePoint {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Restore point ID
        #[arg(long)]
        restore_point_id: String,
    },
    /// Restore a warehouse to a restore point
    #[command(display_order = 36)]
    RestoreToPoint {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Restore point ID
        #[arg(long)]
        restore_point_id: String,
    },

    // ── Query Insights ───────────────────────────────────────────────────
    /// List currently running queries on a warehouse
    #[command(display_order = 40)]
    QueriesRunning {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,
    },
    /// List frequently-run queries (from `queryinsights.frequently_run_queries`)
    #[command(display_order = 41)]
    QueriesFrequent {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Maximum rows to return (default: 100)
        #[arg(long, default_value = "100")]
        top: u32,
    },
    /// List long-running queries (from `queryinsights.long_running_queries`)
    #[command(display_order = 42)]
    QueriesLongRunning {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Maximum rows to return (default: 100)
        #[arg(long, default_value = "100")]
        top: u32,
    },
    /// List completed query history (from `queryinsights.exec_requests_history`)
    #[command(display_order = 43)]
    QueriesHistory {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Maximum rows to return (default: 100)
        #[arg(long, default_value = "100")]
        top: u32,

        /// Filter to queries tagged with this OPTION (LABEL = '...') value
        #[arg(long)]
        label: Option<String>,
    },
    /// Kill a running query session by session ID
    #[command(display_order = 44)]
    QueriesKill {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Session ID to terminate
        #[arg(long)]
        session_id: i32,
    },
    /// Report SQL pool state changes and sustained pressure events (from `queryinsights.sql_pool_insights`)
    #[command(display_order = 45)]
    PoolInsights {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Maximum rows to return (default: 100)
        #[arg(long, default_value = "100")]
        top: u32,
    },

    // ── Statistics ────────────────────────────────────────────────────────
    /// List user-defined statistics on a warehouse or SQL endpoint
    #[command(display_order = 50)]
    StatisticsList {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Filter by table name (schema.table)
        #[arg(long)]
        table: Option<String>,
    },
    /// Show details of a statistic (header, density vector, histogram)
    #[command(display_order = 51)]
    StatisticsShow {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Statistic name to inspect
        #[arg(long)]
        name: String,
    },
    /// Create a user-defined statistic on a column
    #[command(display_order = 52)]
    StatisticsCreate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Schema-qualified table name (e.g., dbo.orders)
        #[arg(long)]
        table: String,

        /// Column name to create statistics on
        #[arg(long)]
        column: String,

        /// Name for the new statistic
        #[arg(long)]
        name: String,
    },
    /// Update (refresh) an existing statistic
    #[command(display_order = 53)]
    StatisticsUpdate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Statistic name to update
        #[arg(long)]
        name: String,
    },
    /// Delete a user-defined statistic
    #[command(display_order = 54)]
    StatisticsDelete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse or Lakehouse ID
        #[arg(long)]
        id: String,

        /// Statistic name to delete
        #[arg(long)]
        name: String,
    },
    /// Report the configured data-retention (time-travel) period, in days
    #[command(display_order = 60)]
    GetRetention {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,
    },
    /// Configure the data-retention (time-travel) period, in days (1-120)
    #[command(display_order = 61)]
    SetRetention {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Warehouse ID
        #[arg(long)]
        id: String,

        /// Retention period in days (1-120). Decreasing it is irreversible (older history is garbage-collected).
        #[arg(long)]
        days: u32,
    },
}

#[allow(clippy::too_many_lines, clippy::large_stack_frames)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &WarehouseCommand) -> Result<()> {
    match command {
        WarehouseCommand::List { workspace } => crud::list(cli, client, workspace).await,
        WarehouseCommand::Show { workspace, id } => crud::show(cli, client, workspace, id).await,
        WarehouseCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
            collation,
        } => {
            crud::create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
                collation.as_deref(),
            )
            .await
        }
        WarehouseCommand::Update {
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
        WarehouseCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => crud::delete_warehouse(cli, client, workspace, id, *hard_delete).await,
        WarehouseCommand::Query {
            workspace,
            id,
            sql,
            via_mcp,
        } => {
            if *via_mcp {
                let sql_text = crate::commands::tds_utils::resolve_sql_input(sql.as_deref())?;
                Box::pin(crate::commands::sql_mcp::execute_via_mcp(
                    cli, client, workspace, id, &sql_text,
                ))
                .await
                .map_err(|e| enrich_forbidden(e, "warehouse query", "Viewer"))
            } else {
                Box::pin(query::query(cli, client, workspace, id, sql.as_deref()))
                    .await
                    .map_err(|e| enrich_forbidden(e, "warehouse query", "Viewer"))
            }
        }
        WarehouseCommand::Plan { workspace, id, sql } => {
            Box::pin(query::plan(cli, client, workspace, id, sql.as_deref()))
                .await
                .map_err(|e| enrich_forbidden(e, "warehouse plan", "Viewer"))
        }
        WarehouseCommand::McpUrl { workspace, id } => {
            mcp::mcp_url(cli, client, workspace, id).await
        }
        WarehouseCommand::ListTables {
            workspace,
            id,
            schema: schema_filter,
        } => {
            Box::pin(schema::list_tables(
                cli,
                client,
                workspace,
                id,
                schema_filter.as_deref(),
            ))
            .await
        }
        WarehouseCommand::DescribeTable {
            workspace,
            id,
            table,
        } => Box::pin(schema::describe_table(cli, client, workspace, id, table)).await,
        WarehouseCommand::CopyInto {
            workspace,
            id,
            table,
            source,
            file_type,
            columns,
            field_terminator,
            row_terminator,
            first_row,
            encoding,
            sas_token,
        } => {
            let args = authoring::CopyIntoArgs {
                table,
                source,
                file_type,
                columns: columns.as_deref(),
                field_terminator: field_terminator.as_deref(),
                row_terminator: row_terminator.as_deref(),
                first_row: *first_row,
                encoding: encoding.as_deref(),
                sas_token: sas_token.as_deref(),
            };
            Box::pin(authoring::copy_into(cli, client, workspace, id, &args)).await
        }
        WarehouseCommand::ConnectionString {
            workspace,
            id,
            guest_tenant_id,
            private_link_type,
        } => {
            admin::connection_string(
                cli,
                client,
                workspace,
                id,
                guest_tenant_id.as_deref(),
                private_link_type.as_deref(),
            )
            .await
        }
        WarehouseCommand::GetSqlPoolsConfig { workspace } => {
            admin::get_sql_pools_config(cli, client, workspace).await
        }
        WarehouseCommand::UpdateSqlPoolsConfig {
            workspace,
            file,
            content,
        } => {
            admin::update_sql_pools_config(
                cli,
                client,
                workspace,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        WarehouseCommand::GetAuditSettings { workspace, id } => {
            admin::get_audit_settings(cli, client, workspace, id).await
        }
        WarehouseCommand::UpdateAuditSettings {
            workspace,
            id,
            state,
            retention_days,
            audit_actions,
            predicate_expression,
        } => {
            admin::update_audit_settings(
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
        WarehouseCommand::SetAuditActions {
            workspace,
            id,
            actions,
        } => admin::set_audit_actions(cli, client, workspace, id, actions).await,
        WarehouseCommand::ListRestorePoints { workspace, id } => {
            restore_points::list_restore_points(cli, client, workspace, id).await
        }
        WarehouseCommand::CreateRestorePoint {
            workspace,
            id,
            name,
            description,
        } => {
            restore_points::create_restore_point(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        WarehouseCommand::ShowRestorePoint {
            workspace,
            id,
            restore_point_id,
        } => restore_points::show_restore_point(cli, client, workspace, id, restore_point_id).await,
        WarehouseCommand::UpdateRestorePoint {
            workspace,
            id,
            restore_point_id,
            name,
            description,
        } => {
            restore_points::update_restore_point(
                cli,
                client,
                workspace,
                id,
                restore_point_id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        WarehouseCommand::DeleteRestorePoint {
            workspace,
            id,
            restore_point_id,
        } => {
            restore_points::delete_restore_point(cli, client, workspace, id, restore_point_id).await
        }
        WarehouseCommand::RestoreToPoint {
            workspace,
            id,
            restore_point_id,
        } => restore_points::restore_to_point(cli, client, workspace, id, restore_point_id).await,
        WarehouseCommand::QueriesRunning { workspace, id } => {
            Box::pin(insights::queries_running(cli, client, workspace, id)).await
        }
        WarehouseCommand::QueriesFrequent { workspace, id, top } => {
            Box::pin(insights::queries_frequent(cli, client, workspace, id, *top)).await
        }
        WarehouseCommand::QueriesLongRunning { workspace, id, top } => {
            Box::pin(insights::queries_long_running(
                cli, client, workspace, id, *top,
            ))
            .await
        }
        WarehouseCommand::QueriesHistory {
            workspace,
            id,
            top,
            label,
        } => {
            Box::pin(insights::queries_history(
                cli,
                client,
                workspace,
                id,
                *top,
                label.as_deref(),
            ))
            .await
        }
        WarehouseCommand::QueriesKill {
            workspace,
            id,
            session_id,
        } => {
            Box::pin(insights::queries_kill(
                cli,
                client,
                workspace,
                id,
                *session_id,
            ))
            .await
        }
        WarehouseCommand::PoolInsights { workspace, id, top } => {
            Box::pin(insights::pool_insights(cli, client, workspace, id, *top)).await
        }
        WarehouseCommand::StatisticsList {
            workspace,
            id,
            table,
        } => {
            Box::pin(statistics::list(
                cli,
                client,
                workspace,
                id,
                table.as_deref(),
            ))
            .await
        }
        WarehouseCommand::StatisticsShow {
            workspace,
            id,
            name,
        } => Box::pin(statistics::show(cli, client, workspace, id, name)).await,
        WarehouseCommand::StatisticsCreate {
            workspace,
            id,
            table,
            column,
            name,
        } => {
            Box::pin(statistics::create(
                cli, client, workspace, id, table, column, name,
            ))
            .await
        }
        WarehouseCommand::StatisticsUpdate {
            workspace,
            id,
            name,
        } => Box::pin(statistics::update(cli, client, workspace, id, name)).await,
        WarehouseCommand::StatisticsDelete {
            workspace,
            id,
            name,
        } => Box::pin(statistics::delete(cli, client, workspace, id, name)).await,
        WarehouseCommand::GetRetention { workspace, id } => {
            Box::pin(retention::get_retention(cli, client, workspace, id)).await
        }
        WarehouseCommand::SetRetention {
            workspace,
            id,
            days,
        } => Box::pin(retention::set_retention(cli, client, workspace, id, *days)).await,
    }
}

/// Get SQL connection string from warehouse or lakehouse metadata.
/// Returns (`server_hostname`, `database_name`).
pub(super) async fn get_connection_string(
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<(String, String)> {
    // Item types whose SQL endpoint lives directly at `properties.connectionString`
    // (Warehouse, WarehouseSnapshot), and those that expose it under
    // `properties.sqlEndpointProperties.connectionString` (Lakehouse,
    // MirroredAzureDatabricksCatalog, MirroredDatabase). Each is tried in turn;
    // the first whose GET succeeds with a non-empty connection string wins.
    let direct = [
        ("warehouses", false),
        ("lakehouses", true),
        ("warehouseSnapshots", false),
        ("mirroredAzureDatabricksCatalogs", true),
        ("mirroredDatabases", true),
    ];
    for (collection, via_sql_endpoint) in direct {
        if let Ok(data) = client
            .get(&format!("/workspaces/{workspace}/{collection}/{id}"))
            .await
            && let Some(pair) = extract_connection(&data, via_sql_endpoint)
        {
            return Ok(pair);
        }
    }

    Err(FabioError {
        code: ErrorCode::NotFound,
        message: "Could not determine SQL connection string. Verify the item is a warehouse, lakehouse, warehouse snapshot, or mirrored database with a SQL endpoint.".into(),
        hint: Some(
            "Only Warehouse, Lakehouse, WarehouseSnapshot, MirroredAzureDatabricksCatalog, and MirroredDatabase items support SQL queries via this command.\n\
             For SQL Databases, use: fabio sql-database query\n\
             For lakehouses, pass the lakehouse ID (not the SQL endpoint ID).\n\
             List items: fabio item list --workspace <WS> --type Warehouse"
                .into(),
        ),
        hint_type: None,
        verify_after: None,
        retriable: None,
        request_id: None,
        more_details: None,
        related_resource: None,
    }.into())
}

/// Extract `(connectionString, displayName)` from an item's GET response.
///
/// When `via_sql_endpoint` is true the connection string is read from
/// `properties.sqlEndpointProperties.connectionString` (`Lakehouse`,
/// `MirroredAzureDatabricksCatalog`, `MirroredDatabase`); otherwise from
/// `properties.connectionString` directly (`Warehouse`, `WarehouseSnapshot`).
/// Returns `None` when the field is missing or empty.
fn extract_connection(data: &Value, via_sql_endpoint: bool) -> Option<(String, String)> {
    let props = data.get("properties")?;
    let conn = if via_sql_endpoint {
        props
            .get("sqlEndpointProperties")
            .and_then(|s| s.get("connectionString"))
    } else {
        props.get("connectionString")
    }
    .and_then(Value::as_str)
    .filter(|c| !c.is_empty())?;
    let db_name = data
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some((conn.to_string(), db_name))
}

/// Helper: resolve connection and execute a TDS query, rendering results as a list.
pub(super) async fn execute_insights_query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql_text: &str,
) -> Result<()> {
    let (connection_string, item_name) = get_connection_string(client, workspace, id).await?;
    let (server, parsed_db) = parse_connection_string(&connection_string);
    let database = if item_name.is_empty() {
        parsed_db
    } else {
        item_name
    };
    execute_and_render_sql(cli, client, &server, &database, sql_text).await
}

/// Helper: resolve connection and execute a TDS statement WITHOUT rendering its
/// result set (for DDL/config statements that return no rows). The caller renders
/// its own status object.
pub(super) async fn execute_insights_statement(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql_text: &str,
) -> Result<()> {
    let (connection_string, item_name) = get_connection_string(client, workspace, id).await?;
    let (server, parsed_db) = parse_connection_string(&connection_string);
    let database = if item_name.is_empty() {
        parsed_db
    } else {
        item_name
    };
    crate::commands::tds_utils::execute_sql_rows(client, &server, &database, sql_text).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_hostname() {
        let (server, db) = parse_connection_string("abc123.datawarehouse.fabric.microsoft.com");
        assert_eq!(server, "abc123.datawarehouse.fabric.microsoft.com");
        assert_eq!(db, "");
    }

    #[test]
    fn parse_hostname_with_port() {
        let (server, db) =
            parse_connection_string("abc123.datawarehouse.fabric.microsoft.com,1433");
        assert_eq!(server, "abc123.datawarehouse.fabric.microsoft.com");
        assert_eq!(db, "");
    }

    #[test]
    fn parse_jdbc_with_database() {
        let (server, db) = parse_connection_string(
            "jdbc:sqlserver://myserver.fabric.microsoft.com;database=MyDB;encrypt=true",
        );
        assert_eq!(server, "myserver.fabric.microsoft.com");
        assert_eq!(db, "MyDB");
    }

    #[test]
    fn parse_adonet_initial_catalog() {
        let (server, db) = parse_connection_string(
            "myserver.database.windows.net,1433;Initial Catalog=SalesDB;Encrypt=True",
        );
        assert_eq!(server, "myserver.database.windows.net");
        assert_eq!(db, "SalesDB");
    }

    #[test]
    fn parse_trims_whitespace() {
        let (server, db) = parse_connection_string("  abc.fabric.microsoft.com  ");
        assert_eq!(server, "abc.fabric.microsoft.com");
        assert_eq!(db, "");
    }

    #[test]
    fn parse_case_insensitive_database_key() {
        let (server, db) = parse_connection_string("host.com;DATABASE=TestDb;encrypt=true");
        assert_eq!(server, "host.com");
        assert_eq!(db, "TestDb");
    }

    #[test]
    fn extract_connection_direct_shape() {
        // Warehouse / WarehouseSnapshot: properties.connectionString
        let data = serde_json::json!({
            "displayName": "MyWH",
            "properties": {"connectionString": "wh.datawarehouse.fabric.microsoft.com"}
        });
        let (conn, name) = extract_connection(&data, false).unwrap();
        assert_eq!(conn, "wh.datawarehouse.fabric.microsoft.com");
        assert_eq!(name, "MyWH");
        // A sql-endpoint shape is NOT read in direct mode.
        assert!(extract_connection(&data, true).is_none());
    }

    #[test]
    fn extract_connection_sql_endpoint_shape() {
        // Lakehouse / MirroredAzureDatabricksCatalog / MirroredDatabase:
        // properties.sqlEndpointProperties.connectionString
        let data = serde_json::json!({
            "displayName": "OpenMir",
            "properties": {"sqlEndpointProperties": {"connectionString": "mir.datawarehouse.fabric.microsoft.com", "id": "abc"}}
        });
        let (conn, name) = extract_connection(&data, true).unwrap();
        assert_eq!(conn, "mir.datawarehouse.fabric.microsoft.com");
        assert_eq!(name, "OpenMir");
        // A direct shape is NOT read in sql-endpoint mode.
        assert!(extract_connection(&data, false).is_none());
    }

    #[test]
    fn extract_connection_empty_or_missing_is_none() {
        assert!(extract_connection(&serde_json::json!({}), false).is_none());
        assert!(
            extract_connection(
                &serde_json::json!({"properties": {"connectionString": ""}}),
                false
            )
            .is_none()
        );
        assert!(
            extract_connection(
                &serde_json::json!({"properties": {"sqlEndpointProperties": {}}}),
                true
            )
            .is_none()
        );
    }
}
