use anyhow::Result;

use crate::errors::{ErrorCode, FabioError};

pub fn validate_predicate_expression(predicate: Option<&str>) -> Result<()> {
    let Some(predicate) = predicate else {
        return Ok(());
    };
    if predicate.chars().count() > 3_000 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Audit predicate expression exceeds the 3,000-character limit.",
            "Shorten --predicate-expression to at most 3,000 characters.",
        )
        .into());
    }
    let trimmed = predicate.trim_start();
    let starts_with_where = trimmed
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("where"))
        && trimmed.get(5..).is_some_and(|rest| {
            rest.chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || c == '(')
        });
    if starts_with_where {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Audit predicate expression must not include the WHERE keyword.",
            "Pass only the predicate, for example: --predicate-expression \"NOT statement LIKE 'SELECT %'\".",
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_predicate_expression;

    #[test]
    fn accepts_empty_and_valid_predicates() {
        validate_predicate_expression(Some("")).unwrap();
        validate_predicate_expression(Some("NOT statement LIKE 'SELECT %'")).unwrap();
    }

    #[test]
    fn rejects_where_keyword_and_overlong_predicate() {
        assert!(validate_predicate_expression(Some(" WHERE action_id = 1")).is_err());
        assert!(validate_predicate_expression(Some("where(action_id = 1)")).is_err());
        assert!(validate_predicate_expression(Some("wherever = 'x'")).is_ok());
        assert!(validate_predicate_expression(Some(&"x".repeat(3_001))).is_err());
    }
}
