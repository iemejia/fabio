//! Shared resolver for query text supplied via a flag, an `@file` path, or stdin.
//!
//! Every query command (SQL, KQL, DAX, GQL/GraphQL) should route its query text
//! through [`resolve_query_input`] so agents can rely on ONE convention: pass the
//! text inline, prefix `@` to read it from a file, or omit it and pipe via stdin.

use std::io;

use anyhow::Result;

use crate::errors::{ErrorCode, FabioError};

/// Resolve query text from three interchangeable sources:
/// - `Some("text")` — inline query text
/// - `Some("@path")` — read the query from the file at `path`
/// - `None` — read the query from stdin (error if empty)
///
/// `lang` is the language label used in diagnostics (e.g. `"GQL"`), `flag` is the
/// CLI flag that carries the text (e.g. `"--gql"`), and `example` is a full
/// example command shown when no input is supplied.
pub fn resolve_query_input(
    value: Option<&str>,
    lang: &str,
    flag: &str,
    example: &str,
) -> Result<String> {
    match value {
        Some(s) if s.starts_with('@') => {
            let file_path = &s[1..];
            std::fs::read_to_string(file_path).map_err(|e| {
                FabioError::not_found(format!("{lang} file not found: {file_path}: {e}")).into()
            })
        }
        Some(s) => Ok(s.to_string()),
        None => {
            let buf = io::read_to_string(io::stdin()).map_err(|e| {
                FabioError::new(
                    ErrorCode::ApiError,
                    format!("Failed to read {lang} from stdin: {e}"),
                )
            })?;
            if buf.trim().is_empty() {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("No {lang} provided. Use {flag}, @file, or pipe via stdin."),
                    example.to_string(),
                )
                .into());
            }
            Ok(buf)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_query_input;

    const EX: &str = "fabio graph-model execute-query --id <ID> --gql \"MATCH (n) RETURN n\"";

    #[test]
    fn inline_text_is_passed_through() {
        let got = resolve_query_input(Some("MATCH (n) RETURN n"), "GQL", "--gql", EX).unwrap();
        assert_eq!(got, "MATCH (n) RETURN n");
    }

    #[test]
    fn at_prefix_reads_from_file() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("fabio_query_input_{}.gql", std::process::id()));
        std::fs::write(&path, "MATCH (x) RETURN x").unwrap();
        let arg = format!("@{}", path.display());
        let got = resolve_query_input(Some(&arg), "GQL", "--gql", EX).unwrap();
        assert_eq!(got, "MATCH (x) RETURN x");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn missing_file_is_a_not_found_error() {
        let err = resolve_query_input(Some("@/no/such/file.gql"), "GQL", "--gql", EX).unwrap_err();
        assert!(
            err.to_string().contains("file not found"),
            "expected a not-found error, got: {err}"
        );
    }
}
