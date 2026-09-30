//! Owned by issue [WAVE-30.08] (ADR-033 Wave 0).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::future::Future;

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
    char_count.div_ceil(4)
}

/// Notification severity (maps to emoji + level).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NotificationLevel {
    Success,
    Info,
    Warning,
    Error,
    Critical,
}

/// A notification to be sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub level: NotificationLevel,
    pub title: String,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
}

/// Sends a self-healing notification through the configured channels.
pub trait SelfHealNotification {
    fn notify_self_heal(description: &str, success: bool) -> Self;
}

/// Runs the optional nightly refinement after memory consolidation.
pub trait NightlyTgd {
    fn run_nightly_tgd(&self) -> impl Future<Output = anyhow::Result<()>> + Send;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SchedulerState {
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_duration_ms: u64,
    pub items_processed: usize,
}

#[cfg(test)]
mod tests {
    use super::estimate_tokens;

    /// Expected token counts for 0..=17 ASCII chars, one entry per char count.
    ///
    /// This is an independent table of literal expectations, not a re-derivation of the
    /// implementation formula, so a one-sided edit of the heuristic below fails the test.
    const EXPECTED_BY_CHAR_COUNT: [usize; 18] =
        [0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5];

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
        // Multi-byte Unicode characters (each is 1 character, but multiple bytes).
        // "こんにちは" is 5 characters (15 bytes in UTF-8).
        // 5 chars / 4 = 1.25 -> rounded up to 2 tokens.
        assert_eq!(estimate_tokens("こんにちは"), 2);

        // "🦀" is 1 character (4 bytes).
        assert_eq!(estimate_tokens("🦀"), 1);

        // "🦀🦀🦀🦀🦀" is 5 characters.
        assert_eq!(estimate_tokens("🦀🦀🦀🦀🦀"), 2);
    }

    #[test]
    fn test_estimate_tokens_every_char_boundary() {
        for (char_count, expected) in EXPECTED_BY_CHAR_COUNT.iter().enumerate() {
            let text = "a".repeat(char_count);
            assert_eq!(
                estimate_tokens(&text),
                *expected,
                "unexpected token count for {char_count} char(s)"
            );
        }
    }

    #[test]
    fn test_estimate_tokens_counts_scalars_not_bytes() {
        // 4 multi-byte chars (16 UTF-8 bytes) must cost 1 token, not 4.
        assert_eq!("🦀🦀🦀🦀".len(), 16);
        assert_eq!(estimate_tokens("🦀🦀🦀🦀"), 1);
        // 5 multi-byte chars (20 UTF-8 bytes) round up to 2 tokens, not 5.
        assert_eq!("🦀🦀🦀🦀🦀".len(), 20);
        assert_eq!(estimate_tokens("🦀🦀🦀🦀🦀"), 2);
    }
}
