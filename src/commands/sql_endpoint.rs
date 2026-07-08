use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{
    capture_query_plan, execute_and_render_sql, parse_connection_string, resolve_sql_input,
};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum SqlEndpointCommand {
    /// List SQL endpoints in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a SQL endpoint
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,
    },
    /// Get the SQL connection string for a SQL endpoint
    #[command(display_order = 3)]
    ConnectionString {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,

        /// Guest tenant ID (if different from the SQL endpoint's tenant)
        #[arg(long)]
        guest_tenant_id: Option<String>,

        /// Private link type: `None` or `Workspace`
        #[arg(long)]
        private_link_type: Option<String>,
    },
    /// Execute a SQL query against a SQL endpoint
    #[command(display_order = 4)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,

        /// SQL query text, @file path, or omit for stdin
        #[arg(long)]
        sql: Option<String>,
    },
    /// Capture the estimated execution plan (`SHOWPLAN_XML`) without executing the query
    #[command(display_order = 5)]
    Plan {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,

        /// SQL query to plan (prefix with @ to read from file, omit to read from stdin)
        #[arg(long)]
        sql: Option<String>,
    },
    /// Refresh metadata for all tables in a SQL endpoint (LRO)
    #[command(display_order = 6)]
    RefreshMetadata {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,
    },
    /// Get SQL audit settings for the endpoint
    #[command(display_order = 10)]
    GetAuditSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,
    },
    /// Update SQL audit settings for the endpoint
    #[command(display_order = 11)]
    UpdateAuditSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
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
    /// Set audit actions and groups for the endpoint
    #[command(display_order = 12)]
    SetAuditActions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,

        /// Audit actions and groups (comma-separated)
        #[arg(long, value_delimiter = ',')]
        actions: Vec<String>,
    },

    // ── Query Insights ───────────────────────────────────────────────────
    /// List currently running queries on a SQL endpoint
    #[command(display_order = 20)]
    QueriesRunning {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,
    },
    /// List frequently-run queries (from `queryinsights.frequently_run_queries`)
    #[command(display_order = 21)]
    QueriesFrequent {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,

        /// Maximum rows to return (default: 100)
        #[arg(long, default_value = "100")]
        top: u32,
    },
    /// List long-running queries (from `queryinsights.long_running_queries`)
    #[command(display_order = 22)]
    QueriesLongRunning {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,

        /// Maximum rows to return (default: 100)
        #[arg(long, default_value = "100")]
        top: u32,
    },
    /// List completed query history (from `queryinsights.exec_requests_history`)
    #[command(display_order = 23)]
    QueriesHistory {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// SQL endpoint ID
        #[arg(long)]
        id: String,

        /// Maximum rows to return (default: 100)
        #[arg(long, default_value = "100")]
        top: u32,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &SqlEndpointCommand) -> Result<()> {
    match command {
        SqlEndpointCommand::List { workspace } => list(cli, client, workspace).await,
        SqlEndpointCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        SqlEndpointCommand::ConnectionString {
            workspace,
            id,
            guest_tenant_id,
            private_link_type,
        } => {
            connection_string(
                cli,
                client,
                workspace,
                id,
                guest_tenant_id.as_deref(),
                private_link_type.as_deref(),
            )
            .await
        }
        SqlEndpointCommand::Query { workspace, id, sql } => {
            Box::pin(query(cli, client, workspace, id, sql.as_deref())).await
        }
        SqlEndpointCommand::Plan { workspace, id, sql } => {
            Box::pin(plan(cli, client, workspace, id, sql.as_deref())).await
        }
        SqlEndpointCommand::RefreshMetadata { workspace, id } => {
            refresh_metadata(cli, client, workspace, id).await
        }
        SqlEndpointCommand::GetAuditSettings { workspace, id } => {
            get_audit_settings(cli, client, workspace, id).await
        }
        SqlEndpointCommand::UpdateAuditSettings {
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
        SqlEndpointCommand::SetAuditActions {
            workspace,
            id,
            actions,
        } => set_audit_actions(cli, client, workspace, id, actions).await,
        SqlEndpointCommand::QueriesRunning { workspace, id } => {
            Box::pin(insights_running(cli, client, workspace, id)).await
        }
        SqlEndpointCommand::QueriesFrequent { workspace, id, top } => {
            Box::pin(insights_frequent(cli, client, workspace, id, *top)).await
        }
        SqlEndpointCommand::QueriesLongRunning { workspace, id, top } => {
            Box::pin(insights_long_running(cli, client, workspace, id, *top)).await
        }
        SqlEndpointCommand::QueriesHistory { workspace, id, top } => {
            Box::pin(insights_history(cli, client, workspace, id, *top)).await
        }
    }
}

// ─── Queries ─────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/sqlEndpoints"),
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
        .get(&format!("/workspaces/{workspace}/sqlEndpoints/{id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn connection_string(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    guest_tenant_id: Option<&str>,
    private_link_type: Option<&str>,
) -> Result<()> {
    let mut url = format!("/workspaces/{workspace}/sqlEndpoints/{id}/connectionString");
    let mut params = Vec::new();
    if let Some(tenant) = guest_tenant_id {
        params.push(format!("guestTenantId={tenant}"));
    }
    if let Some(plt) = private_link_type {
        params.push(format!("privateLinkType={plt}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let data = client.get(&url).await?;
    output::render_object(cli, &data, "connectionString");
    Ok(())
}

async fn query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql: Option<&str>,
) -> Result<()> {
    let sql_text = resolve_sql_input(sql)?;

    // Fetch endpoint metadata to get the display name (used as initial catalog)
    let item = client
        .get(&format!("/workspaces/{workspace}/sqlEndpoints/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "sql-endpoint query", "Viewer"))?;

    let display_name = item
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default();

    // Fetch the connection string
    let conn_data = client
        .get(&format!(
            "/workspaces/{workspace}/sqlEndpoints/{id}/connectionString"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "sql-endpoint query", "Viewer"))?;

    let conn_str = conn_data
        .get("connectionString")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                "SQL endpoint connection string not available.",
                "The SQL endpoint may still be provisioning. Wait and retry, or check with: fabio sql-endpoint show --workspace <WS> --id <ID>",
            )
        })?;

    let (server, parsed_db) = parse_connection_string(conn_str);
    let database = if display_name.is_empty() {
        parsed_db
    } else {
        display_name.to_string()
    };

    execute_and_render_sql(cli, client, &server, &database, &sql_text).await
}

async fn plan(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql: Option<&str>,
) -> Result<()> {
    let sql_text = resolve_sql_input(sql)?;

    // Fetch endpoint metadata to get the display name (used as initial catalog)
    let item = client
        .get(&format!("/workspaces/{workspace}/sqlEndpoints/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "sql-endpoint plan", "Viewer"))?;

    let display_name = item
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default();

    // Fetch the connection string
    let conn_data = client
        .get(&format!(
            "/workspaces/{workspace}/sqlEndpoints/{id}/connectionString"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "sql-endpoint plan", "Viewer"))?;

    let conn_str = conn_data
        .get("connectionString")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                "SQL endpoint connection string not available.",
                "The SQL endpoint may still be provisioning. Wait and retry, or check with: fabio sql-endpoint show --workspace <WS> --id <ID>",
            )
        })?;

    let (server, parsed_db) = parse_connection_string(conn_str);
    let database = if display_name.is_empty() {
        parsed_db
    } else {
        display_name.to_string()
    };

    let plans = capture_query_plan(client, &server, &database, &sql_text).await?;

    let plan_objects: Vec<Value> = plans
        .iter()
        .enumerate()
        .map(|(i, xml)| {
            serde_json::json!({
                "statementIndex": i,
                "planXml": xml
            })
        })
        .collect();
    let obj = serde_json::json!({
        "statementCount": plans.len(),
        "plans": plan_objects
    });
    output::render_object(cli, &obj, "statementCount");

    Ok(())
}

// ─── Mutations ───────────────────────────────────────────────────────────────

async fn refresh_metadata(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "sql-endpoint refresh-metadata",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/sqlEndpoints/{id}/refreshMetadata"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "sql-endpoint refresh-metadata", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "metadata_refreshed" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
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
            "/workspaces/{workspace}/sqlEndpoints/{id}/settings/sqlAudit"
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
        body["state"] = Value::from(s);
    }
    if let Some(days) = retention_days {
        body["retentionDays"] = Value::Number(serde_json::Number::from(days));
    }
    if let Some(actions) = audit_actions {
        body["auditActionsAndGroups"] =
            Value::Array(actions.iter().map(|a| Value::from(a.as_str())).collect());
    }
    if let Some(pred) = predicate_expression {
        body["predicateExpression"] = Value::from(pred);
    }

    if output::dry_run_guard(cli, "sql-endpoint update-audit-settings", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/sqlEndpoints/{id}/settings/sqlAudit"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "sql-endpoint update-audit-settings", "Audit"))?;
    output::render_object(cli, &data, "state");
    Ok(())
}

async fn set_audit_actions(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    actions: &[String],
) -> Result<()> {
    let body = Value::Array(actions.iter().map(|a| Value::from(a.as_str())).collect());

    if output::dry_run_guard(cli, "sql-endpoint set-audit-actions", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/sqlEndpoints/{id}/settings/sqlAudit/setAuditActionsAndGroups"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "sql-endpoint set-audit-actions", "Audit"))?;

    let obj = serde_json::json!({ "id": id, "status": "audit_actions_updated" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Query Insights ──────────────────────────────────────────────────────────

/// Helper: resolve SQL endpoint connection and execute a TDS query.
async fn execute_endpoint_query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql_text: &str,
) -> Result<()> {
    let item = client
        .get(&format!("/workspaces/{workspace}/sqlEndpoints/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "sql-endpoint query", "Viewer"))?;

    let display_name = item
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let conn_data = client
        .get(&format!(
            "/workspaces/{workspace}/sqlEndpoints/{id}/connectionString"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "sql-endpoint query", "Viewer"))?;

    let conn_str = conn_data
        .get("connectionString")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                "SQL endpoint connection string not available.",
                "The SQL endpoint may still be provisioning. Wait and retry.",
            )
        })?;

    let (server, parsed_db) = parse_connection_string(conn_str);
    let database = if display_name.is_empty() {
        parsed_db
    } else {
        display_name.to_string()
    };

    execute_and_render_sql(cli, client, &server, &database, sql_text).await
}

async fn insights_running(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let sql = "SELECT r.session_id, r.status, \
               r.command, r.start_time, r.total_elapsed_time \
               FROM sys.dm_exec_requests r \
               WHERE r.status != 'background' \
               ORDER BY r.total_elapsed_time DESC";
    Box::pin(execute_endpoint_query(cli, client, workspace, id, sql)).await
}

async fn insights_frequent(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    top: u32,
) -> Result<()> {
    let sql = format!(
        "SELECT TOP ({top}) \
         last_run_command, number_of_runs, \
         avg_total_elapsed_time_ms, \
         min_run_total_elapsed_time_ms, \
         max_run_total_elapsed_time_ms, \
         number_of_successful_runs, \
         query_hash \
         FROM queryinsights.frequently_run_queries \
         ORDER BY number_of_runs DESC"
    );
    Box::pin(execute_endpoint_query(cli, client, workspace, id, &sql)).await
}

async fn insights_long_running(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    top: u32,
) -> Result<()> {
    let sql = format!(
        "SELECT TOP ({top}) \
         last_run_command, number_of_runs, \
         median_total_elapsed_time_ms, \
         last_run_total_elapsed_time_ms, \
         last_run_start_time, \
         query_hash \
         FROM queryinsights.long_running_queries \
         ORDER BY median_total_elapsed_time_ms DESC"
    );
    Box::pin(execute_endpoint_query(cli, client, workspace, id, &sql)).await
}

async fn insights_history(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    top: u32,
) -> Result<()> {
    let sql = format!(
        "SELECT TOP ({top}) \
         command, status, \
         total_elapsed_time_ms, \
         login_name, \
         start_time, end_time, \
         row_count, \
         query_hash \
         FROM queryinsights.exec_requests_history \
         ORDER BY start_time DESC"
    );
    Box::pin(execute_endpoint_query(cli, client, workspace, id, &sql)).await
}
