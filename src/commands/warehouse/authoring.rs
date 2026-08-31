//! Warehouse authoring — bulk ingestion via `COPY INTO`.
//!
//! `COPY INTO` is Fabric Warehouse's high-throughput bulk-load statement. It
//! appends rows into an existing table from Azure storage / `OneLake`, completing
//! the authoring loop with `list-tables`/`describe-table` (create table -> COPY
//! INTO -> validate row counts). It is an additive mutation (never deletes or
//! overwrites), so it is gated by `--readonly`, previewed by `--dry-run` (with the
//! SAS secret redacted), and validated (HTTPS storage source, file-type enum)
//! before any network call.

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{
    CopyIntoOptions, build_copy_into_sql, execute_sql_rows, normalize_copy_file_type,
    parse_connection_string, split_schema_qualified, validate_copy_into_source,
};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

/// Flags for a `warehouse copy-into` invocation (grouped to keep the handler
/// signature small).
pub(super) struct CopyIntoArgs<'a> {
    pub table: &'a str,
    pub source: &'a str,
    pub file_type: &'a str,
    pub columns: Option<&'a str>,
    pub field_terminator: Option<&'a str>,
    pub row_terminator: Option<&'a str>,
    pub first_row: Option<u32>,
    pub encoding: Option<&'a str>,
    pub sas_token: Option<&'a str>,
    /// Source authentication mode: `entra-id` (default), `sas`, or
    /// `workspace-identity`. When `None`, it is inferred: `sas` if a `sas_token`
    /// is present, otherwise `entra-id`.
    pub auth_mode: Option<&'a str>,
}

/// Resolve the effective COPY INTO source authentication mode and validate that
/// the supplied flags are internally consistent (e.g. `--auth-mode sas` requires
/// `--sas-token`; `--auth-mode workspace-identity` forbids one). Returns whether
/// the workspace managed identity should be used. Pure for unit testing.
fn resolve_auth_mode(auth_mode: Option<&str>, has_sas: bool) -> Result<bool> {
    let mode = auth_mode.unwrap_or(if has_sas { "sas" } else { "entra-id" });
    match mode {
        "sas" => {
            if !has_sas {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--auth-mode sas requires --sas-token.".to_string(),
                    "Provide the SAS token with --sas-token, or use --auth-mode workspace-identity \
                     (no secret) / --auth-mode entra-id (caller identity).",
                )
                .into());
            }
            Ok(false)
        }
        "workspace-identity" => {
            if has_sas {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--auth-mode workspace-identity cannot be combined with --sas-token."
                        .to_string(),
                    "Drop --sas-token to authenticate with the workspace managed identity, or use \
                     --auth-mode sas to authenticate with the SAS token.",
                )
                .into());
            }
            Ok(true)
        }
        // "entra-id" (or the inferred default) — caller identity, no credential.
        _ => {
            if has_sas {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--auth-mode entra-id cannot be combined with --sas-token.".to_string(),
                    "Drop --sas-token to use the caller's Entra identity, or use --auth-mode sas \
                     to authenticate with the SAS token.",
                )
                .into());
            }
            Ok(false)
        }
    }
}

/// Bulk-load files into a warehouse table with `COPY INTO`.
pub(super) async fn copy_into(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    args: &CopyIntoArgs<'_>,
) -> Result<()> {
    // Input validation — fail fast, before any network call, so a --dry-run of an
    // invalid request still surfaces the validation error.
    let file_type = normalize_copy_file_type(args.file_type)?;
    validate_copy_into_source(args.source)?;

    let (schema, table) = split_schema_qualified(args.table);
    if table.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--table must name a target table (optionally schema-qualified).".to_string(),
            "Example: --table dbo.Orders. The table must already exist; create it first \
             with: fabio warehouse query --sql \"CREATE TABLE ...\".",
        )
        .into());
    }

    // CSV-only options with PARQUET are a mistake — reject with a teaching hint.
    if file_type == "PARQUET"
        && (args.field_terminator.is_some()
            || args.row_terminator.is_some()
            || args.first_row.is_some()
            || args.encoding.is_some())
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--field-terminator/--row-terminator/--first-row/--encoding apply only to CSV."
                .to_string(),
            "Drop those flags for --file-type PARQUET, or use --file-type CSV.",
        )
        .into());
    }

    // Resolve + validate the source authentication mode before any network call.
    let workspace_identity = resolve_auth_mode(args.auth_mode, args.sas_token.is_some())?;

    let opts = CopyIntoOptions {
        schema: schema.as_deref(),
        table: &table,
        source: args.source,
        file_type,
        columns: args.columns,
        field_terminator: args.field_terminator,
        row_terminator: args.row_terminator,
        first_row: args.first_row,
        encoding: args.encoding,
        sas_token: args.sas_token,
        workspace_identity,
    };
    let display_target = match schema.as_deref() {
        Some(s) if !s.is_empty() => format!("{s}.{table}"),
        _ => table.clone(),
    };

    // Dry-run preview AFTER validation, BEFORE any network call. The previewed SQL
    // redacts the SAS secret so it never lands in stdout/logs.
    if output::dry_run_guard(
        cli,
        "warehouse copy-into",
        &serde_json::json!({
            "table": display_target,
            "source": args.source,
            "fileType": file_type,
            "sql": build_copy_into_sql(&opts, true),
        }),
    ) {
        return Ok(());
    }

    // Readonly guard: COPY INTO is a mutation and the TDS path bypasses the HTTP
    // client's readonly guard, so enforce it explicitly here (after the dry-run
    // preview, which is itself read-only and allowed under --readonly).
    if cli.readonly {
        return Err(FabioError::with_hint(
            ErrorCode::ReadonlyMode,
            "Blocked warehouse copy-into — readonly mode is active".to_string(),
            "Remove --readonly (or set FABIO_READONLY=0) to allow this data-loading mutation.",
        )
        .into());
    }

    // Execute over TDS (real SQL, with the secret) and render a tailored envelope.
    let sql = build_copy_into_sql(&opts, false);
    let (connection_string, item_name) =
        super::get_connection_string(client, workspace, id).await?;
    let (server, parsed_db) = parse_connection_string(&connection_string);
    let database = if item_name.is_empty() {
        parsed_db
    } else {
        item_name
    };
    let (columns, rows) = execute_sql_rows(client, &server, &database, &sql)
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse copy-into", "Contributor"))?;

    let mut result = serde_json::json!({
        "status": "loaded",
        "table": display_target,
        "source": args.source,
        "fileType": file_type,
    });
    // COPY INTO may return a summary result set (e.g. rows loaded / rejected); surface
    // it when present.
    if !columns.is_empty() && !rows.is_empty() {
        result["result"] = Value::Array(rows);
    }
    result["hint"] = Value::from(format!(
        "Validate the load: fabio warehouse describe-table --workspace {workspace} --id {id} \
         --table {display_target}, or run a COUNT(*) via fabio warehouse query."
    ));
    output::render_object(cli, &result, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_auth_mode;

    #[test]
    fn infers_sas_when_token_present_and_no_mode() {
        assert!(!resolve_auth_mode(None, true).unwrap());
    }

    #[test]
    fn infers_entra_id_when_no_token_and_no_mode() {
        assert!(!resolve_auth_mode(None, false).unwrap());
    }

    #[test]
    fn workspace_identity_requests_managed_identity() {
        assert!(resolve_auth_mode(Some("workspace-identity"), false).unwrap());
    }

    #[test]
    fn workspace_identity_conflicts_with_sas_token() {
        let err = resolve_auth_mode(Some("workspace-identity"), true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot be combined with --sas-token"));
    }

    #[test]
    fn sas_mode_requires_token() {
        let err = resolve_auth_mode(Some("sas"), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("requires --sas-token"));
    }

    #[test]
    fn sas_mode_with_token_ok() {
        assert!(!resolve_auth_mode(Some("sas"), true).unwrap());
    }

    #[test]
    fn entra_id_mode_conflicts_with_sas_token() {
        let err = resolve_auth_mode(Some("entra-id"), true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot be combined with --sas-token"));
    }
}
