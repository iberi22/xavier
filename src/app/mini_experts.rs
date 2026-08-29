//! Mini-Experts Prompt Template Registry & Fallback Implementation.
//!
//! Provides validation, language normalization, and fallback heuristics for
//! mini-expert agent system prompts.

use std::collections::HashMap;

/// Error types for prompt template validation.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PromptValidationError {
    EmptyTemplate,
    UnbalancedPlaceholders,
    InvalidPlaceholderSyntax,
}

impl std::fmt::Display for PromptValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyTemplate => write!(f, "Prompt template cannot be empty"),
            Self::UnbalancedPlaceholders => write!(f, "Prompt template has unbalanced placeholders"),
            Self::InvalidPlaceholderSyntax => write!(f, "Prompt template has invalid placeholder syntax"),
        }
    }
}

impl std::error::Error for PromptValidationError {}

/// Represents a prompt template for a mini-expert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiniExpertPromptTemplate {
    pub segment: String,
    pub language: String,
    pub template: String,
}

impl MiniExpertPromptTemplate {
    /// Creates a new template instance.
    pub fn new(segment: impl Into<String>, language: impl Into<String>, template: impl Into<String>) -> Self {
        Self {
            segment: segment.into(),
            language: normalize_language(&language.into()),
            template: template.into(),
        }
    }

    /// Validates the prompt template syntax.
    pub fn validate(&self) -> Result<(), PromptValidationError> {
        validate_template_string(&self.template)
    }
}

/// Normalizes language code (e.g., "en-US" -> "en", "ES" -> "es").
pub fn normalize_language(lang: &str) -> String {
    let trimmed = lang.trim().to_lowercase();
    if trimmed.is_empty() {
        return "en".to_string();
    }
    trimmed
        .split(|c: char| c == '-' || c == '_')
        .next()
        .unwrap_or("en")
        .to_string()
}

/// Validates a template string for non-emptiness and balanced `{{` and `}}` placeholders.
pub fn validate_template_string(template: &str) -> Result<(), PromptValidationError> {
    if template.trim().is_empty() {
        return Err(PromptValidationError::EmptyTemplate);
    }

    let mut open_count = 0;
    let bytes = template.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if open_count > 0 {
                // Nested placeholders not allowed
                return Err(PromptValidationError::InvalidPlaceholderSyntax);
            }
            open_count += 1;
            i += 2;
            continue;
        }
        if i + 1 < len && bytes[i] == b'}' && bytes[i + 1] == b'}' {
            if open_count == 0 {
                // Unmatched closing placeholder
                return Err(PromptValidationError::UnbalancedPlaceholders);
            }
            open_count -= 1;
            i += 2;
            continue;
        }
        i += 1;
    }

    if open_count != 0 {
        return Err(PromptValidationError::UnbalancedPlaceholders);
    }

    Ok(())
}

/// Default fallback system prompts per segment when custom templates are missing or invalid.
pub fn get_default_fallback_prompt(segment: &str) -> &'static str {
    match segment.to_lowercase().as_str() {
        "codebase" | "code" => {
            "You are a specialized codebase mini-expert. Analyze code accurately, identify bugs, and propose clean, efficient implementations. Context: {{context}}"
        }
        "security" | "sec" => {
            "You are a specialized security mini-expert. Review inputs and configurations for potential vulnerabilities and compliance issues. Context: {{context}}"
        }
        "architecture" | "arch" => {
            "You are a specialized architecture mini-expert. Provide high-level technical guidance on system structure and module boundaries. Context: {{context}}"
        }
        _ => {
            "You are an on-demand specialized mini-expert assistant for the domain: {{segment}}. Respond concisely and accurately. Context: {{context}}"
        }
    }
}

/// Registry for mini-expert prompt templates with language fallback support.
#[derive(Debug, Default, Clone)]
pub struct MiniExpertPromptRegistry {
    /// Storage key format: `"{segment}:{language}"`
    templates: HashMap<String, MiniExpertPromptTemplate>,
}

impl MiniExpertPromptRegistry {
    /// Creates a new empty prompt registry.
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    /// Registers a custom prompt template for a segment and language.
    pub fn register(&mut self, template: MiniExpertPromptTemplate) -> Result<(), PromptValidationError> {
        template.validate()?;
        let lang = normalize_language(&template.language);
        let key = format!("{}:{}", template.segment.to_lowercase(), lang);
        self.templates.insert(key, template);
        Ok(())
    }

    /// Resolves the prompt template for a segment and target language, applying fallback heuristics.
    ///
    /// Fallback chain:
    /// 1. Requested language custom template (if valid)
    /// 2. English ("en") custom template (if valid)
    /// 3. Built-in default fallback prompt for the segment
    pub fn resolve_template(&self, segment: &str, lang: &str) -> String {
        let norm_segment = segment.trim().to_lowercase();
        let target_lang = normalize_language(lang);

        // 1. Try target language custom template
        let key_target = format!("{}:{}", norm_segment, target_lang);
        if let Some(tpl) = self.templates.get(&key_target) {
            if tpl.validate().is_ok() {
                return tpl.template.clone();
            }
        }

        // 2. Try English fallback template if target was not English
        if target_lang != "en" {
            let key_en = format!("{}:en", norm_segment);
            if let Some(tpl) = self.templates.get(&key_en) {
                if tpl.validate().is_ok() {
                    return tpl.template.clone();
                }
            }
        }

        // 3. Built-in default prompt fallback
        get_default_fallback_prompt(&norm_segment).to_string()
    }

    /// Renders a prompt template for a segment, language, and context string.
    pub fn render_prompt(&self, segment: &str, lang: &str, context: &str) -> String {
        let raw_template = self.resolve_template(segment, lang);
        raw_template
            .replace("{{segment}}", segment)
            .replace("{{context}}", context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mini_experts_prompt_template_and_fallback() {
        let mut registry = MiniExpertPromptRegistry::new();

        // Test 1: Validation of valid template
        let valid_es = MiniExpertPromptTemplate::new(
            "codebase",
            "es-ES",
            "Eres un experto en codigo. Contexto: {{context}}",
        );
        assert!(registry.register(valid_es).is_ok());

        // Test 2: Validation of invalid template (unbalanced placeholders)
        let invalid_tpl = MiniExpertPromptTemplate::new(
            "security",
            "en",
            "Security expert context: {{context",
        );
        assert_eq!(
            registry.register(invalid_tpl),
            Err(PromptValidationError::UnbalancedPlaceholders)
        );

        // Test 3: Validation of empty template
        let empty_tpl = MiniExpertPromptTemplate::new("arch", "en", "   ");
        assert_eq!(
            registry.register(empty_tpl),
            Err(PromptValidationError::EmptyTemplate)
        );

        // Test 4: Resolving registered target language template
        let rendered_es = registry.render_prompt("codebase", "es", "rust functions");
        assert_eq!(rendered_es, "Eres un experto en codigo. Contexto: rust functions");

        // Test 5: English fallback when requested language template is missing
        let valid_en = MiniExpertPromptTemplate::new(
            "security",
            "en",
            "You are a security expert. Context: {{context}}",
        );
        assert!(registry.register(valid_en).is_ok());

        let rendered_sec_fr = registry.render_prompt("security", "fr", "auth service");
        assert_eq!(rendered_sec_fr, "You are a security expert. Context: auth service");

        // Test 6: Built-in default prompt fallback when no custom templates exist
        let rendered_default = registry.render_prompt("architecture", "de", "microservices");
        assert!(rendered_default.contains("architecture mini-expert"));
        assert!(rendered_default.contains("microservices"));
    }
}
