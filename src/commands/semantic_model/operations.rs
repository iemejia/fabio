use std::io;

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

pub(super) async fn bind_connection(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    connection_id: &str,
) -> Result<()> {
    let body = serde_json::json!({ "connectionId": connection_id });

    if output::dry_run_guard(cli, "semantic-model bind-connection", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/semanticModels/{id}/bindConnection"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model bind-connection", "Contributor"))?;

    let obj = serde_json::json!({
        "id": id,
        "connectionId": connection_id,
        "status": "connection_bound"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn unbind_connection(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let body = serde_json::json!({ "connectionId": null });

    if output::dry_run_guard(cli, "semantic-model unbind-connection", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/semanticModels/{id}/bindConnection"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model unbind-connection", "Contributor"))?;

    let obj = serde_json::json!({
        "id": id,
        "status": "connection_unbound"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ── Schema introspection (DAX INFO.VIEW.* — the Analysis Services "Schema
// Rowsets" exposed over the executeQueries endpoint). Returns readable model
// metadata (tables/columns/measures/relationships) without parsing the
// TMDL/TMSL definition.

/// Strip DAX bracket-wrapping from column keys (`[Name]` -> `Name`) for
/// agent-friendly output.
fn strip_bracket_keys(rows: &[Value]) -> Vec<Value> {
    rows.iter()
        .map(|row| {
            row.as_object().map_or_else(
                || row.clone(),
                |obj| {
                    let cleaned: serde_json::Map<String, Value> = obj
                        .iter()
                        .map(|(k, v)| {
                            let key = k
                                .strip_prefix('[')
                                .and_then(|s| s.strip_suffix(']'))
                                .unwrap_or(k)
                                .to_owned();
                            (key, v.clone())
                        })
                        .collect();
                    Value::Object(cleaned)
                },
            )
        })
        .collect()
}

/// Run a DAX query and return the first result table's rows.
async fn run_dax_rows(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    dax: &str,
) -> Result<Vec<Value>> {
    let body = serde_json::json!({
        "queries": [{"query": dax}],
        "serializerSettings": {"includeNulls": true}
    });
    let data = client
        .post_powerbi(
            &format!("/groups/{workspace}/datasets/{id}/executeQueries"),
            &body,
        )
        .await
        .map_err(|e| {
            enrich_dax_error(enrich_forbidden(e, "semantic-model schema query", "Viewer"))
        })?;

    let rows = data
        .get("results")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|t| t.get("tables"))
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|t| t.get("rows"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(rows)
}

/// Render the result of an `INFO.VIEW.<function>()` introspection query.
async fn info_view(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    function: &str,
) -> Result<()> {
    let dax = format!("EVALUATE INFO.VIEW.{function}()");
    let rows = run_dax_rows(client, workspace, id, &dax).await?;
    let items = strip_bracket_keys(&rows);
    let columns: Vec<&str> = items
        .first()
        .and_then(Value::as_object)
        .map_or_else(Vec::new, |first| first.keys().map(String::as_str).collect());
    output::render_list_with_token(
        cli,
        &items,
        &columns,
        &columns,
        columns.first().copied().unwrap_or("Name"),
        None,
    );
    Ok(())
}

pub(super) async fn list_tables(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    info_view(cli, client, workspace, id, "TABLES").await
}

pub(super) async fn list_columns(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    info_view(cli, client, workspace, id, "COLUMNS").await
}

pub(super) async fn list_measures(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    info_view(cli, client, workspace, id, "MEASURES").await
}

pub(super) async fn list_relationships(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    info_view(cli, client, workspace, id, "RELATIONSHIPS").await
}

pub(super) async fn query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    dax: Option<&str>,
    file: Option<&str>,
) -> Result<()> {
    // Resolve DAX query from --dax flag, --file flag, or stdin
    let dax_query = if let Some(d) = dax {
        d.to_string()
    } else if let Some(f) = file {
        std::fs::read_to_string(f).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read DAX file '{f}': {e}"),
                "Provide a valid file path containing a DAX query.".to_string(),
            )
        })?
    } else {
        // Read from stdin
        let buf = io::read_to_string(io::stdin()).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read DAX from stdin: {e}"),
                "Provide DAX via --dax flag, --file flag, or pipe to stdin.".to_string(),
            )
        })?;
        if buf.trim().is_empty() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "No DAX query provided".to_string(),
                "Usage: fabio semantic-model query --workspace <WS> --id <ID> --dax \"EVALUATE MyTable\"\n\
                 Or pipe: echo 'EVALUATE MyTable' | fabio semantic-model query --workspace <WS> --id <ID>"
                    .to_string(),
            )
            .into());
        }
        buf
    };

    let body = serde_json::json!({
        "queries": [{"query": dax_query.trim()}],
        "serializerSettings": {"includeNulls": true}
    });

    let data = client
        .post_powerbi(
            &format!("/groups/{workspace}/datasets/{id}/executeQueries"),
            &body,
        )
        .await
        .map_err(|e| enrich_dax_error(enrich_forbidden(e, "semantic-model query", "Viewer")))?;

    // Extract rows from the response: results[0].tables[0].rows
    let rows = data
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())
        .and_then(|t| t.get("tables"))
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.first())
        .and_then(|t| t.get("rows"))
        .and_then(Value::as_array);

    if let Some(rows) = rows {
        // Build column names from the first row's keys
        let columns: Vec<&str> = rows
            .first()
            .and_then(Value::as_object)
            .map_or_else(Vec::new, |first| first.keys().map(String::as_str).collect());

        let items: Vec<Value> = rows.clone();
        output::render_list_with_token(
            cli,
            &items,
            &columns,
            &columns,
            columns.first().copied().unwrap_or("value"),
            None,
        );
    } else {
        // No rows — might be an error or empty result
        output::render_object(cli, &data, "results");
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn refresh(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    refresh_type: &str,
    objects: Option<&str>,
    commit_mode: Option<&str>,
    max_parallelism: Option<u32>,
    retry_count: Option<u32>,
) -> Result<()> {
    const VALID_TYPES: &[&str] = &[
        "Full",
        "Automatic",
        "ClearValues",
        "Calculate",
        "DataOnly",
        "Defragment",
    ];

    // Case-insensitive normalization
    let refresh_type = VALID_TYPES
        .iter()
        .find(|v| v.eq_ignore_ascii_case(refresh_type))
        .copied()
        .unwrap_or(refresh_type);

    if !VALID_TYPES.contains(&refresh_type) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid refresh type: '{refresh_type}'"),
            format!(
                "--type must be one of: {} (got: '{refresh_type}')",
                VALID_TYPES.join(", ")
            ),
        )
        .into());
    }

    // Parse + validate --objects (enhanced refresh: specific tables/partitions).
    let parsed_objects = match objects {
        Some(raw) => Some(parse_refresh_objects(raw)?),
        None => None,
    };

    // Validate --commit-mode.
    let commit_mode = match commit_mode {
        Some(m) => Some(normalize_commit_mode(m)?),
        None => None,
    };

    let body = build_refresh_body(
        refresh_type,
        parsed_objects,
        commit_mode,
        max_parallelism,
        retry_count,
    );

    if output::dry_run_guard(cli, "semantic-model refresh", &body) {
        return Ok(());
    }

    client
        .post_powerbi(
            &format!("/groups/{workspace}/datasets/{id}/refreshes"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model refresh", "Contributor"))?;

    let obj = serde_json::json!({
        "id": id,
        "type": refresh_type,
        "status": "refresh_triggered"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Parse the `--objects` JSON: an array of `{table, partition?}` entries for
/// granular (enhanced) refresh. Each entry MUST have a `table`.
fn parse_refresh_objects(raw: &str) -> Result<Value> {
    let val: Value = serde_json::from_str(raw).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("--objects is not valid JSON: {e}"),
            r#"Provide a JSON array, e.g. --objects '[{"table":"Sales"},{"table":"Sales","partition":"2024"}]'"#.to_string(),
        )
    })?;
    let arr = val.as_array().ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--objects must be a JSON array".to_string(),
            r#"e.g. --objects '[{"table":"Sales"}]'"#.to_string(),
        )
    })?;
    for (i, entry) in arr.iter().enumerate() {
        let obj = entry.as_object().ok_or_else(|| {
            FabioError::new(
                ErrorCode::InvalidInput,
                format!("--objects[{i}] must be an object with a 'table' field"),
            )
        })?;
        if obj.get("table").and_then(Value::as_str).is_none() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("--objects[{i}] is missing a string 'table' field"),
                r#"Each entry needs a table, e.g. {"table":"Sales"} or {"table":"Sales","partition":"2024"}"#.to_string(),
            )
            .into());
        }
    }
    Ok(val)
}

/// Normalize/validate the enhanced-refresh commit mode.
fn normalize_commit_mode(mode: &str) -> Result<&'static str> {
    match mode.to_ascii_lowercase().as_str() {
        "transactional" => Ok("transactional"),
        "partialbatch" => Ok("partialBatch"),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --commit-mode: '{mode}'"),
            "--commit-mode must be one of: transactional, partialBatch".to_string(),
        )
        .into()),
    }
}

/// Build the refresh request body. With only `type` it is a basic refresh; any
/// of `objects`/`commitMode`/`maxParallelism`/`retryCount` makes it an
/// enhanced refresh (Power BI enhanced-refresh API — the TMSL `refresh` command's
/// granular options over REST).
fn build_refresh_body(
    refresh_type: &str,
    objects: Option<Value>,
    commit_mode: Option<&str>,
    max_parallelism: Option<u32>,
    retry_count: Option<u32>,
) -> Value {
    let mut body = serde_json::json!({ "type": refresh_type });
    let map = body.as_object_mut().expect("object");
    if let Some(objs) = objects {
        map.insert("objects".to_owned(), objs);
    }
    if let Some(cm) = commit_mode {
        map.insert("commitMode".to_owned(), Value::from(cm));
    }
    if let Some(mp) = max_parallelism {
        map.insert("maxParallelism".to_owned(), Value::from(mp));
    }
    if let Some(rc) = retry_count {
        map.insert("retryCount".to_owned(), Value::from(rc));
    }
    body
}

/// Get the execution details of a specific (enhanced) refresh by its request id
/// (from `refresh-status`). Returns object-level status, commitMode, attempts, etc.
pub(super) async fn refresh_details(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    refresh_id: &str,
) -> Result<()> {
    let data = client
        .get_powerbi(&format!(
            "/groups/{workspace}/datasets/{id}/refreshes/{refresh_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model refresh-details", "Viewer"))?;
    output::render_object(cli, &data, "status");
    Ok(())
}

/// Cancel an in-progress enhanced refresh by its request id.
pub(super) async fn cancel_refresh(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    refresh_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "semantic-model cancel-refresh",
        &serde_json::json!({ "id": id, "refreshId": refresh_id }),
    ) {
        return Ok(());
    }
    client
        .delete_powerbi(&format!(
            "/groups/{workspace}/datasets/{id}/refreshes/{refresh_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model cancel-refresh", "Contributor"))?;
    let obj = serde_json::json!({
        "id": id,
        "refreshId": refresh_id,
        "status": "cancellation_requested"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn takeover(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let body = serde_json::json!({});

    if output::dry_run_guard(cli, "semantic-model takeover", &body) {
        return Ok(());
    }

    client
        .post_powerbi(
            &format!("/groups/{workspace}/datasets/{id}/Default.TakeOver"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model takeover", "Admin"))?;

    let obj = serde_json::json!({
        "id": id,
        "status": "takeover_complete",
        "note": "Model is now service-managed (editable in portal)"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Error Enrichment ────────────────────────────────────────────────────────

/// Enrich DAX query errors with actionable hints.
fn enrich_dax_error(err: anyhow::Error) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };

    let msg = &fabio_err.message;
    let msg_lower = msg.to_lowercase();

    // Pattern: model not found
    if msg_lower.contains("dataset not found") || msg_lower.contains("datasetnotfound") {
        return FabioError::with_hint(
            ErrorCode::NotFound,
            msg.clone(),
            "The semantic model ID was not found in this workspace. \
             Use: fabio semantic-model list --workspace <WS> to find available models."
                .to_string(),
        )
        .into();
    }

    // Pattern: model not refreshed / framing required
    if msg_lower.contains("3242524690") || msg_lower.contains("not framed") {
        return FabioError::with_hint(
            fabio_err.code,
            msg.clone(),
            "Direct Lake model needs framing before queries work. \
             Run: fabio semantic-model refresh --workspace <WS> --id <ID> --type Full"
                .to_string(),
        )
        .into();
    }

    // Pattern: DAX syntax error
    if msg_lower.contains("dax") && msg_lower.contains("syntax") {
        return FabioError::with_hint(
            fabio_err.code,
            msg.clone(),
            "DAX query has a syntax error. Ensure EVALUATE is followed by a valid table expression. \
             Example: EVALUATE SUMMARIZE(sales_summary, sales_summary[country], \"Revenue\", SUM(sales_summary[total]))"
                .to_string(),
        )
        .into();
    }

    err
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_bracket_keys_unwraps_dax_columns() {
        let rows = vec![serde_json::json!({
            "[Name]": "Sales",
            "[StorageMode]": "Direct Lake",
            "[IsHidden]": false
        })];
        let out = strip_bracket_keys(&rows);
        let obj = out[0].as_object().unwrap();
        assert_eq!(obj.get("Name").and_then(Value::as_str), Some("Sales"));
        assert_eq!(
            obj.get("StorageMode").and_then(Value::as_str),
            Some("Direct Lake")
        );
        assert_eq!(obj.get("IsHidden").and_then(Value::as_bool), Some(false));
        // No bracketed keys remain.
        assert!(obj.keys().all(|k| !k.starts_with('[')));
    }

    #[test]
    fn strip_bracket_keys_leaves_unbracketed_and_non_objects_untouched() {
        let rows = vec![
            serde_json::json!({"Name": "x", "count": 3}),
            serde_json::json!("scalar"),
        ];
        let out = strip_bracket_keys(&rows);
        assert_eq!(out[0]["Name"], "x");
        assert_eq!(out[0]["count"], 3);
        assert_eq!(out[1], serde_json::json!("scalar"));
    }

    #[test]
    fn build_refresh_body_basic_is_type_only() {
        let body = build_refresh_body("Full", None, None, None, None);
        assert_eq!(body, serde_json::json!({ "type": "Full" }));
    }

    #[test]
    fn build_refresh_body_enhanced_includes_all_fields() {
        let objs = serde_json::json!([{"table": "Sales"}, {"table": "Sales", "partition": "2024"}]);
        let body = build_refresh_body(
            "Full",
            Some(objs.clone()),
            Some("partialBatch"),
            Some(4),
            Some(2),
        );
        assert_eq!(body["type"], "Full");
        assert_eq!(body["objects"], objs);
        assert_eq!(body["commitMode"], "partialBatch");
        assert_eq!(body["maxParallelism"], 4);
        assert_eq!(body["retryCount"], 2);
    }

    #[test]
    fn parse_refresh_objects_validates_shape() {
        // Valid.
        let ok = parse_refresh_objects(r#"[{"table":"Sales"},{"table":"Sales","partition":"Q1"}]"#);
        assert!(ok.is_ok());
        // Not an array.
        assert!(parse_refresh_objects(r#"{"table":"Sales"}"#).is_err());
        // Missing table.
        assert!(parse_refresh_objects(r#"[{"partition":"Q1"}]"#).is_err());
        // Invalid JSON.
        assert!(parse_refresh_objects("not json").is_err());
    }

    #[test]
    fn normalize_commit_mode_accepts_valid_and_rejects_invalid() {
        assert_eq!(
            normalize_commit_mode("transactional").unwrap(),
            "transactional"
        );
        assert_eq!(
            normalize_commit_mode("PartialBatch").unwrap(),
            "partialBatch"
        );
        assert!(normalize_commit_mode("bogus").is_err());
    }

    #[test]
    fn test_enrich_dax_error_dataset_not_found() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::NotFound,
            "Dataset not found in workspace".to_string(),
        )
        .into();

        let enriched = enrich_dax_error(err);
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::NotFound);
        assert!(
            fabio_err
                .hint
                .as_ref()
                .unwrap()
                .contains("semantic-model list")
        );
    }

    #[test]
    fn test_enrich_dax_error_not_framed() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::ApiError,
            "Query failed with error code 3242524690".to_string(),
        )
        .into();

        let enriched = enrich_dax_error(err);
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert!(fabio_err.hint.as_ref().unwrap().contains("framing"));
    }

    #[test]
    fn test_enrich_dax_error_syntax() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::ApiError,
            "DAX syntax error near 'EVALUAT'".to_string(),
        )
        .into();

        let enriched = enrich_dax_error(err);
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert!(fabio_err.hint.as_ref().unwrap().contains("EVALUATE"));
    }

    #[test]
    fn test_enrich_dax_error_passthrough() {
        let err: anyhow::Error =
            FabioError::new(ErrorCode::ApiError, "Some unknown error".to_string()).into();

        let enriched = enrich_dax_error(err);
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        // No hint added — returned as-is
        assert!(fabio_err.hint.is_none());
    }
}
