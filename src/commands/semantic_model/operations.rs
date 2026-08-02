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

/// Fetch an `INFO.VIEW.<function>()` introspection result as cleaned rows
/// (DAX brackets stripped). Public within the crate so other command modules
/// (e.g. `ontology generate`) can reuse the semantic-model schema.
pub async fn fetch_info_view(
    client: &FabricClient,
    workspace: &str,
    id: &str,
    function: &str,
) -> Result<Vec<Value>> {
    let dax = format!("EVALUATE INFO.VIEW.{function}()");
    let rows = run_dax_rows(client, workspace, id, &dax).await?;
    Ok(strip_bracket_keys(&rows))
}

/// Render the result of an `INFO.VIEW.<function>()` introspection query.
async fn info_view(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    function: &str,
) -> Result<()> {
    let items = fetch_info_view(client, workspace, id, function).await?;
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

// ── Scheduled refresh (Power BI refreshSchedule) ──────────────────────────────
// Configure the automatic refresh schedule for an import/Direct Lake model.
// (DirectQuery/Live models use directQueryRefreshSchedule — reach it via
// `fabio rest call --api powerbi`.)

/// Valid weekday names for a refresh schedule.
const SCHEDULE_DAYS: &[&str] = &[
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Valid notify options.
const NOTIFY_OPTIONS: &[&str] = &["NoNotification", "MailOnFailure", "MailOnCompletion"];

/// Validate a refresh time is on the full or half hour (`HH:00` or `HH:30`).
fn validate_schedule_time(t: &str) -> Result<()> {
    let ok = t
        .split_once(':')
        .and_then(|(h, m)| {
            let hour: u8 = h.parse().ok()?;
            (hour <= 23 && (m == "00" || m == "30")).then_some(())
        })
        .is_some();
    if ok {
        Ok(())
    } else {
        Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid refresh time '{t}'"),
            "Times must be on the full or half hour (HH:00 or HH:30), e.g. 07:00, 13:30"
                .to_string(),
        )
        .into())
    }
}

/// Normalize a comma-separated day list to canonical weekday names.
fn normalize_days(raw: &str) -> Result<Vec<String>> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|d| {
            SCHEDULE_DAYS
                .iter()
                .find(|v| v.eq_ignore_ascii_case(d))
                .map(|v| (*v).to_owned())
                .ok_or_else(|| {
                    FabioError::with_hint(
                        ErrorCode::InvalidInput,
                        format!("Invalid day '{d}'"),
                        format!(
                            "--days must be day names from: {}",
                            SCHEDULE_DAYS.join(", ")
                        ),
                    )
                    .into()
                })
        })
        .collect()
}

/// Normalize/validate the notify option.
fn normalize_notify_option(raw: &str) -> Result<&'static str> {
    NOTIFY_OPTIONS
        .iter()
        .find(|v| v.eq_ignore_ascii_case(raw))
        .copied()
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --notify-option '{raw}'"),
                format!(
                    "--notify-option must be one of: {}",
                    NOTIFY_OPTIONS.join(", ")
                ),
            )
            .into()
        })
}

/// Build the `refreshSchedule` PATCH body from the provided fields.
///
/// The API rejects modifying other settings while disabling, so when `enabled`
/// is `Some(false)` the body contains ONLY `enabled: false`.
fn build_schedule_body(
    enabled: Option<bool>,
    days: Option<Vec<String>>,
    times: Option<Vec<String>>,
    time_zone: Option<&str>,
    notify_option: Option<&str>,
) -> Value {
    let mut value = serde_json::Map::new();
    if enabled == Some(false) {
        value.insert("enabled".to_owned(), Value::Bool(false));
        return serde_json::json!({ "value": value });
    }
    if let Some(e) = enabled {
        value.insert("enabled".to_owned(), Value::Bool(e));
    }
    if let Some(d) = days {
        value.insert("days".to_owned(), Value::from(d));
    }
    if let Some(t) = times {
        value.insert("times".to_owned(), Value::from(t));
    }
    if let Some(tz) = time_zone {
        value.insert("localTimeZoneId".to_owned(), Value::from(tz));
    }
    if let Some(n) = notify_option {
        value.insert("notifyOption".to_owned(), Value::from(n));
    }
    serde_json::json!({ "value": value })
}

pub(super) async fn get_refresh_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get_powerbi(&format!(
            "/groups/{workspace}/datasets/{id}/refreshSchedule"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model get-refresh-schedule", "Viewer"))?;
    output::render_object(cli, &data, "enabled");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update_refresh_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    enabled: Option<bool>,
    days: Option<&str>,
    times: Option<&str>,
    time_zone: Option<&str>,
    notify_option: Option<&str>,
) -> Result<()> {
    if enabled.is_none()
        && days.is_none()
        && times.is_none()
        && time_zone.is_none()
        && notify_option.is_none()
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "No schedule fields provided".to_string(),
            "Pass at least one of --enabled, --days, --times, --local-time-zone-id, --notify-option"
                .to_string(),
        )
        .into());
    }

    // The API rejects changing other settings while disabling.
    if enabled == Some(false)
        && (days.is_some() || times.is_some() || time_zone.is_some() || notify_option.is_some())
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Cannot change other settings while disabling the schedule".to_string(),
            "Disable on its own: fabio semantic-model update-refresh-schedule --workspace <WS> --id <ID> --enabled false"
                .to_string(),
        )
        .into());
    }

    let days = match days {
        Some(d) => Some(normalize_days(d)?),
        None => None,
    };
    let times = match times {
        Some(t) => {
            let parsed: Vec<String> = t
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            for time in &parsed {
                validate_schedule_time(time)?;
            }
            Some(parsed)
        }
        None => None,
    };
    let notify_option = match notify_option {
        Some(n) => Some(normalize_notify_option(n)?),
        None => None,
    };

    let body = build_schedule_body(enabled, days, times, time_zone, notify_option);

    if output::dry_run_guard(cli, "semantic-model update-refresh-schedule", &body) {
        return Ok(());
    }

    client
        .patch_powerbi(
            &format!("/groups/{workspace}/datasets/{id}/refreshSchedule"),
            &body,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "semantic-model update-refresh-schedule", "Contributor")
        })?;

    let obj = serde_json::json!({ "id": id, "status": "schedule_updated" });
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
    fn validate_schedule_time_enforces_half_hour() {
        for ok in ["00:00", "07:00", "13:30", "23:30"] {
            assert!(validate_schedule_time(ok).is_ok(), "{ok} should be valid");
        }
        for bad in ["07:15", "24:00", "07:45", "noon"] {
            assert!(
                validate_schedule_time(bad).is_err(),
                "{bad} should be invalid"
            );
        }
    }

    #[test]
    fn normalize_days_canonicalizes_and_rejects() {
        assert_eq!(
            normalize_days("monday, THURSDAY").unwrap(),
            vec!["Monday".to_string(), "Thursday".to_string()]
        );
        assert!(normalize_days("Funday").is_err());
    }

    #[test]
    fn normalize_notify_option_valid_and_invalid() {
        assert_eq!(
            normalize_notify_option("mailonfailure").unwrap(),
            "MailOnFailure"
        );
        assert!(normalize_notify_option("Nope").is_err());
    }

    #[test]
    fn build_schedule_body_disable_is_enabled_only() {
        // Disabling must not carry other settings.
        let body = build_schedule_body(
            Some(false),
            Some(vec!["Monday".to_owned()]),
            Some(vec!["07:00".to_owned()]),
            Some("UTC"),
            Some("MailOnFailure"),
        );
        assert_eq!(body, serde_json::json!({ "value": { "enabled": false } }));
    }

    #[test]
    fn build_schedule_body_only_includes_provided_fields() {
        let body = build_schedule_body(
            Some(true),
            Some(vec!["Tuesday".to_owned()]),
            Some(vec!["06:00".to_owned(), "18:30".to_owned()]),
            None,
            None,
        );
        let v = &body["value"];
        assert_eq!(v["enabled"], true);
        assert_eq!(v["days"], serde_json::json!(["Tuesday"]));
        assert_eq!(v["times"], serde_json::json!(["06:00", "18:30"]));
        assert!(v.get("localTimeZoneId").is_none());
        assert!(v.get("notifyOption").is_none());
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
