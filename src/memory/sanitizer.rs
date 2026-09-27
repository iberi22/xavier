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

/// Helper function to detect periodic heartbeat and cronjob ticks.
///
/// Returns true if the content or path represents periodic heartbeat/cron activity,
/// such as `[tick] cronjob`, `gestalt-thinking-loop ejecutado`, or `cron 30m ejecutado`.
pub fn is_heartbeat_tick(content: &str, path: &str) -> bool {
    let lower_p = path.to_lowercase();
    if lower_p.contains("session/cron_")
        || lower_p.contains("heartbeat")
        || lower_p.contains("gestalt/bus/executions")
    {
        return true;
    }

    let trimmed = content.trim();
    let lower_c = trimmed.to_lowercase();

    if lower_c.contains("[tick]")
        || lower_c.contains("gestalt-thinking-loop ejecutado")
        || lower_c.contains("cron 30m ejecutado")
        || lower_c.contains("cronjob")
        || (lower_c.contains("ejecutado")
            && (lower_c.contains("cron")
                || lower_c.contains("thinking-loop")
                || lower_c.contains("tick")
                || lower_c.contains("loop")))
    {
        return true;
    }

    BOILERPLATE_REGEX.is_match(trimmed)
}

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
        let mut stats = TurnSanitizeStats {
            chars_in: raw.len(),
            ..Default::default()
        };

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
        if self.cfg.drop_cron_boilerplate
            && (BOILERPLATE_REGEX.is_match(trimmed_text) || is_heartbeat_tick(trimmed_text, ""))
            && !IMPORTANT_KEYWORD_REGEX.is_match(trimmed_text)
        {
            stats.dropped_boilerplate = true;
            return (None, stats);
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

// ── Write-time content integrity ──────────────────────────────────────────────

/// Cap on the suspect reasons stored per record. A prose memory full of
/// identifiers can yield hundreds of hits; the marker only has to tell a reader
/// the content is questionable, not to enumerate every occurrence.
const MAX_SUSPECT_REASONS: usize = 10;

/// Outcome of a write-time content integrity check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentCheck {
    /// Reason the content must not be stored at all, if any.
    pub reject: Option<String>,
    /// Short reasons the content looks corrupted even though it is stored
    /// verbatim, e.g. `joined_words:cualesPassed`, `dot_join:tiene.noción`.
    pub suspect: Vec<String>,
}

impl ContentCheck {
    /// True when the content must not be stored.
    pub fn is_rejected(&self) -> bool {
        self.reject.is_some()
    }

    /// Integrity marker to stamp on the stored record.
    pub fn integrity(&self) -> &'static str {
        if self.suspect.is_empty() {
            "verified"
        } else {
            "suspect"
        }
    }
}

/// CJK ideographs, Japanese kana and Korean Hangul.
fn is_cjk_char(c: char) -> bool {
    matches!(c,
        '\u{3005}'..='\u{3007}'
            | '\u{3040}'..='\u{30FF}'
            | '\u{3100}'..='\u{312F}'
            | '\u{31F0}'..='\u{31FF}'
            | '\u{3130}'..='\u{318F}'
            | '\u{31C0}'..='\u{31EF}'
            | '\u{3200}'..='\u{33FF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{A960}'..='\u{A97F}'
            | '\u{AC00}'..='\u{D7FF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{20000}'..='\u{2A6DF}')
}

/// Latin letters, ASCII plus the accented/extended ranges.
///
/// The `is_alphabetic` gate is what keeps the symbols that share those ranges
/// out: `×` (U+00D7) and `÷` (U+00F7) fall inside `00C0..=024F` but are
/// operators, not letters, and treating them as letters would make a formula
/// such as `2×3` look like Latin-dominant text.
fn is_latin_letter(c: char) -> bool {
    c.is_alphabetic()
        && (c.is_ascii_alphabetic()
            || matches!(c,
                '\u{00C0}'..='\u{024F}'
                    | '\u{1D00}'..='\u{1D7F}'
                    | '\u{1D80}'..='\u{1DBF}'
                    | '\u{1E00}'..='\u{1EFF}'
                    | '\u{2C60}'..='\u{2C7F}'
                    | '\u{A720}'..='\u{A7FF}'))
}

/// Lowercase Latin letter: a word that was still being written when a CJK
/// character got glued onto it.
fn is_lowercase_latin(c: char) -> bool {
    is_latin_letter(c) && c.is_lowercase()
}

/// Uppercase Latin letter: an acronym, which CJK prose legitimately embeds.
fn is_uppercase_latin(c: char) -> bool {
    is_latin_letter(c) && c.is_uppercase()
}

/// Control characters that memory content may not carry.
fn is_forbidden_control(c: char) -> bool {
    c.is_control() && c != '\n' && c != '\r' && c != '\t'
}

/// Drops the surrounding punctuation/whitespace of a token, keeping the parts
/// that carry meaning (so `prueba.Word,` -> `prueba.Word`).
fn trim_token(token: &str) -> &str {
    token.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '\''))
}

/// Adjacent-script findings, only meaningful when the content as a whole is
/// Latin dominant.
#[derive(Default)]
struct MixedScript {
    /// Token where a lowercase Latin letter touches a CJK/Kana/Hangul letter
    /// with no boundary between them (`La全区`): a dropped word boundary.
    reject: Option<String>,
    /// Token where an uppercase Latin acronym touches CJK (`API接口`): normal
    /// writing, worth reporting but never worth dropping.
    suspect: Option<String>,
}

/// Scan for the two mixed-script shapes, but only in Latin-dominant content.
///
/// In CJK-dominant text the same shapes are ordinary writing
/// (`这个API接口返回JSON数据`), so the whole scan is skipped there; a mixed token
/// inside Spanish or English prose is where a boundary was actually lost.
fn mixed_script(content: &str) -> MixedScript {
    let mut letters = 0usize;
    let mut cjk_letters = 0usize;
    for c in content.chars().filter(|c| c.is_alphabetic()) {
        letters += 1;
        if is_cjk_char(c) {
            cjk_letters += 1;
        }
    }
    if letters == 0 || cjk_letters * 10 >= letters {
        return MixedScript::default();
    }

    let mut findings = MixedScript::default();
    for token in content.split_whitespace().map(trim_token) {
        let chars: Vec<char> = token.chars().collect();
        for pair in chars.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            if (is_lowercase_latin(left) && is_cjk_char(right))
                || (is_cjk_char(left) && is_lowercase_latin(right))
            {
                findings.reject = Some(token.to_string());
                break;
            }
            if findings.suspect.is_none()
                && ((is_uppercase_latin(left) && is_cjk_char(right))
                    || (is_cjk_char(left) && is_uppercase_latin(right)))
            {
                findings.suspect = Some(token.to_string());
            }
        }
        if findings.reject.is_some() {
            break;
        }
    }
    findings
}

/// Prose view of `content`: fenced code blocks are dropped and inline backtick
/// spans are blanked out, so code is never pattern-matched as prose.
fn prose_lines(content: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_fence = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        lines.push(mask_inline_code(line));
    }
    lines
}

fn mask_inline_code(line: &str) -> String {
    let mut masked = String::with_capacity(line.len());
    let mut in_code = false;
    for c in line.chars() {
        if c == '`' {
            in_code = !in_code;
            masked.push(' ');
        } else if in_code {
            masked.push(' ');
        } else {
            masked.push(c);
        }
    }
    masked
}

/// `cualesPassed`: three or more lowercase letters directly followed by an
/// uppercase letter and another lowercase letter, i.e. two words glued
/// together. Code-ish tokens are filtered out by the caller before this runs.
fn has_joined_word_case(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    let mut lower_run = 0usize;
    for (index, &c) in chars.iter().enumerate() {
        if c.is_lowercase() {
            lower_run += 1;
            continue;
        }
        if c.is_uppercase()
            && lower_run >= 3
            && chars.get(index + 1).is_some_and(|next| next.is_lowercase())
        {
            return true;
        }
        lower_run = 0;
    }
    false
}

/// `tiene.noción`, `prueba.Word`: a period where a space should be, inside
/// prose. Dotted identifiers (`health.status`, `config.json`,
/// `settings.memory.data_dir`) are never flagged.
fn has_dot_joined_word(token: &str) -> bool {
    let Some((left, right)) = token.split_once('.') else {
        return false;
    };
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let Some(left_last) = left.chars().next_back() else {
        return false;
    };
    if !is_latin_letter(left_last) {
        return false;
    }
    let Some(right_first) = right.chars().next() else {
        return false;
    };

    // `word.Word`: capitalized word after the period.
    if is_latin_letter(right_first)
        && right_first.is_uppercase()
        && right.chars().filter(|c| c.is_alphabetic()).count() >= 3
    {
        return true;
    }

    // `word.nón-ascii-word`: the right side carries non-ASCII letters.
    right
        .chars()
        .any(|c| c.is_alphabetic() && !c.is_ascii_alphabetic())
}

fn push_suspect(suspect: &mut Vec<String>, reason: String) {
    if suspect.len() < MAX_SUSPECT_REASONS && !suspect.contains(&reason) {
        suspect.push(reason);
    }
}

/// Validate memory content at write time.
///
/// Xavier stores memories produced by autonomous agents in many languages and
/// in code/JSON payloads, so this gate is deliberately narrow: it rejects the
/// two failure modes that destroy meaning and only *flags* the two that merely
/// look wrong. It never mutates the caller's bytes.
///
/// REJECTED (the write fails, nothing is stored):
/// (a) Control characters other than `\n`, `\r`, `\t`. NUL bytes and escape
///     sequences corrupt the lexical index and make content unprintable, so
///     they are never legitimate memory text.
/// (b) A whitespace-delimited token where a LOWERCASE Latin letter sits
///     immediately next to a CJK/Kana/Hangul letter — no boundary between them —
///     and the whole content is Latin dominant (CJK under 10% of all letters).
///     That is a lost word boundary (`La全区 prioritario`). An UPPERCASE acronym
///     touching CJK is not that: `API接口` inside a Latin-dominant sentence is
///     ordinary technical Spanish, so it is only reported (clause e). The whole
///     scan is skipped in CJK-dominant text, where `这个API接口返回JSON数据` and
///     friends are normal writing.
///
/// FLAGGED as suspect (stored verbatim, stamped `integrity: "suspect"`):
/// (c) A token where three or more lowercase letters are directly followed by
///     an uppercase letter and another lowercase letter (`cualesPassed`).
///     Only prose is scanned: backticks and fenced code blocks are skipped, and
///     tokens with `_`, `::`, `(`, `/` or digits are ignored, so `getMemory()`
///     and `v0.2.15` are not reported.
/// (d) A `word.Word` or `word.nón-ascii` join inside prose (`tiene.noción`),
///     with the same skips plus URLs, emails, paths, numbers, and every token
///     whose part after the dot is lowercase ASCII, so `health.status`,
///     `config.json` and `settings.memory.data_dir` are never flagged.
/// (e) An uppercase Latin acronym glued to CJK inside Latin-dominant content
///     (`API接口`), reported as `mixed_script:<token>`.
///
/// camelCase and dotted identifiers are never rejected: real agent memories are
/// full of them, and a filter that aggressive is a data-loss incident.
pub fn validate_memory_content(content: &str) -> ContentCheck {
    if let Some(c) = content.chars().find(|c| is_forbidden_control(*c)) {
        return ContentCheck {
            reject: Some(format!(
                "content contains the forbidden control character {c:?} (U+{:04X}); only \\n, \\r and \\t are allowed",
                c as u32
            )),
            suspect: Vec::new(),
        };
    }

    let mixed = mixed_script(content);
    if let Some(token) = mixed.reject {
        return ContentCheck {
            reject: Some(format!(
                "content contains the mixed-script token {token:?} (a lowercase Latin letter glued to a CJK letter with no word boundary) while the content is Latin dominant"
            )),
            suspect: Vec::new(),
        };
    }

    let mut suspect = Vec::new();
    if let Some(token) = mixed.suspect {
        push_suspect(&mut suspect, format!("mixed_script:{token}"));
    }
    for line in prose_lines(content) {
        for raw in line.split_whitespace() {
            // Skip code-ish tokens before any pattern matching: identifiers,
            // call sites, paths, scopes, URLs, e-mails and numbers.
            if raw.contains(['_', ':', '(', '/', '@', '\\'])
                || raw.chars().any(|c| c.is_ascii_digit())
            {
                continue;
            }
            let token = trim_token(raw);
            if token.is_empty() {
                continue;
            }
            if has_joined_word_case(token) {
                push_suspect(&mut suspect, format!("joined_words:{token}"));
            }
            if has_dot_joined_word(token) {
                push_suspect(&mut suspect, format!("dot_join:{token}"));
            }
        }
    }

    ContentCheck {
        reject: None,
        suspect,
    }
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

    #[test]
    fn validate_rejects_control_characters_and_mixed_script() {
        let control = validate_memory_content("informe \u{0} trimestral");
        assert!(control.is_rejected());
        assert!(control.reject.unwrap().contains("control character"));

        let mixed =
            validate_memory_content("La\u{5168}\u{533A} prioritario no es agregar features");
        assert!(mixed.is_rejected());
        assert!(mixed.reject.unwrap().contains("mixed-script token"));

        // Newline, carriage return and tab stay allowed.
        assert!(!validate_memory_content("linea 1\nlinea 2\r\ttab").is_rejected());
    }

    /// An uppercase acronym glued to CJK is ordinary technical prose, so it is
    /// reported instead of dropped. Rejecting it was a data-loss bug.
    #[test]
    fn validate_flags_uppercase_acronym_touching_cjk_without_rejecting() {
        let check = validate_memory_content(
            "El informe confirma que el API\u{63a5}\u{53e3} responde bien en producci\u{f3}n.",
        );
        assert!(!check.is_rejected(), "an acronym must not fail the write");
        assert_eq!(
            check.suspect,
            vec!["mixed_script:API\u{63a5}\u{53e3}".to_string()]
        );
        assert_eq!(check.integrity(), "suspect");

        // The same shape glued through a LOWERCASE letter is still corruption.
        assert!(
            validate_memory_content("La regi\u{f3}n\u{63a5}\u{53e3} quedo publicada").is_rejected(),
            "a lowercase Latin letter touching CJK is a lost word boundary"
        );
    }

    /// `×` (U+00D7) and `÷` (U+00F7) sit inside the Latin-1 range but are
    /// operators. Counting them as letters would misjudge what is Latin text.
    #[test]
    fn multiplication_and_division_signs_are_not_latin_letters() {
        assert!(!is_latin_letter('\u{d7}'));
        assert!(!is_latin_letter('\u{f7}'));
        assert!(is_latin_letter('ñ'));
        assert!(is_latin_letter('Z'));
        assert!(!is_latin_letter('全'));
    }

    #[test]
    fn validate_accepts_cjk_dominant_and_dotted_identifiers() {
        // Accepted and left completely clean.
        for content in [
            "\u{8fd9}\u{4e2a}API\u{63a5}\u{53e3}\u{8fd4}\u{56de}JSON\u{6570}\u{636e}",
            "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{6587}\u{7ae0}\u{3082}\u{6b63}\u{5e38}\u{306b}\u{4fdd}\u{5b58}\u{3067}\u{304d}\u{308b}",
            "El gateway reporta que health.status es degraded y settings.memory.data_dir vale /var/lib/xavier",
            "El release v0.2.15 y config.json siguen estables, ver https://github.com/iberi22/xavier",
            "{\"zone\": \"atomic\", \"revision\": 1, \"primary\": true}",
        ] {
            let check = validate_memory_content(content);
            assert!(!check.is_rejected(), "must be accepted: {content}");
            assert!(
                check.suspect.is_empty(),
                "must not be flagged: {content} -> {:?}",
                check.suspect
            );
        }

        // camelCase inside prose is never rejected. It is reported as a suspect
        // because its shape is identical to the observed corruption
        // (`cualesPassed`); the caller keeps the content and the marker tells it
        // apart from verified text.
        let camel = validate_memory_content("Refactor de getMemory() y useState en el panel React");
        assert!(!camel.is_rejected());
        assert_eq!(camel.integrity(), "suspect");
        assert_eq!(camel.suspect, vec!["joined_words:useState".to_string()]);
    }

    #[test]
    fn validate_flags_joins_only_outside_code() {
        let prose = validate_memory_content(
            "ninguno de los cualesPassed review y el agente no tiene.noci\u{f3}n del estado",
        );
        assert!(!prose.is_rejected());
        assert_eq!(prose.integrity(), "suspect");
        assert_eq!(
            prose.suspect,
            vec![
                "joined_words:cualesPassed".to_string(),
                "dot_join:tiene.noci\u{f3}n".to_string()
            ]
        );

        let code = validate_memory_content(
            "```rust\nlet cualesPassed = word.Word;\n```\nEl valor es health.status y data_dir",
        );
        assert!(!code.is_rejected());
        assert!(code.suspect.is_empty(), "{:?}", code.suspect);

        let inline =
            validate_memory_content("Usamos `cualesPassed` y `tiene.noci\u{f3}n` en el codigo");
        assert!(inline.suspect.is_empty(), "{:?}", inline.suspect);
    }

    #[test]
    fn test_is_heartbeat_tick() {
        assert!(is_heartbeat_tick("[tick] cronjob", ""));
        assert!(is_heartbeat_tick("gestalt-thinking-loop ejecutado", ""));
        assert!(is_heartbeat_tick("cron 30m ejecutado", ""));
        assert!(is_heartbeat_tick("arbitrary text", "session/cron_sync"));
        assert!(is_heartbeat_tick(
            "arbitrary text",
            "gestalt/bus/executions/step1.json"
        ));
        assert!(is_heartbeat_tick("arbitrary text", "nodes/heartbeat.json"));
        assert!(!is_heartbeat_tick(
            "User asked how to solve quadratic equation",
            "session/user_chat_1"
        ));
    }
}
