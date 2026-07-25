//! Snippet extraction and title resolution module.
//!
//! Provides utilities to extract concise excerpts and titles from document content,
//! supporting a budget-based size limit, safe character-based clipping to avoid UTF-8
//! multi-byte panics, frontmatter stripping, and query-aware match centering.

use serde_json::Value;

/// Budget bounds for the generated title and snippet in character count.
#[derive(Debug, Clone, Copy)]
pub struct SnippetBudget {
    pub title: usize,
    pub snippet: usize,
}

/// The extracted title, snippet, and original body character count.
#[derive(Debug, Clone)]
pub struct Excerpt {
    pub title: String,
    pub snippet: String,
    pub chars: usize,
}

/// Extracts a query-aware snippet and resolves the title from content and metadata,
/// adhering to the bounds specified in SnippetBudget.
pub fn extract(content: &str, metadata: &Value, query: &str, budget: SnippetBudget) -> Excerpt {
    let (body, frontmatter) = strip_frontmatter(content);
    let title = extract_title(body, frontmatter, metadata);
    let clipped_title = clip_chars(&title, budget.title).to_string();

    let chars_count = body.chars().count();

    let snippet = if query.trim().is_empty() {
        clip_chars(body, budget.snippet).to_string()
    } else {
        let (_, end_opt) = find_window(body, query);
        if let Some(end) = end_opt {
            let start = end - query.trim().chars().count();
            let total_chars = body.chars().count();

            if query.trim().chars().count() >= budget.snippet {
                // Query is larger than or equal to budget, take the start of query
                let chars_vec: Vec<char> = body.chars().collect();
                chars_vec[start..start + budget.snippet].iter().collect()
            } else {
                let extra = budget.snippet - (end - start);
                let half_extra = extra / 2;

                let mut start_idx = if start >= half_extra {
                    start - half_extra
                } else {
                    0
                };

                let mut end_idx = start_idx + budget.snippet;
                if end_idx > total_chars {
                    end_idx = total_chars;
                    if end_idx >= budget.snippet {
                        start_idx = end_idx - budget.snippet;
                    } else {
                        start_idx = 0;
                    }
                }

                let chars_vec: Vec<char> = body.chars().collect();
                chars_vec[start_idx..end_idx].iter().collect()
            }
        } else {
            clip_chars(body, budget.snippet).to_string()
        }
    };

    Excerpt {
        title: clipped_title,
        snippet,
        chars: chars_count,
    }
}

/// Clips a string to at most `max` characters (not bytes) safely without panicking.
pub fn clip_chars(s: &str, max: usize) -> &str {
    if s.is_empty() || max == 0 {
        return "";
    }
    let mut char_count = 0;
    let mut byte_idx = 0;
    for c in s.chars() {
        if char_count == max {
            break;
        }
        char_count += 1;
        byte_idx += c.len_utf8();
    }
    &s[..byte_idx]
}

/// Identifies and extracts frontmatter block from the beginning of the content.
/// Returns a tuple of (body, frontmatter_content).
pub fn strip_frontmatter(content: &str) -> (&str, Option<&str>) {
    let trimmed_start = content.trim_start();
    if !trimmed_start.starts_with("---") {
        return (content, None);
    }

    let mut lines = trimmed_start.lines();
    if let Some(first_line) = lines.next() {
        if first_line.trim() != "---" {
            return (content, None);
        }
    } else {
        return (content, None);
    }

    // Find the closing "---" line and track byte offsets.
    let line_start_offset = trimmed_start.as_ptr() as usize - content.as_ptr() as usize;
    let first_line_len = trimmed_start.lines().next().unwrap_or("").len();
    let mut current_offset = line_start_offset + first_line_len;

    if current_offset < content.len() {
        let rest = &content[current_offset..];
        let bytes_to_skip = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
        current_offset += bytes_to_skip;
    }

    let mut lines_iter = content[current_offset..].lines();
    let mut found_closing = false;
    let mut frontmatter_end_offset = current_offset;
    let mut current_line_start = current_offset;

    while let Some(line) = lines_iter.next() {
        if line.trim() == "---" {
            found_closing = true;
            frontmatter_end_offset = current_line_start + line.len();
            break;
        }
        current_line_start += line.len();
        let rest = &content[current_line_start..];
        if rest.starts_with("\r\n") {
            current_line_start += 2;
        } else if rest.starts_with('\n') || rest.starts_with('\r') {
            current_line_start += 1;
        }
    }

    if found_closing {
        let mut body_start = frontmatter_end_offset;
        let rest = &content[body_start..];
        if rest.starts_with("\r\n") {
            body_start += 2;
        } else if rest.starts_with('\n') || rest.starts_with('\r') {
            body_start += 1;
        }
        let body = &content[body_start..];

        let first_line_end = line_start_offset + first_line_len;
        let mut fm_start = first_line_end;
        let rest_fm = &content[fm_start..];
        if rest_fm.starts_with("\r\n") {
            fm_start += 2;
        } else if rest_fm.starts_with('\n') || rest_fm.starts_with('\r') {
            fm_start += 1;
        }

        let fm_end = current_line_start;
        let frontmatter = &content[fm_start..fm_end];

        (body, Some(frontmatter.trim()))
    } else {
        (content, None)
    }
}

/// Resolves title prioritizing metadata, frontmatter, heading, and then path.
pub fn extract_title(body: &str, frontmatter: Option<&str>, metadata: &Value) -> String {
    if let Some(title) = metadata.get("title").and_then(|v| v.as_str()) {
        if !title.is_empty() {
            return title.to_string();
        }
    }

    if let Some(fm) = frontmatter {
        for line in fm.lines() {
            if let Some((key, value)) = line.split_once(':') {
                if key.trim() == "title" {
                    let clean_val = value.trim().trim_matches('"').trim_matches('\'').trim();
                    if !clean_val.is_empty() {
                        return clean_val.to_string();
                    }
                }
            }
        }
    }

    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let mut chars = trimmed.chars();
            let mut h_count = 0;
            while chars.next() == Some('#') {
                h_count += 1;
            }
            let rest = &trimmed[h_count..];
            if rest.starts_with(' ') {
                let title = rest.trim();
                if !title.is_empty() {
                    return title.to_string();
                }
            }
        }
    }

    if let Some(path) = metadata.get("path").and_then(|v| v.as_str()) {
        return path.to_string();
    }
    if let Some(path) = metadata.get("file_path").and_then(|v| v.as_str()) {
        return path.to_string();
    }

    "Untitled".to_string()
}

/// Finds the first character-based occurrence of query (case-insensitive) in body.
/// Returns `(start_char_idx, Some(end_char_idx))` or `(0, None)` if not found.
pub fn find_window(body: &str, query: &str) -> (usize, Option<usize>) {
    let q = query.trim();
    if q.is_empty() {
        return (0, None);
    }

    let body_lower = body.to_lowercase();
    let q_lower = q.to_lowercase();

    if let Some(byte_idx) = body_lower.find(&q_lower) {
        let char_idx = body_lower[..byte_idx].chars().count();
        let query_char_len = q_lower.chars().count();
        (char_idx, Some(char_idx + query_char_len))
    } else {
        (0, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_clip_chars() {
        assert_eq!(clip_chars("text with emoji 🐙", 5), "text ");
        assert_eq!(clip_chars("🐙 emoji text", 5), "🐙 emo");
        assert_eq!(clip_chars("", 5), "");
        assert_eq!(clip_chars("abc", 0), "");
    }

    #[test]
    fn test_strip_frontmatter() {
        let content = r#"---
title: "Markdown Title"
tags: [test, memory]
---
# Main Heading
This is the document body."#;

        let (body, frontmatter) = strip_frontmatter(content);
        assert_eq!(body, "# Main Heading\nThis is the document body.");
        assert!(frontmatter.is_some());
        let fm = frontmatter.unwrap();
        assert!(fm.contains("title: \"Markdown Title\""));
        assert!(fm.contains("tags: [test, memory]"));
    }

    #[test]
    fn test_strip_frontmatter_none() {
        let content = "# Head\nNo frontmatter here.";
        let (body, frontmatter) = strip_frontmatter(content);
        assert_eq!(body, content);
        assert!(frontmatter.is_none());
    }

    #[test]
    fn test_extract_title_metadata_priority() {
        let metadata = json!({
            "title": "Metadata Title",
            "path": "/docs/ref.md"
        });
        let body = "# Heading Title";
        let frontmatter = Some("title: Frontmatter Title");

        let title = extract_title(body, frontmatter, &metadata);
        assert_eq!(title, "Metadata Title");
    }

    #[test]
    fn test_extract_title_frontmatter_priority() {
        let metadata = json!({
            "path": "/docs/ref.md"
        });
        let body = "# Heading Title";
        let frontmatter = Some("title: Frontmatter Title");

        let title = extract_title(body, frontmatter, &metadata);
        assert_eq!(title, "Frontmatter Title");
    }

    #[test]
    fn test_extract_title_heading_priority() {
        let metadata = json!({
            "path": "/docs/ref.md"
        });
        let body = "# Heading Title\nSome content";
        let frontmatter = None;

        let title = extract_title(body, frontmatter, &metadata);
        assert_eq!(title, "Heading Title");
    }

    #[test]
    fn test_extract_title_fallback_path() {
        let metadata = json!({
            "path": "/docs/ref.md"
        });
        let body = "Some content without headings";
        let frontmatter = None;

        let title = extract_title(body, frontmatter, &metadata);
        assert_eq!(title, "/docs/ref.md");
    }

    #[test]
    fn test_find_window() {
        let body = "This is a simple text search scenario.";
        let (start, end) = find_window(body, "simple");
        assert_eq!(start, 10);
        assert_eq!(end, Some(16));
    }

    #[test]
    fn test_extract_snippet() {
        let content = r#"---
title: "A Great Article"
---
# Welcome
In this article, we discuss the beauty of Rust's safe memory management models.
We will show that Rust eliminates whole classes of bugs."#;

        let budget = SnippetBudget {
            title: 10,
            snippet: 25,
        };

        let metadata = json!({});
        let excerpt = extract(content, &metadata, "safe memory", budget);

        assert_eq!(excerpt.title, "A Great Ar");
        assert_eq!(excerpt.snippet.len(), 25);
        assert!(excerpt.snippet.contains("safe memory"));
    }
}
