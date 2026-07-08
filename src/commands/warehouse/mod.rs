use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{execute_and_render_sql, parse_connection_string};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};

mod admin;
mod crud;
mod insights;
mod query;
mod restore_points;
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

        /// Optional label for the restore point
        #[arg(long)]
        name: Option<String>,
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

        /// New label for the restore point
        #[arg(long)]
        name: Option<String>,
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

        /// Name for the restored warehouse
        #[arg(long)]
        name: String,
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
}

#[allow(
    clippy::too_many_lines,
    clippy::large_stack_frames,
    clippy::large_futures
)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &WarehouseCommand) -> Result<()> {
    match command {
        WarehouseCommand::List { workspace } => crud::list(cli, client, workspace).await,
        WarehouseCommand::Show { workspace, id } => crud::show(cli, client, workspace, id).await,
        WarehouseCommand::Create {
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
        WarehouseCommand::Query { workspace, id, sql } => {
            query::query(cli, client, workspace, id, sql.as_deref())
                .await
                .map_err(|e| enrich_forbidden(e, "warehouse query", "Viewer"))
        }
        WarehouseCommand::Plan { workspace, id, sql } => {
            query::plan(cli, client, workspace, id, sql.as_deref())
                .await
                .map_err(|e| enrich_forbidden(e, "warehouse plan", "Viewer"))
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
        } => {
            admin::update_audit_settings(
                cli,
                client,
                workspace,
                id,
                state.as_deref(),
                *retention_days,
                audit_actions.as_deref(),
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
        } => {
            restore_points::create_restore_point(cli, client, workspace, id, name.as_deref()).await
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
        } => {
            restore_points::update_restore_point(
                cli,
                client,
                workspace,
                id,
                restore_point_id,
                name.as_deref(),
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
            name,
        } => {
            restore_points::restore_to_point(cli, client, workspace, id, restore_point_id, name)
                .await
        }
        WarehouseCommand::QueriesRunning { workspace, id } => {
            insights::queries_running(cli, client, workspace, id).await
        }
        WarehouseCommand::QueriesFrequent { workspace, id, top } => {
            insights::queries_frequent(cli, client, workspace, id, *top).await
        }
        WarehouseCommand::QueriesLongRunning { workspace, id, top } => {
            insights::queries_long_running(cli, client, workspace, id, *top).await
        }
        WarehouseCommand::QueriesHistory { workspace, id, top } => {
            insights::queries_history(cli, client, workspace, id, *top).await
        }
        WarehouseCommand::QueriesKill {
            workspace,
            id,
            session_id,
        } => insights::queries_kill(cli, client, workspace, id, *session_id).await,
        WarehouseCommand::StatisticsList {
            workspace,
            id,
            table,
        } => statistics::list(cli, client, workspace, id, table.as_deref()).await,
        WarehouseCommand::StatisticsShow {
            workspace,
            id,
            name,
        } => statistics::show(cli, client, workspace, id, name).await,
        WarehouseCommand::StatisticsCreate {
            workspace,
            id,
            table,
            column,
            name,
        } => statistics::create(cli, client, workspace, id, table, column, name).await,
        WarehouseCommand::StatisticsUpdate {
            workspace,
            id,
            name,
        } => statistics::update(cli, client, workspace, id, name).await,
        WarehouseCommand::StatisticsDelete {
            workspace,
            id,
            name,
        } => statistics::delete(cli, client, workspace, id, name).await,
    }
}

/// Get SQL connection string from warehouse or lakehouse metadata.
/// Returns (`server_hostname`, `database_name`).
pub(super) async fn get_connection_string(
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<(String, String)> {
    // Try warehouse endpoint first
    if let Ok(data) = client
        .get(&format!("/workspaces/{workspace}/warehouses/{id}"))
        .await
        && let Some(conn) = data
            .get("properties")
            .and_then(|p| p.get("connectionString"))
            .and_then(Value::as_str)
        && !conn.is_empty()
    {
        let db_name = data
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        return Ok((conn.to_string(), db_name));
    }

    // Fall back to lakehouse SQL endpoint
    if let Ok(data) = client
        .get(&format!("/workspaces/{workspace}/lakehouses/{id}"))
        .await
        && let Some(conn) = data
            .get("properties")
            .and_then(|p| p.get("sqlEndpointProperties"))
            .and_then(|s| s.get("connectionString"))
            .and_then(Value::as_str)
        && !conn.is_empty()
    {
        let db_name = data
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        return Ok((conn.to_string(), db_name));
    }

    Err(FabioError {
        code: ErrorCode::NotFound,
        message: "Could not determine SQL connection string. Verify the item is a warehouse or lakehouse with a SQL endpoint.".into(),
        hint: Some(
            "Only Warehouse and Lakehouse items support SQL queries via this command.\n\
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
}
