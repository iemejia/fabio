use std::io::{self, Read};

use anyhow::Result;
use base64::Engine;
use clap::Subcommand;
use mssql_tds::connection::client_context::{ClientContext, TdsAuthenticationMethod};
use mssql_tds::connection::tds_client::{ResultSet, ResultSetClient};
use mssql_tds::connection_provider::tds_connection_provider::TdsConnectionProvider;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::column_value_to_json;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
pub enum SqlDatabaseCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List SQL databases in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long)]
        workspace: String,
    },
    /// Show details of a SQL database
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long)]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,
    },
    /// Create a new SQL database
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long)]
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
    },
    /// Update SQL database properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long)]
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
        #[arg(short, long)]
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
        #[arg(short, long)]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,

        /// Definition format: dacpac or sqlproj (default: dacpac)
        #[arg(long)]
        format: Option<String>,
    },
    /// Update the definition of a SQL database
    #[command(display_order = 11)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long)]
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
        #[arg(short, long)]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,
    },
    /// Stop mirroring for the SQL database
    #[command(display_order = 21)]
    StopMirroring {
        /// Workspace ID
        #[arg(short, long)]
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
        #[arg(short, long)]
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
        #[arg(short, long)]
        workspace: String,

        /// SQL database ID
        #[arg(long)]
        id: String,
    },
    /// Update SQL audit settings for the database
    #[command(display_order = 41)]
    UpdateAuditSettings {
        /// Workspace ID
        #[arg(short, long)]
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
        #[arg(short, long)]
        workspace: String,
    },

    // ── Query & connectivity ─────────────────────────────────────────────
    /// Execute a SQL query against a SQL database via TDS
    #[command(display_order = 60)]
    Query {
        /// Workspace ID
        #[arg(short, long)]
        workspace: String,

        /// SQL database item ID
        #[arg(long)]
        id: String,

        /// SQL query to execute (prefix with @ to read from file, omit to read from stdin)
        #[arg(long)]
        sql: Option<String>,
    },
    /// Show the TDS connection string for a SQL database
    #[command(display_order = 61)]
    ConnectionString {
        /// Workspace ID
        #[arg(short, long)]
        workspace: String,

        /// SQL database item ID
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
        #[arg(short, long)]
        workspace: String,

        /// SQL database item ID
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
}

#[allow(clippy::too_many_lines, clippy::large_futures)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &SqlDatabaseCommand) -> Result<()> {
    match command {
        SqlDatabaseCommand::List { workspace } => list(cli, client, workspace).await,
        SqlDatabaseCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
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
        } => {
            create(
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
            )
            .await
        }
        SqlDatabaseCommand::Update {
            workspace,
            id,
            description,
        } => update(cli, client, workspace, id, description.as_deref()).await,
        SqlDatabaseCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        SqlDatabaseCommand::GetDefinition {
            workspace,
            id,
            format,
        } => get_definition(cli, client, workspace, id, format.as_deref()).await,
        SqlDatabaseCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
            format,
            update_metadata,
        } => {
            update_definition(
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
            start_mirroring(cli, client, workspace, id).await
        }
        SqlDatabaseCommand::StopMirroring { workspace, id } => {
            stop_mirroring(cli, client, workspace, id).await
        }
        SqlDatabaseCommand::RevalidateCmk { workspace, id } => {
            revalidate_cmk(cli, client, workspace, id).await
        }
        SqlDatabaseCommand::GetAuditSettings { workspace, id } => {
            get_audit_settings(cli, client, workspace, id).await
        }
        SqlDatabaseCommand::UpdateAuditSettings {
            workspace,
            id,
            state,
            retention_days,
            audit_actions,
            predicate_expression,
        } => {
            update_audit_settings(
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
        SqlDatabaseCommand::ListDeleted { workspace } => list_deleted(cli, client, workspace).await,
        SqlDatabaseCommand::Query { workspace, id, sql } => {
            query(cli, client, workspace, id, sql.as_deref()).await
        }
        SqlDatabaseCommand::ConnectionString { workspace, id } => {
            connection_string(cli, client, workspace, id).await
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
            import(
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
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/sqlDatabases"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/sqlDatabases/{id}"))
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
    creation_mode: Option<&str>,
    backup_retention_days: Option<i32>,
    collation: Option<&str>,
    source_workspace: Option<&str>,
    source_database: Option<&str>,
    restore_point: Option<&str>,
    restorable_deleted_database_name: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = Value::String(desc.to_string());
    }

    // Build creationPayload based on mode
    let mode = creation_mode.unwrap_or("New");
    match mode {
        "Restore" => {
            let src_ws = source_workspace.ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--source-workspace is required for Restore mode".to_string(),
                    "Example: fabio sql-database create --workspace <WS> --name <NAME> --creation-mode Restore --source-workspace <SRC_WS> --source-database <SRC_ID> --restore-point 2024-01-01T00:00:00Z".to_string(),
                )
            })?;
            let src_db = source_database.ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--source-database is required for Restore mode".to_string(),
                    "Provide the item ID of the source database to restore from".to_string(),
                )
            })?;
            let rp = restore_point.ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--restore-point is required for Restore mode".to_string(),
                    "Provide an ISO 8601 timestamp (e.g., 2024-01-01T00:00:00Z)".to_string(),
                )
            })?;
            body["creationPayload"] = serde_json::json!({
                "creationMode": "Restore",
                "restorePointInTime": rp,
                "sourceDatabaseReference": {
                    "workspaceId": src_ws,
                    "id": src_db
                }
            });
        }
        "RestoreDeletedDatabase" => {
            let deleted_name = restorable_deleted_database_name.ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--restorable-deleted-database-name is required for RestoreDeletedDatabase mode".to_string(),
                    "Use 'fabio sql-database list-deleted' to find available names".to_string(),
                )
            })?;
            let rp = restore_point.ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--restore-point is required for RestoreDeletedDatabase mode".to_string(),
                    "Provide an ISO 8601 timestamp (e.g., 2024-01-01T00:00:00Z)".to_string(),
                )
            })?;
            body["creationPayload"] = serde_json::json!({
                "creationMode": "RestoreDeletedDatabase",
                "restorePointInTime": rp,
                "restorableDeletedDatabaseName": deleted_name
            });
        }
        _ => {
            // "New" mode or default
            let mut payload = serde_json::json!({ "creationMode": "New" });
            if let Some(days) = backup_retention_days {
                payload["backupRetentionDays"] = Value::Number(serde_json::Number::from(days));
            }
            if let Some(c) = collation {
                payload["collation"] = Value::String(c.to_string());
            }
            // Only include creationPayload if there are extra settings
            if backup_retention_days.is_some() || collation.is_some() {
                body["creationPayload"] = payload;
            }
        }
    }

    if output::dry_run_guard(cli, "sql-database create", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/sqlDatabases"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    description: Option<&str>,
) -> Result<()> {
    if description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least --description must be provided".to_string(),
            "Example: fabio sql-database update --workspace <WS> --id <ID> --description \"New desc\""
                .to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(d) = description {
        body["description"] = Value::String(d.to_string());
    }

    if output::dry_run_guard(cli, "sql-database update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/sqlDatabases/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database update", "Contributor"))?;
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
        "sql-database delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/sqlDatabases/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/sqlDatabases/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<&str>,
) -> Result<()> {
    let url = format.map_or_else(
        || format!("/workspaces/{workspace}/sqlDatabases/{id}/getDefinition"),
        |f| format!("/workspaces/{workspace}/sqlDatabases/{id}/getDefinition?format={f}"),
    );

    let data = client
        .post(&url, &serde_json::json!({}), true)
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database get-definition", "Contributor"))?;
    output::render_object(cli, &data, "definition");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    format: Option<&str>,
    update_metadata: bool,
) -> Result<()> {
    let payload_bytes = match (file, content) {
        (Some(path), _) => {
            std::fs::read(path).map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?
        }
        (_, Some(c)) => c.as_bytes().to_vec(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio sql-database update-definition --workspace <WS> --id <ID> --file schema.dacpac".to_string(),
            ).into());
        }
    };

    let encoded = base64::engine::general_purpose::STANDARD.encode(&payload_bytes);
    let fmt = format.unwrap_or("dacpac");
    let extension = match fmt {
        "sqlproj" => "sqlproj",
        _ => "dacpac",
    };

    let body = serde_json::json!({
        "definition": {
            "format": fmt,
            "parts": [
                {
                    "path": format!("definition.{extension}"),
                    "payload": encoded,
                    "payloadType": "InlineBase64"
                }
            ]
        }
    });

    if output::dry_run_guard(
        cli,
        "sql-database update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "format": fmt,
            "contentLength": payload_bytes.len()
        }),
    ) {
        return Ok(());
    }

    let mut url = format!("/workspaces/{workspace}/sqlDatabases/{id}/updateDefinition");
    if update_metadata {
        url.push_str("?updateMetadata=true");
    }

    let data = client
        .post(&url, &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Mirroring ───────────────────────────────────────────────────────────────

async fn start_mirroring(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "sql-database start-mirroring",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/sqlDatabases/{id}/startMirroring"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database start-mirroring", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "mirroring_started" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn stop_mirroring(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "sql-database stop-mirroring",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/sqlDatabases/{id}/stopMirroring"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database stop-mirroring", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "mirroring_stopped" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── CMK ─────────────────────────────────────────────────────────────────────

async fn revalidate_cmk(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "sql-database revalidate-cmk",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/sqlDatabases/{id}/revalidateCMK"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database revalidate-cmk", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "cmk_revalidated" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Audit settings ──────────────────────────────────────────────────────────

async fn get_audit_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/sqlDatabases/{id}/settings/sqlAudit"
        ))
        .await?;
    output::render_object(cli, &data, "state");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_audit_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    state: Option<&str>,
    retention_days: Option<i64>,
    audit_actions: Option<&[String]>,
    predicate_expression: Option<&str>,
) -> Result<()> {
    if state.is_none()
        && retention_days.is_none()
        && audit_actions.is_none()
        && predicate_expression.is_none()
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one audit setting must be provided".to_string(),
            "Options: --state Enabled|Disabled, --retention-days N, --audit-actions GROUP1,GROUP2, --predicate-expression EXPR".to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(s) = state {
        body["state"] = Value::String(s.to_string());
    }
    if let Some(days) = retention_days {
        body["retentionDays"] = Value::Number(serde_json::Number::from(days));
    }
    if let Some(actions) = audit_actions {
        body["auditActionsAndGroups"] =
            Value::Array(actions.iter().map(|a| Value::String(a.clone())).collect());
    }
    if let Some(pred) = predicate_expression {
        body["predicateExpression"] = Value::String(pred.to_string());
    }

    if output::dry_run_guard(cli, "sql-database update-audit-settings", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/sqlDatabases/{id}/settings/sqlAudit"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database update-audit-settings", "Contributor"))?;
    output::render_object(cli, &data, "state");
    Ok(())
}

// ─── Restorable deleted databases ────────────────────────────────────────────

async fn list_deleted(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/sqlDatabases/restorableDeletedDatabases"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &[
            "displayName",
            "properties.restorableDeletedDatabaseName",
            "properties.deletionTimestamp",
        ],
        &["NAME", "RESTORABLE_NAME", "DELETED_AT"],
        "displayName",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

// ─── Query & connectivity ────────────────────────────────────────────────────

/// Resolve SQL database connection info: returns (`server_host`, `port`, `database_name`).
async fn resolve_sql_connection(
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<(String, u16, String)> {
    let data = client
        .get(&format!("/workspaces/{workspace}/sqlDatabases/{id}"))
        .await?;

    let raw_server = data
        .get("properties")
        .and_then(|p| p.get("serverFqdn"))
        .and_then(Value::as_str)
        .or_else(|| {
            data.get("properties")
                .and_then(|p| p.get("connectionString"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::NotFound,
                "Could not determine SQL server for this database. Verify it is provisioned.",
            )
        })?;

    // serverFqdn may include port (e.g., "host.database.fabric.microsoft.com,1433")
    let (host, port) = if let Some((h, p)) = raw_server.rsplit_once(',') {
        (h.to_string(), p.parse::<u16>().unwrap_or(1433))
    } else {
        (raw_server.to_string(), 1433)
    };

    let database = data
        .get("properties")
        .and_then(|p| p.get("databaseName"))
        .and_then(Value::as_str)
        .or_else(|| data.get("displayName").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();

    Ok((host, port, database))
}

#[allow(clippy::too_many_lines)]
async fn query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql: Option<&str>,
) -> Result<()> {
    // Resolve SQL text: --sql flag, @file prefix, or stdin
    let sql_text = match sql {
        Some(s) if s.starts_with('@') => {
            let file_path = &s[1..];
            std::fs::read_to_string(file_path).map_err(|e| {
                FabioError::not_found(format!("SQL file not found: {file_path}: {e}"))
            })?
        }
        Some(s) => s.to_string(),
        None => {
            // Read from stdin
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).map_err(|e| {
                FabioError::new(
                    ErrorCode::ApiError,
                    format!("Failed to read SQL from stdin: {e}"),
                )
            })?;
            if buf.trim().is_empty() {
                return Err(FabioError::new(
                    ErrorCode::ApiError,
                    "No SQL provided. Use --sql, @file, or pipe SQL via stdin.",
                )
                .into());
            }
            buf
        }
    };

    // Resolve connection details
    let (server, port, database) = resolve_sql_connection(client, workspace, id).await?;

    // Acquire AAD token for SQL scope
    let token = client.require_sql_auth().await?;

    // Build TDS connection
    let data_source = format!("tcp:{server},{port}");
    let mut context = ClientContext::with_data_source(&data_source);
    context.database = database;
    context.tds_authentication_method = TdsAuthenticationMethod::AccessToken;
    context.access_token = Some(token);
    context.application_name = "fabio".to_string();
    context.connect_timeout = 30;

    let provider = TdsConnectionProvider {};
    let mut tds_client = provider
        .create_client(context, &data_source, None)
        .await
        .map_err(|e| {
            let msg = format!("{e}");
            let hint = if msg.contains("18456") {
                ". Hint: Fabric SQL Database requires F4+ capacity. On F2, TDS connections \
                 fail with 'Validation of user's permissions failed' due to insufficient \
                 compute, not actual permissions issues. Scale your capacity to F4 or higher."
            } else {
                ""
            };
            FabioError::new(
                ErrorCode::ApiError,
                format!("TDS connection failed: {e}{hint}"),
            )
        })?;

    // Execute SQL
    tds_client
        .execute(sql_text, Some(60), None)
        .await
        .map_err(|e| {
            let msg = format!("{e}");
            let hint = if msg.contains("40515") {
                ". Hint: Fabric SQL Database does not support cross-database queries via \
                 three-part naming ([Database].[schema].[table]). Use a lakehouse SQL \
                 endpoint or warehouse instead — they support cross-database queries to \
                 SQL Databases in the same workspace."
            } else if msg.contains("Invalid object name") && msg.contains("sys.") {
                ". Hint: Fabric SQL Database does not support all SQL Server system views. \
                 Supported: sys.tables, sys.columns, sys.schemas, sys.types, \
                 INFORMATION_SCHEMA.TABLES, INFORMATION_SCHEMA.COLUMNS"
            } else {
                ""
            };
            FabioError::new(
                ErrorCode::ApiError,
                format!("SQL execution failed: {e}{hint}"),
            )
        })?;

    // Collect results
    let mut all_rows: Vec<Value> = Vec::new();
    let mut columns: Vec<String> = Vec::new();

    if let Some(rs) = tds_client.get_current_resultset() {
        // Get column names from metadata
        columns = rs
            .get_metadata()
            .iter()
            .map(|col| col.column_name.clone())
            .collect();

        // Read all rows
        while let Some(row) = rs
            .next_row()
            .await
            .map_err(|e| FabioError::new(ErrorCode::ApiError, format!("Failed to read row: {e}")))?
        {
            let mut obj = serde_json::Map::new();
            for (i, val) in row.into_iter().enumerate() {
                let col_name = columns
                    .get(i)
                    .map_or_else(|| format!("column{i}"), std::clone::Clone::clone);
                obj.insert(col_name, column_value_to_json(&val));
            }
            all_rows.push(Value::Object(obj));
        }
    }

    tds_client
        .close_query()
        .await
        .map_err(|e| FabioError::new(ErrorCode::ApiError, format!("Failed to close query: {e}")))?;

    // Render output
    if all_rows.is_empty() {
        let obj = serde_json::json!({
            "rows_affected": 0,
            "message": "Query executed successfully (no result set returned)."
        });
        output::render_object(cli, &obj, "message");
    } else {
        let col_refs: Vec<&str> = columns.iter().map(String::as_str).collect();
        output::render_list(cli, &all_rows, &col_refs, &col_refs, &columns[0]);
    }

    Ok(())
}

async fn connection_string(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let (server, port, database) = resolve_sql_connection(client, workspace, id).await?;
    let conn_str = format!(
        "Server=tcp:{server},{port};Initial Catalog={database};Encrypt=True;TrustServerCertificate=False;Authentication=ActiveDirectoryDefault"
    );
    let obj = serde_json::json!({
        "server": server,
        "port": port,
        "database": database,
        "connectionString": conn_str
    });
    output::render_object(cli, &obj, "connectionString");
    Ok(())
}

// ─── Import ──────────────────────────────────────────────────────────────────

/// Inferred SQL column type from data inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InferredType {
    Unknown, // Not yet observed any non-empty value
    Int,
    BigInt,
    Float,
    Bit,
    Date,
    NVarChar(usize), // max observed length
}

impl InferredType {
    fn to_sql(&self) -> String {
        match self {
            Self::Unknown => "NVARCHAR(200)".to_string(),
            Self::Int => "INT".to_string(),
            Self::BigInt => "BIGINT".to_string(),
            Self::Float => "FLOAT".to_string(),
            Self::Bit => "BIT".to_string(),
            Self::Date => "DATE".to_string(),
            Self::NVarChar(len) => {
                // Use at least 50, or 2x observed length, cap at MAX
                let size = (*len * 2).clamp(50, 4000);
                format!("NVARCHAR({size})")
            }
        }
    }

    /// Widen type when conflicting values are seen.
    fn widen(&self, other: &Self) -> Self {
        match (self, other) {
            // Unknown takes any type from first observation
            (Self::Unknown, b) => b.clone(),
            (a, Self::Unknown) => a.clone(),
            (a, b) if a == b => a.clone(),
            // Int + BigInt → BigInt
            (Self::Int, Self::BigInt) | (Self::BigInt, Self::Int) => Self::BigInt,
            // Int/BigInt + Float → Float
            (Self::Int | Self::BigInt, Self::Float) | (Self::Float, Self::Int | Self::BigInt) => {
                Self::Float
            }
            // Anything + NVarChar → NVarChar (take max length)
            (Self::NVarChar(a), Self::NVarChar(b)) => Self::NVarChar(*a.max(b)),
            (Self::NVarChar(a), _) => Self::NVarChar(*a),
            (_, Self::NVarChar(b)) => Self::NVarChar(*b),
            // Fallback: wider type wins
            _ => Self::NVarChar(100),
        }
    }
}

/// Infer SQL type from a string value.
fn infer_type_from_str(val: &str) -> InferredType {
    if val.is_empty() {
        return InferredType::Unknown;
    }
    // Try integer
    if val.parse::<i32>().is_ok() {
        return InferredType::Int;
    }
    if val.parse::<i64>().is_ok() {
        return InferredType::BigInt;
    }
    // Try float
    if val.parse::<f64>().is_ok() {
        return InferredType::Float;
    }
    // Try boolean
    if val.eq_ignore_ascii_case("true") || val.eq_ignore_ascii_case("false") {
        return InferredType::Bit;
    }
    // Try date (YYYY-MM-DD)
    if val.len() == 10
        && val.chars().nth(4) == Some('-')
        && val.chars().nth(7) == Some('-')
        && val[..4].parse::<u16>().is_ok()
        && val[5..7].parse::<u8>().is_ok()
        && val[8..10].parse::<u8>().is_ok()
    {
        return InferredType::Date;
    }
    InferredType::NVarChar(val.len())
}

/// Infer SQL type from a JSON value.
fn infer_type_from_json(val: &Value) -> InferredType {
    match val {
        Value::Null => InferredType::Unknown,
        Value::Bool(_) => InferredType::Bit,
        Value::Number(n) => n.as_i64().map_or(InferredType::Float, |i| {
            if i32::try_from(i).is_ok() {
                InferredType::Int
            } else {
                InferredType::BigInt
            }
        }),
        Value::String(s) => infer_type_from_str(s),
        _ => InferredType::NVarChar(200), // arrays/objects → serialize as string
    }
}

/// Escape a SQL string value (strip null bytes and double single quotes).
fn sql_escape(val: &str) -> String {
    val.replace('\0', "").replace('\'', "''")
}

/// Format a value as a SQL literal.
fn value_to_sql_literal(val: &str, col_type: &InferredType) -> String {
    if val.is_empty() {
        return "NULL".to_string();
    }
    match col_type {
        InferredType::Int | InferredType::BigInt | InferredType::Float => {
            // Validate it's actually numeric, fallback to NULL
            if val.parse::<f64>().is_ok() {
                val.to_string()
            } else {
                "NULL".to_string()
            }
        }
        InferredType::Bit => {
            if val.eq_ignore_ascii_case("true") || val == "1" {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        InferredType::Unknown | InferredType::Date | InferredType::NVarChar(_) => {
            format!("N'{}'", sql_escape(val))
        }
    }
}

/// Format a JSON value as a SQL literal.
fn json_value_to_sql_literal(val: &Value, col_type: &InferredType) -> String {
    match val {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.is_empty() {
                "NULL".to_string()
            } else {
                match col_type {
                    InferredType::Int | InferredType::BigInt | InferredType::Float => {
                        if s.parse::<f64>().is_ok() {
                            s.clone()
                        } else {
                            "NULL".to_string()
                        }
                    }
                    _ => format!("N'{}'", sql_escape(s)),
                }
            }
        }
        _ => {
            // Serialize complex types to string
            let s = val.to_string();
            format!("N'{}'", sql_escape(&s))
        }
    }
}

/// Schema inference result for CSV files: (`column_names`, `inferred_types`, `rows_as_strings`).
type CsvSchema = (Vec<String>, Vec<InferredType>, Vec<Vec<String>>);

/// Read CSV file and return (columns, types, rows as Vec<Vec<String>>).
fn read_csv_file(path: &str) -> Result<CsvSchema> {
    let mut reader = csv::Reader::from_path(path).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Cannot read CSV file: {e}"),
            format!("Verify the file exists at: {path}"),
        )
    })?;

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| FabioError::new(ErrorCode::InvalidInput, format!("Invalid CSV headers: {e}")))?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();

    if headers.is_empty() {
        return Err(FabioError::new(ErrorCode::InvalidInput, "CSV file has no columns").into());
    }

    let mut col_types: Vec<InferredType> = vec![InferredType::Unknown; headers.len()];
    let mut rows: Vec<Vec<String>> = Vec::new();

    for result in reader.records() {
        let record = result.map_err(|e| {
            FabioError::new(ErrorCode::InvalidInput, format!("Invalid CSV row: {e}"))
        })?;

        let row: Vec<String> = record.iter().map(|v| v.trim().to_string()).collect();
        // Infer types from this row
        for (i, val) in row.iter().enumerate() {
            if i < col_types.len() && !val.is_empty() {
                let inferred = infer_type_from_str(val);
                col_types[i] = col_types[i].widen(&inferred);
            }
        }
        rows.push(row);
    }

    Ok((headers, col_types, rows))
}

/// Schema inference result for JSON files: (`column_names`, `inferred_types`, `rows_as_json_values`).
type JsonSchema = (Vec<String>, Vec<InferredType>, Vec<Vec<Value>>);

/// Read JSON file (array of objects) and return (columns, types, rows as Vec<Vec<Value>>).
fn read_json_file(path: &str) -> Result<JsonSchema> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Cannot read JSON file: {e}"),
            format!("Verify the file exists at: {path}"),
        )
    })?;

    let array: Vec<Value> = serde_json::from_str(&content).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid JSON: {e}"),
            "Expected a JSON array of objects, e.g. [{\"col1\": \"val1\", ...}, ...]".to_string(),
        )
    })?;

    if array.is_empty() {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            "JSON array is empty — nothing to import",
        )
        .into());
    }

    // Collect all unique keys in order of first appearance
    let mut columns: Vec<String> = Vec::new();
    for obj in &array {
        if let Value::Object(map) = obj {
            for key in map.keys() {
                if !columns.contains(key) {
                    columns.push(key.clone());
                }
            }
        }
    }

    if columns.is_empty() {
        return Err(FabioError::new(ErrorCode::InvalidInput, "JSON objects have no keys").into());
    }

    // Infer types and collect rows
    let mut col_types: Vec<InferredType> = vec![InferredType::Unknown; columns.len()];
    let mut rows: Vec<Vec<Value>> = Vec::new();

    for obj in &array {
        if let Value::Object(map) = obj {
            let mut row: Vec<Value> = Vec::with_capacity(columns.len());
            for (i, col) in columns.iter().enumerate() {
                let val = map.get(col).unwrap_or(&Value::Null);
                if !val.is_null() {
                    let inferred = infer_type_from_json(val);
                    col_types[i] = col_types[i].widen(&inferred);
                }
                row.push(val.clone());
            }
            rows.push(row);
        }
    }

    Ok((columns, col_types, rows))
}

/// Sanitize a table name for SQL (bracket-quote).
fn sanitize_table_name(name: &str) -> String {
    // Remove dangerous chars, bracket-quote
    let clean: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == ' ' || *c == '-')
        .collect();
    format!("[{clean}]")
}

/// Derive table name from file path.
fn table_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported_data")
        .to_string()
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn import(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: &str,
    table: Option<&str>,
    no_create_table: bool,
    drop_if_exists: bool,
    batch_size: usize,
) -> Result<()> {
    // Determine table name
    let table_name = table.map_or_else(|| table_name_from_path(file), ToString::to_string);
    let safe_table = sanitize_table_name(&table_name);

    // Detect format from file extension
    let ext = std::path::Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Parse file and generate SQL
    let (columns, col_types, sql_batches) = match ext.as_str() {
        "csv" => {
            let (columns, col_types, rows) = read_csv_file(file)?;
            let batches =
                generate_csv_insert_batches(&safe_table, &columns, &col_types, &rows, batch_size);
            (columns, col_types, batches)
        }
        "json" => {
            let (columns, col_types, rows) = read_json_file(file)?;
            let batches =
                generate_json_insert_batches(&safe_table, &columns, &col_types, &rows, batch_size);
            (columns, col_types, batches)
        }
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Unsupported file format: .{ext}"),
                "Supported formats: .csv, .json".to_string(),
            )
            .into());
        }
    };

    let total_rows = sql_batches.iter().map(|b| b.1).sum::<usize>();

    // Generate CREATE TABLE DDL
    let create_ddl = generate_create_table(&safe_table, &columns, &col_types);
    let drop_ddl = format!("DROP TABLE IF EXISTS {safe_table};");

    // Show dry-run info
    let dry_body = serde_json::json!({
        "table": table_name,
        "file": file,
        "format": ext,
        "columns": columns,
        "total_rows": total_rows,
        "batch_count": sql_batches.len(),
        "create_table_ddl": create_ddl,
        "drop_if_exists": drop_if_exists,
    });
    if output::dry_run_guard(cli, "sql-database import", &dry_body) {
        return Ok(());
    }

    // Connect via TDS
    let (server, port, database) = resolve_sql_connection(client, workspace, id).await?;
    let token = client.require_sql_auth().await?;

    let data_source = format!("tcp:{server},{port}");
    let mut context = ClientContext::with_data_source(&data_source);
    context.database = database;
    context.tds_authentication_method = TdsAuthenticationMethod::AccessToken;
    context.access_token = Some(token);
    context.application_name = "fabio".to_string();
    context.connect_timeout = 30;

    let provider = TdsConnectionProvider {};
    let mut tds_client = provider
        .create_client(context, &data_source, None)
        .await
        .map_err(|e| {
            let msg = format!("{e}");
            let hint = if msg.contains("18456") {
                ". Hint: Fabric SQL Database requires F4+ capacity. On F2, TDS connections \
                 fail with 'Validation of user's permissions failed' due to insufficient \
                 compute, not actual permissions issues. Scale your capacity to F4 or higher."
            } else {
                ""
            };
            FabioError::new(
                ErrorCode::ApiError,
                format!("TDS connection failed: {e}{hint}"),
            )
        })?;

    // Execute DROP TABLE if requested
    if drop_if_exists {
        tds_client
            .execute(drop_ddl.clone(), Some(60), None)
            .await
            .map_err(|e| {
                FabioError::new(
                    ErrorCode::ApiError,
                    format!("Failed to drop existing table: {e}"),
                )
            })?;
        tds_client.close_query().await.ok();
    }

    // Execute CREATE TABLE
    if !no_create_table {
        tds_client
            .execute(create_ddl.clone(), Some(60), None)
            .await
            .map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::ApiError,
                    format!("Failed to create table: {e}"),
                    if drop_if_exists {
                        String::new()
                    } else {
                        "If the table already exists, use --drop-if-exists or --no-create-table"
                            .to_string()
                    },
                )
            })?;
        tds_client.close_query().await.ok();
    }

    // Execute INSERT batches
    let mut inserted = 0usize;
    for (batch_sql, batch_rows) in &sql_batches {
        tds_client
            .execute(batch_sql.clone(), Some(120), None)
            .await
            .map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::ApiError,
                    format!("INSERT failed at row {inserted}: {e}"),
                    format!("Successfully inserted {inserted}/{total_rows} rows before failure"),
                )
            })?;
        tds_client.close_query().await.ok();
        inserted += batch_rows;
    }

    // Render success output
    let result = serde_json::json!({
        "table": table_name,
        "rows_inserted": inserted,
        "columns": columns.len(),
        "file": file,
        "message": format!("Successfully imported {inserted} rows into {safe_table}")
    });
    output::render_object(cli, &result, "message");
    Ok(())
}

/// Generate CREATE TABLE DDL.
fn generate_create_table(table: &str, columns: &[String], types: &[InferredType]) -> String {
    let col_defs: Vec<String> = columns
        .iter()
        .zip(types.iter())
        .map(|(name, typ)| {
            let safe_col = format!("[{}]", name.replace(']', "]]"));
            format!("    {safe_col} {} NULL", typ.to_sql())
        })
        .collect();

    format!("CREATE TABLE {table} (\n{}\n);", col_defs.join(",\n"))
}

/// Generate batched INSERT statements for CSV data.
fn generate_csv_insert_batches(
    table: &str,
    columns: &[String],
    types: &[InferredType],
    rows: &[Vec<String>],
    batch_size: usize,
) -> Vec<(String, usize)> {
    let col_list: String = columns
        .iter()
        .map(|c| format!("[{}]", c.replace(']', "]]")))
        .collect::<Vec<_>>()
        .join(", ");

    rows.chunks(batch_size)
        .map(|chunk| {
            let values: Vec<String> = chunk
                .iter()
                .map(|row| {
                    let vals: Vec<String> = row
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            let col_type = types.get(i).unwrap_or(&InferredType::NVarChar(200));
                            value_to_sql_literal(v, col_type)
                        })
                        .collect();
                    format!("({})", vals.join(", "))
                })
                .collect();

            let sql = format!(
                "INSERT INTO {table} ({col_list}) VALUES\n{};",
                values.join(",\n")
            );
            (sql, chunk.len())
        })
        .collect()
}

/// Generate batched INSERT statements for JSON data.
fn generate_json_insert_batches(
    table: &str,
    columns: &[String],
    types: &[InferredType],
    rows: &[Vec<Value>],
    batch_size: usize,
) -> Vec<(String, usize)> {
    let col_list: String = columns
        .iter()
        .map(|c| format!("[{}]", c.replace(']', "]]")))
        .collect::<Vec<_>>()
        .join(", ");

    rows.chunks(batch_size)
        .map(|chunk| {
            let values: Vec<String> = chunk
                .iter()
                .map(|row| {
                    let vals: Vec<String> = row
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            let col_type = types.get(i).unwrap_or(&InferredType::NVarChar(200));
                            json_value_to_sql_literal(v, col_type)
                        })
                        .collect();
                    format!("({})", vals.join(", "))
                })
                .collect();

            let sql = format!(
                "INSERT INTO {table} ({col_list}) VALUES\n{};",
                values.join(",\n")
            );
            (sql, chunk.len())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_type_int() {
        assert_eq!(infer_type_from_str("42"), InferredType::Int);
        assert_eq!(infer_type_from_str("-1"), InferredType::Int);
    }

    #[test]
    fn infer_type_bigint() {
        assert_eq!(infer_type_from_str("9000000000"), InferredType::BigInt);
    }

    #[test]
    fn infer_type_float() {
        assert_eq!(infer_type_from_str("3.14"), InferredType::Float);
        assert_eq!(infer_type_from_str("1299.99"), InferredType::Float);
    }

    #[test]
    fn infer_type_date() {
        assert_eq!(infer_type_from_str("2024-01-15"), InferredType::Date);
    }

    #[test]
    fn infer_type_bool() {
        assert_eq!(infer_type_from_str("true"), InferredType::Bit);
        assert_eq!(infer_type_from_str("false"), InferredType::Bit);
    }

    #[test]
    fn infer_type_string() {
        assert_eq!(
            infer_type_from_str("hello world"),
            InferredType::NVarChar(11)
        );
    }

    #[test]
    fn infer_type_empty() {
        assert_eq!(infer_type_from_str(""), InferredType::Unknown);
    }

    #[test]
    fn widen_int_float() {
        assert_eq!(
            InferredType::Int.widen(&InferredType::Float),
            InferredType::Float
        );
    }

    #[test]
    fn widen_int_bigint() {
        assert_eq!(
            InferredType::Int.widen(&InferredType::BigInt),
            InferredType::BigInt
        );
    }

    #[test]
    fn widen_nvarchar_max_length() {
        assert_eq!(
            InferredType::NVarChar(10).widen(&InferredType::NVarChar(50)),
            InferredType::NVarChar(50)
        );
    }

    #[test]
    fn widen_int_nvarchar() {
        // Once we see a string in an int column, it becomes nvarchar
        assert_eq!(
            InferredType::Int.widen(&InferredType::NVarChar(20)),
            InferredType::NVarChar(20)
        );
    }

    #[test]
    fn widen_unknown_takes_first_type() {
        assert_eq!(
            InferredType::Unknown.widen(&InferredType::Int),
            InferredType::Int
        );
        assert_eq!(
            InferredType::Unknown.widen(&InferredType::Date),
            InferredType::Date
        );
    }

    #[test]
    fn sql_escape_quotes() {
        assert_eq!(sql_escape("it's"), "it''s");
        assert_eq!(sql_escape("no quotes"), "no quotes");
    }

    #[test]
    fn sql_escape_null_bytes() {
        assert_eq!(sql_escape("hello\0world"), "helloworld");
        assert_eq!(sql_escape("\0"), "");
        assert_eq!(sql_escape("it\0's"), "it''s");
    }

    #[test]
    fn value_to_literal_int() {
        assert_eq!(value_to_sql_literal("42", &InferredType::Int), "42");
    }

    #[test]
    fn value_to_literal_string() {
        assert_eq!(
            value_to_sql_literal("hello", &InferredType::NVarChar(10)),
            "N'hello'"
        );
    }

    #[test]
    fn value_to_literal_empty() {
        assert_eq!(
            value_to_sql_literal("", &InferredType::NVarChar(10)),
            "NULL"
        );
    }

    #[test]
    fn value_to_literal_date() {
        assert_eq!(
            value_to_sql_literal("2024-01-15", &InferredType::Date),
            "N'2024-01-15'"
        );
    }

    #[test]
    fn generate_create_table_basic() {
        let cols = vec!["id".to_string(), "name".to_string()];
        let types = vec![InferredType::Int, InferredType::NVarChar(50)];
        let ddl = generate_create_table("[test]", &cols, &types);
        assert!(ddl.contains("CREATE TABLE [test]"));
        assert!(ddl.contains("[id] INT NULL"));
        assert!(ddl.contains("[name] NVARCHAR(100) NULL"));
    }

    #[test]
    fn table_name_from_path_extracts_stem() {
        assert_eq!(table_name_from_path("/tmp/data/orders.csv"), "orders");
        assert_eq!(table_name_from_path("customers.json"), "customers");
    }

    #[test]
    fn infer_json_types() {
        assert_eq!(infer_type_from_json(&Value::from(42)), InferredType::Int);
        assert_eq!(
            infer_type_from_json(&Value::from(1.23)),
            InferredType::Float
        );
        assert_eq!(infer_type_from_json(&Value::from(true)), InferredType::Bit);
        assert_eq!(
            infer_type_from_json(&Value::from("hello")),
            InferredType::NVarChar(5)
        );
        assert_eq!(infer_type_from_json(&Value::Null), InferredType::Unknown);
    }

    #[test]
    fn json_literal_null() {
        assert_eq!(
            json_value_to_sql_literal(&Value::Null, &InferredType::Int),
            "NULL"
        );
    }

    #[test]
    fn json_literal_number() {
        assert_eq!(
            json_value_to_sql_literal(&Value::from(42), &InferredType::Int),
            "42"
        );
        assert_eq!(
            json_value_to_sql_literal(&Value::from(1.23), &InferredType::Float),
            "1.23"
        );
    }

    #[test]
    fn json_literal_bool() {
        assert_eq!(
            json_value_to_sql_literal(&Value::from(true), &InferredType::Bit),
            "1"
        );
        assert_eq!(
            json_value_to_sql_literal(&Value::from(false), &InferredType::Bit),
            "0"
        );
    }

    #[test]
    fn json_literal_string() {
        assert_eq!(
            json_value_to_sql_literal(&Value::from("hello"), &InferredType::NVarChar(10)),
            "N'hello'"
        );
    }

    #[test]
    fn csv_insert_batches() {
        let cols = vec!["id".to_string(), "name".to_string()];
        let types = vec![InferredType::Int, InferredType::NVarChar(10)];
        let rows = vec![
            vec!["1".to_string(), "Alice".to_string()],
            vec!["2".to_string(), "Bob".to_string()],
            vec!["3".to_string(), "Charlie".to_string()],
        ];
        let batches = generate_csv_insert_batches("[test]", &cols, &types, &rows, 2);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].1, 2); // first batch: 2 rows
        assert_eq!(batches[1].1, 1); // second batch: 1 row
        assert!(batches[0].0.contains("(1, N'Alice')"));
        assert!(batches[0].0.contains("(2, N'Bob')"));
        assert!(batches[1].0.contains("(3, N'Charlie')"));
    }
}
