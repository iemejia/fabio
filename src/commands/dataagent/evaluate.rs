//! `data-agent evaluate` — batch-run a set of questions against a published
//! data agent.
//!
//! This is an *evaluation primitive*, not a judge: it runs each question
//! (optionally several times) through the published agent's Assistants endpoint
//! and emits the answers (and, with `--show-steps`, the execution steps). When a
//! question row carries an `expected` answer, fabio adds a **naive** string-match
//! signal (`match.exact` / `match.contains`) — it deliberately does NOT perform
//! semantic/LLM grading, leaving that judgment to the agent consuming fabio.
//!
//! Mirrors the Python `fabric.dataagent.evaluation.evaluate_data_agent` shape
//! (questions + expected answers, `num_query_repeats`) without the LLM critic.

use std::time::Duration;

use anyhow::Result;
use serde_json::Value;

use super::query::{QueryOptions, run_assistant_query};
use crate::cli::Cli;
use crate::client::{self, FabricClient};
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// One question to evaluate, with an optional expected answer.
struct QuestionSpec {
    question: String,
    expected: Option<String>,
}

/// Batch-run `questions` against a published data agent and report the answers.
#[allow(clippy::too_many_arguments)]
pub(super) async fn evaluate(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    questions_file: &str,
    published_url: Option<&str>,
    repeats: u32,
    show_steps: bool,
    stage: &str,
    timeout: u64,
) -> Result<()> {
    if repeats == 0 {
        return Err(FabioError::invalid_input("--repeats must be at least 1").into());
    }

    let content = std::fs::read_to_string(questions_file).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Failed to read questions file '{questions_file}': {e}"),
            "Provide a JSON array (of strings or {\"question\",\"expected\"} objects) or a CSV/TSV \
             file with a 'question' column (optional 'expected' column).",
        )
    })?;
    let specs = parse_questions(&content, questions_file)?;
    if specs.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("No questions found in '{questions_file}'"),
            "Ensure the file has at least one non-empty question.",
        )
        .into());
    }

    // Resolve the published endpoint once and reuse it for every question.
    let resolved_url = if let Some(url) = published_url {
        client::validate_trusted_url(url, "--published-url")?;
        url.to_string()
    } else {
        // Only a published agent is queryable; surface the same guidance as `query`.
        super::query::resolve_published_url(client, workspace, id, stage).await?
    };

    let token = client.require_auth().await?;
    let max_wait = Duration::from_secs(timeout);

    let mut results = Vec::with_capacity(specs.len());
    let mut total_runs: u64 = 0;
    let mut failed_runs: u64 = 0;

    for spec in &specs {
        let mut answers = Vec::with_capacity(repeats as usize);
        for _ in 0..repeats {
            total_runs += 1;
            // Each run uses its own throwaway thread so questions stay independent.
            let opts = QueryOptions {
                thread_id: None,
                keep_thread: false,
                show_steps,
                download_dir: None,
            };
            match run_assistant_query(&resolved_url, &token, &spec.question, &opts, max_wait).await
            {
                Ok(res) => {
                    let mut entry = serde_json::json!({ "answer": res.answer });
                    if let Some(steps) = res.steps {
                        entry["steps"] = steps;
                    }
                    answers.push(entry);
                }
                Err(e) => {
                    failed_runs += 1;
                    answers.push(serde_json::json!({ "error": e.to_string() }));
                }
            }
        }

        let mut entry = serde_json::json!({
            "question": spec.question,
            "answers": answers,
        });
        if let Some(expected) = &spec.expected {
            entry["expected"] = Value::from(expected.as_str());
            entry["match"] = compute_match(&entry["answers"], expected);
        }
        results.push(entry);
    }

    // Fail fast only if every single run errored (e.g. agent not published) —
    // otherwise return partial results so the caller can inspect what worked.
    if failed_runs == total_runs {
        let first_error = results
            .iter()
            .flat_map(|r| r["answers"].as_array().cloned().unwrap_or_default())
            .find_map(|a| a.get("error").and_then(Value::as_str).map(String::from))
            .unwrap_or_else(|| "all evaluation runs failed".to_string());
        return Err(FabioError::with_hint(
            ErrorCode::ApiError,
            format!("All {total_runs} evaluation run(s) failed: {first_error}"),
            "Verify the agent is published and reachable: fabio data-agent query --workspace <WS> --id <ID> --prompt \"hi\". Publish with: fabio data-agent publish --workspace <WS> --id <ID>.",
        )
        .into());
    }

    let out = serde_json::json!({
        "questionCount": specs.len(),
        "repeats": repeats,
        "runCount": total_runs,
        "failedRuns": failed_runs,
        "results": results,
    });
    output::render_object(cli, &out, "questionCount");
    Ok(())
}

/// Parse a questions file (JSON or delimited) into question specs.
fn parse_questions(content: &str, file: &str) -> Result<Vec<QuestionSpec>> {
    let ext = std::path::Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let trimmed = content.trim_start();
    let looks_json = ext == "json" || trimmed.starts_with('[') || trimmed.starts_with('{');

    if looks_json {
        let value: Value = serde_json::from_str(content).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to parse JSON questions file '{file}': {e}"),
                "Expected a JSON array of strings or of objects like {\"question\":\"...\",\"expected\":\"...\"}.",
            )
        })?;
        parse_questions_json(&value, file)
    } else {
        parse_questions_delimited(content, &ext)
    }
}

/// Parse questions from a JSON value (array of strings or of objects).
fn parse_questions_json(value: &Value, file: &str) -> Result<Vec<QuestionSpec>> {
    let arr = value.as_array().ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("JSON questions file '{file}' must be an array"),
            "Use a JSON array of strings or of {\"question\",\"expected\"} objects.",
        )
    })?;

    let mut specs = Vec::with_capacity(arr.len());
    for item in arr {
        match item {
            Value::String(s) => {
                let q = s.trim();
                if !q.is_empty() {
                    specs.push(QuestionSpec {
                        question: q.to_string(),
                        expected: None,
                    });
                }
            }
            Value::Object(_) => {
                let question = item
                    .get("question")
                    .or_else(|| item.get("prompt"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if question.is_empty() {
                    continue;
                }
                let expected = item
                    .get("expected")
                    .or_else(|| item.get("expected_answer"))
                    .or_else(|| item.get("expectedAnswer"))
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                specs.push(QuestionSpec {
                    question: question.to_string(),
                    expected,
                });
            }
            _ => {}
        }
    }
    Ok(specs)
}

/// Parse questions from a delimited (CSV/TSV) file with a `question` column.
///
/// A `question` header is required; an optional `expected`/`expected_answer`
/// column supplies the reference answer. If no recognizable header is present,
/// the first column of every row is treated as the question.
fn parse_questions_delimited(content: &str, ext: &str) -> Result<Vec<QuestionSpec>> {
    let delimiter = if ext == "tsv" { b'\t' } else { b',' };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .flexible(true)
        .from_reader(content.as_bytes());

    let headers = reader.headers().map_err(|e| {
        FabioError::new(
            ErrorCode::InvalidInput,
            format!("Failed to parse questions file headers: {e}"),
        )
    })?;
    let question_idx = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("question") || h.eq_ignore_ascii_case("prompt"));
    let expected_idx = headers.iter().position(|h| {
        h.eq_ignore_ascii_case("expected")
            || h.eq_ignore_ascii_case("expected_answer")
            || h.eq_ignore_ascii_case("answer")
    });
    // If there is no `question` header, treat column 0 as the question and the
    // header row itself as data.
    let q_idx = question_idx.unwrap_or(0);
    let treat_header_as_row = question_idx.is_none();

    let mut specs = Vec::new();
    if treat_header_as_row {
        let first = headers.get(q_idx).unwrap_or("").trim();
        if !first.is_empty() {
            specs.push(QuestionSpec {
                question: first.to_string(),
                expected: None,
            });
        }
    }

    for record in reader.records() {
        let record = record.map_err(|e| {
            FabioError::new(
                ErrorCode::InvalidInput,
                format!("Failed to parse questions file row: {e}"),
            )
        })?;
        let question = record.get(q_idx).unwrap_or("").trim();
        if question.is_empty() {
            continue;
        }
        let expected = expected_idx
            .and_then(|i| record.get(i))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        specs.push(QuestionSpec {
            question: question.to_string(),
            expected,
        });
    }
    Ok(specs)
}

/// Compute a naive string-match signal for an expected answer.
///
/// This is a convenience heuristic ONLY — NOT semantic grading. It reports
/// whether any produced answer, after case/whitespace normalization, equals
/// (`exact`) or contains (`contains`) the expected text.
fn compute_match(answers: &Value, expected: &str) -> Value {
    let want = normalize(expected);
    let mut exact = false;
    let mut contains = false;
    if let Some(arr) = answers.as_array() {
        for a in arr {
            if let Some(ans) = a.get("answer").and_then(Value::as_str) {
                let got = normalize(ans);
                if got == want {
                    exact = true;
                }
                if !want.is_empty() && got.contains(&want) {
                    contains = true;
                }
            }
        }
    }
    serde_json::json!({
        "exact": exact,
        "contains": contains,
        "note": "naive string comparison (case/whitespace-insensitive); not semantic grading",
    })
}

/// Lower-case and collapse runs of ASCII whitespace to single spaces, trimmed.
fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_questions_json_array_of_strings() {
        let specs = parse_questions(r#"["a?", "b?"]"#, "q.json").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].question, "a?");
        assert!(specs[0].expected.is_none());
    }

    #[test]
    fn parse_questions_json_array_of_objects() {
        let json = r#"[{"question":"total sales?","expected":"42"},{"question":"top product?"}]"#;
        let specs = parse_questions(json, "q.json").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].expected.as_deref(), Some("42"));
        assert!(specs[1].expected.is_none());
    }

    #[test]
    fn parse_questions_json_accepts_expected_answer_alias() {
        let json = r#"[{"question":"q","expected_answer":"e"}]"#;
        let specs = parse_questions(json, "q.json").unwrap();
        assert_eq!(specs[0].expected.as_deref(), Some("e"));
    }

    #[test]
    fn parse_questions_json_skips_blank_and_typeless() {
        let json = r#"["ok", "", "   ", 5, {"question":""}]"#;
        let specs = parse_questions(json, "q.json").unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].question, "ok");
    }

    #[test]
    fn parse_questions_csv_with_headers() {
        let csv = "question,expected\ntotal sales?,42\ntop product?,\n";
        let specs = parse_questions(csv, "q.csv").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].question, "total sales?");
        assert_eq!(specs[0].expected.as_deref(), Some("42"));
        assert!(specs[1].expected.is_none());
    }

    #[test]
    fn parse_questions_csv_handles_quoted_commas() {
        let csv = "question,expected\n\"revenue, by month?\",\"grew, then fell\"\n";
        let specs = parse_questions(csv, "q.csv").unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].question, "revenue, by month?");
        assert_eq!(specs[0].expected.as_deref(), Some("grew, then fell"));
    }

    #[test]
    fn parse_questions_tsv_delimiter() {
        let tsv = "question\texpected\nwhat?\tthis\n";
        let specs = parse_questions(tsv, "q.tsv").unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].question, "what?");
        assert_eq!(specs[0].expected.as_deref(), Some("this"));
    }

    #[test]
    fn parse_questions_headerless_first_column() {
        // No `question` header → each row's first column is the question,
        // including the first line.
        let csv = "just one question?\nanother question?\n";
        let specs = parse_questions(csv, "q.csv").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].question, "just one question?");
        assert_eq!(specs[1].question, "another question?");
    }

    #[test]
    fn compute_match_exact_and_contains() {
        let answers = serde_json::json!([{ "answer": "The Total is 42 units" }]);
        let m = compute_match(&answers, "total is 42");
        assert_eq!(m["exact"], false);
        assert_eq!(m["contains"], true);

        let m2 = compute_match(&answers, "  the   TOTAL is 42 units ");
        assert_eq!(m2["exact"], true);
        assert_eq!(m2["contains"], true);
    }

    #[test]
    fn compute_match_ignores_error_entries() {
        let answers = serde_json::json!([{ "error": "boom" }]);
        let m = compute_match(&answers, "anything");
        assert_eq!(m["exact"], false);
        assert_eq!(m["contains"], false);
    }

    #[test]
    fn normalize_collapses_whitespace_and_case() {
        assert_eq!(normalize("  Hello\t World\n"), "hello world");
    }
}
