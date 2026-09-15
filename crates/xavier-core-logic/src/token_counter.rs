//! Token counting, UTF-8 safety estimation, and streaming diff/AST token compression utilities.

use serde::{Deserialize, Serialize};

/// Estimate the token count of a given text in a UTF-8 multi-byte safe manner.
///
/// Standard subword tokenization heuristic: ~1 token per 4 characters (rounded up).
pub fn estimate_tokens_utf8(text: &str) -> usize {
    let char_count = text.chars().count();
    if char_count == 0 {
        0
    } else {
        (char_count + 3) / 4
    }
}

/// Statistics describing token savings achieved by token compression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompressionStats {
    pub original_tokens: usize,
    pub compressed_tokens: usize,
}

impl CompressionStats {
    /// Calculate the absolute number of tokens saved.
    pub fn tokens_saved(&self) -> usize {
        self.original_tokens.saturating_sub(self.compressed_tokens)
    }

    /// Calculate the token savings percentage (0.0 to 100.0).
    pub fn savings_percentage(&self) -> f64 {
        if self.original_tokens == 0 {
            0.0
        } else {
            (self.tokens_saved() as f64 / self.original_tokens as f64) * 100.0
        }
    }

    /// Format statistics as standard representation e.g. "Tokens saved: 42 (18.5%)".
    pub fn display_summary(&self) -> String {
        format!(
            "Tokens saved: {} ({:.1}%)",
            self.tokens_saved(),
            self.savings_percentage()
        )
    }
}

/// Compress repeated AST snippets and redundant streaming diff lines using
/// sliding window duplicate elimination.
///
/// Collapses consecutive identical or highly redundant code/diff lines into
/// condensed representation while preserving structural context.
pub fn collapse_repeated_ast_snippets(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut result = Vec::new();
    let mut idx = 0;

    while idx < lines.len() {
        let current_line = lines[idx];
        let mut repeat_count = 1;

        // Check for consecutive duplicate lines
        while idx + repeat_count < lines.len() && lines[idx + repeat_count] == current_line {
            repeat_count += 1;
        }

        if repeat_count >= 3 {
            result.push(current_line.to_string());
            result.push(format!("// [... repeated AST/diff line x{} ...]", repeat_count - 1));
            idx += repeat_count;
            continue;
        }

        // Check for sliding window of multi-line block repetitions (window size 2..=4)
        let mut block_collapsed = false;
        for window_size in (2..=4).rev() {
            if idx + window_size * 2 <= lines.len() {
                let block_a = &lines[idx..idx + window_size];
                let block_b = &lines[idx + window_size..idx + window_size * 2];

                if block_a == block_b {
                    let mut block_repeats = 1;
                    while idx + (block_repeats + 1) * window_size <= lines.len()
                        && &lines[idx + block_repeats * window_size..idx + (block_repeats + 1) * window_size] == block_a
                    {
                        block_repeats += 1;
                    }

                    if block_repeats >= 2 {
                        for line in block_a {
                            result.push(line.to_string());
                        }
                        result.push(format!(
                            "// [... repeated AST pattern x{} ({}-line block) ...]",
                            block_repeats - 1,
                            window_size
                        ));
                        idx += block_repeats * window_size;
                        block_collapsed = true;
                        break;
                    }
                }
            }
        }

        if !block_collapsed {
            result.push(current_line.to_string());
            idx += 1;
        }
    }

    // Preserve trailing newline if present in original input
    let mut compressed = result.join("\n");
    if text.ends_with('\n') {
        compressed.push('\n');
    }
    compressed
}

/// Compress text and calculate instantaneous token compression statistics.
pub fn compress_and_calculate_stats(text: &str) -> (String, CompressionStats) {
    let compressed = collapse_repeated_ast_snippets(text);
    let original_tokens = estimate_tokens_utf8(text);
    let compressed_tokens = estimate_tokens_utf8(&compressed);

    let stats = CompressionStats {
        original_tokens,
        compressed_tokens,
    };

    (compressed, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_utf8_multibyte_safe() {
        assert_eq!(estimate_tokens_utf8(""), 0);
        assert_eq!(estimate_tokens_utf8("hello"), 2);
        // Multi-byte UTF-8 Spanish/Emoji test: 5 chars -> 2 tokens
        assert_eq!(estimate_tokens_utf8("caminó"), 2);
        assert_eq!(estimate_tokens_utf8("🦀🦀🦀🦀"), 1);
        assert_eq!(estimate_tokens_utf8("🦀🦀🦀🦀🦀"), 2);
    }

    #[test]
    fn test_collapse_repeated_ast_snippets_single_lines() {
        let input = "fn main() {\n    let x = 1;\n    let x = 1;\n    let x = 1;\n    let x = 1;\n}\n";
        let compressed = collapse_repeated_ast_snippets(input);
        assert!(compressed.contains("// [... repeated AST/diff line x3 ...]"));
        assert!(compressed.contains("let x = 1;"));
    }

    #[test]
    fn test_collapse_repeated_ast_snippets_block_pattern() {
        let input = "diff --git a/file.rs b/file.rs\n+ line a\n+ line b\n+ line a\n+ line b\n+ line a\n+ line b\n";
        let compressed = collapse_repeated_ast_snippets(input);
        assert!(compressed.contains("repeated AST pattern"));
    }

    #[test]
    fn test_compression_stats_savings_ratio() {
        let stats = CompressionStats {
            original_tokens: 100,
            compressed_tokens: 75,
        };
        assert_eq!(stats.tokens_saved(), 25);
        assert_eq!(stats.savings_percentage(), 25.0);
        assert_eq!(stats.display_summary(), "Tokens saved: 25 (25.0%)");
    }

    #[test]
    fn test_compress_and_calculate_stats_integration() {
        let line = "+ let mut buffer = vec![0u8; 1024]; // allocate temporary buffer for streaming diff token compression\n";
        let input = line.repeat(10);
        let (compressed, stats) = compress_and_calculate_stats(&input);
        assert!(compressed.contains("repeated AST/diff line"));
        assert!(stats.tokens_saved() > 0);
        assert!(stats.savings_percentage() > 0.0);
    }
}
