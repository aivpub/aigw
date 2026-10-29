//! Tool-name prefix validation.
//!
//! Tool names are `{alias}__{tool}` and splitting always takes the FIRST `__`.
//! An alias containing `__` would make `a__b__tool` ambiguous, and an alias
//! ending in `_` would make `a___tool` ambiguous; both are removed at
//! registration time rather than guessed at call time.

const MAX_ALIAS_LEN: usize = 32;

pub fn validate_alias(alias: &str) -> Result<(), &'static str> {
    if alias.is_empty() {
        return Err("alias must not be empty");
    }
    if alias.len() > MAX_ALIAS_LEN {
        return Err("alias must be at most 32 characters");
    }
    if alias.contains("__") {
        return Err("alias must not contain '__'");
    }
    if alias.ends_with('_') {
        return Err("alias must not end with '_'");
    }
    if !alias
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
    {
        return Err("alias may only contain A-Za-z0-9_.-");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_aliases() {
        for alias in ["github", "my-wiki", "a.b", "a_b", "A1"] {
            assert!(validate_alias(alias).is_ok(), "should accept {alias:?}");
        }
    }

    #[test]
    fn rejects_empty() {
        assert!(validate_alias("").is_err());
    }

    #[test]
    fn rejects_double_underscore() {
        assert!(validate_alias("a__b").is_err());
    }

    #[test]
    fn rejects_trailing_underscore() {
        assert!(validate_alias("a_").is_err());
    }

    #[test]
    fn rejects_over_length() {
        let alias = "a".repeat(MAX_ALIAS_LEN);
        assert!(validate_alias(&alias).is_ok(), "32 is the boundary");
        assert!(validate_alias(&"a".repeat(MAX_ALIAS_LEN + 1)).is_err());
    }

    #[test]
    fn rejects_disallowed_characters() {
        for alias in ["a b", "a/b", "a@b", "日本語"] {
            assert!(validate_alias(alias).is_err(), "should reject {alias:?}");
        }
    }
}
