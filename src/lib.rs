/// Truncate a string to the first `limit` whitespace-delimited words.
pub fn truncate_to_n_tokens(text: &str, limit: usize) -> String {
    text.split_whitespace().take(limit).collect::<Vec<_>>().join(" ")
}

/// Fetch the model name from the `AUTO_COMMIT_MODEL` environment variable.
/// Defaults to `DEFAULT_MODEL` when the variable is not set.
pub const DEFAULT_MODEL: &str = "gpt-4.1-nano";

pub fn get_model_from_env() -> String {
    std::env::var("AUTO_COMMIT_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_to_n_tokens() {
        let text = "a b c d e";
        assert_eq!(truncate_to_n_tokens(text, 3), "a b c");
    }

    #[test]
    fn test_get_model_from_env_default() {
        std::env::remove_var("AUTO_COMMIT_MODEL");
        assert_eq!(get_model_from_env(), DEFAULT_MODEL);
    }

    #[test]
    fn test_get_model_from_env_custom() {
        std::env::set_var("AUTO_COMMIT_MODEL", "custom-model");
        assert_eq!(get_model_from_env(), "custom-model");
        std::env::remove_var("AUTO_COMMIT_MODEL");
    }
}
