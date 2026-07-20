//! Logical concern: Validation helpers for Xavier settings.
//!
//! This module contains functions to validate and sanitize configuration values.

use crate::settings::XavierSettings;

/// Returns Some(trimmed_string) if not empty, otherwise None.
pub fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Helper function to validate if a string is a valid http or https URL.
pub fn is_http_url(value: &str) -> bool {
    if let Ok(parsed) = url::Url::parse(value) {
        parsed.scheme() == "http" || parsed.scheme() == "https"
    } else {
        value.starts_with("http://") || value.starts_with("https://")
    }
}

/// Validates the configuration when using a local LLM provider.
pub fn validate_local_config(settings: &XavierSettings) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let provider = settings.models.provider.as_str();

    if provider == "local" || provider == "ollama" {
        if settings.models.local_llm_url.trim().is_empty() {
            errors.push("local_llm_url is empty. Set it to http://localhost:11434/v1 or run `xavier setup --local`".to_string());
        } else if !is_http_url(&settings.models.local_llm_url) {
            errors.push(format!(
                "local_llm_url '{}' is not a valid http/https URL",
                settings.models.local_llm_url
            ));
        }

        if settings.models.local_llm_model.trim().is_empty() {
            errors.push(
                "local_llm_model is empty (set it to qwen3-coder or another Ollama model)"
                    .to_string(),
            );
        }

        if settings.models.embedding_url.trim().is_empty() {
            errors.push(
                "embedding_url is empty (set it to http://localhost:11434/api/embeddings)"
                    .to_string(),
            );
        } else if !is_http_url(&settings.models.embedding_url) {
            errors.push(format!(
                "embedding_url '{}' is not a valid http/https URL",
                settings.models.embedding_url
            ));
        }

        if settings.models.embedding_model.trim().is_empty() {
            errors.push(
                "embedding_model is empty (set it to embeddinggemma or another Ollama model)"
                    .to_string(),
            );
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_local_config() {
        let mut settings = XavierSettings::default();
        settings.models.provider = "local".to_string();
        settings.models.local_llm_url = "http://localhost:11434/v1".to_string();
        settings.models.local_llm_model = "qwen3-coder".to_string();
        settings.models.embedding_url = "http://localhost:11434/api/embeddings".to_string();
        settings.models.embedding_model = "embeddinggemma".to_string();

        assert!(validate_local_config(&settings).is_ok());
    }

    #[test]
    fn test_local_llm_url_empty() {
        let mut settings = XavierSettings::default();
        settings.models.provider = "local".to_string();
        settings.models.local_llm_url = "".to_string();
        settings.models.local_llm_model = "qwen3-coder".to_string();
        settings.models.embedding_url = "http://localhost:11434/api/embeddings".to_string();
        settings.models.embedding_model = "embeddinggemma".to_string();

        let res = validate_local_config(&settings);
        assert!(res.is_err());
        let errs = res.unwrap_err();
        assert!(errs.iter().any(|e| e.contains("local_llm_url is empty")));
    }

    #[test]
    fn test_local_llm_url_invalid_scheme() {
        let mut settings = XavierSettings::default();
        settings.models.provider = "local".to_string();
        settings.models.local_llm_url = "ftp://localhost:11434/v1".to_string();
        settings.models.local_llm_model = "qwen3-coder".to_string();
        settings.models.embedding_url = "http://localhost:11434/api/embeddings".to_string();
        settings.models.embedding_model = "embeddinggemma".to_string();

        let res = validate_local_config(&settings);
        assert!(res.is_err());
        let errs = res.unwrap_err();
        assert!(errs
            .iter()
            .any(|e| e.contains("not a valid http/https URL")));
    }

    #[test]
    fn test_cloud_config_ignored() {
        let mut settings = XavierSettings::default();
        settings.models.provider = "cloud".to_string();
        // everything else empty should still pass for non-local/non-ollama
        settings.models.local_llm_url = "".to_string();
        settings.models.local_llm_model = "".to_string();
        settings.models.embedding_url = "".to_string();
        settings.models.embedding_model = "".to_string();

        assert!(validate_local_config(&settings).is_ok());
    }
}
