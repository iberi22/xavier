//! Plain-text builder (numbered headings, fixed windows).
//!
//! Every detected heading starts a new virtual page, so node page ranges are
//! disjoint by construction. Without headings the text is cut into fixed
//! line windows with one flat node per window.

use crate::error::PageIndexError;
use crate::model::{DocumentTree, Page, TreeNode};

/// Default virtual page size in lines.
pub const DEFAULT_LINES_PER_PAGE: usize = 60;

const MAX_HEADING_CHARS: usize = 100;
const MAX_CAPS_CHARS: usize = 60;
const MAX_CAPS_WORDS: usize = 8;
const MAX_LEVEL: u8 = 6;

/// Result of a text builder run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltText {
    pub tree: DocumentTree,
    pub pages: Vec<Page>,
    /// Label stored in `Document.builder`.
    pub builder: &'static str,
}

/// A detected heading at a 0-based line index; `level` starts at 1.
#[derive(Debug, Clone)]
pub(crate) struct Heading {
    pub level: u8,
    pub title: String,
    pub line: usize,
}

/// Build a tree from plain text using heading heuristics.
pub fn build(doc_id: &str, text: &str, lines_per_page: usize) -> Result<BuiltText, PageIndexError> {
    let lines: Vec<&str> = text.lines().collect();
    let mut headings = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let prev_blank = i == 0 || lines[i - 1].trim().is_empty();
        if let Some((level, title)) = detect_heading(line, prev_blank) {
            headings.push(Heading {
                level,
                title,
                line: i,
            });
        }
    }
    build_from_headings(doc_id, &lines, &headings, lines_per_page, "plain-headings")
}

/// Shared tail of the plain and legal builders.
pub(crate) fn build_from_headings(
    doc_id: &str,
    lines: &[&str],
    headings: &[Heading],
    lines_per_page: usize,
    builder: &'static str,
) -> Result<BuiltText, PageIndexError> {
    if lines.iter().all(|l| l.trim().is_empty()) {
        return Err(PageIndexError::Build("document has no text".into()));
    }
    let lpp = lines_per_page.max(1);
    if headings.is_empty() {
        return Ok(fixed_windows(doc_id, lines, lpp));
    }
    let (pages, roots) = assemble(doc_id, lines, headings, lpp);
    let mut tree = DocumentTree {
        doc_id: doc_id.to_string(),
        roots,
    };
    tree.assign_node_ids();
    Ok(BuiltText {
        tree,
        pages,
        builder,
    })
}

struct Flat {
    level: u8,
    title: String,
    start: u32,
    end: u32,
}

fn assemble(
    doc_id: &str,
    lines: &[&str],
    headings: &[Heading],
    lpp: usize,
) -> (Vec<Page>, Vec<TreeNode>) {
    let mut pages: Vec<Page> = Vec::new();
    let push_segment = |seg: &[&str], pages: &mut Vec<Page>| -> u32 {
        let first = pages.len() as u32 + 1;
        for chunk in seg.chunks(lpp) {
            pages.push(Page {
                doc_id: doc_id.to_string(),
                page_no: pages.len() as u32 + 1,
                text: chunk.join("\n"),
            });
        }
        first
    };

    let lead = &lines[..headings[0].line];
    let lead_has_text = lead.iter().any(|l| !l.trim().is_empty());
    let preamble_start = lead_has_text.then(|| push_segment(lead, &mut pages));

    let mut flat: Vec<Flat> = Vec::with_capacity(headings.len());
    for (i, h) in headings.iter().enumerate() {
        let end_line = headings.get(i + 1).map_or(lines.len(), |n| n.line);
        let start = push_segment(&lines[h.line..end_line], &mut pages);
        flat.push(Flat {
            level: h.level,
            title: h.title.clone(),
            start,
            end: 0,
        });
    }
    let total = pages.len() as u32;
    for i in 0..flat.len() {
        let level = flat[i].level;
        flat[i].end = flat[i + 1..]
            .iter()
            .find(|n| n.level <= level)
            .map_or(total, |n| n.start - 1);
    }

    let mut roots = Vec::new();
    if let Some(start) = preamble_start {
        roots.push(make_node(
            "Preamble".into(),
            start,
            flat[0].start - 1,
            &pages,
            Vec::new(),
        ));
    }
    let mut i = 0;
    roots.extend(nest(&flat, &mut i, 0, &pages));
    (pages, roots)
}

fn nest(flat: &[Flat], i: &mut usize, parent_level: u8, pages: &[Page]) -> Vec<TreeNode> {
    let mut out = Vec::new();
    while *i < flat.len() && flat[*i].level > parent_level {
        let f = &flat[*i];
        *i += 1;
        let children = nest(flat, i, f.level, pages);
        out.push(make_node(f.title.clone(), f.start, f.end, pages, children));
    }
    out
}

fn make_node(
    title: String,
    start: u32,
    end: u32,
    pages: &[Page],
    children: Vec<TreeNode>,
) -> TreeNode {
    let words: usize = pages[(start - 1) as usize..end as usize]
        .iter()
        .map(|p| p.text.split_whitespace().count())
        .sum();
    TreeNode {
        node_id: String::new(),
        title,
        start_page: start,
        end_page: end,
        summary: None,
        token_estimate: (words as f64 * 1.3).ceil() as u32,
        children,
    }
}

fn fixed_windows(doc_id: &str, lines: &[&str], lpp: usize) -> BuiltText {
    let mut pages = Vec::new();
    let mut roots = Vec::new();
    for (i, chunk) in lines.chunks(lpp).enumerate() {
        let page_no = i as u32 + 1;
        pages.push(Page {
            doc_id: doc_id.to_string(),
            page_no,
            text: chunk.join("\n"),
        });
        let first = i * lpp + 1;
        let title = format!("Lines {}-{}", first, first + chunk.len() - 1);
        roots.push(make_node(title, page_no, page_no, &pages, Vec::new()));
    }
    let mut tree = DocumentTree {
        doc_id: doc_id.to_string(),
        roots,
    };
    tree.assign_node_ids();
    BuiltText {
        tree,
        pages,
        builder: "plain-windows",
    }
}

fn detect_heading(line: &str, prev_blank: bool) -> Option<(u8, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_HEADING_CHARS {
        return None;
    }
    let title = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(level) = numbered_level(&title) {
        return Some((level, title));
    }
    if is_chapter(&title) {
        return Some((1, title));
    }
    if prev_blank && is_caps_title(&title) {
        return Some((1, title));
    }
    None
}

/// `1.`, `1.1`, `2.3.4 Title`: depth is the number of segments.
fn numbered_level(title: &str) -> Option<u8> {
    let (num, rest) = title.split_once(' ')?;
    let has_trailing_dot = num.ends_with('.');
    let segments: Vec<&str> = num.trim_end_matches('.').split('.').collect();
    if segments
        .iter()
        .any(|s| s.is_empty() || s.len() > 3 || !s.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    if segments.len() == 1 && !has_trailing_dot {
        return None;
    }
    let first = rest.chars().next()?;
    if !first.is_alphabetic() || matches!(rest.chars().last(), Some('.' | ',' | ';')) {
        return None;
    }
    Some((segments.len() as u8).min(MAX_LEVEL))
}

/// `Chapter 3`, `CHAPTER IV: Scope`, `Capitulo 2`.
fn is_chapter(title: &str) -> bool {
    let mut words = title.split_whitespace();
    let kw = words.next().map(str::to_lowercase);
    if !matches!(kw.as_deref(), Some("chapter" | "capítulo" | "capitulo")) {
        return false;
    }
    words.next().is_some_and(|n| {
        let n = n.trim_end_matches([':', '.', '-']);
        !n.is_empty()
            && (n.chars().all(|c| c.is_ascii_digit())
                || n.chars().all(|c| "IVXLCDMivxlcdm".contains(c)))
    })
}

fn is_caps_title(title: &str) -> bool {
    let letters = title.chars().filter(|c| c.is_alphabetic()).count();
    letters >= 3
        && title.chars().count() <= MAX_CAPS_CHARS
        && title.split_whitespace().count() <= MAX_CAPS_WORDS
        && !title.chars().any(|c| c.is_lowercase())
        && !title.ends_with(['.', ',', ';', ':'])
}
