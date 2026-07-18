//! Unified token estimation utility.

/// Estimates the number of tokens in a given text.
///
/// We default to estimating 1 token per 4 characters (chars / 4, rounded up). This is a standard
/// heuristic for English text under typical subword tokenizers (such as those used by OpenAI,
/// Anthropic, or LLaMA-based models), where 1 token is roughly 4 characters (or ~0.75 words).
///
/// This implementation counts Unicode scalar values rather than raw bytes to avoid overestimating
/// non-ASCII or multi-byte characters.
///
/// If the text is empty, it returns 0. For non-empty strings, it returns at least 1.
pub fn estimate_tokens(text: &str) -> usize {
    let char_count = text.chars().count();
    if char_count == 0 {
        return 0;
    }
    // Perform integer division that rounds up: (char_count + 3) / 4.
    // This is equivalent to `(char_count as f64 / 4.0).ceil() as usize`.
    (char_count + 3) / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_short() {
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("ab"), 1);
        assert_eq!(estimate_tokens("abc"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
    }

    #[test]
    fn test_estimate_tokens_round_up() {
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert_eq!(estimate_tokens("abcdefghi"), 3);
    }

    #[test]
    fn test_estimate_tokens_unicode() {
        // Multi-byte Unicode characters (each is 1 character, but multiple bytes)
        // "こんにちは" is 5 characters (15 bytes in UTF-8).
        // 5 chars / 4 = 1.25 -> rounded up to 2 tokens.
        assert_eq!(estimate_tokens("こんにちは"), 2);

        // "🦀" is 1 character (4 bytes).
        assert_eq!(estimate_tokens("🦀"), 1);

        // "🦀🦀🦀🦀🦀" is 5 characters.
        assert_eq!(estimate_tokens("🦀🦀🦀🦀🦀"), 2);
    }
}
