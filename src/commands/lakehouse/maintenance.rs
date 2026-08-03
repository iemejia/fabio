use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

use super::{iceberg_warehouse, read_json_body};

// ─── Materialized Lake Views ─────────────────────────────────────────────────

pub(super) async fn refresh_materialized_views(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let body = serde_json::json!({});

    if output::dry_run_guard(cli, "lakehouse refresh-materialized-views", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/lakehouses/{id}/jobs/refreshMaterializedLakeViews/instances"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse refresh-materialized-views", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "refresh_triggered" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

pub(super) async fn list_materialized_views_schedules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!(
                "/workspaces/{workspace}/lakehouses/{id}/jobs/refreshMaterializedLakeViews/schedules"
            ),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "lakehouse list-materialized-views-schedules", "Viewer")
        })?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "enabled", "createdDateTime"],
        &["ID", "ENABLED", "CREATED"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn get_materialized_views_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    schedule_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/lakehouses/{id}/jobs/refreshMaterializedLakeViews/schedules/{schedule_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse get-materialized-views-schedule", "Viewer"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn create_materialized_views_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "create-materialized-views-schedule")?;

    if output::dry_run_guard(cli, "lakehouse create-materialized-views-schedule", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/lakehouses/{id}/jobs/refreshMaterializedLakeViews/schedules"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse create-materialized-views-schedule", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update_materialized_views_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    schedule_id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "update-materialized-views-schedule")?;

    if output::dry_run_guard(cli, "lakehouse update-materialized-views-schedule", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/lakehouses/{id}/jobs/refreshMaterializedLakeViews/schedules/{schedule_id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse update-materialized-views-schedule", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete_materialized_views_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    schedule_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "lakehouse delete-materialized-views-schedule",
        &serde_json::json!({ "workspace": workspace, "id": id, "scheduleId": schedule_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/lakehouses/{id}/jobs/refreshMaterializedLakeViews/schedules/{schedule_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse delete-materialized-views-schedule", "Contributor"))?;

    let obj = serde_json::json!({ "scheduleId": schedule_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Table Maintenance ───────────────────────────────────────────────────────

pub(super) async fn run_table_maintenance(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = match (file, content) {
        (Some(f), _) => {
            let text = std::fs::read_to_string(f)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{f}': {e}"))?;
            serde_json::from_str(&text)?
        }
        (_, Some(c)) => serde_json::from_str(c)?,
        (None, None) => serde_json::json!({}),
    };

    if output::dry_run_guard(cli, "lakehouse run-table-maintenance", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/lakehouses/{id}/jobs/tableMaintenance/instances"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse run-table-maintenance", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "maintenance_triggered" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn optimize_table(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    schema: Option<&str>,
    vorder: bool,
    zorder: Option<&[String]>,
) -> Result<()> {
    let mut optimize_settings = serde_json::json!({ "vOrder": vorder });
    if let Some(cols) = zorder
        && !cols.is_empty()
    {
        optimize_settings["zOrderBy"] = serde_json::json!(cols);
    }

    let mut execution_data = serde_json::json!({
        "tableName": table,
        "optimizeSettings": optimize_settings,
    });
    if let Some(s) = schema {
        execution_data["schemaName"] = serde_json::json!(s);
    }

    let body = serde_json::json!({ "executionData": execution_data });

    if output::dry_run_guard(cli, "lakehouse optimize-table", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/jobs/instances?jobType=TableMaintenance"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse optimize-table", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({
            "table": table,
            "status": "optimize_triggered",
            "vOrder": vorder,
            "zOrderBy": zorder.unwrap_or(&[]),
        });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

pub(super) async fn vacuum_table(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
    schema: Option<&str>,
    retain_hours: u64,
) -> Result<()> {
    // Format retention period as D:HH:MM:SS
    let days = retain_hours / 24;
    let hours = retain_hours % 24;
    let retention_period = format!("{days}:{hours:02}:00:00");

    let mut execution_data = serde_json::json!({
        "tableName": table,
        "vacuumSettings": {
            "retentionPeriod": retention_period,
        },
    });
    if let Some(s) = schema {
        execution_data["schemaName"] = serde_json::json!(s);
    }

    let body = serde_json::json!({ "executionData": execution_data });

    if output::dry_run_guard(cli, "lakehouse vacuum-table", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/jobs/instances?jobType=TableMaintenance"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse vacuum-table", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({
            "table": table,
            "status": "vacuum_triggered",
            "retentionPeriod": retention_period,
        });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Table Schema ────────────────────────────────────────────────────────────

pub(super) async fn table_schema(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
) -> Result<()> {
    // Try the Iceberg REST Catalog first (more reliable, no checkpoint issues)
    if let Ok(schema) = table_schema_via_iceberg(client, workspace, id, table).await {
        let result = serde_json::json!({
            "table": table,
            "schema_type": "struct",
            "fields": schema,
        });
        output::render_object(cli, &result, "table");
        return Ok(());
    }

    // Fallback: Parse Delta log files from OneLake DFS
    table_schema_via_delta_log(cli, client, workspace, id, table).await
}

/// Try to get table schema via the Iceberg REST Catalog API.
/// Returns the fields array on success, or an error if the API is unavailable.
async fn table_schema_via_iceberg(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
) -> Result<Vec<Value>> {
    let wh = iceberg_warehouse(workspace, id);
    let encoded_table = urlencoding::encode(table);
    let path = format!("iceberg/v1/{wh}/namespaces/dbo/tables/{encoded_table}");
    let result = client.get_onelake_table_api(&path).await?;

    let metadata = result
        .get("metadata")
        .ok_or_else(|| FabioError::new(ErrorCode::ApiError, "No metadata in response"))?;
    let current_schema_id = metadata
        .get("current-schema-id")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let schemas = metadata
        .get("schemas")
        .and_then(|s| s.as_array())
        .ok_or_else(|| FabioError::new(ErrorCode::ApiError, "No schemas in metadata"))?;

    let active_schema = schemas
        .iter()
        .find(|s| s.get("schema-id").and_then(Value::as_u64).unwrap_or(0) == current_schema_id)
        .or_else(|| schemas.last())
        .ok_or_else(|| FabioError::new(ErrorCode::ApiError, "No schema found"))?;

    let fields = active_schema
        .get("fields")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // Convert Iceberg field format to match existing Delta log output format
    let converted: Vec<Value> = fields
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.get("name").unwrap_or(&Value::Null),
                "type": f.get("type").unwrap_or(&Value::Null),
                "nullable": !f.get("required").and_then(Value::as_bool).unwrap_or(false),
                "metadata": {}
            })
        })
        .collect();

    Ok(converted)
}

/// Fallback: parse schema from Delta log commit files.
async fn table_schema_via_delta_log(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    table: &str,
) -> Result<()> {
    // List from root (no directory param) to avoid the DFS virtual lakehouse-in-lakehouse
    // view that doubles top-level dirs when a directory param is specified.
    let files = client
        .list_onelake_files(workspace, id, None)
        .await
        .map_err(|e| {
            let msg = format!("Failed to read Delta log for table '{table}': {e}");
            FabioError::with_hint(ErrorCode::NotFound, msg, format!("Verify the table exists with: fabio lakehouse list-tables --workspace {workspace} --id {id}"))
        })?;

    // Filter to .json commit files under {item_id}/Tables/{table}/_delta_log/
    let delta_log_prefix = format!("{id}/Tables/{table}/_delta_log/");
    let mut json_files: Vec<&str> = files
        .iter()
        .filter_map(|f| f["name"].as_str())
        .filter(|name| {
            // Must be under the delta_log directory
            let Some(suffix) = name.strip_prefix(delta_log_prefix.as_str()) else {
                return false;
            };
            // Must be a direct child (no further path separators) and a .json file
            !suffix.contains('/')
                && std::path::Path::new(suffix)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    json_files.sort_unstable_by(|a, b| b.cmp(a));

    if json_files.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::NotFound,
            format!("No schema metadata found in Delta log for table '{table}'"),
            format!("The table may have no commits yet or uses checkpoint-only format. Verify the table exists with: fabio lakehouse list-tables --workspace {workspace} --id {id}"),
        )
        .into());
    }

    // Iterate from newest commit to oldest, looking for metaData with schemaString
    for file_path in &json_files {
        // Strip the item-id prefix to get the path for download
        // e.g., "{item_id}/Tables/mytable/_delta_log/00000000000000000000.json"
        //     → "Tables/mytable/_delta_log/00000000000000000000.json"
        let download_path = file_path
            .strip_prefix(&format!("{id}/"))
            .unwrap_or(file_path)
            .to_string();

        let Ok(bytes) = client
            .download_onelake_file(workspace, id, &download_path)
            .await
        else {
            continue; // Skip files we can't read
        };

        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue; // Skip non-UTF-8 files (parquet checkpoints)
        };

        // Delta commit files are NDJSON — one JSON object per line
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(obj) = serde_json::from_str::<Value>(line) else {
                continue;
            };

            if let Some(metadata) = obj.get("metaData")
                && let Some(schema_str) = metadata.get("schemaString").and_then(Value::as_str)
            {
                // Parse the schema string (which is itself JSON)
                let schema: Value = serde_json::from_str(schema_str).map_err(|e| {
                        FabioError::with_hint(
                            ErrorCode::ApiError,
                            format!("Failed to parse schema from Delta log: {e}"),
                            "The Delta log schema metadata is malformed. Try querying the table directly with: fabio lakehouse query",
                        )
                    })?;

                // Extract fields array and build output
                let fields = schema
                    .get("fields")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();

                let result = serde_json::json!({
                    "table": table,
                    "schema_type": schema.get("type").unwrap_or(&Value::Null),
                    "fields": fields,
                });
                output::render_object(cli, &result, "table");
                return Ok(());
            }
        }
    }

    Err(FabioError::with_hint(
        ErrorCode::NotFound,
        format!("No schema metadata found in Delta log for table '{table}'"),
        "Schema metadata may only exist in Parquet checkpoint files (tables with 10+ commits). Try querying the table directly with: fabio lakehouse query",
    )
    .into())
}
