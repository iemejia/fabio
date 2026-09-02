//! Cosmos DB data-plane: document operations (query, bulk import, single-document
//! CRUD, and export).

use std::io::{Read, Write};

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;
use crate::parallel::{self, BatchSummary};

use super::data_plane::CosmosClient;

const QUERY_EXAMPLE: &str = "fabio cosmos-db-database query --workspace <WS> --id <ID> --container products --query-text \"SELECT * FROM c WHERE c.price > 100\"";

#[allow(clippy::too_many_arguments)]
pub(super) async fn query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    container: &str,
    query_text: Option<&str>,
    parameters: &[String],
    partition_key: Option<&str>,
    max_item_count: Option<u32>,
    endpoint: Option<&str>,
) -> Result<()> {
    let text = crate::commands::query_input::resolve_query_input(
        query_text,
        "Cosmos NoSQL query",
        "--query-text",
        QUERY_EXAMPLE,
    )?;
    let params = parse_parameters(parameters)?;
    let pk = partition_key.map(cli_partition_value);

    let cosmos = CosmosClient::connect(client, workspace, id, endpoint).await?;

    // Follow continuation tokens until the (optional) --limit is satisfied or the
    // result set is exhausted. --all fetches every page.
    let mut documents: Vec<Value> = Vec::new();
    let mut total_ru = 0.0_f64;
    let mut continuation: Option<String> = None;
    loop {
        let resp = cosmos
            .query(
                container,
                &text,
                &params,
                pk.as_ref(),
                max_item_count,
                continuation.as_deref(),
            )
            .await?;
        if let Some(ru) = resp.request_charge {
            total_ru += ru;
        }
        if let Some(arr) = resp.body.get("Documents").and_then(Value::as_array) {
            documents.extend(arr.iter().cloned());
        }
        continuation = resp.continuation;
        let reached_limit = cli.limit.is_some_and(|l| documents.len() >= l);
        if continuation.is_none() || (!cli.all && (reached_limit || cli.limit.is_none())) {
            break;
        }
    }

    if cli.verbose {
        eprintln!("[cosmos] query consumed {total_ru:.2} RU");
    }
    // Derive table columns from the first document's top-level keys so `-o table`
    // reflects the projection. JSON output (default) always carries full documents.
    let columns: Vec<String> = documents
        .first()
        .and_then(Value::as_object)
        .map_or_else(|| vec!["id".to_string()], |o| o.keys().cloned().collect());
    let col_refs: Vec<&str> = columns.iter().map(String::as_str).collect();
    let headers: Vec<String> = columns.iter().map(|c| c.to_uppercase()).collect();
    let header_refs: Vec<&str> = headers.iter().map(String::as_str).collect();
    output::render_list(cli, &documents, &col_refs, &header_refs, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn import(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    container: &str,
    source: Option<&str>,
    format: &str,
    mode: &str,
    concurrency: Option<usize>,
    continue_on_error: bool,
    endpoint: Option<&str>,
) -> Result<()> {
    let upsert = normalize_mode(mode)?;
    if client.is_readonly() {
        return Err(FabioError::with_hint(
            ErrorCode::ReadonlyMode,
            "Blocked cosmos-db-database import — readonly mode is active".to_string(),
            "Remove --readonly flag or set FABIO_READONLY=0 to allow document writes.",
        )
        .into());
    }
    let raw = read_source(source)?;
    let documents = parse_documents(&raw, format)?;
    if documents.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "No documents found in the import source.".to_string(),
            "Provide a JSONL file (one JSON object per line) or a JSON array via --source or stdin.",
        )
        .into());
    }

    if output::dry_run_guard(
        cli,
        "cosmos-db-database import",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "container": container,
            "documentCount": documents.len(),
            "mode": if upsert { "upsert" } else { "insert" },
        }),
    ) {
        return Ok(());
    }

    let cosmos = CosmosClient::connect(client, workspace, id, endpoint).await?;

    // Resolve the container's partition-key path once so each document's key can
    // be derived automatically.
    let pk_path = resolve_partition_key_path(&cosmos, container).await?;

    // Pair each document with its extracted partition-key value up front so a
    // missing key is reported deterministically (not as a racy parallel error).
    let mut prepared: Vec<(Value, Value)> = Vec::with_capacity(documents.len());
    let mut names: Vec<String> = Vec::with_capacity(documents.len());
    let mut early_failures: Vec<parallel::FailureDetail> = Vec::new();
    for (i, doc) in documents.into_iter().enumerate() {
        let label = doc
            .get("id")
            .and_then(Value::as_str)
            .map_or_else(|| format!("document[{i}]"), ToString::to_string);
        if let Some(pk) = extract_partition_key(&doc, &pk_path) {
            prepared.push((doc, pk));
            names.push(label);
        } else {
            if !continue_on_error {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!(
                        "Document {label} is missing partition-key path '{pk_path}' required by container '{container}'."
                    ),
                    "Ensure every document contains the container's partition-key field, or pass --continue-on-error to skip invalid rows.",
                )
                .into());
            }
            early_failures.push(parallel::FailureDetail {
                item: label,
                error: format!("missing partition-key path '{pk_path}'"),
                code: ErrorCode::InvalidInput.to_string(),
            });
        }
    }

    let concurrency = concurrency
        .unwrap_or_else(parallel::default_concurrency)
        .max(1);
    let container = container.to_string();
    let cosmos = std::sync::Arc::new(cosmos);
    let results = parallel::execute_parallel(prepared, concurrency, move |(doc, pk)| {
        let cosmos = cosmos.clone();
        let container = container.clone();
        async move {
            cosmos
                .write_document(&container, &doc, &pk, upsert)
                .await
                .map(|_| ())
        }
    })
    .await;

    let mut summary = BatchSummary::from_results(&results, &names);
    // Fold in the documents skipped before dispatch (missing partition key).
    summary.total += early_failures.len();
    summary.failed += early_failures.len();
    summary.failures.extend(early_failures);

    render_import_result(cli, &summary, if upsert { "upserted" } else { "inserted" })
}

fn render_import_result(cli: &Cli, summary: &BatchSummary, verb: &str) -> Result<()> {
    if summary.all_succeeded() {
        let obj = serde_json::json!({
            "documentsImported": summary.succeeded,
            "mode": verb,
            "status": "imported",
        });
        output::render_object(cli, &obj, "status");
        Ok(())
    } else {
        let obj = serde_json::json!({
            "documentsImported": summary.succeeded,
            "documentsFailed": summary.failed,
            "failures": summary.failures,
            "mode": verb,
            "status": "partial_failure",
        });
        output::render_object(cli, &obj, "status");
        Err(FabioError::with_hint(
            ErrorCode::ApiError,
            format!(
                "Import partially failed: {}/{} documents {verb}",
                summary.succeeded, summary.total
            ),
            "Inspect the 'failures' array. Re-run with the same --source to retry (upsert is idempotent).",
        )
        .into())
    }
}

// ── Single-document CRUD ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn get_document(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    container: &str,
    document_id: &str,
    partition_key: &str,
    endpoint: Option<&str>,
) -> Result<()> {
    let pk = cli_partition_value(partition_key);
    let cosmos = CosmosClient::connect(client, workspace, id, endpoint).await?;
    let resp = cosmos.read_document(container, document_id, &pk).await?;
    output::render_object(cli, &resp.body, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn create_document(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    container: &str,
    file: Option<&str>,
    content: Option<&str>,
    partition_key: Option<&str>,
    mode: &str,
    endpoint: Option<&str>,
) -> Result<()> {
    let upsert = normalize_mode(mode)?;
    let document = read_single_document(file, content)?;

    if output::dry_run_guard(
        cli,
        "cosmos-db-database create-document",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "container": container,
            "documentId": document.get("id"),
            "mode": if upsert { "upsert" } else { "insert" },
        }),
    ) {
        return Ok(());
    }

    let cosmos = CosmosClient::connect(client, workspace, id, endpoint).await?;

    // Resolve the partition-key value: explicit --partition-key wins, else derive
    // it from the document using the container's partition-key path.
    let pk = if let Some(raw) = partition_key {
        cli_partition_value(raw)
    } else {
        let pk_path = resolve_partition_key_path(&cosmos, container).await?;
        extract_partition_key(&document, &pk_path).ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!(
                    "Document is missing partition-key path '{pk_path}' required by container '{container}'."
                ),
                "Include the partition-key field in the document, or pass --partition-key explicitly.",
            )
        })?
    };

    let resp = cosmos
        .write_document(container, &document, &pk, upsert)
        .await?;
    output::render_object(cli, &resp.body, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn delete_document(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    container: &str,
    document_id: &str,
    partition_key: &str,
    endpoint: Option<&str>,
) -> Result<()> {
    if document_id.trim().is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Document id must not be empty for deletion.".to_string(),
            "Provide the exact document id. Example: fabio cosmos-db-database delete-document --container products --document-id p1 --partition-key electronics",
        )
        .into());
    }
    if output::dry_run_guard(
        cli,
        "cosmos-db-database delete-document",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "container": container,
            "documentId": document_id,
            "partitionKey": partition_key,
        }),
    ) {
        return Ok(());
    }
    let pk = cli_partition_value(partition_key);
    let cosmos = CosmosClient::connect(client, workspace, id, endpoint).await?;
    cosmos.delete_document(container, document_id, &pk).await?;
    let obj = serde_json::json!({ "documentId": document_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ── Export ───────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn export(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    container: &str,
    query_text: Option<&str>,
    output_file: Option<&str>,
    endpoint: Option<&str>,
) -> Result<()> {
    let text = query_text.unwrap_or("SELECT * FROM c").to_string();
    let cosmos = CosmosClient::connect(client, workspace, id, endpoint).await?;

    // Page through the full result set (export is unbounded by design; --limit
    // still caps it for previews).
    let mut documents: Vec<Value> = Vec::new();
    let mut continuation: Option<String> = None;
    loop {
        let resp = cosmos
            .query(container, &text, &[], None, None, continuation.as_deref())
            .await?;
        if let Some(arr) = resp.body.get("Documents").and_then(Value::as_array) {
            documents.extend(arr.iter().cloned());
        }
        continuation = resp.continuation;
        let reached_limit = cli.limit.is_some_and(|l| documents.len() >= l);
        if continuation.is_none() || reached_limit {
            break;
        }
    }
    if let Some(limit) = cli.limit {
        documents.truncate(limit);
    }

    // Serialize as JSONL (one JSON object per line). Cosmos system metadata
    // fields (_rid/_self/_etag/_attachments/_ts) are stripped so the export is
    // clean, re-importable user data.
    let mut jsonl = String::new();
    for doc in &documents {
        jsonl.push_str(&serde_json::to_string(&strip_system_fields(doc))?);
        jsonl.push('\n');
    }

    if let Some(path) = output_file {
        let mut f = std::fs::File::create(path).map_err(|e| {
            FabioError::new(
                ErrorCode::ApiError,
                format!("Failed to create export file '{path}': {e}"),
            )
        })?;
        f.write_all(jsonl.as_bytes()).map_err(|e| {
            FabioError::new(
                ErrorCode::ApiError,
                format!("Failed to write '{path}': {e}"),
            )
        })?;
        let obj = serde_json::json!({
            "documentsExported": documents.len(),
            "file": path,
            "status": "exported",
        });
        output::render_object(cli, &obj, "status");
    } else {
        // Stream JSONL to stdout for piping (two-way I/O). Bypass the envelope so
        // the output is directly consumable by `fabio ... import --source -` style
        // pipelines and jq.
        print!("{jsonl}");
    }
    Ok(())
}

// ── Pure helpers ────────────────────────────────────────────────────────────

/// Read the import source: a file path, or stdin when `None`.
fn read_source(source: Option<&str>) -> Result<String> {
    if let Some(path) = source {
        std::fs::read_to_string(path).map_err(|e| {
            FabioError::not_found(format!("Import source not found: {path}: {e}")).into()
        })
    } else {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).map_err(|e| {
            FabioError::new(ErrorCode::ApiError, format!("Failed to read stdin: {e}"))
        })?;
        Ok(buf)
    }
}

/// Read a single JSON document from `--file`, inline `--content`, or stdin.
fn read_single_document(file: Option<&str>, content: Option<&str>) -> Result<Value> {
    let raw = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| FabioError::not_found(format!("Document file not found: {path}: {e}")))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => read_source(None)?,
    };
    let value: Value = serde_json::from_str(raw.trim()).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid JSON document: {e}"),
            "Provide a single JSON object via --file, --content, or stdin.",
        )
    })?;
    if !value.is_object() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "A Cosmos document must be a JSON object.".to_string(),
            "Wrap the value in an object, e.g. {\"id\":\"1\",\"pk\":\"x\"}.",
        )
        .into());
    }
    Ok(value)
}

/// Parse import content into documents. `format` is `jsonl`, `json-array`, or
/// `auto` (array if the content starts with `[`, else JSONL).
pub(super) fn parse_documents(content: &str, format: &str) -> Result<Vec<Value>> {
    let effective = if format.eq_ignore_ascii_case("auto") {
        if content.trim_start().starts_with('[') {
            "json-array"
        } else {
            "jsonl"
        }
    } else {
        format
    };

    match effective.to_ascii_lowercase().as_str() {
        "jsonl" | "ndjson" => {
            let mut docs = Vec::new();
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let doc: Value = serde_json::from_str(trimmed).map_err(|e| {
                    FabioError::with_hint(
                        ErrorCode::InvalidInput,
                        format!("Invalid JSON on line {}: {e}", i + 1),
                        "Each non-empty line must be a single JSON object (JSONL/NDJSON).",
                    )
                })?;
                docs.push(doc);
            }
            Ok(docs)
        }
        "json-array" | "array" | "json" => {
            let parsed: Value = serde_json::from_str(content.trim()).map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Invalid JSON array: {e}"),
                    "The source must be a JSON array of objects, or use --format jsonl.",
                )
            })?;
            match parsed {
                Value::Array(a) => Ok(a),
                other => Ok(vec![other]),
            }
        }
        other => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Unknown import format: {other}"),
            "Valid values: jsonl, json-array, auto.",
        )
        .into()),
    }
}

/// Extract the partition-key value from a document given a Cosmos key path
/// (e.g. `/categoryId` or nested `/address/zip`). Returns `None` if any
/// segment is absent.
pub(super) fn extract_partition_key(doc: &Value, pk_path: &str) -> Option<Value> {
    let mut current = doc;
    for segment in pk_path.split('/').filter(|s| !s.is_empty()) {
        current = current.get(segment)?;
    }
    Some(current.clone())
}

/// Strip Cosmos server-generated system metadata fields from a document so an
/// export is clean, re-importable user data. Leaves `id` and all user fields.
fn strip_system_fields(doc: &Value) -> Value {
    const SYSTEM_FIELDS: [&str; 5] = ["_rid", "_self", "_etag", "_attachments", "_ts"];
    if let Value::Object(map) = doc {
        let cleaned: serde_json::Map<String, Value> = map
            .iter()
            .filter(|(k, _)| !SYSTEM_FIELDS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Value::Object(cleaned)
    } else {
        doc.clone()
    }
}

/// Interpret a `--partition-key` CLI string: a bare number/bool/null is used as
/// its JSON value; anything else is treated as a string.
pub(super) fn cli_partition_value(raw: &str) -> Value {
    serde_json::from_str::<Value>(raw)
        .ok()
        .filter(|v| v.is_number() || v.is_boolean() || v.is_null())
        .unwrap_or_else(|| Value::from(raw))
}

/// Normalize the `--mode` flag to an upsert boolean. `upsert` → true,
/// `insert`/`create` → false.
pub(super) fn normalize_mode(mode: &str) -> Result<bool> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "upsert" => Ok(true),
        "insert" | "create" => Ok(false),
        other => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --mode: {other}"),
            "Valid values: upsert (default, idempotent), insert.",
        )
        .into()),
    }
}

/// Parse `key=value` query parameters into Cosmos `{name:"@key", value:...}` form.
/// The value is JSON-parsed when possible (numbers/bools), else kept as a string.
fn parse_parameters(params: &[String]) -> Result<Vec<Value>> {
    let mut out = Vec::with_capacity(params.len());
    for p in params {
        let (name, raw) = p.split_once('=').ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --parameter (expected name=value): {p}"),
                "Example: --parameter min=100 --parameter category=electronics",
            )
        })?;
        let name = if name.starts_with('@') {
            name.to_string()
        } else {
            format!("@{name}")
        };
        let value = serde_json::from_str::<Value>(raw)
            .ok()
            .filter(|v| v.is_number() || v.is_boolean() || v.is_null())
            .unwrap_or_else(|| Value::from(raw));
        out.push(serde_json::json!({ "name": name, "value": value }));
    }
    Ok(out)
}

/// Resolve a container's partition-key path (first entry of `partitionKey.paths`).
async fn resolve_partition_key_path(cosmos: &CosmosClient, container: &str) -> Result<String> {
    let resp = cosmos.get_container(container).await?;
    resp.body
        .get("partitionKey")
        .and_then(|pk| pk.get("paths"))
        .and_then(Value::as_array)
        .and_then(|paths| paths.first())
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                format!("Could not determine partition-key path for container '{container}'."),
            )
            .into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_documents_jsonl() {
        let content = "{\"id\":\"a\"}\n\n{\"id\":\"b\"}\n";
        let docs = parse_documents(content, "jsonl").unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[1]["id"], "b");
    }

    #[test]
    fn parse_documents_json_array() {
        let docs = parse_documents("[{\"id\":\"a\"},{\"id\":\"b\"}]", "json-array").unwrap();
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn parse_documents_auto_detects_array_vs_jsonl() {
        assert_eq!(parse_documents("  [ {\"x\":1} ]", "auto").unwrap().len(), 1);
        assert_eq!(
            parse_documents("{\"x\":1}\n{\"x\":2}", "auto")
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn parse_documents_reports_bad_line() {
        let err = parse_documents("{\"ok\":1}\nnot json", "jsonl").unwrap_err();
        assert!(err.to_string().contains("line 2"), "got: {err}");
    }

    #[test]
    fn extract_partition_key_top_level_and_nested() {
        let doc = serde_json::json!({"categoryId": "electronics", "address": {"zip": 90210}});
        assert_eq!(
            extract_partition_key(&doc, "/categoryId"),
            Some(Value::from("electronics"))
        );
        assert_eq!(
            extract_partition_key(&doc, "/address/zip"),
            Some(Value::from(90210))
        );
        assert_eq!(extract_partition_key(&doc, "/missing"), None);
    }

    #[test]
    fn cli_partition_value_types() {
        assert_eq!(
            cli_partition_value("electronics"),
            Value::from("electronics")
        );
        assert_eq!(cli_partition_value("42"), Value::from(42));
        assert_eq!(cli_partition_value("true"), Value::from(true));
        // A quoted-looking string stays a string.
        assert_eq!(cli_partition_value("00123"), Value::from("00123"));
    }

    #[test]
    fn normalize_mode_values() {
        assert!(normalize_mode("upsert").unwrap());
        assert!(!normalize_mode("INSERT").unwrap());
        assert!(normalize_mode("replace").is_err());
    }

    #[test]
    fn strip_system_fields_removes_cosmos_metadata() {
        let doc = serde_json::json!({
            "id": "p1", "sku": "A", "qty": 5,
            "_rid": "x", "_self": "y", "_etag": "z", "_attachments": "a", "_ts": 1
        });
        let cleaned = strip_system_fields(&doc);
        assert_eq!(cleaned, serde_json::json!({"id":"p1","sku":"A","qty":5}));
    }

    #[test]
    fn strip_system_fields_passthrough_non_object() {
        assert_eq!(strip_system_fields(&Value::from(5)), Value::from(5));
    }

    #[test]
    fn read_single_document_rejects_non_object() {
        let err = read_single_document(None, Some("[1,2,3]")).unwrap_err();
        assert!(
            err.to_string().contains("must be a JSON object"),
            "got: {err}"
        );
    }

    #[test]
    fn read_single_document_parses_inline_object() {
        let doc = read_single_document(None, Some("{\"id\":\"a\",\"pk\":\"x\"}")).unwrap();
        assert_eq!(doc["id"], "a");
    }

    #[test]
    fn parse_parameters_adds_at_and_types() {
        let params =
            parse_parameters(&["min=100".to_string(), "cat=electronics".to_string()]).unwrap();
        assert_eq!(params[0]["name"], "@min");
        assert_eq!(params[0]["value"], Value::from(100));
        assert_eq!(params[1]["name"], "@cat");
        assert_eq!(params[1]["value"], Value::from("electronics"));
    }
}
