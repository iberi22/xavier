//! Snippet extraction and title resolution module.
//!
//! Re-exports snippet utilities from `xavier-core-logic` and provides
//! smart word-boundary snippet clipping.

pub use xavier_core_logic::snippet::{
    extract, extract_title, find_window, strip_frontmatter, Excerpt, SnippetBudget,
};

/// Helper to check whether a character represents a word/clause boundary delimiter.
fn is_boundary_char(c: char) -> bool {
    c.is_whitespace()
        || c.is_ascii_punctuation()
        || matches!(
            c,
            '—' | '–'
                | '…'
                | '。'
                | '、'
                | '，'
                | '；'
                | '：'
                | '！'
                | '？'
                | '”'
                | '’'
                | '»'
                | '«'
        )
}

/// Clips a string up to `max` characters (not bytes), cutting cleanly at a word
/// (whitespace or punctuation) boundary before `max` characters to avoid splitting
/// words in half.
pub fn clip_chars(s: &str, max: usize) -> &str {
    if s.is_empty() || max == 0 {
        return "";
    }
    let char_count = s.chars().count();
    if char_count <= max {
        return s;
    }

    // Find the byte offset corresponding to `max` chars
    let mut byte_limit = 0;
    for (i, c) in s.chars().enumerate() {
        if i == max {
            break;
        }
        byte_limit += c.len_utf8();
    }

    let prefix = &s[..byte_limit];
    let next_char = s[byte_limit..].chars().next().unwrap_or(' ');
    if is_boundary_char(next_char) {
        return prefix.trim_end();
    }

    // Search backwards for the last whitespace or punctuation boundary
    if let Some((idx, _)) = prefix.char_indices().rfind(|(_, c)| is_boundary_char(*c)) {
        let trimmed = &prefix[..idx];
        let trimmed_end = trimmed.trim_end();
        if !trimmed_end.is_empty() {
            return trimmed_end;
        }
    }

    // Fallback: if no boundary exists (e.g. single unbroken word/token),
    // truncate at character limit
    prefix
}

/// Truncates `text` at a whitespace or punctuation boundary before `max_chars`
/// and appends a clean ellipsis `...` only if truncated.
pub fn clip_smart_boundary(text: &str, max_chars: usize) -> String {
    if text.is_empty() || max_chars == 0 {
        return String::new();
    }
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    let clipped = clip_chars(text, max_chars);
    let clean = clipped.trim_end_matches(['.', ' ', '\t', '\r', '\n']);
    if clean.is_empty() {
        String::new()
    } else {
        format!("{clean}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_within_budget_no_ellipsis() {
        let text = "cargo clippy";
        assert_eq!(clip_smart_boundary(text, 20), "cargo clippy");
        assert_eq!(clip_smart_boundary(text, 12), "cargo clippy");
    }

    #[test]
    fn test_avoids_cutting_word_in_half() {
        let text = "cargo clippy for linting";
        // 10 chars ("cargo clip") rigidly cuts "clippy" into "clip"
        // With smart boundary it backtracks to "cargo" and appends clean ellipsis
        assert_eq!(clip_smart_boundary(text, 10), "cargo...");
        // 15 chars ("cargo clippy fo") cuts "for", preserving full word "clippy"
        assert_eq!(clip_smart_boundary(text, 15), "cargo clippy...");
    }

    #[test]
    fn test_punctuation_boundaries() {
        let text = "hello, world! how are you?";
        assert_eq!(clip_smart_boundary(text, 14), "hello, world!...");

        let text_dots = "completed task successfully.";
        assert_eq!(clip_smart_boundary(text_dots, 18), "completed task...");
    }

    #[test]
    fn test_empty_and_zero_max_chars() {
        assert_eq!(clip_smart_boundary("", 10), "");
        assert_eq!(clip_smart_boundary("hello", 0), "");
    }

    #[test]
    fn test_unbroken_token_fallback() {
        let long_token = "Supercalifragilisticexpialidocious";
        assert_eq!(clip_smart_boundary(long_token, 10), "Supercalif...");
    }

    #[test]
    fn test_code_blocks() {
        let code = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
        let clipped = clip_smart_boundary(code, 15);
        assert!(clipped.ends_with("..."));
        assert!(!clipped.contains("println"));
    }

    #[test]
    fn test_multibyte_utf8_strings() {
        let emoji_text = "🦀 🚀 🎯 ⚡ 📦 testing multi-byte UTF-8 emojis";
        let clipped = clip_smart_boundary(emoji_text, 8);
        assert_eq!(clipped, "🦀 🚀 🎯 ⚡...");

        let cjk_text = "你好世界，欢迎使用 Xavier 记忆系统";
        let clipped_cjk = clip_smart_boundary(cjk_text, 7);
        assert_eq!(clipped_cjk, "你好世界...");
    }
}
