use anyhow::Result;
use mssql_tds::connection::client_context::{ClientContext, TdsAuthenticationMethod};
use mssql_tds::connection_provider::tds_connection_provider::TdsConnectionProvider;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::query::resolve_sql_connection;

// ─── Type inference ──────────────────────────────────────────────────────────

/// Inferred SQL column type from data inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InferredType {
    Unknown, // Not yet observed any non-empty value
    Int,
    BigInt,
    Float,
    Bit,
    Date,
    NVarChar(usize), // max observed length
}

impl InferredType {
    fn to_sql(&self) -> String {
        match self {
            Self::Unknown => "NVARCHAR(200)".to_string(),
            Self::Int => "INT".to_string(),
            Self::BigInt => "BIGINT".to_string(),
            Self::Float => "FLOAT".to_string(),
            Self::Bit => "BIT".to_string(),
            Self::Date => "DATE".to_string(),
            Self::NVarChar(len) => {
                // Use at least 50, or 2x observed length, cap at MAX
                let size = (*len * 2).clamp(50, 4000);
                format!("NVARCHAR({size})")
            }
        }
    }

    /// Widen type when conflicting values are seen.
    fn widen(&self, other: &Self) -> Self {
        match (self, other) {
            // Unknown takes any type from first observation
            (Self::Unknown, b) => b.clone(),
            (a, Self::Unknown) => a.clone(),
            (a, b) if a == b => a.clone(),
            // Int + BigInt → BigInt
            (Self::Int, Self::BigInt) | (Self::BigInt, Self::Int) => Self::BigInt,
            // Int/BigInt + Float → Float
            (Self::Int | Self::BigInt, Self::Float) | (Self::Float, Self::Int | Self::BigInt) => {
                Self::Float
            }
            // Anything + NVarChar → NVarChar (take max length)
            (Self::NVarChar(a), Self::NVarChar(b)) => Self::NVarChar(*a.max(b)),
            (Self::NVarChar(a), _) => Self::NVarChar(*a),
            (_, Self::NVarChar(b)) => Self::NVarChar(*b),
            // Fallback: wider type wins
            _ => Self::NVarChar(100),
        }
    }
}

/// Infer SQL type from a string value.
fn infer_type_from_str(val: &str) -> InferredType {
    if val.is_empty() {
        return InferredType::Unknown;
    }
    // Try integer
    if val.parse::<i32>().is_ok() {
        return InferredType::Int;
    }
    if val.parse::<i64>().is_ok() {
        return InferredType::BigInt;
    }
    // Try float
    if val.parse::<f64>().is_ok() {
        return InferredType::Float;
    }
    // Try boolean
    if val.eq_ignore_ascii_case("true") || val.eq_ignore_ascii_case("false") {
        return InferredType::Bit;
    }
    // Try date (YYYY-MM-DD)
    if val.len() == 10
        && val.chars().nth(4) == Some('-')
        && val.chars().nth(7) == Some('-')
        && val[..4].parse::<u16>().is_ok()
        && val[5..7].parse::<u8>().is_ok()
        && val[8..10].parse::<u8>().is_ok()
    {
        return InferredType::Date;
    }
    InferredType::NVarChar(val.len())
}

/// Infer SQL type from a JSON value.
fn infer_type_from_json(val: &Value) -> InferredType {
    match val {
        Value::Null => InferredType::Unknown,
        Value::Bool(_) => InferredType::Bit,
        Value::Number(n) => n.as_i64().map_or(InferredType::Float, |i| {
            if i32::try_from(i).is_ok() {
                InferredType::Int
            } else {
                InferredType::BigInt
            }
        }),
        Value::String(s) => infer_type_from_str(s),
        _ => InferredType::NVarChar(200), // arrays/objects → serialize as string
    }
}

// ─── SQL formatting helpers ──────────────────────────────────────────────────

/// Escape a SQL string value (strip null bytes and double single quotes).
fn sql_escape(val: &str) -> String {
    val.replace('\0', "").replace('\'', "''")
}

/// Format a value as a SQL literal.
fn value_to_sql_literal(val: &str, col_type: &InferredType) -> String {
    if val.is_empty() {
        return "NULL".to_string();
    }
    match col_type {
        InferredType::Int | InferredType::BigInt | InferredType::Float => {
            // Validate it's actually numeric, fallback to NULL
            if val.parse::<f64>().is_ok() {
                val.to_string()
            } else {
                "NULL".to_string()
            }
        }
        InferredType::Bit => {
            if val.eq_ignore_ascii_case("true") || val == "1" {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        InferredType::Unknown | InferredType::Date | InferredType::NVarChar(_) => {
            format!("N'{}'", sql_escape(val))
        }
    }
}

/// Format a JSON value as a SQL literal.
fn json_value_to_sql_literal(val: &Value, col_type: &InferredType) -> String {
    match val {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.is_empty() {
                "NULL".to_string()
            } else {
                match col_type {
                    InferredType::Int | InferredType::BigInt | InferredType::Float => {
                        if s.parse::<f64>().is_ok() {
                            s.clone()
                        } else {
                            "NULL".to_string()
                        }
                    }
                    _ => format!("N'{}'", sql_escape(s)),
                }
            }
        }
        _ => {
            // Serialize complex types to string
            let s = val.to_string();
            format!("N'{}'", sql_escape(&s))
        }
    }
}

// ─── File reading & schema inference ─────────────────────────────────────────

/// Schema inference result for CSV files: (`column_names`, `inferred_types`, `rows_as_strings`).
type CsvSchema = (Vec<String>, Vec<InferredType>, Vec<Vec<String>>);

/// Read CSV file and return (columns, types, rows as Vec<Vec<String>>).
fn read_csv_file(path: &str) -> Result<CsvSchema> {
    let mut reader = csv::Reader::from_path(path).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Cannot read CSV file: {e}"),
            format!("Verify the file exists at: {path}"),
        )
    })?;

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| FabioError::new(ErrorCode::InvalidInput, format!("Invalid CSV headers: {e}")))?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();

    if headers.is_empty() {
        return Err(FabioError::new(ErrorCode::InvalidInput, "CSV file has no columns").into());
    }

    let mut col_types: Vec<InferredType> = vec![InferredType::Unknown; headers.len()];
    let mut rows: Vec<Vec<String>> = Vec::new();

    for result in reader.records() {
        let record = result.map_err(|e| {
            FabioError::new(ErrorCode::InvalidInput, format!("Invalid CSV row: {e}"))
        })?;

        let row: Vec<String> = record.iter().map(|v| v.trim().to_string()).collect();
        // Infer types from this row
        for (i, val) in row.iter().enumerate() {
            if i < col_types.len() && !val.is_empty() {
                let inferred = infer_type_from_str(val);
                col_types[i] = col_types[i].widen(&inferred);
            }
        }
        rows.push(row);
    }

    Ok((headers, col_types, rows))
}

/// Schema inference result for JSON files: (`column_names`, `inferred_types`, `rows_as_json_values`).
type JsonSchema = (Vec<String>, Vec<InferredType>, Vec<Vec<Value>>);

/// Read JSON file (array of objects) and return (columns, types, rows as Vec<Vec<Value>>).
fn read_json_file(path: &str) -> Result<JsonSchema> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::NotFound,
            format!("Cannot read JSON file: {e}"),
            format!("Verify the file exists at: {path}"),
        )
    })?;

    let array: Vec<Value> = serde_json::from_str(&content).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid JSON: {e}"),
            "Expected a JSON array of objects, e.g. [{\"col1\": \"val1\", ...}, ...]".to_string(),
        )
    })?;

    if array.is_empty() {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            "JSON array is empty — nothing to import",
        )
        .into());
    }

    // Collect all unique keys in order of first appearance
    let mut columns: Vec<String> = Vec::new();
    for obj in &array {
        if let Value::Object(map) = obj {
            for key in map.keys() {
                if !columns.contains(key) {
                    columns.push(key.clone());
                }
            }
        }
    }

    if columns.is_empty() {
        return Err(FabioError::new(ErrorCode::InvalidInput, "JSON objects have no keys").into());
    }

    // Infer types and collect rows
    let mut col_types: Vec<InferredType> = vec![InferredType::Unknown; columns.len()];
    let mut rows: Vec<Vec<Value>> = Vec::new();

    for obj in &array {
        if let Value::Object(map) = obj {
            let mut row: Vec<Value> = Vec::with_capacity(columns.len());
            for (i, col) in columns.iter().enumerate() {
                let val = map.get(col).unwrap_or(&Value::Null);
                if !val.is_null() {
                    let inferred = infer_type_from_json(val);
                    col_types[i] = col_types[i].widen(&inferred);
                }
                row.push(val.clone());
            }
            rows.push(row);
        }
    }

    Ok((columns, col_types, rows))
}

// ─── SQL generation ──────────────────────────────────────────────────────────

/// Sanitize a table name for SQL (bracket-quote).
fn sanitize_table_name(name: &str) -> String {
    // Remove dangerous chars, bracket-quote
    let clean: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == ' ' || *c == '-')
        .collect();
    format!("[{clean}]")
}

/// Derive table name from file path.
fn table_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported_data")
        .to_string()
}

/// Generate CREATE TABLE DDL.
fn generate_create_table(table: &str, columns: &[String], types: &[InferredType]) -> String {
    let col_defs: Vec<String> = columns
        .iter()
        .zip(types.iter())
        .map(|(name, typ)| {
            let safe_col = format!("[{}]", name.replace(']', "]]"));
            format!("    {safe_col} {} NULL", typ.to_sql())
        })
        .collect();

    format!("CREATE TABLE {table} (\n{}\n);", col_defs.join(",\n"))
}

/// Generate batched INSERT statements for CSV data.
fn generate_csv_insert_batches(
    table: &str,
    columns: &[String],
    types: &[InferredType],
    rows: &[Vec<String>],
    batch_size: usize,
) -> Vec<(String, usize)> {
    let col_list: String = columns
        .iter()
        .map(|c| format!("[{}]", c.replace(']', "]]")))
        .collect::<Vec<_>>()
        .join(", ");

    rows.chunks(batch_size)
        .map(|chunk| {
            let values: Vec<String> = chunk
                .iter()
                .map(|row| {
                    let vals: Vec<String> = row
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            let col_type = types.get(i).unwrap_or(&InferredType::NVarChar(200));
                            value_to_sql_literal(v, col_type)
                        })
                        .collect();
                    format!("({})", vals.join(", "))
                })
                .collect();

            let sql = format!(
                "INSERT INTO {table} ({col_list}) VALUES\n{};",
                values.join(",\n")
            );
            (sql, chunk.len())
        })
        .collect()
}

/// Generate batched INSERT statements for JSON data.
fn generate_json_insert_batches(
    table: &str,
    columns: &[String],
    types: &[InferredType],
    rows: &[Vec<Value>],
    batch_size: usize,
) -> Vec<(String, usize)> {
    let col_list: String = columns
        .iter()
        .map(|c| format!("[{}]", c.replace(']', "]]")))
        .collect::<Vec<_>>()
        .join(", ");

    rows.chunks(batch_size)
        .map(|chunk| {
            let values: Vec<String> = chunk
                .iter()
                .map(|row| {
                    let vals: Vec<String> = row
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            let col_type = types.get(i).unwrap_or(&InferredType::NVarChar(200));
                            json_value_to_sql_literal(v, col_type)
                        })
                        .collect();
                    format!("({})", vals.join(", "))
                })
                .collect();

            let sql = format!(
                "INSERT INTO {table} ({col_list}) VALUES\n{};",
                values.join(",\n")
            );
            (sql, chunk.len())
        })
        .collect()
}

// ─── Import command ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn import(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: &str,
    table: Option<&str>,
    no_create_table: bool,
    drop_if_exists: bool,
    batch_size: usize,
) -> Result<()> {
    crate::client::validate_uuid(workspace, "--workspace")?;
    crate::client::validate_uuid(id, "--id")?;
    validate_import_mode(no_create_table, drop_if_exists)?;
    // Determine table name
    let table_name = table.map_or_else(|| table_name_from_path(file), ToString::to_string);
    let safe_table = sanitize_table_name(&table_name);

    // Detect format from file extension
    let ext = std::path::Path::new(file)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Parse file and generate SQL
    let (columns, col_types, sql_batches) = match ext.as_str() {
        "csv" => {
            let (columns, col_types, rows) = read_csv_file(file)?;
            let batches =
                generate_csv_insert_batches(&safe_table, &columns, &col_types, &rows, batch_size);
            (columns, col_types, batches)
        }
        "json" => {
            let (columns, col_types, rows) = read_json_file(file)?;
            let batches =
                generate_json_insert_batches(&safe_table, &columns, &col_types, &rows, batch_size);
            (columns, col_types, batches)
        }
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Unsupported file format: .{ext}"),
                "Supported formats: .csv, .json".to_string(),
            )
            .into());
        }
    };

    let total_rows = sql_batches.iter().map(|b| b.1).sum::<usize>();

    // Generate CREATE TABLE DDL
    let create_ddl = generate_create_table(&safe_table, &columns, &col_types);
    let drop_ddl = format!("DROP TABLE IF EXISTS {safe_table};");

    // Show dry-run info
    let dry_body = serde_json::json!({
        "workspace":workspace,"id":id,"target":safe_table,
        "table": table_name,
        "file": file,
        "format": ext,
        "columns": columns,
        "total_rows": total_rows,
        "batch_count": sql_batches.len(),
        "create_table_ddl": create_ddl,
        "drop_if_exists": drop_if_exists,
        "drop_table_ddl":drop_if_exists.then_some(&drop_ddl),
    });
    if output::dry_run_guard_maybe_destructive(
        cli,
        "sql-database import",
        &dry_body,
        drop_if_exists,
    ) {
        return Ok(());
    }

    if cli.readonly {
        return Err(FabioError::new(
            ErrorCode::ReadonlyMode,
            "SQL data import is blocked by --readonly; use --dry-run to preview.",
        )
        .into());
    }

    // Connect via TDS
    let (server, port, database) = resolve_sql_connection(client, workspace, id).await?;
    let token = client.require_sql_auth().await?;

    let data_source = format!("tcp:{server},{port}");
    let mut context = ClientContext::with_data_source(&data_source);
    context.database = database;
    context.tds_authentication_method = TdsAuthenticationMethod::AccessToken;
    context.access_token = Some(token);
    context.application_name = "fabio".to_string();
    context.connect_timeout = 30;

    let provider = TdsConnectionProvider {};
    let mut tds_client = provider
        .create_client(context, &data_source, None)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            let hint = if msg.contains("18456") {
                ". Hint: Fabric SQL Database requires F4+ capacity. On F2, TDS connections \
                 fail with 'Validation of user's permissions failed' due to insufficient \
                 compute, not actual permissions issues. Scale your capacity to F4 or higher."
            } else {
                ""
            };
            FabioError::new(
                ErrorCode::ApiError,
                format!("TDS connection failed: {e}{hint}"),
            )
        })?;

    // Execute DROP TABLE if requested
    if drop_if_exists {
        tds_client
            .execute(drop_ddl.clone(), Some(60), None)
            .await
            .map_err(|e| {
                FabioError::new(
                    ErrorCode::ApiError,
                    format!("Failed to drop existing table: {e}"),
                )
            })?;
        tds_client.close_query().await.ok();
    }

    // Execute CREATE TABLE
    if !no_create_table {
        tds_client
            .execute(create_ddl.clone(), Some(60), None)
            .await
            .map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::ApiError,
                    format!("Failed to create table: {e}"),
                    if drop_if_exists {
                        String::new()
                    } else {
                        "If the table already exists, use --drop-if-exists or --no-create-table"
                            .to_string()
                    },
                )
            })?;
        tds_client.close_query().await.ok();
    }

    // Execute INSERT batches
    let mut inserted = 0usize;
    for (batch_sql, batch_rows) in &sql_batches {
        tds_client
            .execute(batch_sql.clone(), Some(120), None)
            .await
            .map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::ApiError,
                    format!("INSERT failed at row {inserted}: {e}"),
                    format!("Successfully inserted {inserted}/{total_rows} rows before failure"),
                )
            })?;
        tds_client.close_query().await.ok();
        inserted += batch_rows;
    }

    // Render success output
    let result = serde_json::json!({
        "table": table_name,
        "rows_inserted": inserted,
        "columns": columns.len(),
        "file": file,
        "destructive":drop_if_exists,
        "warnings":if drop_if_exists {vec!["--drop-if-exists was active: any existing table/data was replaced without a backup."]} else {Vec::new()},
        "message": format!("Successfully imported {inserted} rows into {safe_table}")
    });
    output::render_object(cli, &result, "message");
    Ok(())
}

fn validate_import_mode(no_create_table: bool, drop_if_exists: bool) -> Result<()> {
    if drop_if_exists && no_create_table {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,
            "--drop-if-exists cannot be combined with --no-create-table: dropping the table would leave no table for the import.",
            "Review whether to replace the table or import into the existing table. No connection or drop was attempted.").into());
    }
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[test]
    fn drop_without_recreation_is_rejected_before_connection_or_preview() {
        for (no_create, drop_existing) in [(false, false), (false, true), (true, false)] {
            assert!(super::validate_import_mode(no_create, drop_existing).is_ok());
        }
        let error = super::validate_import_mode(true, true).unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<crate::errors::FabioError>()
                .unwrap()
                .code,
            crate::errors::ErrorCode::InvalidInput
        );
    }

    use super::*;

    #[test]
    fn infer_type_int() {
        assert_eq!(infer_type_from_str("42"), InferredType::Int);
        assert_eq!(infer_type_from_str("-1"), InferredType::Int);
    }

    #[test]
    fn infer_type_bigint() {
        assert_eq!(infer_type_from_str("9000000000"), InferredType::BigInt);
    }

    #[test]
    fn infer_type_float() {
        assert_eq!(infer_type_from_str("3.14"), InferredType::Float);
        assert_eq!(infer_type_from_str("1299.99"), InferredType::Float);
    }

    #[test]
    fn infer_type_date() {
        assert_eq!(infer_type_from_str("2024-01-15"), InferredType::Date);
    }

    #[test]
    fn infer_type_bool() {
        assert_eq!(infer_type_from_str("true"), InferredType::Bit);
        assert_eq!(infer_type_from_str("false"), InferredType::Bit);
    }

    #[test]
    fn infer_type_string() {
        assert_eq!(
            infer_type_from_str("hello world"),
            InferredType::NVarChar(11)
        );
    }

    #[test]
    fn infer_type_empty() {
        assert_eq!(infer_type_from_str(""), InferredType::Unknown);
    }

    #[test]
    fn widen_int_float() {
        assert_eq!(
            InferredType::Int.widen(&InferredType::Float),
            InferredType::Float
        );
    }

    #[test]
    fn widen_int_bigint() {
        assert_eq!(
            InferredType::Int.widen(&InferredType::BigInt),
            InferredType::BigInt
        );
    }

    #[test]
    fn widen_nvarchar_max_length() {
        assert_eq!(
            InferredType::NVarChar(10).widen(&InferredType::NVarChar(50)),
            InferredType::NVarChar(50)
        );
    }

    #[test]
    fn widen_int_nvarchar() {
        // Once we see a string in an int column, it becomes nvarchar
        assert_eq!(
            InferredType::Int.widen(&InferredType::NVarChar(20)),
            InferredType::NVarChar(20)
        );
    }

    #[test]
    fn widen_unknown_takes_first_type() {
        assert_eq!(
            InferredType::Unknown.widen(&InferredType::Int),
            InferredType::Int
        );
        assert_eq!(
            InferredType::Unknown.widen(&InferredType::Date),
            InferredType::Date
        );
    }

    #[test]
    fn sql_escape_quotes() {
        assert_eq!(sql_escape("it's"), "it''s");
        assert_eq!(sql_escape("no quotes"), "no quotes");
    }

    #[test]
    fn sql_escape_null_bytes() {
        assert_eq!(sql_escape("hello\0world"), "helloworld");
        assert_eq!(sql_escape("\0"), "");
        assert_eq!(sql_escape("it\0's"), "it''s");
    }

    #[test]
    fn value_to_literal_int() {
        assert_eq!(value_to_sql_literal("42", &InferredType::Int), "42");
    }

    #[test]
    fn value_to_literal_string() {
        assert_eq!(
            value_to_sql_literal("hello", &InferredType::NVarChar(10)),
            "N'hello'"
        );
    }

    #[test]
    fn value_to_literal_empty() {
        assert_eq!(
            value_to_sql_literal("", &InferredType::NVarChar(10)),
            "NULL"
        );
    }

    #[test]
    fn value_to_literal_date() {
        assert_eq!(
            value_to_sql_literal("2024-01-15", &InferredType::Date),
            "N'2024-01-15'"
        );
    }

    #[test]
    fn generate_create_table_basic() {
        let cols = vec!["id".to_string(), "name".to_string()];
        let types = vec![InferredType::Int, InferredType::NVarChar(50)];
        let ddl = generate_create_table("[test]", &cols, &types);
        assert!(ddl.contains("CREATE TABLE [test]"));
        assert!(ddl.contains("[id] INT NULL"));
        assert!(ddl.contains("[name] NVARCHAR(100) NULL"));
    }

    #[test]
    fn table_name_from_path_extracts_stem() {
        assert_eq!(table_name_from_path("/tmp/data/orders.csv"), "orders");
        assert_eq!(table_name_from_path("customers.json"), "customers");
    }

    #[test]
    fn infer_json_types() {
        assert_eq!(infer_type_from_json(&Value::from(42)), InferredType::Int);
        assert_eq!(
            infer_type_from_json(&Value::from(1.23)),
            InferredType::Float
        );
        assert_eq!(infer_type_from_json(&Value::from(true)), InferredType::Bit);
        assert_eq!(
            infer_type_from_json(&Value::from("hello")),
            InferredType::NVarChar(5)
        );
        assert_eq!(infer_type_from_json(&Value::Null), InferredType::Unknown);
    }

    #[test]
    fn json_literal_null() {
        assert_eq!(
            json_value_to_sql_literal(&Value::Null, &InferredType::Int),
            "NULL"
        );
    }

    #[test]
    fn json_literal_number() {
        assert_eq!(
            json_value_to_sql_literal(&Value::from(42), &InferredType::Int),
            "42"
        );
        assert_eq!(
            json_value_to_sql_literal(&Value::from(1.23), &InferredType::Float),
            "1.23"
        );
    }

    #[test]
    fn json_literal_bool() {
        assert_eq!(
            json_value_to_sql_literal(&Value::from(true), &InferredType::Bit),
            "1"
        );
        assert_eq!(
            json_value_to_sql_literal(&Value::from(false), &InferredType::Bit),
            "0"
        );
    }

    #[test]
    fn json_literal_string() {
        assert_eq!(
            json_value_to_sql_literal(&Value::from("hello"), &InferredType::NVarChar(10)),
            "N'hello'"
        );
    }

    #[test]
    fn csv_insert_batches() {
        let cols = vec!["id".to_string(), "name".to_string()];
        let types = vec![InferredType::Int, InferredType::NVarChar(10)];
        let rows = vec![
            vec!["1".to_string(), "Alice".to_string()],
            vec!["2".to_string(), "Bob".to_string()],
            vec!["3".to_string(), "Charlie".to_string()],
        ];
        let batches = generate_csv_insert_batches("[test]", &cols, &types, &rows, 2);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].1, 2); // first batch: 2 rows
        assert_eq!(batches[1].1, 1); // second batch: 1 row
        assert!(batches[0].0.contains("(1, N'Alice')"));
        assert!(batches[0].0.contains("(2, N'Bob')"));
        assert!(batches[1].0.contains("(3, N'Charlie')"));
    }
}
