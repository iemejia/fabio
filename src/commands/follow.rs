//! Backend-agnostic bounded "follow" (tail) streaming.
//!
//! Some data sources are request/response only (Kusto queries, T-SQL DMV
//! snapshots) — there is no server-push stream. To let an agent/operator watch
//! live-changing data, [`follow_stream`] POLLS a caller-supplied fetch closure on
//! an interval and emits one NDJSON object per cycle to stdout, then a final
//! `follow_complete` summary. It is ALWAYS bounded — by `--max-duration`, the
//! global `--limit`, or Ctrl-C — so it never hangs an agent/CI caller.
//!
//! Used by KQL query (`eventhouse`/`kql-database query --follow`, Kusto backend)
//! and warehouse query monitoring (`queries-running --follow`, TDS backend).

use std::io::Write;

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;

/// Bounds and de-duplication for a `--follow` stream.
#[derive(Debug, Default, Clone)]
pub struct FollowOptions {
    /// Seconds between polls (default 5).
    pub interval: Option<u64>,
    /// Total seconds to follow before stopping (default 60) — the safety bound.
    pub max_duration: Option<u64>,
    /// Emit only rows whose value in this column exceeds the max seen (incremental tail).
    pub dedup_column: Option<String>,
}

impl FollowOptions {
    /// Reject the follow-only flags (`--interval`/`--max-duration`/`--dedup-column`)
    /// when `--follow` is not set, so a caller who forgets `--follow` gets a clear
    /// error instead of silently-ignored flags.
    pub fn validate(&self, follow: bool) -> Result<()> {
        if !follow
            && (self.interval.is_some()
                || self.max_duration.is_some()
                || self.dedup_column.is_some())
        {
            return Err(crate::errors::FabioError::with_hint(
                crate::errors::ErrorCode::InvalidInput,
                "--interval, --max-duration, and --dedup-column require --follow".to_string(),
                "Add --follow to stream on an interval, or drop those flags for a one-shot snapshot.".to_string(),
            )
            .into());
        }
        Ok(())
    }
}

/// Poll `fetch` every `interval`, streaming one NDJSON object per cycle to
/// stdout, bounded by `max_duration`, the global `--limit`, or Ctrl-C, then a
/// final `follow_complete` summary. Always terminates.
///
/// `fetch` returns `(rows, columns)` for a cycle (an `Err` is reported as an
/// `{cycle, error}` line and does not stop the stream). `stop_when` is checked on
/// each cycle's RAW rows (before `--dedup-column` filtering); returning `true`
/// stops the stream early with reason `"complete"` — used to watch a job/operation
/// until it reaches a terminal state. For an unbounded tail (query monitoring),
/// pass `|_| false`.
pub async fn follow_stream<F, S>(
    cli: &Cli,
    opts: &FollowOptions,
    mut fetch: F,
    stop_when: S,
) -> Result<()>
where
    F: AsyncFnMut() -> Result<(Vec<Value>, Vec<String>)>,
    S: Fn(&[Value]) -> bool,
{
    let interval = std::time::Duration::from_secs(opts.interval.unwrap_or(5).max(1));
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_secs(opts.max_duration.unwrap_or(60));
    let row_limit = cli.limit;

    let mut cycle: u64 = 0;
    let mut total_emitted: usize = 0;
    let mut last_max: Option<Value> = None;
    let mut stop_reason = "max_duration";

    loop {
        cycle += 1;
        let started = tokio::time::Instant::now();

        let mut reached_terminal = false;
        let event = match fetch().await {
            Ok((rows, columns)) => {
                reached_terminal = stop_when(&rows);
                let new_rows = match opts.dedup_column.as_deref() {
                    Some(col) => filter_new_rows(&rows, col, &mut last_max),
                    None => rows,
                };
                total_emitted += new_rows.len();
                serde_json::json!({
                    "cycle": cycle,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "count": new_rows.len(),
                    "columns": columns,
                    "rows": new_rows,
                })
            }
            Err(e) => serde_json::json!({
                "cycle": cycle,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "error": e.to_string(),
            }),
        };

        emit(cli, &event);

        if reached_terminal {
            stop_reason = "complete";
            break;
        }
        if row_limit.is_some_and(|lim| total_emitted >= lim) {
            stop_reason = "limit";
            break;
        }

        let next = started + interval;
        tokio::select! {
            () = tokio::time::sleep_until(next.min(deadline)) => {
                if tokio::time::Instant::now() >= deadline {
                    break; // stop_reason keeps its default "max_duration"
                }
            }
            _ = tokio::signal::ctrl_c() => {
                stop_reason = "interrupted";
                break;
            }
        }
    }

    emit(
        cli,
        &serde_json::json!({
            "status": "follow_complete",
            "reason": stop_reason,
            "cycles": cycle,
            "rows_emitted": total_emitted,
        }),
    );
    Ok(())
}

/// Write one NDJSON line to stdout (respecting `--quiet`), flushed for streaming.
fn emit(cli: &Cli, event: &Value) {
    if cli.quiet {
        return;
    }
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{}", serde_json::to_string(event).unwrap_or_default());
    let _ = out.flush();
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn follow_options_validate_requires_follow() {
        let o = FollowOptions {
            interval: Some(2),
            ..Default::default()
        };
        assert!(o.validate(false).is_err());
        assert!(o.validate(true).is_ok());
        assert!(FollowOptions::default().validate(false).is_ok());
    }

    #[test]
    fn value_gt_numeric_and_string() {
        assert!(value_gt(&json!(5), &json!(3)));
        assert!(!value_gt(&json!(3), &json!(5)));
        assert!(value_gt(&json!("2026-01-02"), &json!("2026-01-01")));
        assert!(!value_gt(&json!("a"), &json!("b")));
    }

    #[test]
    fn filter_new_rows_emits_only_newer_and_advances_threshold() {
        let rows = vec![json!({"seq": 1}), json!({"seq": 3}), json!({"seq": 2})];
        let mut last = None;
        let out = filter_new_rows(&rows, "seq", &mut last);
        assert_eq!(out.len(), 3);
        assert_eq!(last, Some(json!(3)));

        let out2 = filter_new_rows(&rows, "seq", &mut last);
        assert!(out2.is_empty());

        let more = vec![json!({"seq": 4}), json!({"seq": 3})];
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
