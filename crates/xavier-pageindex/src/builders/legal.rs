//! Legal-text builder (chapters, articles, clauses).
//!
//! The heading table is ported from `src/documents/legal_chunker.rs` without
//! the `regex` crate: every original pattern is an anchored, case-insensitive
//! keyword that tolerates OCR letter spacing ("A R T I C U L O"), and the
//! trailing capture groups there are all optional, so a keyword prefix match
//! is equivalent for the keyword itself. Unlike the original (which would also
//! treat "Title to the goods" as a heading), the text after the keyword must
//! be a word boundary followed by nothing or a numeral/ordinal.

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
    let hit = |kws: &[&str], numbered: bool| {
        starts_with_keyword(&normalized, kws, numbered) || starts_with_keyword(line, kws, numbered)
    };
    if hit(PREAMBLE, false) || hit(CHAPTER, true) {
        Some(LEVEL_CHAPTER)
    } else if hit(ARTICLE, true) || hit(CLAUSE, true) {
        Some(LEVEL_ARTICLE)
    } else if hit(PARAGRAPH, true) {
        Some(LEVEL_PARAGRAPH)
    } else {
        None
    }
}

/// Case-insensitive prefix match allowing whitespace between keyword letters.
/// `Í`/`Á` in a keyword also accept the unaccented letter.
fn starts_with_keyword(line: &str, keywords: &[&str], numbered: bool) -> bool {
    keywords
        .iter()
        .any(|kw| matches_keyword(line.trim_start(), kw, numbered))
}

/// Ordinal words (Spanish and English) accepted after a keyword.
const ORDINALS: &[&str] = &[
    "PRIMERO",
    "PRIMERA",
    "SEGUNDO",
    "SEGUNDA",
    "TERCERO",
    "TERCERA",
    "CUARTO",
    "CUARTA",
    "QUINTO",
    "QUINTA",
    "SEXTO",
    "SEXTA",
    "SEPTIMO",
    "SÉPTIMO",
    "SEPTIMA",
    "SÉPTIMA",
    "OCTAVO",
    "OCTAVA",
    "NOVENO",
    "NOVENA",
    "DECIMO",
    "DÉCIMO",
    "DECIMA",
    "DÉCIMA",
    "FIRST",
    "SECOND",
    "THIRD",
    "FOURTH",
    "FIFTH",
    "SIXTH",
    "SEVENTH",
    "EIGHTH",
    "NINTH",
    "TENTH",
    "ONE",
    "TWO",
    "UNICO",
    "ÚNICO",
    "PRELIMINAR",
    "FINAL",
];

/// True when `token` is a canonical roman numeral (I..MMMCMXCIX).
fn is_roman(token: &str) -> bool {
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut rest = token;
    let mut prev = u32::MAX;
    let mut total = 0u32;
    for (value, sym) in TABLE {
        let mut count = 0;
        while let Some(r) = rest.strip_prefix(sym) {
            rest = r;
            count += 1;
            total += value;
            // Only M, C, X and I may repeat, at most 3 times.
            if count > 3 || (count > 1 && !matches!(sym, "M" | "C" | "X" | "I")) {
                return false;
            }
        }
        if count > 0 && value > prev {
            return false;
        }
        if count > 0 {
            prev = value;
        }
    }
    rest.is_empty() && total > 0
}

/// Numeral or ordinal right after a keyword: "3", "3bis", "IV", "Primera".
fn is_numeral_token(token: &str) -> bool {
    let upper = token.to_uppercase();
    let core = upper.trim_end_matches(['º', 'ª', '°']);
    if core.is_empty() {
        return false;
    }
    core.starts_with(|c: char| c.is_ascii_digit()) || is_roman(core) || ORDINALS.contains(&core)
}

/// Heading grammar for the text after a keyword: end of line, or an
/// optional separator followed by a numeral/ordinal token.
fn valid_heading_tail(rest: &str, numbered: bool) -> bool {
    if !numbered {
        return true;
    }
    let rest = rest.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, ':' | '.' | '-' | '–' | '—' | '#')
    });
    if rest.is_empty() {
        return true;
    }
    let token: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, 'º' | 'ª' | '°'))
        .collect();
    is_numeral_token(&token)
}

fn matches_keyword(line: &str, keyword: &str, numbered: bool) -> bool {
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
    let rest: String = chars.collect();
    // The keyword must end on a word boundary ("Articles", "Chaptered" fail);
    // "ART." ends in punctuation and needs no boundary.
    let boundary = keyword.ends_with('.') || !rest.starts_with(|c: char| c.is_alphanumeric());
    boundary && valid_heading_tail(&rest, numbered)
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
