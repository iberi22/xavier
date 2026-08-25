//! Executor Bridge — Apply PreciseChange deltas and calculate token savings benchmarks.
//!
//! Issue #1435: Direct execution hook for Issue Context Packager.
//! Applies PreciseChange snippets directly to source text with exact character/line
//! validation, mismatch verification, and provides token efficiency metrics.

use crate::codebase::snapshot::PreciseChange;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// Metrics and token savings benchmark report comparing full-file transfer vs precise delta packaging.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenSavingsReport {
    /// Estimated tokens for sending the full file.
    pub full_file_tokens: usize,
    /// Estimated tokens for sending only the precise delta (before + after snippets + metadata).
    pub precise_delta_tokens: usize,
    /// Absolute token reduction.
    pub tokens_saved: usize,
    /// Percentage savings (0.0 to 100.0%).
    pub savings_percentage: f64,
}

/// Estimate token count using standard 3.8 - 4.0 chars per token heuristic for source code.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let chars = text.chars().count();
    ((chars as f64) / 3.8).ceil() as usize
}

/// Calculate token savings achieved by using `PreciseChange` instead of transmitting full file content.
pub fn calculate_token_savings(full_content: &str, changes: &[PreciseChange]) -> TokenSavingsReport {
    let full_file_tokens = estimate_tokens(full_content);
    
    let mut delta_text = String::new();
    for change in changes {
        delta_text.push_str(&change.file);
        delta_text.push_str(&change.symbol);
        delta_text.push_str(&change.before_snippet);
        delta_text.push_str(&change.after_snippet);
    }
    
    let precise_delta_tokens = estimate_tokens(&delta_text);
    let tokens_saved = if full_file_tokens > precise_delta_tokens {
        full_file_tokens - precise_delta_tokens
    } else {
        0
    };
    
    let savings_percentage = if full_file_tokens > 0 {
        ((tokens_saved as f64) / (full_file_tokens as f64)) * 100.0
    } else {
        0.0
    };

    TokenSavingsReport {
        full_file_tokens,
        precise_delta_tokens,
        tokens_saved,
        savings_percentage,
    }
}

/// Apply a single [`PreciseChange`] to the provided file content.
pub fn apply_precise_change(content: &str, change: &PreciseChange) -> Result<String> {
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // Insertion at line (when before_snippet is empty)
    if change.before_snippet.trim().is_empty() && !change.after_snippet.is_empty() {
        let insert_idx = if change.start_line == 0 {
            0
        } else if (change.start_line as usize) <= total_lines {
            (change.start_line as usize) - 1
        } else {
            total_lines
        };

        let mut new_lines = Vec::with_capacity(total_lines + 10);
        new_lines.extend_from_slice(&lines[..insert_idx]);
        for after_line in change.after_snippet.lines() {
            new_lines.push(after_line);
        }
        new_lines.extend_from_slice(&lines[insert_idx..]);
        let mut res = new_lines.join("\n");
        if content.ends_with('\n') {
            res.push('\n');
        }
        return Ok(res);
    }

    // Line-range replacement mode
    if change.start_line > 0 && (change.start_line as usize) <= total_lines {
        let start_idx = (change.start_line as usize) - 1;
        let end_idx = (change.end_line as usize).min(total_lines);

        if start_idx <= end_idx {
            let target_slice = &lines[start_idx..end_idx];
            let current_target = target_slice.join("\n");

            // Normalize whitespace for resilient matching
            if current_target.trim() == change.before_snippet.trim() {
                let mut new_lines = Vec::new();
                new_lines.extend_from_slice(&lines[..start_idx]);
                if !change.after_snippet.is_empty() {
                    for line in change.after_snippet.lines() {
                        new_lines.push(line);
                    }
                }
                new_lines.extend_from_slice(&lines[end_idx..]);
                let mut res = new_lines.join("\n");
                if content.ends_with('\n') {
                    res.push('\n');
                }
                return Ok(res);
            }
        }
    }

    // Fallback: Exact character substring replacement
    let before_trimmed = change.before_snippet.trim();
    if !before_trimmed.is_empty() {
        if let Some(pos) = content.find(&change.before_snippet) {
            let mut new_content = String::with_capacity(content.len() + change.after_snippet.len());
            new_content.push_str(&content[..pos]);
            new_content.push_str(&change.after_snippet);
            new_content.push_str(&content[pos + change.before_snippet.len()..]);
            return Ok(new_content);
        }

        // Try trimmed substring search
        if let Some(pos) = content.find(before_trimmed) {
            let mut new_content = String::with_capacity(content.len() + change.after_snippet.len());
            new_content.push_str(&content[..pos]);
            new_content.push_str(change.after_snippet.trim());
            new_content.push_str(&content[pos + before_trimmed.len()..]);
            return Ok(new_content);
        }
    }

    Err(anyhow!(
        "PreciseChange mismatch: Target snippet for symbol '{}' at lines {}-{} in '{}' did not match file content",
        change.symbol,
        change.start_line,
        change.end_line,
        change.file
    ))
}

/// Apply a sequence of [`PreciseChange`] objects in order.
pub fn apply_precise_changes(initial_content: &str, changes: &[PreciseChange]) -> Result<String> {
    let mut current = initial_content.to_string();
    for change in changes {
        current = apply_precise_change(&current, change)
            .with_context(|| format!("Failed applying change to symbol '{}'", change.symbol))?;
    }
    Ok(current)
}
