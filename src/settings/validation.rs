//! Logical concern: Validation helpers for Xavier settings.
//!
//! This module contains functions to validate and sanitize configuration values.

/// Returns Some(trimmed_string) if not empty, otherwise None.
pub fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
