//! Unified token budget and estimation module.

use serde::{Deserialize, Serialize};

/// Estimates the number of tokens in a given text.
///
/// Uses the heuristic: 1 token ≈ 4 characters.
/// Documented as chars().count().div_ceil(4).
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// Truncates the text to stay within the specified token budget.
///
/// Returns a tuple with the (truncated_text, was_truncated).
pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> (String, bool) {
    let current_tokens = estimate_tokens(text);
    if current_tokens <= max_tokens {
        return (text.to_string(), false);
    }

    // Since 1 token ≈ 4 chars, we take roughly max_tokens * 4 chars.
    // We use chars().take() to be safe with multi-byte characters.
    let max_chars = max_tokens * 4;
    let truncated: String = text.chars().take(max_chars).collect();

    (truncated, true)
}

/// Helper struct for managing context budgets.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TokenBudget {
    pub max_tokens: usize,
    pub max_docs: usize,
}

impl TokenBudget {
    pub fn new(max_tokens: usize, max_docs: usize) -> Self {
        Self {
            max_tokens,
            max_docs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("12345678"), 2);
    }

    #[test]
    fn test_truncate_to_tokens() {
        let text = "this is a relatively long text that should be truncated";
        // "this is a relatively long text that should be truncated" is 55 chars
        // 55 / 4 = 14 tokens
        let (truncated, was_truncated) = truncate_to_tokens(text, 5);
        assert!(was_truncated);
        // 5 tokens * 4 = 20 chars
        assert_eq!(truncated.chars().count(), 20);
        assert_eq!(truncated, "this is a relatively");

        let (not_truncated, was_truncated_2) = truncate_to_tokens(text, 20);
        assert!(!was_truncated_2);
        assert_eq!(not_truncated, text);
    }
}
