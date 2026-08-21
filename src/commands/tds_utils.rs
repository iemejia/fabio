use std::io;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{Duration, NaiveDate, NaiveTime};
use mssql_tds::connection::client_context::{ClientContext, TdsAuthenticationMethod};
use mssql_tds::connection::tds_client::{ResultSet, ResultSetClient};
use mssql_tds::connection_provider::tds_connection_provider::TdsConnectionProvider;
use mssql_tds::datatypes::column_values::ColumnValues;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

/// A hint for TDS/SQL auth failures caused by using a Fabric-scoped static token.
///
/// A TDS connection authenticates to Azure SQL (`database.windows.net`), which
/// rejects a Fabric-audience token. When the generic `FABIO_ACCESS_TOKEN` is set
/// but no SQL-specific `FABIO_SQL_ACCESS_TOKEN` is, the SQL scope falls back to
/// the (Fabric) generic token and the login fails. Returns the corrective hint
/// in that case, else `None`. Pure (env-based) for testing via env injection.
fn sql_scope_token_hint() -> Option<String> {
    let has_generic = std::env::var("FABIO_ACCESS_TOKEN").is_ok_and(|t| !t.is_empty());
    let has_sql = std::env::var("FABIO_SQL_ACCESS_TOKEN").is_ok_and(|t| !t.is_empty());
    sql_scope_token_hint_for(has_generic, has_sql)
}

/// Pure core of [`sql_scope_token_hint`]: hint iff a generic (Fabric) static
/// token is present but no SQL-specific one.
fn sql_scope_token_hint_for(has_generic: bool, has_sql: bool) -> Option<String> {
    (has_generic && !has_sql).then(|| {
        "This looks like a SQL auth failure: FABIO_ACCESS_TOKEN is Fabric-scoped, but TDS \
         needs a SQL-audience token. Set FABIO_SQL_ACCESS_TOKEN=$(az account get-access-token \
         --resource https://database.windows.net --query accessToken -o tsv), or unset \
         FABIO_ACCESS_TOKEN to use `az login` / `fabio auth login` (which mint a correct \
         token per audience)."
            .to_string()
    })
}

/// Build a TDS connection-failure error, adding the SQL-scope token hint when the
/// failure is likely a Fabric-token-for-SQL misconfiguration.
fn tds_connection_error(e: &impl std::fmt::Display) -> FabioError {
    let msg = format!("TDS connection failed: {e}");
    sql_scope_token_hint().map_or_else(
        || FabioError::new(ErrorCode::ApiError, msg.clone()),
        |hint| FabioError::with_hint(ErrorCode::ApiError, msg.clone(), hint),
    )
}

/// Resolve SQL text from flag, @file, or stdin.
pub fn resolve_sql_input(sql: Option<&str>) -> anyhow::Result<String> {
    match sql {
        Some(s) if s.starts_with('@') => {
            let file_path = &s[1..];
            std::fs::read_to_string(file_path).map_err(|e| {
                FabioError::not_found(format!("SQL file not found: {file_path}: {e}")).into()
            })
        }
        Some(s) => Ok(s.to_string()),
        None => {
            let buf = io::read_to_string(io::stdin()).map_err(|e| {
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
            Ok(buf)
        }
    }
}

/// Build the SQL for the SQL Pool Insights view (`queryinsights.sql_pool_insights`),
/// which logs pool state changes and sustained pressure events for the built-in
/// SELECT / NON-SELECT SQL pools of a Warehouse or Lakehouse SQL analytics endpoint.
/// Pure function for testing.
#[must_use]
pub fn pool_insights_sql(top: u32) -> String {
    format!(
        "SELECT TOP ({top}) \
         sql_pool_name, \
         timestamp, \
         is_optimized_for_reads, \
         is_pool_under_pressure, \
         max_resource_percentage, \
         cache_cooldown_minutes, \
         current_workspace_capacity \
         FROM queryinsights.sql_pool_insights \
         ORDER BY timestamp DESC"
    )
}

/// Build the `queries-history` SQL over `queryinsights.exec_requests_history`.
///
/// Includes the query `label` and the performance columns
/// (`allocated_cpu_time_ms`, `data_scanned_remote_storage_mb`,
/// `data_scanned_memory_mb`, `data_scanned_disk_mb`) needed to compare query
/// executions (e.g. to assess data-clustering effectiveness by label). When
/// `label` is provided it filters `WHERE label = N'...'` (single quotes are
/// doubled to prevent injection).
pub fn queries_history_sql(top: u32, label: Option<&str>) -> String {
    let where_clause = label.map_or_else(String::new, |l| {
        let escaped = l.replace('\'', "''");
        format!(" WHERE label = N'{escaped}'")
    });
    format!(
        "SELECT TOP ({top}) \
         command, status, \
         label, \
         total_elapsed_time_ms, \
         allocated_cpu_time_ms, \
         data_scanned_remote_storage_mb, \
         data_scanned_memory_mb, \
         data_scanned_disk_mb, \
         login_name, \
         start_time, end_time, \
         row_count, \
         query_hash \
         FROM queryinsights.exec_requests_history{where_clause} \
         ORDER BY start_time DESC"
    )
}

/// Build an `UPDATE STATISTICS` statement that first resolves the OWNING table
/// of a statistic from `sys.stats` (a statistic is per-table), then runs the
/// statement via dynamic SQL (the object name can't be a variable in
/// `UPDATE`/`DROP STATISTICS`). Shared by `warehouse` + `sql-database`.
pub fn build_update_statistics_sql(name: &str) -> String {
    stats_ddl_sql(name, false)
}

/// Build a `DROP STATISTICS <schema>.<table>.<stat>` statement (see
/// [`build_update_statistics_sql`]).
pub fn build_drop_statistics_sql(name: &str) -> String {
    stats_ddl_sql(name, true)
}

fn stats_ddl_sql(name: &str, drop: bool) -> String {
    let esc = name.replace('\'', "''");
    // UPDATE STATISTICS <table> (<stat>)   vs   DROP STATISTICS <table>.<stat>
    let stmt = if drop {
        format!("N'DROP STATISTICS ' + @tbl + N'.' + QUOTENAME(N'{esc}')")
    } else {
        format!("N'UPDATE STATISTICS ' + @tbl + N' (' + QUOTENAME(N'{esc}') + N')'")
    };
    format!(
        "DECLARE @tbl NVARCHAR(500); \
         SELECT @tbl = QUOTENAME(SCHEMA_NAME(t.schema_id)) + '.' + QUOTENAME(t.name) \
         FROM sys.stats s JOIN sys.tables t ON s.object_id = t.object_id \
         WHERE s.name = N'{esc}'; \
         IF @tbl IS NULL RAISERROR('Statistic not found: {esc}', 16, 1); \
         DECLARE @sql NVARCHAR(1000) = {stmt}; \
         EXEC sp_executesql @sql;"
    )
}

/// Parse a connection string into (server, database).
pub fn parse_connection_string(connection_string: &str) -> (String, String) {
    let cleaned = connection_string
        .trim()
        .trim_start_matches("jdbc:sqlserver://")
        .trim_start_matches("jdbc:");

    // Extract server: everything before the first ';' or ','
    let server = cleaned
        .split(';')
        .next()
        .unwrap_or(cleaned)
        .split(',')
        .next()
        .unwrap_or(cleaned)
        .to_string();

    // Extract database from key-value pairs (case-insensitive)
    let database = cleaned
        .split(';')
        .find_map(|part| {
            let lower = part.trim().to_lowercase();
            if lower.starts_with("database=") || lower.starts_with("initial catalog=") {
                part.trim().split('=').nth(1).map(str::to_string)
            } else {
                None
            }
        })
        .unwrap_or_default();

    (server, database)
}

/// Resolve a lakehouse's SQL analytics endpoint to `(server, database)`.
///
/// The database is the lakehouse `displayName` (Fabric names the SQL catalog
/// after the lakehouse), falling back to the catalog parsed from the connection
/// string. Errors if the SQL endpoint has not been provisioned yet.
pub async fn resolve_lakehouse_sql(
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> anyhow::Result<(String, String)> {
    let data = client
        .get(&format!("/workspaces/{workspace}/lakehouses/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse", "Viewer"))?;

    let connection_string = data
        .get("properties")
        .and_then(|p| p.get("sqlEndpointProperties"))
        .and_then(|s| s.get("connectionString"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                "Lakehouse SQL endpoint not available.",
                "Wait for provisioning to complete, then retry.",
            )
        })?;

    let display_name = data
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let (server, parsed_db) = parse_connection_string(connection_string);
    let database = if display_name.is_empty() {
        parsed_db
    } else {
        display_name.to_string()
    };

    Ok((server, database))
}

/// Execute a SQL query over TDS and render results.
///
/// `server` is the hostname (without port), `database` is the initial catalog.
pub async fn execute_and_render_sql(
    cli: &Cli,
    client: &FabricClient,
    server: &str,
    database: &str,
    sql_text: &str,
) -> anyhow::Result<()> {
    let (columns, all_rows) = execute_sql_rows(client, server, database, sql_text).await?;

    // Distinguish "no result set" (DDL/DML like CREATE/INSERT — no columns) from a
    // result set that simply matched zero rows (a SELECT always returns column
    // metadata). A 0-row SELECT must render as the list envelope `{"data":[],
    // "count":0}` so agents that iterate/filter `data` behave consistently —
    // rendering it as a scalar `{"rows_affected":0,"message":…}` object (the old
    // behavior) broke `queries-history --label X`, `queries-running`,
    // `statistics-list`, and every other TDS list/insight command whenever it
    // legitimately matched nothing.
    if produced_result_set(&columns) {
        let col_refs: Vec<&str> = columns.iter().map(String::as_str).collect();
        output::render_list(cli, &all_rows, &col_refs, &col_refs, &columns[0]);
    } else {
        let obj = serde_json::json!({
            "rows_affected": 0,
            "message": "Query executed successfully (no result set returned)."
        });
        output::render_object(cli, &obj, "message");
    }

    Ok(())
}

/// Whether a TDS execution produced an actual result set. A `SELECT` always
/// returns column metadata (even when it matches zero rows), whereas DDL/DML
/// (`CREATE`/`INSERT`/`UPDATE`/…) returns no columns. This is the correct
/// discriminator for list-vs-scalar rendering — using row-emptiness instead
/// would misrender a legitimately-empty query result as a "no result set"
/// scalar object. Pure.
#[inline]
pub const fn produced_result_set(columns: &[String]) -> bool {
    !columns.is_empty()
}

/// Execute a SQL query over TDS and return `(column_names, rows)`.
///
/// Rows are returned as `serde_json::Value::Object`s keyed by column name.
/// This is the row-returning counterpart of [`execute_and_render_sql`], for
/// callers that need to consume the result set programmatically rather than
/// render it to stdout.
pub async fn execute_sql_rows(
    client: &FabricClient,
    server: &str,
    database: &str,
    sql_text: &str,
) -> anyhow::Result<(Vec<String>, Vec<Value>)> {
    // Heap-allocate the TDS connect+execute+collect future. The tiberius client
    // state machine is large (~16KB), so embedding it inline would trip
    // clippy::large_futures in every caller on windows-msvc (where the future is
    // a few hundred bytes larger than on Linux). Boxing it here at the single
    // leaf shrinks this fn's future to a pointer and keeps ALL transitive
    // callers small. See clippy.toml (future-size-threshold) and AGENTS.md.
    Box::pin(async move {
        // Acquire AAD token for SQL scope
        let token = client.require_sql_auth().await?;

        // Build TDS connection
        let data_source = format!("tcp:{server},1433");
        let mut context = ClientContext::with_data_source(&data_source);
        context.database = database.to_string();
        context.tds_authentication_method = TdsAuthenticationMethod::AccessToken;
        context.access_token = Some(token);
        context.application_name = "fabio".to_string();
        context.connect_timeout = 30;

        let provider = TdsConnectionProvider {};
        let mut tds_client = provider
            .create_client(context, &data_source, None)
            .await
            .map_err(|e| tds_connection_error(&e))?;

        // Execute SQL
        tds_client
            .execute(sql_text.to_string(), Some(60), None)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                let hint = if msg.contains("Invalid object name") && msg.contains("sys.") {
                    ". Hint: Fabric Warehouse/Lakehouse SQL does not support all SQL Server \
                     system views. Supported: sys.tables, sys.columns, sys.schemas, \
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
            columns = rs
                .get_metadata()
                .iter()
                .map(|col| col.column_name.clone())
                .collect();

            while let Some(row) = rs.next_row().await.map_err(|e| {
                FabioError::new(ErrorCode::ApiError, format!("Failed to read row: {e}"))
            })? {
                let mut obj = serde_json::Map::with_capacity(columns.len());
                for (i, val) in row.into_iter().enumerate() {
                    let col_name = columns
                        .get(i)
                        .map_or_else(|| format!("column{i}"), std::clone::Clone::clone);
                    obj.insert(col_name, column_value_to_json(&val));
                }
                all_rows.push(Value::Object(obj));
            }
        }

        tds_client.close_query().await.map_err(|e| {
            FabioError::new(ErrorCode::ApiError, format!("Failed to close query: {e}"))
        })?;

        Ok((columns, all_rows))
    })
    .await
}

/// Convert a TDS `ColumnValues` to a `serde_json::Value`.
/// Convert a TDS day count measured from 0001-01-01 (day 0) to a `NaiveDate`.
/// chrono's `from_num_days_from_ce` treats day 1 as 0001-01-01, so add 1.
fn ce_date(days: i64) -> Option<NaiveDate> {
    i32::try_from(days)
        .ok()
        .and_then(|d| NaiveDate::from_num_days_from_ce_opt(d + 1))
}

/// Convert a day count measured from 1900-01-01 (legacy DATETIME/SMALLDATETIME).
fn date_1900(days: i64) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(1900, 1, 1).and_then(|b| b.checked_add_signed(Duration::days(days)))
}

/// Build a `NaiveTime` from nanoseconds since midnight.
fn time_from_nanos(nanos: u64) -> Option<NaiveTime> {
    let secs = u32::try_from(nanos / 1_000_000_000).ok()?;
    let nano = u32::try_from(nanos % 1_000_000_000).ok()?;
    NaiveTime::from_num_seconds_from_midnight_opt(secs, nano)
}

/// Format a DATETIME2 (`days` since 0001-01-01, `ns_ticks` in 100-ns ticks).
fn fmt_datetime2(days: i64, ns_ticks: u64) -> Value {
    let nanos = ns_ticks.saturating_mul(100);
    ce_date(days).zip(time_from_nanos(nanos)).map_or_else(
        || Value::from(format!("{days} days + {ns_ticks} ticks")),
        |(d, t)| Value::from(d.and_time(t).format("%Y-%m-%dT%H:%M:%S%.9f").to_string()),
    )
}

/// Format a DATETIMEOFFSET. The wire format stores the datetime2 in UTC + a
/// separate offset (minutes); add the offset back to render the local time.
fn fmt_datetimeoffset(days: i64, ns_ticks: u64, off: i16) -> Value {
    let nanos = ns_ticks.saturating_mul(100);
    let sign = if off < 0 { '-' } else { '+' };
    let (oh, om) = (off.unsigned_abs() / 60, off.unsigned_abs() % 60);
    ce_date(days)
        .zip(time_from_nanos(nanos))
        .and_then(|(d, t)| {
            d.and_time(t)
                .checked_add_signed(Duration::minutes(i64::from(off)))
        })
        .map_or_else(
            || Value::from(format!("{days} days + offset {sign}{oh:02}:{om:02}")),
            |dt| {
                Value::from(format!(
                    "{}{sign}{oh:02}:{om:02}",
                    dt.format("%Y-%m-%dT%H:%M:%S%.9f")
                ))
            },
        )
}

pub fn column_value_to_json(val: &ColumnValues) -> Value {
    match val {
        ColumnValues::Null => Value::Null,
        ColumnValues::TinyInt(v) => Value::from(*v),
        ColumnValues::SmallInt(v) => Value::from(*v),
        ColumnValues::Int(v) => Value::from(*v),
        ColumnValues::BigInt(v) => Value::from(*v),
        ColumnValues::Real(v) => {
            serde_json::Number::from_f64(f64::from(*v)).map_or(Value::Null, Value::Number)
        }
        ColumnValues::Float(v) => {
            serde_json::Number::from_f64(*v).map_or(Value::Null, Value::Number)
        }
        ColumnValues::Bit(v) => Value::from(*v),
        ColumnValues::String(s) => Value::from(s.to_utf8_string()),
        ColumnValues::Decimal(d) | ColumnValues::Numeric(d) => {
            // Render as string to avoid precision loss
            Value::from(d.to_string())
        }
        ColumnValues::Uuid(u) => Value::from(u.to_string()),
        ColumnValues::DateTime(dt) => {
            // Legacy DATETIME: days since 1900-01-01, time in 1/300-second ticks.
            let nanos = u64::from(dt.time) * 10_000_000 / 3;
            match (date_1900(i64::from(dt.days)), time_from_nanos(nanos)) {
                (Some(d), Some(t)) => Value::from(format!(
                    "{}T{}",
                    d.format("%Y-%m-%d"),
                    t.format("%H:%M:%S%.3f")
                )),
                _ => Value::from(format!("{} days since 1900-01-01", dt.days)),
            }
        }
        ColumnValues::Date(d) => {
            // TDS DATE: days since 0001-01-01.
            let days = i64::from(d.get_days());
            ce_date(days).map_or_else(
                || Value::from(format!("{days} days since 0001-01-01")),
                |dt| Value::from(dt.format("%Y-%m-%d").to_string()),
            )
        }
        ColumnValues::Time(t) => {
            // `time_nanoseconds` is actually a count of 100-ns ticks.
            let nanos = t.time_nanoseconds.saturating_mul(100);
            time_from_nanos(nanos).map_or_else(
                || Value::from(format!("{} ticks", t.time_nanoseconds)),
                |tm| Value::from(tm.format("%H:%M:%S%.9f").to_string()),
            )
        }
        ColumnValues::DateTime2(dt2) => {
            fmt_datetime2(i64::from(dt2.days), dt2.time.time_nanoseconds)
        }
        ColumnValues::DateTimeOffset(dto) => fmt_datetimeoffset(
            i64::from(dto.datetime2.days),
            dto.datetime2.time.time_nanoseconds,
            dto.offset,
        ),
        ColumnValues::SmallDateTime(sdt) => {
            // SMALLDATETIME: days since 1900-01-01, time in minutes since midnight.
            let days = i64::from(sdt.days);
            let mins = u32::from(sdt.time);
            match (
                date_1900(days),
                NaiveTime::from_hms_opt(mins / 60, mins % 60, 0),
            ) {
                (Some(d), Some(t)) => {
                    Value::from(format!("{}T{}", d.format("%Y-%m-%d"), t.format("%H:%M")))
                }
                _ => Value::from(format!("{days} days since 1900 + {mins} minutes")),
            }
        }
        ColumnValues::Money(m) => {
            let lsb_i64 = i64::from(m.lsb_part) & 0x0000_0000_FFFF_FFFF;
            let val = lsb_i64 | (i64::from(m.msb_part) << 32);
            #[allow(clippy::cast_precision_loss)]
            let amount = (val as f64) / 10000.0;
            serde_json::Number::from_f64(amount).map_or(Value::Null, Value::Number)
        }
        ColumnValues::SmallMoney(sm) => {
            let amount = f64::from(sm.int_val) / 10000.0;
            serde_json::Number::from_f64(amount).map_or(Value::Null, Value::Number)
        }
        ColumnValues::Bytes(b) => Value::from(BASE64.encode(b)),
        ColumnValues::Xml(xml) => Value::from(xml.as_string()),
        ColumnValues::Json(j) => {
            // Try to parse as JSON value, fall back to string
            let s = j.as_string();
            serde_json::from_str(&s).unwrap_or_else(|_| Value::from(s))
        }
        ColumnValues::Vector(v) => Value::from(format!("{v:?}")),
    }
}

/// Capture the estimated execution plan (`SHOWPLAN_XML`) for a SQL query without executing it.
///
/// Returns a vector of plan XML strings, one per statement in the batch.
pub async fn capture_query_plan(
    client: &FabricClient,
    server: &str,
    database: &str,
    sql_text: &str,
) -> anyhow::Result<Vec<String>> {
    // Heap-allocate the TDS future (see execute_sql_rows for the rationale):
    // the tiberius client state machine would otherwise trip
    // clippy::large_futures in every caller on windows-msvc.
    Box::pin(async move {
        let token = client.require_sql_auth().await?;

        let data_source = format!("tcp:{server},1433");
        let mut context = ClientContext::with_data_source(&data_source);
        context.database = database.to_string();
        context.tds_authentication_method = TdsAuthenticationMethod::AccessToken;
        context.access_token = Some(token);
        context.application_name = "fabio".to_string();
        context.connect_timeout = 30;

        let provider = TdsConnectionProvider {};
        let mut tds_client = provider
            .create_client(context, &data_source, None)
            .await
            .map_err(|e| tds_connection_error(&e))?;

        // Enable SHOWPLAN_XML — the server returns plan XML instead of executing the query
        tds_client
            .execute("SET SHOWPLAN_XML ON".to_string(), Some(10), None)
            .await
            .map_err(|e| {
                FabioError::new(
                    ErrorCode::ApiError,
                    format!("Failed to enable SHOWPLAN_XML: {e}"),
                )
            })?;
        // Consume any result from SET command
        tds_client.close_query().await.ok();

        // Execute the user's SQL — server returns plan XML as a result set
        tds_client
            .execute(sql_text.to_string(), Some(60), None)
            .await
            .map_err(|e| {
                FabioError::new(
                    ErrorCode::ApiError,
                    format!("Failed to get execution plan: {e}"),
                )
            })?;

        // Each statement in the batch produces one row with one XML column
        let mut plans: Vec<String> = Vec::new();
        if let Some(rs) = tds_client.get_current_resultset() {
            while let Some(row) = rs.next_row().await.map_err(|e| {
                FabioError::new(ErrorCode::ApiError, format!("Failed to read plan row: {e}"))
            })? {
                for val in &row {
                    let xml = match val {
                        ColumnValues::Xml(xml) => xml.as_string(),
                        ColumnValues::String(s) => s.to_utf8_string(),
                        _ => continue,
                    };
                    if !xml.is_empty() {
                        plans.push(xml);
                    }
                }
            }
        }

        tds_client.close_query().await.ok();

        // Disable SHOWPLAN_XML (cleanup, best-effort)
        tds_client
            .execute("SET SHOWPLAN_XML OFF".to_string(), Some(10), None)
            .await
            .ok();
        tds_client.close_query().await.ok();

        if plans.is_empty() {
            return Err(FabioError::new(
                ErrorCode::ApiError,
                "No execution plan returned. The query may be invalid or unsupported.",
            )
            .into());
        }

        Ok(plans)
    })
    .await
}

// ─── Fabric Data Warehouse remote MCP server URLs ────────────────────────────
//
// The remote Fabric Data Warehouse MCP server (preview) is a Microsoft-hosted MCP
// endpoint that exposes a single T-SQL execution tool (live tool name
// `execute_query`; the docs call it `executeSQL`) over streamable HTTP. External
// MCP clients (VS Code agent mode, GitHub Copilot, Copilot Studio, Azure AI
// Foundry, ...) connect to it, signing in with Fabric credentials; the user's
// existing Fabric + SQL permissions apply. fabio constructs the deterministic URLs
// so agents don't have to guess the `/mcp/dataPlane/.../sqlEndpoint` shape. This is
// the Warehouse / SQL-analytics-endpoint analog of `kql-database mcp-url` and
// `ontology mcp-url`.
//
// See: <https://learn.microsoft.com/fabric/data-warehouse/data-warehouse-mcp-server>

/// Build the **global** Fabric Data Warehouse remote MCP server URL.
///
/// The global endpoint lets an agent work across warehouses by supplying the
/// workspace/item context per-prompt: `{base}/mcp/dataPlane/sqlEndpoint`.
#[must_use]
pub fn global_sql_mcp_url(base: &str) -> String {
    format!("{}/mcp/dataPlane/sqlEndpoint", base.trim_end_matches('/'))
}

/// Build the **item-scoped** Fabric Data Warehouse remote MCP server URL.
///
/// The item-scoped endpoint binds the connection to a specific warehouse or SQL
/// analytics endpoint item:
/// `{base}/mcp/dataPlane/workspaces/{workspace}/items/{id}/sqlEndpoint`.
#[must_use]
pub fn item_sql_mcp_url(base: &str, workspace: &str, id: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/mcp/dataPlane/workspaces/{workspace}/items/{id}/sqlEndpoint")
}

/// Render the Fabric Data Warehouse remote MCP server URLs (item-scoped + global)
/// for a Warehouse or SQL analytics endpoint item.
///
/// `exists` is a best-effort existence signal (a failed lookup means the id is
/// wrong, not that the URL is malformed). `not_found_hint` is emitted in place of
/// the consumption note when `exists` is false. Both URLs are derived from the
/// trusted `client::fabric_base_url()` (HTTPS) and are only *emitted* — fabio does
/// not call them, so no bearer token is ever sent to them.
pub fn render_sql_mcp_url(
    cli: &Cli,
    workspace: &str,
    id: &str,
    exists: bool,
    not_found_hint: &str,
) {
    let base = crate::client::fabric_base_url();
    let item = item_sql_mcp_url(base, workspace, id);
    let global = global_sql_mcp_url(base);

    let mut result = serde_json::json!({
        "id": id,
        "mcpUrl": item,
        "globalMcpUrl": global,
        "transport": "http",
        "exists": exists,
    });
    if exists {
        result["note"] = Value::from(
            "Consume the item-scoped mcpUrl as a remote MCP server (HTTP transport) from VS \
             Code agent mode, GitHub Copilot, Copilot Studio, Azure AI Foundry, or any MCP \
             client, signing in with a Fabric credential that has access to the item; it binds \
             the connection to this warehouse / SQL analytics endpoint. Use globalMcpUrl to \
             instead select the warehouse per-prompt (supply workspace + item context in chat). \
             The remote server (preview) exposes a single T-SQL execution tool (`execute_query`, \
             taking workspaceId/itemId/query; the docs also call it `executeSQL`) — no separate \
             schema/metadata tools, so agents discover schema via INFORMATION_SCHEMA/sys.* queries. \
             For scripted execution, execution plans, query insights, and statistics, prefer \
             fabio's native commands: warehouse/sql-endpoint query, plan, queries-*, statistics-*.",
        );
    } else {
        result["hint"] = Value::from(not_found_hint.to_string());
    }
    output::render_object(cli, &result, "mcpUrl");
}

#[cfg(test)]
mod tests {
    use super::*;
    use mssql_tds::datatypes::sql_json::SqlJson;
    use mssql_tds::datatypes::sql_string::{EncodingType, SqlString};
    use mssql_tds::token::tokens::SqlCollation;

    #[test]
    fn produced_result_set_distinguishes_select_from_ddl() {
        // A SELECT (even matching 0 rows) has column metadata → a result set,
        // must render as a list envelope, not a "no result set" scalar.
        assert!(produced_result_set(&["label".to_string()]));
        assert!(produced_result_set(&[
            "year".to_string(),
            "avg".to_string()
        ]));
        // DDL/DML (CREATE/INSERT/UPDATE) returns no columns → no result set.
        assert!(!produced_result_set(&[]));
    }

    #[test]
    fn ce_date_converts_days_to_iso() {
        // 0001-01-01 is TDS day 0.
        assert_eq!(ce_date(0).unwrap().to_string(), "0001-01-01");
        // 739630 days since 0001-01-01 == 2026-01-15 (live-verified InvoiceDate).
        assert_eq!(ce_date(739_630).unwrap().to_string(), "2026-01-15");
    }

    #[test]
    fn date_1900_bases_on_1900_01_01() {
        assert_eq!(date_1900(0).unwrap().to_string(), "1900-01-01");
        assert_eq!(date_1900(1).unwrap().to_string(), "1900-01-02");
    }

    #[test]
    fn time_from_nanos_builds_time() {
        // 14:30:45.5 = 52245.5 s = 52_245_500_000_000 ns
        let t = time_from_nanos(52_245_500_000_000).unwrap();
        assert_eq!(t.to_string(), "14:30:45.500");
        assert_eq!(time_from_nanos(0).unwrap().to_string(), "00:00:00");
    }

    #[test]
    fn sql_scope_hint_only_when_generic_token_without_sql_token() {
        // Fabric token set, no SQL token -> hint (the failure case).
        assert!(sql_scope_token_hint_for(true, false).is_some());
        // SQL token present -> no hint (correct setup).
        assert!(sql_scope_token_hint_for(true, true).is_none());
        // No static tokens at all (credential chain) -> no hint.
        assert!(sql_scope_token_hint_for(false, false).is_none());
        assert!(sql_scope_token_hint_for(false, true).is_none());
        // The hint names the corrective env var.
        assert!(
            sql_scope_token_hint_for(true, false)
                .unwrap()
                .contains("FABIO_SQL_ACCESS_TOKEN")
        );
    }

    #[test]
    fn null_converts_to_null() {
        assert_eq!(column_value_to_json(&ColumnValues::Null), Value::Null);
    }

    #[test]
    fn tinyint_converts_to_number() {
        assert_eq!(
            column_value_to_json(&ColumnValues::TinyInt(42)),
            Value::from(42)
        );
    }

    #[test]
    fn smallint_converts_to_number() {
        assert_eq!(
            column_value_to_json(&ColumnValues::SmallInt(-100)),
            Value::from(-100)
        );
    }

    #[test]
    fn int_converts_to_number() {
        assert_eq!(
            column_value_to_json(&ColumnValues::Int(123_456)),
            Value::from(123_456)
        );
    }

    #[test]
    fn bigint_converts_to_number() {
        assert_eq!(
            column_value_to_json(&ColumnValues::BigInt(9_000_000_000)),
            Value::from(9_000_000_000_i64)
        );
    }

    #[test]
    fn bit_true_converts_to_bool() {
        assert_eq!(
            column_value_to_json(&ColumnValues::Bit(true)),
            Value::from(true)
        );
    }

    #[test]
    fn bit_false_converts_to_bool() {
        assert_eq!(
            column_value_to_json(&ColumnValues::Bit(false)),
            Value::from(false)
        );
    }

    #[test]
    fn string_utf8_converts_to_string() {
        let s = SqlString::new(b"hello".to_vec(), EncodingType::Utf8);
        assert_eq!(
            column_value_to_json(&ColumnValues::String(s)),
            Value::from("hello")
        );
    }

    #[test]
    fn string_utf16_converts_to_string() {
        // "Hi" encoded as UTF-16LE: H=0x48,0x00 i=0x69,0x00
        let bytes = vec![0x48, 0x00, 0x69, 0x00];
        let s = SqlString::new(bytes, EncodingType::Utf16);
        assert_eq!(
            column_value_to_json(&ColumnValues::String(s)),
            Value::from("Hi")
        );
    }

    #[test]
    fn string_utf16_unicode_chars() {
        // "cafe\u{0301}" = "café" in UTF-16LE: c=0x63,0x00 a=0x61,0x00 f=0x66,0x00 e=0x65,0x00 \u0301=0x01,0x03
        let bytes = vec![0x63, 0x00, 0x61, 0x00, 0x66, 0x00, 0x65, 0x00, 0x01, 0x03];
        let s = SqlString::new(bytes, EncodingType::Utf16);
        assert_eq!(
            column_value_to_json(&ColumnValues::String(s)),
            Value::from("cafe\u{0301}")
        );
    }

    #[test]
    fn string_lcid_us_english_converts_to_string() {
        // US English (LCID 0x0409) uses Windows-1252 encoding
        // "Hello" in Windows-1252 is same as ASCII
        let collation = SqlCollation {
            info: 0x0409, // US English LCID
            lcid_language_id: 0,
            col_flags: 0,
            sort_id: 0,
        };
        let s = SqlString::new(b"Hello".to_vec(), EncodingType::LcidBased(collation));
        assert_eq!(
            column_value_to_json(&ColumnValues::String(s)),
            Value::from("Hello")
        );
    }

    #[test]
    fn float_converts_to_number() {
        let result = column_value_to_json(&ColumnValues::Float(1.23));
        assert!(result.is_number());
    }

    #[test]
    fn real_converts_to_number() {
        let result = column_value_to_json(&ColumnValues::Real(2.5));
        assert!(result.is_number());
    }

    #[test]
    fn bytes_converts_to_base64() {
        let result = column_value_to_json(&ColumnValues::Bytes(vec![0x48, 0x65, 0x6c]));
        assert_eq!(result, Value::from("SGVs"));
    }

    #[test]
    fn json_valid_parses_as_json() {
        let j = SqlJson::from(r#"{"key":"value"}"#.to_string());
        let result = column_value_to_json(&ColumnValues::Json(j));
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn json_invalid_falls_back_to_string() {
        let j = SqlJson::from("not valid json".to_string());
        let result = column_value_to_json(&ColumnValues::Json(j));
        assert_eq!(result, Value::from("not valid json"));
    }

    #[test]
    fn pool_insights_sql_targets_view_with_top_and_order() {
        let sql = pool_insights_sql(25);
        assert!(sql.contains("FROM queryinsights.sql_pool_insights"));
        assert!(sql.contains("TOP (25)"));
        assert!(sql.contains("ORDER BY timestamp DESC"));
        assert!(sql.contains("is_pool_under_pressure"));
        assert!(sql.contains("max_resource_percentage"));
    }

    #[test]
    fn queries_history_sql_no_label() {
        let sql = queries_history_sql(50, None);
        assert!(sql.contains("SELECT TOP (50)"));
        assert!(sql.contains("label,"));
        assert!(sql.contains("allocated_cpu_time_ms,"));
        assert!(sql.contains("data_scanned_remote_storage_mb,"));
        assert!(sql.contains("FROM queryinsights.exec_requests_history ORDER BY"));
        assert!(!sql.contains("WHERE"));
    }

    #[test]
    fn queries_history_sql_with_label() {
        let sql = queries_history_sql(10, Some("Clustered"));
        assert!(sql.contains("WHERE label = N'Clustered'"));
        assert!(sql.contains("ORDER BY start_time DESC"));
    }

    #[test]
    fn queries_history_sql_escapes_label_quotes() {
        let sql = queries_history_sql(10, Some("O'Brien"));
        // Single quote is doubled to prevent injection.
        assert!(sql.contains("WHERE label = N'O''Brien'"));
    }

    #[test]
    fn global_sql_mcp_url_is_data_plane_sql_endpoint() {
        assert_eq!(
            global_sql_mcp_url("https://api.fabric.microsoft.com/v1"),
            "https://api.fabric.microsoft.com/v1/mcp/dataPlane/sqlEndpoint"
        );
        // Trailing slash on the base is normalized.
        assert_eq!(
            global_sql_mcp_url("https://api.fabric.microsoft.com/v1/"),
            "https://api.fabric.microsoft.com/v1/mcp/dataPlane/sqlEndpoint"
        );
    }

    #[test]
    fn item_sql_mcp_url_binds_workspace_and_item() {
        assert_eq!(
            item_sql_mcp_url("https://api.fabric.microsoft.com/v1", "ws-1", "item-2"),
            "https://api.fabric.microsoft.com/v1/mcp/dataPlane/workspaces/ws-1/items/item-2/sqlEndpoint"
        );
        assert_eq!(
            item_sql_mcp_url("https://api.fabric.microsoft.com/v1/", "ws-1", "item-2"),
            "https://api.fabric.microsoft.com/v1/mcp/dataPlane/workspaces/ws-1/items/item-2/sqlEndpoint"
        );
    }

    #[test]
    fn sql_mcp_urls_are_https() {
        // Security invariant: emitted MCP endpoints are always HTTPS (trusted base).
        let base = crate::client::fabric_base_url();
        assert!(global_sql_mcp_url(base).starts_with("https://"));
        assert!(item_sql_mcp_url(base, "w", "i").starts_with("https://"));
    }
}
