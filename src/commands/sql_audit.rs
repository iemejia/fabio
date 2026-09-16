use anyhow::Result;

use crate::errors::{ErrorCode, FabioError};

pub const MAX_PREDICATE_EXPRESSION_LENGTH: usize = 3_000;

pub fn validate_predicate_expression(predicate: &str) -> Result<()> {
    let length = predicate.chars().count();
    if length > MAX_PREDICATE_EXPRESSION_LENGTH {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Audit predicate expression is {length} characters; the maximum is {MAX_PREDICATE_EXPRESSION_LENGTH}"
            ),
            "Shorten --predicate-expression to at most 3000 characters.",
        )
        .into());
    }
    let trimmed = predicate.trim_start();
    // Reject a leading WHERE keyword. The keyword boundary is ANY non-identifier
    // character (or end of string) — not just whitespace — so adjacent forms like
    // `WHERE(statement = 1)` are also caught, while an identifier that merely starts
    // with "where" (e.g. `whereabouts`) is still allowed.
    let starts_with_where_keyword = trimmed
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("where"))
        && trimmed
            .chars()
            .nth(5)
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
    if starts_with_where_keyword {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Audit predicate expression must not include the WHERE keyword",
            "Pass only the predicate, for example: --predicate-expression \"statement LIKE 'SELECT %'\".",
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_PREDICATE_EXPRESSION_LENGTH, validate_predicate_expression};

    #[test]
    fn accepts_empty_and_valid_predicates() {
        assert!(validate_predicate_expression("").is_ok());
        assert!(validate_predicate_expression("NOT statement LIKE 'SELECT %'").is_ok());
    }

    #[test]
    fn rejects_where_keyword_and_overlong_predicates() {
        assert!(validate_predicate_expression("WHERE action_id = 1").is_err());
        assert!(
            validate_predicate_expression(&"x".repeat(MAX_PREDICATE_EXPRESSION_LENGTH + 1))
                .is_err()
        );
    }

    #[test]
    fn rejects_where_keyword_adjacent_to_non_identifier() {
        // The keyword boundary is any non-identifier char, not only whitespace.
        assert!(validate_predicate_expression("WHERE(action_id = 1)").is_err());
        assert!(validate_predicate_expression("  where(action_id = 1)").is_err());
        assert!(validate_predicate_expression("WHERE").is_err());
        assert!(validate_predicate_expression("WHERE\t action_id = 1").is_err());
    }

    #[test]
    fn allows_identifier_that_merely_starts_with_where() {
        // "whereabouts" is a column name, not the WHERE keyword.
        assert!(validate_predicate_expression("whereabouts = 'north'").is_ok());
        assert!(validate_predicate_expression("where_clause IS NOT NULL").is_ok());
    }
}
