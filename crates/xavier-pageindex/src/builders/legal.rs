//! Legal-text builder (chapters, articles, clauses).
//!
//! The heading table is ported from `src/documents/legal_chunker.rs` without
//! the `regex` crate: every original pattern is an anchored, case-insensitive
//! keyword that tolerates OCR letter spacing ("A R T I C U L O"), and the
//! trailing capture groups there are all optional, so a keyword prefix match
//! is equivalent.

use crate::builders::plain::{build_from_headings, BuiltText, Heading};
use crate::error::PageIndexError;

const CHAPTER: &[&str] = &["CAPÍTULO", "TÍTULO", "CHAPTER", "TITLE"];
const ARTICLE: &[&str] = &["ARTÍCULO", "ART.", "ARTICLE"];
const CLAUSE: &[&str] = &["CLÁUSULA", "CLAUSE"];
const PARAGRAPH: &[&str] = &["PARÁGRAFO", "PARAGRAPH", "SUB-CLÁUSULA", "SUBCLAUSULA"];
const PREAMBLE: &[&str] = &["CONSIDERANDO", "ANTECEDENTES", "PREAMBLE", "WHEREAS"];

/// Hierarchy depth of a heading kind.
const LEVEL_CHAPTER: u8 = 1;
const LEVEL_ARTICLE: u8 = 2;
const LEVEL_PARAGRAPH: u8 = 3;

/// Build a chapter > article/clause > paragraph tree from legal text.
pub fn build(doc_id: &str, text: &str, lines_per_page: usize) -> Result<BuiltText, PageIndexError> {
    let lines: Vec<&str> = text.lines().collect();
    let headings: Vec<Heading> = lines
        .iter()
        .enumerate()
        .filter_map(|(line, raw)| {
            let trimmed = raw.trim();
            detect_header(trimmed).map(|level| Heading {
                level,
                title: clean_whitespace(trimmed),
                line,
            })
        })
        .collect();
    build_from_headings(doc_id, &lines, &headings, lines_per_page, "legal")
}

/// Same precedence as the original `detect_header`.
fn detect_header(line: &str) -> Option<u8> {
    let normalized = clean_whitespace(line);
    let hit =
        |kws: &[&str]| starts_with_keyword(&normalized, kws) || starts_with_keyword(line, kws);
    if hit(PREAMBLE) || hit(CHAPTER) {
        Some(LEVEL_CHAPTER)
    } else if hit(ARTICLE) || hit(CLAUSE) {
        Some(LEVEL_ARTICLE)
    } else if hit(PARAGRAPH) {
        Some(LEVEL_PARAGRAPH)
    } else {
        None
    }
}

/// Case-insensitive prefix match allowing whitespace between keyword letters.
/// `Í`/`Á` in a keyword also accept the unaccented letter.
fn starts_with_keyword(line: &str, keywords: &[&str]) -> bool {
    keywords
        .iter()
        .any(|kw| matches_keyword(line.trim_start(), kw))
}

fn matches_keyword(line: &str, keyword: &str) -> bool {
    let mut chars = line.chars().peekable();
    for (idx, want) in keyword.chars().enumerate() {
        if idx > 0 {
            while chars.next_if(|c| c.is_whitespace()).is_some() {}
        }
        let Some(got) = chars.next() else {
            return false;
        };
        let got = got.to_uppercase().next().unwrap_or(got);
        let ok = got == want || (want == 'Í' && got == 'I') || (want == 'Á' && got == 'A');
        if !ok {
            return false;
        }
    }
    true
}

/// Normalize OCR spacing ("A R T Í C U L O" -> "ARTÍCULO").
fn clean_whitespace(input: &str) -> String {
    collapse_ocr_spaces(input)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn collapse_ocr_spaces(input: &str) -> String {
    let words: Vec<&str> = input.split_whitespace().collect();
    let single = |w: &str| -> Option<char> {
        let mut it = w.chars();
        match (it.next(), it.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    };
    let mut result = String::new();
    let mut i = 0;
    while i < words.len() {
        if let Some(first) = single(words[i]) {
            let is_alpha = first.is_alphabetic();
            let is_digit = first.is_ascii_digit();
            if is_alpha || is_digit {
                let mut j = i;
                while let Some(c) = words.get(j).and_then(|w| single(w)) {
                    if (is_alpha && c.is_alphabetic()) || (is_digit && c.is_ascii_digit()) {
                        result.push(c);
                        j += 1;
                    } else {
                        break;
                    }
                }
                result.push(' ');
                i = j;
                continue;
            }
        }
        result.push_str(words[i]);
        result.push(' ');
        i += 1;
    }
    result.trim().to_string()
}
