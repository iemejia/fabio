use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{self, FabricClient};
use crate::commands::kql_utils;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For step-by-step setup, run: fabio context workflow rti-pipeline\nReturns a complete RTI pipeline recipe with exact command syntax."
)]
pub enum KqlQuerysetCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List KQL querysets in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a KQL queryset
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL queryset ID
        #[arg(long)]
        id: String,
    },
    /// Create a new KQL queryset
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Queryset display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update KQL queryset properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL queryset ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a KQL queryset
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL queryset ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a KQL queryset
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL queryset ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a KQL queryset
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL queryset ID
        #[arg(long)]
        id: String,

        /// KQL queryset file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// KQL queryset content (inline)
        #[arg(long)]
        content: Option<String>,
    },

    // ── Query Execution ──────────────────────────────────────────────────
    /// Run a saved query tab from the queryset against its configured data source
    #[command(display_order = 8)]
    Run {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL queryset ID
        #[arg(long)]
        id: String,

        /// Tab name or zero-based index to execute (default: first tab)
        #[arg(long)]
        tab: Option<String>,

        /// Override the Kusto query URI (default: from queryset data source)
        #[arg(long)]
        query_uri: Option<String>,
    },

    /// Add a saved query tab bound to a KQL database (authors the `RealTimeQueryset.json` data source + tab)
    #[command(display_order = 7)]
    AddTab {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// KQL queryset ID
        #[arg(long)]
        id: String,

        /// The KQL database (in an eventhouse) the tab queries
        #[arg(long = "kql-database")]
        kql_database: String,

        /// Tab title
        #[arg(long)]
        title: String,

        /// KQL query text (inline, `@file`, or omit to read from stdin)
        #[arg(long)]
        kql: Option<String>,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &KqlQuerysetCommand) -> Result<()> {
    match command {
        KqlQuerysetCommand::List { workspace } => list(cli, client, workspace).await,
        KqlQuerysetCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        KqlQuerysetCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        KqlQuerysetCommand::Update {
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
        KqlQuerysetCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        KqlQuerysetCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        KqlQuerysetCommand::UpdateDefinition {
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
        KqlQuerysetCommand::Run {
            workspace,
            id,
            tab,
            query_uri,
        } => {
            run(
                cli,
                client,
                workspace,
                id,
                tab.as_deref(),
                query_uri.as_deref(),
            )
            .await
        }
        KqlQuerysetCommand::AddTab {
            workspace,
            id,
            kql_database,
            title,
            kql,
        } => {
            add_tab(
                cli,
                client,
                workspace,
                id,
                kql_database,
                title,
                kql.as_deref(),
            )
            .await
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "kqlQuerysets",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "kqlQuerysets", workspace, id).await
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(cli, "kql-queryset create", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/kqlQuerysets"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "kql-queryset create", "Member"))?;
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
        "kql-queryset",
        "kqlQuerysets",
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
        "kql-queryset",
        "kqlQuerysets",
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
        "kql-queryset",
        "kqlQuerysets",
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
    let script = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio kql-queryset update-definition --workspace <WS> --id <ID> --file query.kql".to_string(),
            ).into());
        }
    };

    let body =
        crate::definition_spec::build_update_definition_body(&script, "RealTimeQueryset.json");

    if output::dry_run_guard(
        cli,
        "kql-queryset update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": script.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/kqlQuerysets/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "kql-queryset update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Run (Query Execution) ───────────────────────────────────────────────────

/// Run a saved query tab from the queryset definition against its configured data source.
#[allow(clippy::too_many_lines)]
async fn run(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    tab_selector: Option<&str>,
    query_uri_override: Option<&str>,
) -> Result<()> {
    // 1. Fetch queryset definition (LRO)
    let def_data = client
        .post(
            &format!("/workspaces/{workspace}/kqlQuerysets/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "kql-queryset run", "Viewer"))?;

    // 2. Find RealTimeQueryset.json part and decode it
    let queryset = decode_queryset_definition(&def_data)?;

    // 3. Extract data sources and tabs
    let qs = queryset.get("queryset").ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Queryset definition missing 'queryset' root object.".to_string(),
            "The queryset may be empty. Use 'kql-queryset update-definition' to save queries."
                .to_string(),
        )
    })?;

    let data_sources = qs
        .get("dataSources")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Queryset has no data sources configured.".to_string(),
                "Update the queryset definition with data source info (clusterUri, databaseName)."
                    .to_string(),
            )
        })?;

    let tabs = qs.get("tabs").and_then(Value::as_array).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Queryset has no tabs (saved queries).".to_string(),
            "Update the queryset definition to add tabs with KQL queries.".to_string(),
        )
    })?;

    if tabs.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Queryset has no tabs (saved queries).".to_string(),
            "Update the queryset definition to add tabs with KQL queries.".to_string(),
        )
        .into());
    }

    // 4. Select tab by name or index
    let tab = select_tab(tabs, tab_selector)?;

    // 5. Get the KQL content from the tab
    let kql_text = tab.get("content").and_then(Value::as_str).ok_or_else(|| {
        FabioError::new(
            ErrorCode::InvalidInput,
            "Selected tab has no 'content' field (KQL query text).".to_string(),
        )
    })?;

    if kql_text.trim().is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Selected tab has empty KQL query content.".to_string(),
            "Update the queryset definition with a non-empty query in the tab.".to_string(),
        )
        .into());
    }

    // 6. Resolve data source for this tab
    let ds_id = tab.get("dataSourceId").and_then(Value::as_str);
    let data_source = resolve_data_source(data_sources, ds_id)?;

    let cluster_uri = query_uri_override
        .map(|u| {
            client::validate_trusted_url(u, "--query-uri")?;
            Ok::<_, anyhow::Error>(u.trim_end_matches('/').to_string())
        })
        .transpose()?
        .or_else(|| {
            data_source
                .get("clusterUri")
                .and_then(Value::as_str)
                .map(|u| u.trim_end_matches('/').to_string())
        })
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Could not determine Kusto query URI from queryset data source.".to_string(),
                "Provide --query-uri manually or update the queryset definition with clusterUri."
                    .to_string(),
            )
        })?;

    // Validate clusterUri from definition against trusted domains (prevents token
    // exfiltration via crafted updateDefinition with a malicious clusterUri)
    client::validate_trusted_url(&cluster_uri, "clusterUri (from queryset definition)")?;

    // A `type: "Fabric"` data source (a Fabric KQL database) names its database
    // as `databaseItemName`; an `AzureDataExplorer` source uses `databaseName`.
    let db_name = data_source
        .get("databaseName")
        .or_else(|| data_source.get("databaseItemName"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    // 7. Execute KQL query via shared utility
    let (rows, columns) = kql_utils::execute_kql(client, &cluster_uri, db_name, kql_text).await?;

    // 8. Render output
    if rows.is_empty() {
        let obj = serde_json::json!({
            "rows_returned": 0,
            "tab": tab.get("title").and_then(Value::as_str).unwrap_or(""),
            "message": "Query executed successfully (no results returned)."
        });
        output::render_object(cli, &obj, "message");
    } else {
        let col_refs: Vec<&str> = columns.iter().map(String::as_str).collect();
        output::render_list(cli, &rows, &col_refs, &col_refs, &columns[0]);
    }

    Ok(())
}

/// A Fabric KQL-database data source for a queryset tab.
struct FabricDbSource<'a> {
    cluster_uri: &'a str,
    db_item_id: &'a str,
    db_item_name: &'a str,
}

/// Add (or reuse) a Fabric KQL-database data source and append a query tab to a
/// queryset's `RealTimeQueryset.json` content. Pure read-modify-write so it is
/// unit-testable; ids are supplied by the caller.
fn queryset_add_tab(
    existing: &Value,
    source: &FabricDbSource,
    title: &str,
    content: &str,
    new_ds_id: &str,
    tab_id: &str,
) -> Value {
    let mut queryset = existing
        .get("queryset")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if queryset.get("version").is_none() {
        queryset["version"] = Value::from("1.0.0");
    }
    let mut data_sources = queryset
        .get_mut("dataSources")
        .and_then(Value::as_array_mut)
        .map(std::mem::take)
        .unwrap_or_default();
    // Reuse an existing Fabric data source for this KQL database, else add one.
    let existing_ds_id = data_sources
        .iter()
        .find(|ds| {
            ds.get("type").and_then(Value::as_str) == Some("Fabric")
                && ds.get("databaseItemId").and_then(Value::as_str) == Some(source.db_item_id)
        })
        .and_then(|ds| ds.get("id").and_then(Value::as_str))
        .map(str::to_string);
    let ds_id = existing_ds_id.unwrap_or_else(|| {
        data_sources.push(serde_json::json!({
            "id": new_ds_id,
            "clusterUri": source.cluster_uri,
            "type": "Fabric",
            "databaseItemId": source.db_item_id,
            "databaseItemName": source.db_item_name
        }));
        new_ds_id.to_string()
    });
    queryset["dataSources"] = Value::Array(data_sources);

    let mut tabs = queryset
        .get_mut("tabs")
        .and_then(Value::as_array_mut)
        .map(std::mem::take)
        .unwrap_or_default();
    tabs.push(serde_json::json!({
        "id": tab_id,
        "title": title,
        "content": content,
        "dataSourceId": ds_id
    }));
    queryset["tabs"] = Value::Array(tabs);

    serde_json::json!({ "queryset": queryset })
}

/// Add a query tab bound to a KQL database (authors the RealTimeQueryset.json).
async fn add_tab(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    kql_database: &str,
    title: &str,
    kql: Option<&str>,
) -> Result<()> {
    let content = crate::commands::query_input::resolve_query_input(
        kql,
        "KQL",
        "--kql",
        "fabio kql-queryset add-tab --id <ID> --kql-database <DB> --title <T> --kql \"Sales | count\"",
    )?;

    if output::dry_run_guard(
        cli,
        "kql-queryset add-tab",
        &serde_json::json!({ "queryset": id, "title": title, "kqlDatabase": kql_database }),
    ) {
        return Ok(());
    }

    // Resolve the KQL database's cluster URI + name (for the Fabric data source).
    let db = client
        .get(&format!(
            "/workspaces/{workspace}/kqlDatabases/{kql_database}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "kql-queryset add-tab", "Viewer"))?;
    let cluster_uri = db
        .pointer("/properties/queryServiceUri")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "KQL database has no queryServiceUri (cluster URI).".to_string(),
            )
        })?
        .trim_end_matches('/')
        .to_string();
    let db_name = db
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // Read-modify-write the queryset definition.
    let def_data = client
        .post(
            &format!("/workspaces/{workspace}/kqlQuerysets/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "kql-queryset add-tab", "Contributor"))?;
    let existing = decode_queryset_definition(&def_data).unwrap_or_else(|_| serde_json::json!({}));

    let new_ds_id = uuid::Uuid::new_v4().to_string();
    let tab_id = uuid::Uuid::new_v4().to_string();
    let new_def = queryset_add_tab(
        &existing,
        &FabricDbSource {
            cluster_uri: &cluster_uri,
            db_item_id: kql_database,
            db_item_name: &db_name,
        },
        title,
        &content,
        &new_ds_id,
        &tab_id,
    );

    let payload = BASE64.encode(serde_json::to_vec(&new_def).unwrap_or_default());
    let body = serde_json::json!({
        "definition": { "parts": [{
            "path": "RealTimeQueryset.json",
            "payload": payload,
            "payloadType": "InlineBase64"
        }]}
    });
    client
        .post(
            &format!("/workspaces/{workspace}/kqlQuerysets/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "kql-queryset add-tab", "Contributor"))?;

    output::render_object(
        cli,
        &serde_json::json!({
            "status": "tab_added",
            "queryset": id,
            "title": title,
            "tabId": tab_id
        }),
        "status",
    );
    Ok(())
}

/// Decode RealTimeQueryset.json from the getDefinition response.
fn decode_queryset_definition(def_data: &Value) -> Result<Value> {
    let parts = def_data
        .get("definition")
        .and_then(|d| d.get("parts"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "Unexpected definition response: missing 'definition.parts' array.".to_string(),
            )
        })?;

    let queryset_part = parts
        .iter()
        .find(|p| p.get("path").and_then(Value::as_str) == Some("RealTimeQueryset.json"))
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                "No 'RealTimeQueryset.json' part found in queryset definition.".to_string(),
                "The queryset may be empty or in an unexpected format.".to_string(),
            )
        })?;

    let payload = queryset_part
        .get("payload")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "RealTimeQueryset.json part has no payload.".to_string(),
            )
        })?;

    let decoded_bytes = BASE64.decode(payload).map_err(|e| {
        FabioError::new(
            ErrorCode::ApiError,
            format!("Failed to decode RealTimeQueryset.json base64 payload: {e}"),
        )
    })?;

    let decoded_str = String::from_utf8(decoded_bytes).map_err(|e| {
        FabioError::new(
            ErrorCode::ApiError,
            format!("RealTimeQueryset.json payload is not valid UTF-8: {e}"),
        )
    })?;

    // Handle empty queryset (just "{}")
    let trimmed = decoded_str.trim();
    if trimmed == "{}" || trimmed.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Queryset definition is empty (no saved queries).".to_string(),
            "Use 'fabio kql-queryset update-definition' to save queries into the queryset."
                .to_string(),
        )
        .into());
    }

    serde_json::from_str(&decoded_str).map_err(|e| {
        FabioError::new(
            ErrorCode::ApiError,
            format!("Failed to parse RealTimeQueryset.json content: {e}"),
        )
        .into()
    })
}

/// Select a tab from the queryset by name (title) or zero-based index.
fn select_tab<'a>(tabs: &'a [Value], selector: Option<&str>) -> Result<&'a Value> {
    match selector {
        None => {
            // Default: first tab
            Ok(&tabs[0])
        }
        Some(s) => {
            // Try as zero-based index first
            if let Ok(idx) = s.parse::<usize>() {
                return tabs.get(idx).ok_or_else(|| {
                    let tab_names: Vec<&str> = tabs
                        .iter()
                        .filter_map(|t| t.get("title").and_then(Value::as_str))
                        .collect();
                    FabioError::with_hint(
                        ErrorCode::NotFound,
                        format!(
                            "Tab index {idx} out of range (queryset has {} tabs).",
                            tabs.len()
                        ),
                        format!("Available tabs: {}", tab_names.join(", ")),
                    )
                    .into()
                });
            }

            // Try by title (case-insensitive match)
            let found = tabs.iter().find(|t| {
                t.get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|title| title.eq_ignore_ascii_case(s))
            });

            found.ok_or_else(|| {
                let tab_names: Vec<&str> = tabs
                    .iter()
                    .filter_map(|t| t.get("title").and_then(Value::as_str))
                    .collect();
                FabioError::with_hint(
                    ErrorCode::NotFound,
                    format!("Tab '{s}' not found in queryset."),
                    format!("Available tabs: {}", tab_names.join(", ")),
                )
                .into()
            })
        }
    }
}

/// Resolve the data source from the queryset for a given tab.
fn resolve_data_source<'a>(data_sources: &'a [Value], ds_id: Option<&str>) -> Result<&'a Value> {
    if data_sources.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Queryset has no data sources configured.".to_string(),
            "Update the queryset definition with data source info (clusterUri, databaseName)."
                .to_string(),
        )
        .into());
    }

    ds_id.map_or_else(
        || Ok(&data_sources[0]),
        |id| {
            data_sources
                .iter()
                .find(|ds| ds.get("id").and_then(Value::as_str) == Some(id))
                .ok_or_else(|| {
                    FabioError::with_hint(
                        ErrorCode::NotFound,
                        format!("Data source '{id}' referenced by tab not found in queryset."),
                        "Verify the queryset definition has matching dataSourceId entries."
                            .to_string(),
                    )
                    .into()
                })
        },
    )
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::kql_utils::{parse_kusto_v1_response, parse_kusto_v2_response};

    #[test]
    fn test_queryset_add_tab_from_empty() {
        let def = queryset_add_tab(
            &serde_json::json!({}),
            &FabricDbSource {
                cluster_uri: "https://c.kusto.fabric.microsoft.com",
                db_item_id: "db-item-id",
                db_item_name: "MyDb",
            },
            "ByRegion",
            "Sales | count",
            "new-ds",
            "new-tab",
        );
        let qs = &def["queryset"];
        assert_eq!(qs["version"], "1.0.0");
        let ds = qs["dataSources"].as_array().unwrap();
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0]["id"], "new-ds");
        assert_eq!(ds[0]["type"], "Fabric");
        assert_eq!(ds[0]["databaseItemId"], "db-item-id");
        assert_eq!(ds[0]["databaseItemName"], "MyDb");
        let tabs = qs["tabs"].as_array().unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0]["id"], "new-tab");
        assert_eq!(tabs[0]["title"], "ByRegion");
        assert_eq!(tabs[0]["content"], "Sales | count");
        assert_eq!(tabs[0]["dataSourceId"], "new-ds");
    }

    #[test]
    fn test_queryset_add_tab_reuses_existing_datasource() {
        let existing = serde_json::json!({
            "queryset": {
                "version": "1.0.0",
                "dataSources": [{
                    "id": "existing-ds",
                    "clusterUri": "https://c.kusto.fabric.microsoft.com",
                    "type": "Fabric",
                    "databaseItemId": "db-item-id",
                    "databaseItemName": "MyDb"
                }],
                "tabs": [{
                    "id": "t1", "title": "First", "content": "Sales | take 1", "dataSourceId": "existing-ds"
                }]
            }
        });
        let def = queryset_add_tab(
            &existing,
            &FabricDbSource {
                cluster_uri: "https://c.kusto.fabric.microsoft.com",
                db_item_id: "db-item-id",
                db_item_name: "MyDb",
            },
            "Second",
            "Sales | count",
            "unused-new-ds",
            "new-tab",
        );
        let qs = &def["queryset"];
        // No new data source added — the existing one is reused.
        assert_eq!(qs["dataSources"].as_array().unwrap().len(), 1);
        let tabs = qs["tabs"].as_array().unwrap();
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[1]["dataSourceId"], "existing-ds");
        assert_eq!(tabs[1]["title"], "Second");
    }

    #[test]
    fn test_decode_queryset_definition_success() {
        let payload = r#"{"queryset":{"version":"1.0.0","dataSources":[{"id":"ds1","clusterUri":"https://test.kusto.fabric.microsoft.com","type":"AzureDataExplorer","databaseName":"TestDb"}],"tabs":[{"id":"t1","content":"T | count","title":"CountTab","dataSourceId":"ds1"}]}}"#;
        let encoded = BASE64.encode(payload.as_bytes());
        let def_data = serde_json::json!({
            "definition": {
                "parts": [{
                    "path": "RealTimeQueryset.json",
                    "payload": encoded,
                    "payloadType": "InlineBase64"
                }]
            }
        });

        let result = decode_queryset_definition(&def_data).unwrap();
        assert_eq!(
            result["queryset"]["dataSources"][0]["databaseName"],
            "TestDb"
        );
        assert_eq!(result["queryset"]["tabs"][0]["content"], "T | count");
    }

    #[test]
    fn test_decode_queryset_definition_empty() {
        let encoded = BASE64.encode(b"{}");
        let def_data = serde_json::json!({
            "definition": {
                "parts": [{
                    "path": "RealTimeQueryset.json",
                    "payload": encoded,
                    "payloadType": "InlineBase64"
                }]
            }
        });

        let result = decode_queryset_definition(&def_data);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("empty"));
    }

    #[test]
    fn test_decode_queryset_definition_missing_part() {
        let def_data = serde_json::json!({
            "definition": {
                "parts": [{
                    "path": "other.json",
                    "payload": "e30=",
                    "payloadType": "InlineBase64"
                }]
            }
        });

        let result = decode_queryset_definition(&def_data);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("RealTimeQueryset.json"));
    }

    #[test]
    fn test_select_tab_default_first() {
        let tabs = vec![
            serde_json::json!({"id": "t1", "title": "First", "content": "Q1"}),
            serde_json::json!({"id": "t2", "title": "Second", "content": "Q2"}),
        ];
        let tab = select_tab(&tabs, None).unwrap();
        assert_eq!(tab["title"], "First");
    }

    #[test]
    fn test_select_tab_by_index() {
        let tabs = vec![
            serde_json::json!({"id": "t1", "title": "First", "content": "Q1"}),
            serde_json::json!({"id": "t2", "title": "Second", "content": "Q2"}),
        ];
        let tab = select_tab(&tabs, Some("1")).unwrap();
        assert_eq!(tab["title"], "Second");
    }

    #[test]
    fn test_select_tab_by_name() {
        let tabs = vec![
            serde_json::json!({"id": "t1", "title": "SalesByType", "content": "Q1"}),
            serde_json::json!({"id": "t2", "title": "HighValue", "content": "Q2"}),
        ];
        let tab = select_tab(&tabs, Some("HighValue")).unwrap();
        assert_eq!(tab["id"], "t2");
    }

    #[test]
    fn test_select_tab_by_name_case_insensitive() {
        let tabs = vec![serde_json::json!({"id": "t1", "title": "SalesByType", "content": "Q1"})];
        let tab = select_tab(&tabs, Some("salesbytype")).unwrap();
        assert_eq!(tab["id"], "t1");
    }

    #[test]
    fn test_select_tab_not_found() {
        let tabs = vec![serde_json::json!({"id": "t1", "title": "First", "content": "Q1"})];
        let result = select_tab(&tabs, Some("NonExistent"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("NonExistent"));
    }

    #[test]
    fn test_select_tab_index_out_of_range() {
        let tabs = vec![serde_json::json!({"id": "t1", "title": "First", "content": "Q1"})];
        let result = select_tab(&tabs, Some("5"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("out of range"));
    }

    #[test]
    fn test_resolve_data_source_by_id() {
        let sources = vec![
            serde_json::json!({"id": "ds1", "clusterUri": "https://a.kusto.fabric.microsoft.com", "databaseName": "Db1"}),
            serde_json::json!({"id": "ds2", "clusterUri": "https://b.kusto.fabric.microsoft.com", "databaseName": "Db2"}),
        ];
        let ds = resolve_data_source(&sources, Some("ds2")).unwrap();
        assert_eq!(ds["databaseName"], "Db2");
    }

    #[test]
    fn test_resolve_data_source_default_first() {
        let sources = vec![
            serde_json::json!({"id": "ds1", "clusterUri": "https://a.kusto.fabric.microsoft.com", "databaseName": "Db1"}),
        ];
        let ds = resolve_data_source(&sources, None).unwrap();
        assert_eq!(ds["databaseName"], "Db1");
    }

    #[test]
    fn test_resolve_data_source_not_found() {
        let sources = vec![
            serde_json::json!({"id": "ds1", "clusterUri": "https://a.kusto.fabric.microsoft.com", "databaseName": "Db1"}),
        ];
        let result = resolve_data_source(&sources, Some("nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_data_source_empty() {
        let sources: Vec<Value> = vec![];
        let result = resolve_data_source(&sources, None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no data sources"));
    }

    #[test]
    fn test_parse_kusto_v1_response() {
        let resp = serde_json::json!({
            "Tables": [{
                "TableName": "Table_0",
                "Columns": [
                    {"ColumnName": "Count", "DataType": "Int64"}
                ],
                "Rows": [[42]]
            }]
        });
        let (rows, columns) = parse_kusto_v1_response(&resp).unwrap();
        assert_eq!(columns, vec!["Count"]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["Count"], 42);
    }

    #[test]
    fn test_parse_kusto_v2_response() {
        let frames = serde_json::json!([
            {"FrameType": "DataSetHeader", "IsProgressive": false},
            {
                "FrameType": "DataTable",
                "TableKind": "PrimaryResult",
                "TableName": "PrimaryResult",
                "Columns": [{"ColumnName": "event_type", "ColumnType": "string"}],
                "Rows": [["purchase"], ["refund"]]
            },
            {"FrameType": "DataSetCompletion", "HasErrors": false}
        ]);
        let (rows, columns) = parse_kusto_v2_response(&frames).unwrap();
        assert_eq!(columns, vec!["event_type"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["event_type"], "purchase");
        assert_eq!(rows[1]["event_type"], "refund");
    }

    #[test]
    fn test_parse_kusto_v2_response_with_error() {
        let frames = serde_json::json!([
            {"FrameType": "DataSetHeader", "IsProgressive": false},
            {"FrameType": "DataSetCompletion", "HasErrors": true, "OneApiErrors": "Syntax error"}
        ]);
        let result = parse_kusto_v2_response(&frames);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Syntax error"));
    }
}
