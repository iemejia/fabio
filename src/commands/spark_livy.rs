//! Interactive Fabric **Livy API** — create sessions and run Spark code.
//!
//! This is the INTERACTIVE Livy API, scoped to a lakehouse
//! (`/workspaces/{ws}/lakehouses/{lh}/livyApi/versions/{v}/sessions`). It is
//! DISTINCT from the read-only MONITORING API used by `spark list-livy-sessions`
//! / `spark get-livy-session` (`/workspaces/{ws}/spark/livySessions`): the
//! monitoring API only lists what is running, whereas this API CREATES sessions
//! and RUNS statements (Spark/PySpark/Spark-SQL/R).
//!
//! Every wait is bounded by `--timeout`, so a session or statement can never hang
//! the caller. `spark run` is the agent-native one-shot primitive: create a
//! session → wait for `idle` → run one statement → return output → delete the
//! session (the session is always cleaned up, even on error).
//!
//! API contract verified live (2023-12-01):
//! - create: `POST …/sessions {name?, conf?}` → `{id, artifactId}`
//! - state:  `GET …/sessions/{id}` → `{state}` (`not_started`/`starting`/`idle`/`busy`/`dead`)
//! - run:    `POST …/sessions/{id}/statements {code, kind}` → `{id, state}`, then
//!   poll `GET …/statements/{sid}` → `{state, output:{status, data:{"text/plain"}}}`
//! - delete: `DELETE …/sessions/{id}`

use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// The Fabric interactive Livy API version segment (verified live).
const LIVY_VERSION: &str = "2023-12-01";
/// Default seconds to wait for session-idle / statement-available.
const DEFAULT_TIMEOUT_SECS: u64 = 300;
/// Seconds between state polls.
const POLL_INTERVAL_SECS: u64 = 5;

fn sessions_base(ws: &str, lh: &str) -> String {
    format!("/workspaces/{ws}/lakehouses/{lh}/livyApi/versions/{LIVY_VERSION}/sessions")
}

/// Map the user-facing `--language` to the Livy statement `kind`.
/// `scala` runs as Livy `spark`; `r` runs as `sparkr`. Pure/testable.
fn statement_kind(language: &str) -> &'static str {
    match language {
        "scala" => "spark",
        "sql" => "sql",
        "r" => "sparkr",
        _ => "pyspark",
    }
}

/// Resolve the `--code` value (inline, `@file`, or `@-`/piped stdin) into text.
fn resolve_code_input(code: Option<&str>) -> Result<String> {
    match code {
        // `@-` and omitted (piped) both read stdin.
        Some("@-") | None => read_stdin_code(),
        Some(s) if s.starts_with('@') => {
            let path = &s[1..];
            std::fs::read_to_string(path).map_err(|e| {
                FabioError::not_found(format!("Code file not found: {path}: {e}")).into()
            })
        }
        Some(s) => Ok(s.to_string()),
    }
}

fn read_stdin_code() -> Result<String> {
    let buf = std::io::read_to_string(std::io::stdin()).map_err(|e| {
        FabioError::new(
            ErrorCode::ApiError,
            format!("Failed to read code from stdin: {e}"),
        )
    })?;
    if buf.trim().is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "No code provided.".to_string(),
            "Use --code \"<code>\", --code @file, or pipe code via stdin.".to_string(),
        )
        .into());
    }
    Ok(buf)
}

/// Parse the optional `--conf` JSON object.
fn parse_conf(conf: Option<&str>) -> Result<Option<Value>> {
    let Some(conf) = conf else { return Ok(None) };
    let value: Value = serde_json::from_str(conf).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --conf JSON: {e}"),
            "Provide a JSON object of Spark settings, e.g. --conf '{\"spark.executor.cores\":\"4\"}'."
                .to_string(),
        )
    })?;
    if !value.is_object() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--conf must be a JSON object".to_string(),
            "Example: --conf '{\"spark.executor.memory\":\"8g\"}'".to_string(),
        )
        .into());
    }
    Ok(Some(value))
}

/// A session state that will never become idle (terminal failure).
fn is_dead_state(state: &str) -> bool {
    matches!(
        state,
        "dead" | "error" | "killed" | "shutting_down" | "success"
    )
}

async fn get_session_state(client: &FabricClient, ws: &str, lh: &str, sid: &str) -> Result<String> {
    let data = client
        .get(&format!("{}/{sid}", sessions_base(ws, lh)))
        .await?;
    Ok(data
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string())
}

/// Poll a session until it reaches `idle`, bounded by `timeout_secs`. Returns the
/// final state (`idle`) or a teaching error on a dead state / timeout.
async fn wait_for_idle(
    client: &FabricClient,
    ws: &str,
    lh: &str,
    sid: &str,
    timeout_secs: u64,
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let state = get_session_state(client, ws, lh, sid).await?;
        if state == "idle" {
            return Ok(state);
        }
        if is_dead_state(&state) {
            return Err(FabioError::with_hint(
                ErrorCode::ApiError,
                format!("Livy session '{sid}' entered terminal state '{state}' before becoming ready."),
                "Check the lakehouse capacity is running and retry. Inspect the session with `spark get-livy-session`."
                    .to_string(),
            )
            .into());
        }
        if Instant::now() >= deadline {
            return Err(FabioError::with_hint(
                ErrorCode::Timeout,
                format!("Livy session '{sid}' did not reach 'idle' within {timeout_secs}s (last state: {state})."),
                "Increase --timeout (a cold Spark session can take ~2 min to start), or poll with `spark get-livy-session`."
                    .to_string(),
            )
            .into());
        }
        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}

/// Submit a statement and poll it to completion, bounded by `timeout_secs`.
/// Returns the completed statement object (`{state, output, …}`).
async fn submit_and_wait_statement(
    client: &FabricClient,
    ws: &str,
    lh: &str,
    sid: &str,
    code: &str,
    kind: &str,
    timeout_secs: u64,
) -> Result<Value> {
    let submitted = client
        .post(
            &format!("{}/{sid}/statements", sessions_base(ws, lh)),
            &json!({ "code": code, "kind": kind }),
            false,
        )
        .await?;
    let stmt_id = submitted.get("id").and_then(Value::as_i64).ok_or_else(|| {
        FabioError::new(
            ErrorCode::ApiError,
            "Statement response had no id".to_string(),
        )
    })?;

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let stmt = client
            .get(&format!(
                "{}/{sid}/statements/{stmt_id}",
                sessions_base(ws, lh)
            ))
            .await?;
        let state = stmt
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        // Terminal statement states.
        if matches!(state, "available" | "cancelled" | "error") {
            return Ok(stmt);
        }
        if Instant::now() >= deadline {
            return Err(FabioError::with_hint(
                ErrorCode::Timeout,
                format!("Statement {stmt_id} did not finish within {timeout_secs}s (last state: {state})."),
                "Increase --timeout for long-running Spark jobs.".to_string(),
            )
            .into());
        }
        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}

/// Extract a renderable result from a completed statement object. Returns
/// `(output_object, is_error)`. Pure/testable.
///
/// `text` is the `text/plain` block (Scala/PySpark REPL output). `data` is the
/// FULL `output.data` map so nothing is lost — SQL results arrive under
/// `data["application/json"]` as `{schema, data, truncated}`, not as text/plain.
fn render_statement_output(stmt: &Value) -> (Value, bool) {
    let output = stmt.get("output").cloned().unwrap_or(Value::Null);
    let status = output.get("status").and_then(Value::as_str).unwrap_or("");
    let is_error = status == "error";
    let data = output.get("data").cloned().unwrap_or(Value::Null);
    let text = data
        .get("text/plain")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut result = json!({
        "statementId": stmt.get("id").cloned().unwrap_or(Value::Null),
        "state": stmt.get("state").cloned().unwrap_or(Value::Null),
        "status": if status.is_empty() { Value::Null } else { json!(status) },
        "text": text,
        "data": data,
    });
    if is_error {
        result["ename"] = output.get("ename").cloned().unwrap_or(Value::Null);
        result["evalue"] = output.get("evalue").cloned().unwrap_or(Value::Null);
        result["traceback"] = output.get("traceback").cloned().unwrap_or(Value::Null);
    }
    (result, is_error)
}

// ─── Command handlers ────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn create_livy_session(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: &str,
    name: Option<&str>,
    conf: Option<&str>,
    wait: bool,
    timeout: Option<u64>,
) -> Result<()> {
    let conf = parse_conf(conf)?;
    if output::dry_run_guard(
        cli,
        "spark create-livy-session",
        &json!({ "workspace": workspace, "lakehouse": lakehouse, "name": name, "wait": wait }),
    ) {
        return Ok(());
    }

    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    if let Some(c) = conf {
        body["conf"] = c;
    }
    let created = client
        .post(&sessions_base(workspace, lakehouse), &body, false)
        .await?;
    let sid = created
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "Create session returned no id".to_string(),
            )
        })?
        .to_string();

    let timeout_secs = timeout.unwrap_or(DEFAULT_TIMEOUT_SECS);
    let state = if wait {
        wait_for_idle(client, workspace, lakehouse, &sid, timeout_secs).await?
    } else {
        get_session_state(client, workspace, lakehouse, &sid)
            .await
            .unwrap_or_else(|_| "not_started".to_string())
    };

    let mut out = json!({
        "sessionId": sid,
        "artifactId": created.get("artifactId").cloned().unwrap_or(Value::Null),
        "state": state,
    });
    if !wait {
        out["hint"] = json!(
            "Session is starting. Run code once it is 'idle' with `spark run-statement --session-id <id>` (add --wait here to block until ready)."
        );
    }
    output::render_object(cli, &out, "sessionId");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_statement(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: &str,
    session_id: &str,
    code: Option<&str>,
    language: &str,
    timeout: Option<u64>,
) -> Result<()> {
    let code = resolve_code_input(code)?;
    if output::dry_run_guard(
        cli,
        "spark run-statement",
        &json!({
            "workspace": workspace,
            "lakehouse": lakehouse,
            "sessionId": session_id,
            "language": language,
            "codeLength": code.len(),
        }),
    ) {
        return Ok(());
    }

    let kind = statement_kind(language);
    let timeout_secs = timeout.unwrap_or(DEFAULT_TIMEOUT_SECS);
    let stmt = submit_and_wait_statement(
        client,
        workspace,
        lakehouse,
        session_id,
        &code,
        kind,
        timeout_secs,
    )
    .await?;
    let (mut result, is_error) = render_statement_output(&stmt);
    result["sessionId"] = json!(session_id);
    output::render_object(cli, &result, "text");
    if is_error {
        anyhow::bail!("Statement returned an error result (see ename/evalue/traceback).");
    }
    Ok(())
}

pub async fn delete_livy_session(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: &str,
    session_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "spark delete-livy-session",
        &json!({ "workspace": workspace, "lakehouse": lakehouse, "sessionId": session_id }),
    ) {
        return Ok(());
    }
    client
        .delete(&format!(
            "{}/{session_id}",
            sessions_base(workspace, lakehouse)
        ))
        .await?;
    output::render_object(
        cli,
        &json!({ "sessionId": session_id, "status": "deleted" }),
        "status",
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: &str,
    code: Option<&str>,
    language: &str,
    conf: Option<&str>,
    timeout: Option<u64>,
) -> Result<()> {
    let code = resolve_code_input(code)?;
    let conf = parse_conf(conf)?;
    if output::dry_run_guard(
        cli,
        "spark run",
        &json!({
            "workspace": workspace,
            "lakehouse": lakehouse,
            "language": language,
            "codeLength": code.len(),
        }),
    ) {
        return Ok(());
    }

    // Create an ephemeral session.
    let mut body = json!({ "name": "fabio-spark-run" });
    if let Some(c) = conf {
        body["conf"] = c;
    }
    let created = client
        .post(&sessions_base(workspace, lakehouse), &body, false)
        .await?;
    let sid = created
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "Create session returned no id".to_string(),
            )
        })?
        .to_string();

    // Run the whole flow, then ALWAYS delete the session (even on error/timeout).
    let kind = statement_kind(language);
    let timeout_secs = timeout.unwrap_or(DEFAULT_TIMEOUT_SECS);
    let outcome: Result<Value> = async {
        wait_for_idle(client, workspace, lakehouse, &sid, timeout_secs).await?;
        submit_and_wait_statement(
            client,
            workspace,
            lakehouse,
            &sid,
            &code,
            kind,
            timeout_secs,
        )
        .await
    }
    .await;
    // Best-effort cleanup — never mask the primary outcome.
    let _ = client
        .delete(&format!("{}/{sid}", sessions_base(workspace, lakehouse)))
        .await;

    let stmt = outcome?;
    let (mut result, is_error) = render_statement_output(&stmt);
    result["sessionId"] = json!(sid);
    result["sessionDeleted"] = json!(true);
    output::render_object(cli, &result, "text");
    if is_error {
        anyhow::bail!("Statement returned an error result (see ename/evalue/traceback).");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statement_kind_maps_languages() {
        assert_eq!(statement_kind("pyspark"), "pyspark");
        assert_eq!(statement_kind("scala"), "spark");
        assert_eq!(statement_kind("sql"), "sql");
        assert_eq!(statement_kind("r"), "sparkr");
        // Unknown/default → pyspark.
        assert_eq!(statement_kind("whatever"), "pyspark");
    }

    #[test]
    fn sessions_base_uses_verified_version() {
        assert_eq!(
            sessions_base("ws", "lh"),
            "/workspaces/ws/lakehouses/lh/livyApi/versions/2023-12-01/sessions"
        );
    }

    #[test]
    fn parse_conf_accepts_object_rejects_other() {
        assert!(parse_conf(None).unwrap().is_none());
        assert!(
            parse_conf(Some(r#"{"spark.executor.cores":"4"}"#))
                .unwrap()
                .is_some()
        );
        assert!(parse_conf(Some("[1,2]")).is_err());
        assert!(parse_conf(Some("not json")).is_err());
    }

    #[test]
    fn dead_states_are_terminal() {
        assert!(is_dead_state("dead"));
        assert!(is_dead_state("error"));
        assert!(is_dead_state("killed"));
        assert!(!is_dead_state("idle"));
        assert!(!is_dead_state("starting"));
    }

    #[test]
    fn render_output_ok_extracts_text() {
        let stmt = json!({
            "id": 1, "state": "available",
            "output": {"status": "ok", "data": {"text/plain": "res1: Long = 3"}}
        });
        let (out, is_err) = render_statement_output(&stmt);
        assert!(!is_err);
        assert_eq!(out["text"], "res1: Long = 3");
        assert_eq!(out["status"], "ok");
    }

    #[test]
    fn render_output_surfaces_sql_application_json() {
        // SQL results arrive under data["application/json"], NOT text/plain.
        let stmt = json!({
            "id": 3, "state": "available",
            "output": {"status": "ok", "data": {"application/json": {
                "schema": {"fields": [{"name": "a"}]}, "data": [[1]], "truncated": false
            }}}
        });
        let (out, is_err) = render_statement_output(&stmt);
        assert!(!is_err);
        // text/plain is absent, but the structured table is preserved under data.
        assert!(out["text"].is_null());
        assert_eq!(out["data"]["application/json"]["data"][0][0], 1);
    }

    #[test]
    fn render_output_error_surfaces_traceback() {
        let stmt = json!({
            "id": 2, "state": "available",
            "output": {"status": "error", "ename": "NameError", "evalue": "x undefined", "traceback": ["line 1"]}
        });
        let (out, is_err) = render_statement_output(&stmt);
        assert!(is_err);
        assert_eq!(out["ename"], "NameError");
        assert_eq!(out["evalue"], "x undefined");
        assert_eq!(out["traceback"][0], "line 1");
    }
}
