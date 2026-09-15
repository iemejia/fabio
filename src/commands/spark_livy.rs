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
//! High-Concurrency (HC): `spark run --high-concurrency [--session-tag <tag>]` uses
//! the separate `/highConcurrencySessions` endpoint. HC sessions sharing a
//! `sessionTag` are packed onto ONE underlying Spark session (each with an isolated
//! REPL) — concurrent `spark run` invocations with the same tag share compute. The
//! HC id / underlying `sessionId` / `replId` plumbing is hidden behind the one-shot.
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

/// The High-Concurrency (HC) session endpoint. HC sessions can be packed onto a
/// shared underlying Spark session via a common `sessionTag`; each gets an
/// isolated REPL. Statements route through `/repls/{replId}/statements` (using the
/// underlying `sessionId`, not the HC id); the HC id is used to GET/DELETE.
fn hc_base(ws: &str, lh: &str) -> String {
    format!(
        "/workspaces/{ws}/lakehouses/{lh}/livyApi/versions/{LIVY_VERSION}/highConcurrencySessions"
    )
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
/// Case-insensitive: regular sessions report `dead`, HC sessions report `Dead`.
fn is_dead_state(state: &str) -> bool {
    let s = state.to_ascii_lowercase();
    matches!(
        s.as_str(),
        "dead" | "error" | "killed" | "failed" | "shutting_down" | "success"
    )
}

/// Whether a session state is the ready state (case-insensitive: `idle`/`Idle`).
const fn is_idle_state(state: &str) -> bool {
    state.eq_ignore_ascii_case("idle")
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
        if is_idle_state(&state) {
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

/// Poll an HC session until it reaches `Idle`, bounded by `timeout_secs`. Returns
/// the `(underlying_livy_session_id, repl_id)` needed to submit statements — both
/// are only populated once the HC session is `Idle`.
async fn wait_for_hc_idle(
    client: &FabricClient,
    ws: &str,
    lh: &str,
    hc_id: &str,
    timeout_secs: u64,
) -> Result<(String, String)> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let data = client.get(&format!("{}/{hc_id}", hc_base(ws, lh))).await?;
        let state = data
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if is_idle_state(state) {
            let session_id = data
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let repl_id = data
                .get("replId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if session_id.is_empty() || repl_id.is_empty() {
                return Err(FabioError::new(
                    ErrorCode::ApiError,
                    "HC session is Idle but did not return sessionId/replId.".to_string(),
                )
                .into());
            }
            return Ok((session_id.to_string(), repl_id.to_string()));
        }
        if is_dead_state(state) {
            return Err(FabioError::with_hint(
                ErrorCode::ApiError,
                format!("HC Livy session '{hc_id}' entered terminal state '{state}' before becoming ready."),
                "Check the lakehouse capacity is running and retry.".to_string(),
            )
            .into());
        }
        if Instant::now() >= deadline {
            return Err(FabioError::with_hint(
                ErrorCode::Timeout,
                format!("HC Livy session '{hc_id}' did not reach 'Idle' within {timeout_secs}s (last state: {state})."),
                "Increase --timeout; acquiring an HC session can take a couple of minutes.".to_string(),
            )
            .into());
        }
        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}

/// Submit a statement to a statements collection URL and poll it to completion,
/// bounded by `timeout_secs`. Returns the completed statement object. `statements_url`
/// is the full collection URL: `…/sessions/{id}/statements` (regular) or
/// `…/highConcurrencySessions/{sessionId}/repls/{replId}/statements` (HC).
async fn submit_and_wait_statement(
    client: &FabricClient,
    statements_url: &str,
    code: &str,
    kind: &str,
    timeout_secs: u64,
) -> Result<Value> {
    let submitted = client
        .post(
            statements_url,
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
        let stmt = client.get(&format!("{statements_url}/{stmt_id}")).await?;
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
    let statements_url = format!(
        "{}/{session_id}/statements",
        sessions_base(workspace, lakehouse)
    );
    let stmt =
        submit_and_wait_statement(client, &statements_url, &code, kind, timeout_secs).await?;
    let (mut result, is_error) = render_statement_output(&stmt);
    result["sessionId"] = json!(session_id);
    output::render_object(cli, &result, "text");
    if is_error {
        anyhow::bail!("Statement returned an error result (see ename/evalue/traceback).");
    }
    Ok(())
}

/// Submit a statement WITHOUT waiting (async): POST the code and return the
/// statement id immediately, so the caller can poll with `get-statement` or stop
/// it with `cancel-statement`. Use `run-statement` for the submit-and-wait case.
pub async fn submit_statement(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: &str,
    session_id: &str,
    code: Option<&str>,
    language: &str,
) -> Result<()> {
    let code = resolve_code_input(code)?;
    if output::dry_run_guard(
        cli,
        "spark submit-statement",
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
    let statements_url = format!(
        "{}/{session_id}/statements",
        sessions_base(workspace, lakehouse)
    );
    let submitted = client
        .post(
            &statements_url,
            &json!({ "code": code, "kind": kind }),
            false,
        )
        .await?;
    let out = json!({
        "sessionId": session_id,
        "statementId": submitted.get("id").cloned().unwrap_or(Value::Null),
        "state": submitted.get("state").cloned().unwrap_or(Value::Null),
        "hint": "Statement submitted (not awaited). Poll it with `spark get-statement --statement-id <id>`, or stop it with `spark cancel-statement --statement-id <id>`.",
    });
    output::render_object(cli, &out, "statementId");
    Ok(())
}

/// Get the current state and output of a statement (read-only). Unlike
/// `run-statement`, this does NOT wait or fail on a statement error — it reports
/// whatever the statement's current state/output is (poll it until `available`).
pub async fn get_statement(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: &str,
    session_id: &str,
    statement_id: &str,
) -> Result<()> {
    let stmt = client
        .get(&format!(
            "{}/{session_id}/statements/{statement_id}",
            sessions_base(workspace, lakehouse)
        ))
        .await?;
    let (mut result, _is_error) = render_statement_output(&stmt);
    result["sessionId"] = json!(session_id);
    output::render_object(cli, &result, "text");
    Ok(())
}

/// Cancel a running statement (stops the Spark compute). A statement whose caller
/// timed out keeps running and consuming capacity until cancelled — use this to
/// stop it.
pub async fn cancel_statement(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    lakehouse: &str,
    session_id: &str,
    statement_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "spark cancel-statement",
        &json!({
            "workspace": workspace,
            "lakehouse": lakehouse,
            "sessionId": session_id,
            "statementId": statement_id,
        }),
    ) {
        return Ok(());
    }
    let resp = client
        .post(
            &format!(
                "{}/{session_id}/statements/{statement_id}/cancel",
                sessions_base(workspace, lakehouse)
            ),
            &json!({}),
            false,
        )
        .await?;
    let msg = resp
        .get("msg")
        .and_then(Value::as_str)
        .unwrap_or("cancel requested");
    output::render_object(
        cli,
        &json!({ "sessionId": session_id, "statementId": statement_id, "status": msg }),
        "status",
    );
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
    high_concurrency: bool,
    session_tag: Option<&str>,
    timeout: Option<u64>,
) -> Result<()> {
    let code = resolve_code_input(code)?;
    let conf = parse_conf(conf)?;
    if session_tag.is_some() && !high_concurrency {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--session-tag requires --high-concurrency".to_string(),
            "Add --high-concurrency (session packing only applies to HC sessions), or drop --session-tag."
                .to_string(),
        )
        .into());
    }
    if output::dry_run_guard(
        cli,
        "spark run",
        &json!({
            "workspace": workspace,
            "lakehouse": lakehouse,
            "language": language,
            "highConcurrency": high_concurrency,
            "sessionTag": session_tag,
            "codeLength": code.len(),
        }),
    ) {
        return Ok(());
    }

    let kind = statement_kind(language);
    let timeout_secs = timeout.unwrap_or(DEFAULT_TIMEOUT_SECS);

    let stmt = if high_concurrency {
        oneshot_hc(
            client,
            workspace,
            lakehouse,
            &code,
            kind,
            conf,
            session_tag,
            timeout_secs,
        )
        .await?
    } else {
        oneshot_regular(
            client,
            workspace,
            lakehouse,
            &code,
            kind,
            conf,
            timeout_secs,
        )
        .await?
    };

    let (mut result, is_error) = render_statement_output(&stmt);
    result["sessionDeleted"] = json!(true);
    if high_concurrency {
        result["highConcurrency"] = json!(true);
    }
    output::render_object(cli, &result, "text");
    if is_error {
        anyhow::bail!("Statement returned an error result (see ename/evalue/traceback).");
    }
    Ok(())
}

/// Regular one-shot: create an ephemeral session → wait idle → run one statement →
/// ALWAYS delete the session (even on error). Returns the completed statement.
async fn oneshot_regular(
    client: &FabricClient,
    ws: &str,
    lh: &str,
    code: &str,
    kind: &str,
    conf: Option<Value>,
    timeout_secs: u64,
) -> Result<Value> {
    let mut body = json!({ "name": "fabio-spark-run" });
    if let Some(c) = conf {
        body["conf"] = c;
    }
    let created = client.post(&sessions_base(ws, lh), &body, false).await?;
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

    let outcome: Result<Value> = async {
        wait_for_idle(client, ws, lh, &sid, timeout_secs).await?;
        let statements_url = format!("{}/{sid}/statements", sessions_base(ws, lh));
        submit_and_wait_statement(client, &statements_url, code, kind, timeout_secs).await
    }
    .await;
    // Best-effort cleanup — never mask the primary outcome.
    let _ = client
        .delete(&format!("{}/{sid}", sessions_base(ws, lh)))
        .await;
    outcome
}

/// HC one-shot: acquire a High-Concurrency session (optionally packed onto a shared
/// Spark session via `session_tag`), run the statement through its isolated REPL,
/// and ALWAYS delete the HC session. The HC id / underlying sessionId / replId
/// plumbing is hidden here. Returns the completed statement.
#[allow(clippy::too_many_arguments)]
async fn oneshot_hc(
    client: &FabricClient,
    ws: &str,
    lh: &str,
    code: &str,
    kind: &str,
    conf: Option<Value>,
    session_tag: Option<&str>,
    timeout_secs: u64,
) -> Result<Value> {
    let mut hc_body = json!({});
    if let Some(tag) = session_tag {
        hc_body["sessionTag"] = json!(tag);
    }
    if let Some(c) = conf {
        hc_body["conf"] = c;
    }
    let created = client.post(&hc_base(ws, lh), &hc_body, false).await?;
    let hc_id = created
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "Create HC session returned no id".to_string(),
            )
        })?
        .to_string();

    let outcome: Result<Value> = async {
        let (livy_session_id, repl_id) =
            wait_for_hc_idle(client, ws, lh, &hc_id, timeout_secs).await?;
        let statements_url = format!(
            "{}/{livy_session_id}/repls/{repl_id}/statements",
            hc_base(ws, lh)
        );
        submit_and_wait_statement(client, &statements_url, code, kind, timeout_secs).await
    }
    .await;
    // Best-effort cleanup — delete uses the HC id.
    let _ = client.delete(&format!("{}/{hc_id}", hc_base(ws, lh))).await;
    outcome
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
    fn state_checks_are_case_insensitive() {
        // HC sessions report Capitalized states (Idle, Dead, Failed).
        assert!(is_idle_state("Idle"));
        assert!(is_idle_state("idle"));
        assert!(!is_idle_state("AcquiringHighConcurrencySession"));
        assert!(is_dead_state("Dead"));
        assert!(is_dead_state("Failed"));
        assert!(is_dead_state("Killed"));
    }

    #[test]
    fn hc_base_uses_high_concurrency_endpoint() {
        assert_eq!(
            hc_base("ws", "lh"),
            "/workspaces/ws/lakehouses/lh/livyApi/versions/2023-12-01/highConcurrencySessions"
        );
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
