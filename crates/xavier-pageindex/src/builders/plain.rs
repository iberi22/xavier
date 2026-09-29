//! Plain-text builder (numbered headings, fixed windows).
//!
//! Text is cut into fixed-size virtual pages (line and char budgets) that
//! headings share; a node spans the pages from its first to its last line, so
//! consecutive siblings may share one boundary page. Without headings the
//! text is cut into fixed line windows with one flat node per window.

use crate::error::PageIndexError;
use crate::model::{DocumentTree, Page, TreeNode};

/// Default virtual page size in lines.
pub const DEFAULT_LINES_PER_PAGE: usize = 60;

/// Char budget after which a virtual page is closed (besides the line cap).
pub const PAGE_CHAR_BUDGET: usize = 3000;

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
    /// First line (0-based) of the section.
    line: usize,
    /// Exclusive end line of the section, including nested sections.
    end_line: usize,
}

/// Split lines into virtual pages: a page closes at `lpp` lines or once it
/// holds `PAGE_CHAR_BUDGET` chars. Returns the pages and the 1-based page of
/// every line.
fn paginate(doc_id: &str, lines: &[&str], lpp: usize) -> (Vec<Page>, Vec<u32>) {
    let mut pages: Vec<Page> = Vec::new();
    let mut line_page = Vec::with_capacity(lines.len());
    let mut chunk: Vec<&str> = Vec::new();
    let mut chars = 0;
    let flush = |chunk: &mut Vec<&str>, pages: &mut Vec<Page>| {
        pages.push(Page {
            doc_id: doc_id.to_string(),
            page_no: pages.len() as u32 + 1,
            text: chunk.join("\n"),
        });
        chunk.clear();
    };
    for line in lines {
        line_page.push(pages.len() as u32 + 1);
        chunk.push(line);
        chars += line.chars().count() + 1;
        if chunk.len() >= lpp || chars >= PAGE_CHAR_BUDGET {
            flush(&mut chunk, &mut pages);
            chars = 0;
        }
    }
    if !chunk.is_empty() {
        flush(&mut chunk, &mut pages);
    }
    (pages, line_page)
}

fn assemble(
    doc_id: &str,
    lines: &[&str],
    headings: &[Heading],
    lpp: usize,
) -> (Vec<Page>, Vec<TreeNode>) {
    let (pages, line_page) = paginate(doc_id, lines, lpp);

    let mut flat: Vec<Flat> = headings
        .iter()
        .map(|h| Flat {
            level: h.level,
            title: h.title.clone(),
            line: h.line,
            end_line: lines.len(),
        })
        .collect();
    for i in 0..flat.len() {
        let level = flat[i].level;
        if let Some(next) = flat[i + 1..].iter().find(|n| n.level <= level) {
            flat[i].end_line = next.line;
        }
    }

    let ctx = Ctx {
        lines,
        line_page: &line_page,
    };
    let mut roots = Vec::new();
    let first = flat[0].line;
    if lines[..first].iter().any(|l| !l.trim().is_empty()) {
        roots.push(ctx.node("Preamble".into(), 0, first, Vec::new()));
    }
    let mut i = 0;
    roots.extend(nest(&flat, &mut i, 0, &ctx));
    (pages, roots)
}

struct Ctx<'a> {
    lines: &'a [&'a str],
    line_page: &'a [u32],
}

impl Ctx<'_> {
    /// Node over lines `start..end`; sections may share boundary pages.
    fn node(&self, title: String, start: usize, end: usize, children: Vec<TreeNode>) -> TreeNode {
        let words: usize = self.lines[start..end]
            .iter()
            .map(|l| l.split_whitespace().count())
            .sum();
        TreeNode {
            node_id: String::new(),
            title,
            start_page: self.line_page[start],
            end_page: self.line_page[end - 1],
            summary: None,
            token_estimate: (words as f64 * 1.3).ceil() as u32,
            children,
        }
    }
}

fn nest(flat: &[Flat], i: &mut usize, parent_level: u8, ctx: &Ctx) -> Vec<TreeNode> {
    let mut out = Vec::new();
    while *i < flat.len() && flat[*i].level > parent_level {
        let f = &flat[*i];
        *i += 1;
        let children = nest(flat, i, f.level, ctx);
        out.push(ctx.node(f.title.clone(), f.line, f.end_line, children));
    }
    out
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
        let words: usize = chunk.iter().map(|l| l.split_whitespace().count()).sum();
        roots.push(TreeNode {
            node_id: String::new(),
            title,
            start_page: page_no,
            end_page: page_no,
            summary: None,
            token_estimate: (words as f64 * 1.3).ceil() as u32,
            children: Vec::new(),
        });
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
