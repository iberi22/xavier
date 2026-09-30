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

    #[test]
    fn token_estimate_matches_context() {
        for text in ["", "a", "abcd", "abcde", "こんにちは", "🦀🦀🦀🦀🦀"] {
            assert_eq!(estimate_tokens(text), crate::context::estimate_tokens(text));
        }
    }
}
