use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using, run: fabio context schema Eventhouse | fabio context workflow rti-pipeline\nReturns definition templates and step-by-step setup recipes."
)]
pub enum EventhouseCommand {
    /// List eventhouses in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of an eventhouse
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventhouse ID
        #[arg(long)]
        id: String,
    },
    /// Create a new eventhouse
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventhouse display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,

        /// Minimum consumption units to keep the eventhouse always-on
        /// (create-time only, `creationPayload.minimumConsumptionUnits`).
        #[arg(long)]
        min_consumption_units: Option<f64>,
    },
    /// Update eventhouse properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventhouse ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete an eventhouse
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventhouse ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of an eventhouse
    #[command(name = "get-definition", display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventhouse ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of an eventhouse
    #[command(name = "update-definition", display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventhouse ID
        #[arg(long)]
        id: String,

        /// Path to eventhouse properties file
        #[arg(long)]
        file: Option<String>,

        /// Inline eventhouse properties content (JSON)
        #[arg(long)]
        content: Option<String>,
    },

    // ── Data plane (Kusto query) ─────────────────────────────────────────
    /// Run a KQL (or `.`-management, or T-SQL) query against the eventhouse cluster.
    ///
    /// Kusto queries are request/response and always terminate — so the default is
    /// a one-shot query (optionally bounded by `--timeout`). For continuous
    /// monitoring of live-ingesting data, use `--follow`: fabio polls the query on
    /// an interval and streams NDJSON (one JSON object per cycle), always bounded by
    /// `--max-duration`, `--limit`, or Ctrl-C so it never hangs.
    #[command(display_order = 8)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventhouse ID
        #[arg(long)]
        id: String,

        /// KQL/T-SQL/management text (inline, `@file`, or piped via stdin)
        #[arg(long)]
        kql: Option<String>,

        /// KQL database name to run against (default: the eventhouse's sole
        /// database; required when the eventhouse has more than one)
        #[arg(long)]
        database: Option<String>,

        /// Override the cluster query URI (must be a trusted Kusto endpoint)
        #[arg(long)]
        query_uri: Option<String>,

        /// Server-side query timeout in seconds (Kusto `servertimeout`, max 3600).
        /// Bounds a long-running query so it can never hang the caller.
        #[arg(long)]
        timeout: Option<u64>,

        /// Continuously re-run the query, streaming NDJSON until `--max-duration`,
        /// `--limit`, or Ctrl-C. Kusto has no server-push streaming, so this polls.
        #[arg(long)]
        follow: bool,

        /// Seconds between polls in `--follow` mode (default 5)
        #[arg(long)]
        interval: Option<u64>,

        /// Total seconds to follow before stopping — the agent-safety bound
        /// (default 60). Even in `--follow` the command always terminates.
        #[arg(long)]
        max_duration: Option<u64>,

        /// In `--follow`, only emit rows whose value in this column is greater than
        /// the max seen so far (incremental tail). Without it, each cycle re-emits
        /// the full result (watch semantics).
        #[arg(long)]
        dedup_column: Option<String>,
    },

    /// List the KQL databases hosted in the eventhouse cluster
    #[command(name = "list-databases", display_order = 9)]
    ListDatabases {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventhouse ID
        #[arg(long)]
        id: String,
    },

    /// Print the eventhouse cluster query URI (deterministic; agents cannot guess it)
    #[command(name = "query-uri", display_order = 10)]
    QueryUri {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventhouse ID
        #[arg(long)]
        id: String,
    },

    /// Print the eventhouse cluster ingestion URI
    #[command(name = "ingestion-uri", display_order = 11)]
    IngestionUri {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventhouse ID
        #[arg(long)]
        id: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &EventhouseCommand) -> Result<()> {
    match command {
        EventhouseCommand::List { workspace } => list(cli, client, workspace).await,
        EventhouseCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        EventhouseCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
            min_consumption_units,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
                *min_consumption_units,
            )
            .await
        }
        EventhouseCommand::Update {
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
        EventhouseCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        EventhouseCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        EventhouseCommand::UpdateDefinition {
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
        EventhouseCommand::Query {
            workspace,
            id,
            kql,
            database,
            query_uri,
            timeout,
            follow,
            interval,
            max_duration,
            dedup_column,
        } => {
            Box::pin(query(
                cli,
                client,
                workspace,
                id,
                kql.as_deref(),
                database.as_deref(),
                query_uri.as_deref(),
                *timeout,
                *follow,
                *interval,
                *max_duration,
                dedup_column.as_deref(),
            ))
            .await
        }
        EventhouseCommand::ListDatabases { workspace, id } => {
            list_databases(cli, client, workspace, id).await
        }
        EventhouseCommand::QueryUri { workspace, id } => {
            print_uri(cli, client, workspace, id, UriKind::Query).await
        }
        EventhouseCommand::IngestionUri { workspace, id } => {
            print_uri(cli, client, workspace, id, UriKind::Ingestion).await
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "eventhouses",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "eventhouses", workspace, id).await
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
    min_consumption_units: Option<f64>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }
    if let Some(units) = min_consumption_units {
        // Always-on minimum consumption is set at create time via creationPayload.
        body["creationPayload"] = serde_json::json!({ "minimumConsumptionUnits": units });
    }

    if output::dry_run_guard(
        cli,
        "eventhouse create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "sensitivityLabel": sensitivity_label,
            "minConsumptionUnits": min_consumption_units
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/eventhouses"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "eventhouse create", "Member"))?;
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
    crate::commands::crud::update(
        cli,
        client,
        "eventhouse",
        "eventhouses",
        "Contributor",
        workspace,
        id,
        name,
        description,
    )
    .await
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    crate::commands::crud::delete(
        cli,
        client,
        "eventhouse",
        "eventhouses",
        "Member",
        workspace,
        id,
        hard_delete,
    )
    .await
}

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    crate::commands::crud::get_definition(
        cli,
        client,
        "eventhouse",
        "eventhouses",
        "Contributor",
        workspace,
        id,
        decode,
    )
    .await
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let raw = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio eventhouse update-definition --workspace <WS> --id <ID> --file props.json".to_string(),
            ).into());
        }
    };

    let body =
        crate::definition_spec::build_update_definition_body(&raw, "EventhouseProperties.json");

    if output::dry_run_guard(
        cli,
        "eventhouse update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": raw.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/eventhouses/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventhouse update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ── Data plane ───────────────────────────────────────────────────────────────

/// The queryable/ingestion properties of an eventhouse cluster.
struct EventhouseProps {
    query_uri: Option<String>,
    ingestion_uri: Option<String>,
    /// KQL database item ids hosted in the cluster.
    database_ids: Vec<String>,
}

/// Fetch and extract the eventhouse's cluster URIs and hosted database ids.
async fn resolve_eventhouse_props(
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<EventhouseProps> {
    let data = client
        .get(&format!("/workspaces/{workspace}/eventhouses/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "eventhouse query", "Viewer"))?;
    let props = data.get("properties");
    let str_prop = |k: &str| {
        props
            .and_then(|p| p.get(k))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let database_ids = props
        .and_then(|p| p.get("databasesItemIds"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(EventhouseProps {
        query_uri: str_prop("queryServiceUri"),
        ingestion_uri: str_prop("ingestionServiceUri"),
        database_ids,
    })
}

/// Resolve the display names of the KQL databases hosted in the eventhouse.
async fn resolve_database_names(
    client: &FabricClient,
    workspace: &str,
    database_ids: &[String],
) -> Vec<String> {
    let mut names = Vec::new();
    for db_id in database_ids {
        if let Ok(db) = client
            .get(&format!("/workspaces/{workspace}/kqlDatabases/{db_id}"))
            .await
            && let Some(name) = db.get("displayName").and_then(Value::as_str)
        {
            names.push(name.to_owned());
        }
    }
    names
}

/// Resolve (`query_uri`, `database_name`) for a Kusto query against the eventhouse.
async fn resolve_query_target(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    database_override: Option<&str>,
    uri_override: Option<&str>,
    is_mgmt: bool,
) -> Result<(String, String)> {
    let props = resolve_eventhouse_props(client, workspace, id).await?;

    let query_uri = if let Some(uri) = uri_override {
        crate::client::validate_trusted_url(uri, "--query-uri")?;
        uri.trim_end_matches('/').to_string()
    } else {
        let uri = props.query_uri.ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                "Eventhouse has no queryServiceUri property".to_string(),
                "Pass --query-uri <cluster-uri> explicitly (e.g. https://<cluster>.kusto.fabric.microsoft.com).".to_string(),
            )
        })?;
        crate::client::validate_trusted_url(&uri, "queryServiceUri")?;
        uri.trim_end_matches('/').to_string()
    };

    if let Some(db) = database_override {
        return Ok((query_uri, db.to_owned()));
    }

    let names = resolve_database_names(client, workspace, &props.database_ids).await;
    match names.len() {
        1 => Ok((query_uri, names[0].clone())),
        0 if is_mgmt => Ok((query_uri, "NetDefaultDB".to_string())),
        0 => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Eventhouse has no KQL databases to query".to_string(),
            "Create a KQL database first: fabio kql-database create --workspace <WS> --eventhouse <ID> --name <DB>.".to_string(),
        )
        .into()),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Eventhouse hosts {} databases; specify one with --database",
                names.len()
            ),
            format!("Available: {}. Example: --database {}", names.join(", "), names[0]),
        )
        .into()),
    }
}

#[allow(clippy::too_many_arguments)]
async fn query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    kql: Option<&str>,
    database: Option<&str>,
    query_uri: Option<&str>,
    timeout: Option<u64>,
    follow: bool,
    interval: Option<u64>,
    max_duration: Option<u64>,
    dedup_column: Option<&str>,
) -> Result<()> {
    // Follow-only flags require --follow.
    if !follow && (interval.is_some() || max_duration.is_some() || dedup_column.is_some()) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--interval, --max-duration, and --dedup-column require --follow".to_string(),
            "Add --follow to stream the query on an interval, or drop those flags for a one-shot query.".to_string(),
        )
        .into());
    }

    let kql_text = crate::commands::kql_utils::resolve_kql_input(kql)?;
    let is_mgmt = kql_text.trim_start().starts_with('.');
    let (kusto_uri, db_name) =
        resolve_query_target(client, workspace, id, database, query_uri, is_mgmt).await?;

    if follow {
        return follow_query(
            cli,
            client,
            &kusto_uri,
            &db_name,
            &kql_text,
            timeout,
            interval.unwrap_or(5),
            max_duration.unwrap_or(60),
            dedup_column,
        )
        .await;
    }

    let (rows, columns) = crate::commands::kql_utils::execute_kql_with_timeout(
        client, &kusto_uri, &db_name, &kql_text, timeout,
    )
    .await?;
    crate::commands::kql_utils::render_kql_results(cli, &rows, &columns);
    Ok(())
}

/// Continuously poll the query, streaming one NDJSON object per cycle to stdout,
/// bounded by `max_duration`, the global `--limit`, or Ctrl-C. Kusto has no
/// server-push streaming, so "continuous" is client-side polling that always
/// terminates — safe for an agent/CI caller.
#[allow(clippy::too_many_arguments)]
async fn follow_query(
    cli: &Cli,
    client: &FabricClient,
    kusto_uri: &str,
    db_name: &str,
    kql_text: &str,
    timeout: Option<u64>,
    interval_secs: u64,
    max_duration_secs: u64,
    dedup_column: Option<&str>,
) -> Result<()> {
    use std::io::Write;

    let interval = std::time::Duration::from_secs(interval_secs.max(1));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(max_duration_secs);
    let row_limit = cli.limit;

    let mut cycle: u64 = 0;
    let mut total_emitted: usize = 0;
    let mut last_max: Option<Value> = None;
    let mut stop_reason = "max_duration";

    loop {
        cycle += 1;
        let started = tokio::time::Instant::now();

        match crate::commands::kql_utils::execute_kql_with_timeout(
            client, kusto_uri, db_name, kql_text, timeout,
        )
        .await
        {
            Ok((rows, columns)) => {
                let new_rows = if let Some(col) = dedup_column {
                    filter_new_rows(&rows, col, &mut last_max)
                } else {
                    rows
                };
                total_emitted += new_rows.len();
                if !cli.quiet {
                    let event = serde_json::json!({
                        "cycle": cycle,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                        "count": new_rows.len(),
                        "columns": columns,
                        "rows": new_rows,
                    });
                    let mut out = std::io::stdout();
                    let _ = writeln!(out, "{}", serde_json::to_string(&event).unwrap_or_default());
                    let _ = out.flush();
                }
            }
            Err(e) => {
                if !cli.quiet {
                    let event = serde_json::json!({
                        "cycle": cycle,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                        "error": e.to_string(),
                    });
                    let mut out = std::io::stdout();
                    let _ = writeln!(out, "{}", serde_json::to_string(&event).unwrap_or_default());
                    let _ = out.flush();
                }
            }
        }

        if row_limit.is_some_and(|lim| total_emitted >= lim) {
            stop_reason = "limit";
            break;
        }

        // Sleep until the next interval, but wake early on Ctrl-C or the deadline.
        let next = started + interval;
        tokio::select! {
            () = tokio::time::sleep_until(next.min(deadline)) => {
                if tokio::time::Instant::now() >= deadline {
                    // stop_reason keeps its default "max_duration"
                    break;
                }
            }
            _ = tokio::signal::ctrl_c() => {
                stop_reason = "interrupted";
                break;
            }
        }
    }

    // Final summary line (NDJSON): the follow always terminates.
    if !cli.quiet {
        let summary = serde_json::json!({
            "status": "follow_complete",
            "reason": stop_reason,
            "cycles": cycle,
            "rows_emitted": total_emitted,
        });
        let mut out = std::io::stdout();
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string(&summary).unwrap_or_default()
        );
        let _ = out.flush();
    }
    Ok(())
}

/// Return the rows whose `column` value is strictly greater than `last_max`,
/// updating `last_max` to the greatest value seen. Used for incremental tailing.
fn filter_new_rows(rows: &[Value], column: &str, last_max: &mut Option<Value>) -> Vec<Value> {
    let threshold = last_max.clone();
    let mut cycle_max = threshold.clone();
    let mut out = Vec::new();
    for row in rows {
        let Some(v) = row.get(column) else { continue };
        if threshold.as_ref().is_none_or(|m| value_gt(v, m)) {
            out.push(row.clone());
        }
        if cycle_max.as_ref().is_none_or(|m| value_gt(v, m)) {
            cycle_max = Some(v.clone());
        }
    }
    *last_max = cycle_max;
    out
}

/// Order two JSON scalars: numerically when both are numbers, else by string.
fn value_gt(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x > y,
        _ => a.as_str().unwrap_or("") > b.as_str().unwrap_or(""),
    }
}

async fn list_databases(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let props = resolve_eventhouse_props(client, workspace, id).await?;
    let mut items: Vec<Value> = Vec::new();
    for db_id in &props.database_ids {
        if let Ok(db) = client
            .get(&format!("/workspaces/{workspace}/kqlDatabases/{db_id}"))
            .await
        {
            items.push(serde_json::json!({
                "id": db_id,
                "displayName": db.get("displayName").and_then(Value::as_str).unwrap_or_default(),
                "description": db.get("description").and_then(Value::as_str).unwrap_or_default(),
            }));
        } else {
            items.push(serde_json::json!({ "id": db_id }));
        }
    }
    output::render_list(
        cli,
        &items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "displayName",
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum UriKind {
    Query,
    Ingestion,
}

async fn print_uri(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    kind: UriKind,
) -> Result<()> {
    let props = resolve_eventhouse_props(client, workspace, id).await?;
    let (label, uri) = match kind {
        UriKind::Query => ("queryUri", props.query_uri),
        UriKind::Ingestion => ("ingestionUri", props.ingestion_uri),
    };
    let uri = uri.ok_or_else(|| {
        FabioError::new(
            ErrorCode::NotFound,
            format!("Eventhouse has no {label} property"),
        )
    })?;
    let obj = serde_json::json!({ "id": id, label: uri });
    output::render_object(cli, &obj, label);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn value_gt_numeric_and_string() {
        assert!(value_gt(&json!(5), &json!(3)));
        assert!(!value_gt(&json!(3), &json!(5)));
        assert!(value_gt(&json!("2026-01-02"), &json!("2026-01-01")));
        assert!(!value_gt(&json!("a"), &json!("b")));
    }

    #[test]
    fn filter_new_rows_emits_only_newer_and_advances_threshold() {
        let rows = vec![
            json!({"seq": 1, "v": "a"}),
            json!({"seq": 3, "v": "b"}),
            json!({"seq": 2, "v": "c"}),
        ];
        let mut last = None;
        // First cycle: no threshold → all rows pass; threshold advances to max (3).
        let out = filter_new_rows(&rows, "seq", &mut last);
        assert_eq!(out.len(), 3);
        assert_eq!(last, Some(json!(3)));

        // Second cycle with the same data → nothing is newer than 3.
        let out2 = filter_new_rows(&rows, "seq", &mut last);
        assert!(out2.is_empty());
        assert_eq!(last, Some(json!(3)));

        // A newer row (seq 4) is emitted; threshold advances to 4.
        let more = vec![json!({"seq": 4, "v": "d"}), json!({"seq": 3, "v": "b"})];
        let out3 = filter_new_rows(&more, "seq", &mut last);
        assert_eq!(out3.len(), 1);
        assert_eq!(out3[0]["seq"], json!(4));
        assert_eq!(last, Some(json!(4)));
    }

    #[test]
    fn filter_new_rows_skips_rows_missing_the_column() {
        let rows = vec![json!({"other": 1}), json!({"seq": 10})];
        let mut last = None;
        let out = filter_new_rows(&rows, "seq", &mut last);
        assert_eq!(out.len(), 1);
        assert_eq!(last, Some(json!(10)));
    }
}
