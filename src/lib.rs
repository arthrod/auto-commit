pub fn truncate_to_n_tokens(text: &str, limit: usize) -> String {
    text.split_whitespace().take(limit).collect::<Vec<_>>().join(" ")
}

pub fn get_model_from_env() -> String {
    std::env::var("AUTO_COMMIT_MODEL").unwrap_or_else(|_| "gpt-4.1-nano".to_string())
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
        assert_eq!(get_model_from_env(), "gpt-4.1-nano");
    }

    #[test]
    fn test_get_model_from_env_custom() {
        std::env::set_var("AUTO_COMMIT_MODEL", "custom-model");
        assert_eq!(get_model_from_env(), "custom-model");
        std::env::remove_var("AUTO_COMMIT_MODEL");
    }

    #[test]
    fn test_truncate_to_n_tokens_zero_limit() {
        let text = "a b c d e";
        assert_eq!(truncate_to_n_tokens(text, 0), "");
    }

    #[test]
    fn test_truncate_to_n_tokens_empty_string() {
        let text = "";
        assert_eq!(truncate_to_n_tokens(text, 5), "");
    }

    #[test]
    fn test_truncate_to_n_tokens_limit_exceeds_tokens() {
        let text = "hello world";
        assert_eq!(truncate_to_n_tokens(text, 5), "hello world");
    }

    #[test]
    fn test_truncate_to_n_tokens_whitespace_normalization() {
        let text = " a   b  c ";
        assert_eq!(truncate_to_n_tokens(text, 2), "a b");
    }

    #[test]
    fn test_truncate_to_n_tokens_unicode_characters() {
        let text = "你好 世界 Rust 编程";
        assert_eq!(truncate_to_n_tokens(text, 3), "你好 世界 Rust");
    }

    #[test]
    fn test_get_model_from_env_empty_string() {
        std::env::set_var("AUTO_COMMIT_MODEL", "");
        assert_eq!(get_model_from_env(), "");
        std::env::remove_var("AUTO_COMMIT_MODEL");
    }

    #[test]
    fn test_get_model_from_env_whitespace_value() {
        let custom = "   ";
        std::env::set_var("AUTO_COMMIT_MODEL", custom);
        assert_eq!(get_model_from_env(), custom);
        std::env::remove_var("AUTO_COMMIT_MODEL");
    }

    #[test]
    fn test_get_model_from_env_unicode_value() {
        let custom = "模型一";
        std::env::set_var("AUTO_COMMIT_MODEL", custom);
        assert_eq!(get_model_from_env(), custom);
        std::env::remove_var("AUTO_COMMIT_MODEL");
    }
}