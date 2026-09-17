//! Pre-vectorization session sanitizer for agent chat transcripts and execution logs
//!
//! Provides noise reduction, boilerplate pruning, tool output condensation,
//! base64 truncation, and sliding-window deduplication before embedding or storage.

use crate::kernel::filters::{filter_cargo, filter_git, filter_grep, strip_ansi};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::LazyLock;

static BASE64_RUN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9+/=]{200,}").unwrap());

static BOILERPLATE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(cron\s*(job|trigger|heartbeat)|scheduled\s*task\s*tick|system\s*reminder:)")
        .unwrap()
});

static IMPORTANT_KEYWORD_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(error|failed|failure|completed|todo|decision|decided|panic)\b").unwrap()
});

/// Raw turn representation before sanitization and vectorization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTurn {
    pub session_id: String,
    pub turn_index: usize,
    pub role: String,
    pub content: String,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub model: Option<String>,
    pub file_paths: Vec<String>,
    pub meta: serde_json::Value,
}

/// Configuration options for session transcript sanitization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizeConfig {
    pub max_chars_per_turn: usize,
    pub min_chars_keep: usize,
    pub drop_cron_boilerplate: bool,
    pub truncate_base64_runs: bool,
    pub collapse_tool_output: bool,
    pub dedup_window: usize,
}

impl Default for SanitizeConfig {
    fn default() -> Self {
        Self {
            max_chars_per_turn: 8000,
            min_chars_keep: 12,
            drop_cron_boilerplate: true,
            truncate_base64_runs: true,
            collapse_tool_output: true,
            dedup_window: 3,
        }
    }
}

/// Execution metrics for a sanitization batch.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SanitizeReport {
    pub kept: usize,
    pub dropped_empty: usize,
    pub dropped_boilerplate: usize,
    pub dropped_dedup: usize,
    pub ansi_stripped: usize,
    pub base64_truncated: usize,
    pub tool_collapsed: usize,
    pub chars_in: usize,
    pub chars_out: usize,
}

/// Universal pre-vectorization sanitizer.
pub struct Sanitizer {
    cfg: SanitizeConfig,
}

impl Sanitizer {
    pub fn new(cfg: SanitizeConfig) -> Self {
        Self { cfg }
    }

    /// Sanitize a single turn's raw text content.
    ///
    /// Returns `None` if the turn should be dropped completely (e.g. empty or pure boilerplate).
    pub fn sanitize_turn(&self, raw: &str) -> Option<String> {
        let (res, _) = self.sanitize_turn_with_stats(raw);
        res
    }

    fn sanitize_turn_with_stats(&self, raw: &str) -> (Option<String>, TurnSanitizeStats) {
        let mut stats = TurnSanitizeStats::default();
        stats.chars_in = raw.len();

        if raw.is_empty() {
            return (None, stats);
        }

        // 1. Strip ANSI escape sequences
        let clean_ansi = strip_ansi(raw);
        if clean_ansi.len() != raw.len() {
            stats.ansi_stripped = true;
        }

        // 2. Normalize line endings (\r\n -> \n) and collapse excessive newlines
        let normalized = clean_ansi.replace("\r\n", "\n");
        let mut collapsed_lines = Vec::new();
        let mut empty_line_count = 0;
        for line in normalized.lines() {
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                empty_line_count += 1;
                if empty_line_count <= 2 {
                    collapsed_lines.push("");
                }
            } else {
                empty_line_count = 0;
                collapsed_lines.push(trimmed);
            }
        }
        let normalized_text = collapsed_lines.join("\n");
        let trimmed_text = normalized_text.trim();

        if trimmed_text.len() < self.cfg.min_chars_keep {
            return (None, stats);
        }

        // 3. Drop boilerplate (cron triggers, heartbeats) unless errors/decisions exist
        if self.cfg.drop_cron_boilerplate && BOILERPLATE_REGEX.is_match(trimmed_text) {
            if !IMPORTANT_KEYWORD_REGEX.is_match(trimmed_text) {
                stats.dropped_boilerplate = true;
                return (None, stats);
            }
        }

        // 4. Truncate long Base64 runs
        let mut processed = trimmed_text.to_string();
        if self.cfg.truncate_base64_runs && BASE64_RUN_REGEX.is_match(&processed) {
            stats.base64_truncated = true;
            processed = BASE64_RUN_REGEX
                .replace_all(&processed, |caps: &regex::Captures| {
                    let len = caps[0].len();
                    format!("[base64 payload {} chars omitted]", len)
                })
                .to_string();
        }

        // 5. Collapse tool outputs (cargo / git / grep)
        if self.cfg.collapse_tool_output {
            if processed.contains("test result:")
                || processed.contains("... ok\n")
                || processed.contains("... FAILED\n")
            {
                let filtered = filter_cargo(&processed);
                if filtered.len() < processed.len() {
                    stats.tool_collapsed = true;
                    processed = filtered;
                }
            } else if processed.contains("diff --git") || processed.contains("On branch ") {
                let filtered = filter_git(&processed);
                if filtered.len() < processed.len() {
                    stats.tool_collapsed = true;
                    processed = filtered;
                }
            } else if processed.lines().count() > 60
                && processed.lines().take(5).any(|l| l.contains(':'))
            {
                let filtered = filter_grep(&processed);
                if filtered.len() < processed.len() {
                    stats.tool_collapsed = true;
                    processed = filtered;
                }
            }
        }

        // 6. Hard truncation at max_chars_per_turn
        if processed.len() > self.cfg.max_chars_per_turn {
            let mut end_idx = self.cfg.max_chars_per_turn;
            while !processed.is_char_boundary(end_idx) && end_idx > 0 {
                end_idx -= 1;
            }
            let truncated_diff = processed.len() - end_idx;
            let mut truncated_text = processed[..end_idx].to_string();
            truncated_text.push_str(&format!("\n... [truncated {} chars]", truncated_diff));
            processed = truncated_text;
        }

        stats.chars_out = processed.len();
        (Some(processed), stats)
    }

    /// Sanitize a batch of turns in place and produce a comprehensive report.
    pub fn sanitize_turns(&self, turns: &mut Vec<RawTurn>) -> SanitizeReport {
        let mut report = SanitizeReport::default();
        let mut cleaned_turns = Vec::with_capacity(turns.len());
        let mut recent_hashes: VecDeque<u64> = VecDeque::with_capacity(self.cfg.dedup_window + 1);

        for turn in turns.drain(..) {
            report.chars_in += turn.content.len();

            let (sanitized_content_opt, stats) = self.sanitize_turn_with_stats(&turn.content);

            if stats.ansi_stripped {
                report.ansi_stripped += 1;
            }
            if stats.base64_truncated {
                report.base64_truncated += 1;
            }
            if stats.tool_collapsed {
                report.tool_collapsed += 1;
            }

            if let Some(content) = sanitized_content_opt {
                // Sliding window deduplication
                if self.cfg.dedup_window > 0 {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    content.hash(&mut hasher);
                    let h = hasher.finish();

                    if recent_hashes.contains(&h) {
                        report.dropped_dedup += 1;
                        continue;
                    }

                    if recent_hashes.len() >= self.cfg.dedup_window {
                        recent_hashes.pop_front();
                    }
                    recent_hashes.push_back(h);
                }

                report.chars_out += content.len();
                report.kept += 1;

                let mut cleaned_turn = turn;
                cleaned_turn.content = content;
                cleaned_turns.push(cleaned_turn);
            } else {
                if stats.dropped_boilerplate {
                    report.dropped_boilerplate += 1;
                } else {
                    report.dropped_empty += 1;
                }
            }
        }

        *turns = cleaned_turns;
        report
    }
}

#[derive(Default)]
struct TurnSanitizeStats {
    chars_in: usize,
    chars_out: usize,
    ansi_stripped: bool,
    base64_truncated: bool,
    tool_collapsed: bool,
    dropped_boilerplate: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_in_turn() {
        let sanitizer = Sanitizer::new(SanitizeConfig::default());
        let raw = "\x1B[32mSuccess\x1B[0m: Operation completed in 5ms";
        let out = sanitizer.sanitize_turn(raw).unwrap();
        assert_eq!(out, "Success: Operation completed in 5ms");
    }

    #[test]
    fn test_drop_cron_boilerplate() {
        let sanitizer = Sanitizer::new(SanitizeConfig::default());
        let boilerplate = "cron trigger: tick at 2026-09-16 00:00:00";
        assert!(sanitizer.sanitize_turn(boilerplate).is_none());

        // Retain if error/failure exists
        let important = "cron trigger: tick failed with panic in worker";
        let out = sanitizer.sanitize_turn(important).unwrap();
        assert!(out.contains("failed with panic"));
    }

    #[test]
    fn test_truncate_base64() {
        let sanitizer = Sanitizer::new(SanitizeConfig::default());
        let long_b64 = "A".repeat(300);
        let raw = format!("Payload header: {} tail", long_b64);
        let out = sanitizer.sanitize_turn(&raw).unwrap();
        assert!(out.contains("[base64 payload 300 chars omitted]"));
        assert!(!out.contains(&long_b64));
    }

    #[test]
    fn test_sliding_window_dedup() {
        let sanitizer = Sanitizer::new(SanitizeConfig::default());
        let mut turns = vec![
            RawTurn {
                session_id: "s1".to_string(),
                turn_index: 0,
                role: "user".to_string(),
                content: "Identical prompt message repeated".to_string(),
                timestamp: None,
                model: None,
                file_paths: vec![],
                meta: serde_json::Value::Null,
            },
            RawTurn {
                session_id: "s1".to_string(),
                turn_index: 1,
                role: "user".to_string(),
                content: "Identical prompt message repeated".to_string(),
                timestamp: None,
                model: None,
                file_paths: vec![],
                meta: serde_json::Value::Null,
            },
        ];

        let report = sanitizer.sanitize_turns(&mut turns);
        assert_eq!(report.kept, 1);
        assert_eq!(report.dropped_dedup, 1);
        assert_eq!(turns.len(), 1);
    }
}
