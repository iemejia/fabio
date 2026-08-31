use anyhow::Result;
use serde_json::Value;

use super::stage_prefix;
use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::resolve_datasource_id;

/// List few-shot examples for a specific data source.
///
/// Uses: `GET /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}/fewshots` (staging)
///   or: `GET /workspaces/{ws}/dataAgents/{id}/datasources/{dsId}/fewshots` (published)
pub(super) async fn list_fewshots(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    stage: &str,
) -> Result<()> {
    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let prefix = stage_prefix(stage);

    let resp = client
        .get_list(
            &format!(
                "/workspaces/{workspace}/dataAgents/{id}{prefix}/datasources/{ds_id}/fewshots"
            ),
            "value",
            true,
            None,
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "question", "query"],
        &["ID", "QUESTION", "QUERY"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

/// Add a few-shot example to a data source.
///
/// Uses: `POST /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}/fewshots`
#[allow(clippy::too_many_arguments)]
pub(super) async fn add_fewshot(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    question: &str,
    query_text: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "data-agent add-fewshot",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
            "question": question,
            "query": query_text,
        }),
    ) {
        return Ok(());
    }

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;

    let body = serde_json::json!({
        "question": question,
        "query": query_text,
    });

    let resp = client
        .post(
            &format!(
                "/workspaces/{workspace}/dataAgents/{id}/staging/datasources/{ds_id}/fewshots"
            ),
            &body,
            false,
        )
        .await?;

    let result = if resp.is_null() || resp.as_object().is_some_and(serde_json::Map::is_empty) {
        serde_json::json!({
            "status": "fewshot_added",
            "question": question,
            "query": query_text,
        })
    } else {
        let mut r = resp;
        if let Some(obj) = r.as_object_mut() {
            obj.insert("status".to_string(), Value::from("fewshot_added"));
        }
        r
    };
    output::render_object(cli, &result, "status");
    Ok(())
}

/// Remove a few-shot example by ID.
///
/// Uses: `DELETE /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}/fewshots/{fewshotId}`
pub(super) async fn remove_fewshot(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    fewshot_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "data-agent remove-fewshot",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
            "fewshotId": fewshot_id,
        }),
    ) {
        return Ok(());
    }

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;

    client
        .delete(&format!(
            "/workspaces/{workspace}/dataAgents/{id}/staging/datasources/{ds_id}/fewshots/{fewshot_id}"
        ))
        .await?;

    let result = serde_json::json!({
        "id": id,
        "status": "fewshot_removed",
        "fewshotId": fewshot_id,
    });
    output::render_object(cli, &result, "status");
    Ok(())
}

/// Show a specific few-shot example by ID.
///
/// Uses: `GET /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}/fewshots/{fewshotId}` (staging)
///   or: `GET /workspaces/{ws}/dataAgents/{id}/datasources/{dsId}/fewshots/{fewshotId}` (published)
#[allow(clippy::too_many_arguments)]
pub(super) async fn show_fewshot(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    fewshot_id: &str,
    stage: &str,
) -> Result<()> {
    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let prefix = stage_prefix(stage);

    let data = client
        .get(&format!(
            "/workspaces/{workspace}/dataAgents/{id}{prefix}/datasources/{ds_id}/fewshots/{fewshot_id}"
        ))
        .await?;

    output::render_object(cli, &data, "id");
    Ok(())
}

/// Update an existing few-shot example (question and/or query).
///
/// Uses: `PATCH /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}/fewshots/{fewshotId}`
#[allow(clippy::too_many_arguments)]
pub(super) async fn update_fewshot(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    fewshot_id: &str,
    question: Option<&str>,
    answer: Option<&str>,
) -> Result<()> {
    if question.is_none() && answer.is_none() {
        return Err(FabioError::invalid_input(
            "At least one of --question or --answer must be provided",
        )
        .into());
    }

    if output::dry_run_guard(
        cli,
        "data-agent update-fewshot",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
            "fewshotId": fewshot_id,
            "question": question,
            "answer": answer,
        }),
    ) {
        return Ok(());
    }

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;

    let mut body = serde_json::Map::new();
    if let Some(q) = question {
        body.insert("question".to_string(), Value::from(q));
    }
    if let Some(a) = answer {
        body.insert("query".to_string(), Value::from(a));
    }

    let resp = client
        .patch(
            &format!(
                "/workspaces/{workspace}/dataAgents/{id}/staging/datasources/{ds_id}/fewshots/{fewshot_id}"
            ),
            &Value::Object(body),
        )
        .await?;

    let result = if resp.is_null() || resp.as_object().is_some_and(serde_json::Map::is_empty) {
        serde_json::json!({
            "id": id,
            "status": "fewshot_updated",
            "fewshotId": fewshot_id,
            "question": question,
            "answer": answer,
        })
    } else {
        let mut r = resp;
        if let Some(obj) = r.as_object_mut() {
            obj.insert("status".to_string(), Value::from("fewshot_updated"));
        }
        r
    };
    output::render_object(cli, &result, "status");
    Ok(())
}

/// Delete all few-shot examples for a data source.
///
/// Uses: `POST /workspaces/{ws}/dataAgents/{id}/staging/datasources/{dsId}/fewshots/deleteAll`
pub(super) async fn clear_fewshots(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "data-agent clear-fewshots",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
        }),
    ) {
        return Ok(());
    }

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;

    client
        .post(
            &format!(
                "/workspaces/{workspace}/dataAgents/{id}/staging/datasources/{ds_id}/fewshots/deleteAll"
            ),
            &serde_json::json!({}),
            false,
        )
        .await?;

    let result = serde_json::json!({
        "id": id,
        "status": "fewshots_cleared",
        "datasource": datasource,
    });
    output::render_object(cli, &result, "status");
    Ok(())
}

/// Bulk upload few-shot examples from a JSON or CSV file.
///
/// Uses: `POST .../fewshots` in a loop for each item.
#[allow(clippy::too_many_lines)]
pub(super) async fn upload_fewshots(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    datasource: &str,
    file: &str,
) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("Failed to read file '{file}': {e}"))?;

    // Detect format by file extension
    let ext = std::path::Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let items: Vec<Value> = if ext == "csv" || ext == "tsv" {
        parse_fewshots_csv(&content, file)?
    } else {
        serde_json::from_str(&content).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in '{file}': {e}"),
                r#"Expected JSON format: [{"question":"...","query":"..."}] or use .csv file with question,query columns"#,
            )
        })?
    };

    if items.is_empty() {
        return Err(
            FabioError::invalid_input("File contains no few-shot examples (empty array)").into(),
        );
    }

    // Validate all entries have question + query
    for (i, item) in items.iter().enumerate() {
        if item.get("question").and_then(Value::as_str).is_none() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Item {i} is missing 'question' field"),
                r#"Each item must have: {{"question":"...", "query":"..."}}"#,
            )
            .into());
        }
        if item.get("query").and_then(Value::as_str).is_none() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Item {i} is missing 'query' field"),
                r#"Each item must have: {{"question":"...", "query":"..."}}"#,
            )
            .into());
        }
    }

    if output::dry_run_guard(
        cli,
        "data-agent upload-fewshots",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "datasource": datasource,
            "file": file,
            "count": items.len(),
        }),
    ) {
        return Ok(());
    }

    let ds_id = resolve_datasource_id(client, workspace, id, datasource).await?;
    let fewshots_url =
        format!("/workspaces/{workspace}/dataAgents/{id}/staging/datasources/{ds_id}/fewshots");

    let mut added = 0;
    let mut errors = 0;

    for item in &items {
        let question = item
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let query_text = item
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let body = serde_json::json!({
            "question": question,
            "query": query_text,
        });

        match client.post(&fewshots_url, &body, false).await {
            Ok(_) => added += 1,
            Err(_) => errors += 1,
        }
    }

    let result = serde_json::json!({
        "status": "fewshots_uploaded",
        "added": added,
        "errors": errors,
        "total": added,
    });
    output::render_object(cli, &result, "status");
    Ok(())
}

// ─── Private Helpers ─────────────────────────────────────────────────────────

/// Parse few-shot examples from a CSV/TSV file.
///
/// Expects columns named `question` and `query` (case-insensitive headers).
/// TSV is auto-detected from `.tsv` extension.
fn parse_fewshots_csv(content: &str, file: &str) -> Result<Vec<Value>> {
    let ext = std::path::Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let delimiter = if ext == "tsv" { b'\t' } else { b',' };

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .flexible(true)
        .from_reader(content.as_bytes());

    // Find column indices for "question" and "query" (case-insensitive)
    let headers = reader.headers().map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Failed to parse CSV headers in '{file}': {e}"),
            "CSV must have a header row with 'question' and 'query' columns",
        )
    })?;

    let question_idx = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("question"));
    let query_idx = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("query") || h.eq_ignore_ascii_case("answer"));

    let question_idx = question_idx.ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("CSV file '{file}' is missing a 'question' column header"),
            format!(
                "Found columns: {}. Expected: question,query",
                headers.iter().collect::<Vec<_>>().join(", ")
            ),
        )
    })?;
    let query_idx = query_idx.ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("CSV file '{file}' is missing a 'query' (or 'answer') column header"),
            format!(
                "Found columns: {}. Expected: question,query",
                headers.iter().collect::<Vec<_>>().join(", ")
            ),
        )
    })?;

    let mut items = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let record = record.map_err(|e| {
            FabioError::new(
                ErrorCode::InvalidInput,
                format!("Failed to parse CSV row {i} in '{file}': {e}"),
            )
        })?;

        let question = record.get(question_idx).unwrap_or("").trim();
        let query = record.get(query_idx).unwrap_or("").trim();

        if question.is_empty() || query.is_empty() {
            continue; // Skip empty rows
        }

        items.push(serde_json::json!({
            "question": question,
            "query": query,
        }));
    }

    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_csv_fewshots_basic() {
        let csv = "question,query\nHow many?,SELECT COUNT(*) FROM t\nMax price?,SELECT MAX(price) FROM p\n";
        let items = parse_fewshots_csv(csv, "test.csv").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["question"], "How many?");
        assert_eq!(items[0]["query"], "SELECT COUNT(*) FROM t");
        assert_eq!(items[1]["question"], "Max price?");
    }

    #[test]
    fn parse_csv_fewshots_case_insensitive_headers() {
        let csv = "Question,Query\nTest?,SELECT 1\n";
        let items = parse_fewshots_csv(csv, "test.csv").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["question"], "Test?");
    }

    #[test]
    fn parse_csv_fewshots_answer_column() {
        let csv = "question,answer\nHow many?,SELECT COUNT(*) FROM t\n";
        let items = parse_fewshots_csv(csv, "test.csv").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["query"], "SELECT COUNT(*) FROM t");
    }

    #[test]
    fn parse_csv_fewshots_tsv() {
        let tsv = "question\tquery\nHow many?\tSELECT COUNT(*) FROM t\n";
        let items = parse_fewshots_csv(tsv, "data.tsv").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["question"], "How many?");
    }

    #[test]
    fn parse_csv_fewshots_missing_question_column() {
        let csv = "prompt,query\nHow?,SELECT 1\n";
        let err = parse_fewshots_csv(csv, "bad.csv").unwrap_err();
        assert!(err.to_string().contains("question"));
    }

    #[test]
    fn parse_csv_fewshots_skips_empty_rows() {
        let csv =
            "question,query\nHow many?,SELECT COUNT(*) FROM t\n,\nMax?,SELECT MAX(x) FROM y\n";
        let items = parse_fewshots_csv(csv, "test.csv").unwrap();
        assert_eq!(items.len(), 2);
    }
}
