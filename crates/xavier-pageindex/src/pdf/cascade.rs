//! PDF strategy cascade: bookmarks, then layout headings (pdfium), then an
//! optional LLM-generated table of contents, then fixed page windows.
//!
//! Every strategy that cannot produce a usable tree falls through to the next
//! one; only unreadable input (malformed, encrypted) is an error.

use std::collections::{HashMap, HashSet};

use crate::error::PageIndexError;
use crate::model::{DocumentTree, Page, TreeNode};
use crate::pdf::outline::{self, OutlineNode};
use crate::summarize::Summarizer;

/// Pages per node in the last-resort fixed-window tree.
pub const FIXED_WINDOW_PAGES: u32 = 5;
/// Documents with fewer pages than this may legitimately have a tiny tree.
pub const MIN_PAGES_FOR_STRUCTURE: u32 = 6;
/// Fewest nodes a tree needs to count as a structure for a larger document.
pub const MIN_TREE_NODES: usize = 2;
/// Leaves spanning more pages than this are split into page windows.
pub const MAX_PAGES_PER_NODE: u32 = 10;
/// Pages per child when an oversized leaf is split.
pub const SPLIT_WINDOW_PAGES: u32 = 5;
/// Leading pages shown to the LLM when it is asked for a table of contents.
const TOC_SCAN_PAGES: usize = 12;
/// Chars of each scanned page shown to the LLM.
const TOC_PAGE_CHARS: usize = 3000;
/// Fewest heading candidates for the layout strategy to count as a structure.
#[cfg(feature = "pdf-layout")]
const MIN_LAYOUT_HEADINGS: usize = 2;
/// Fewest verified LLM entries for the LLM strategy to count as a structure.
const MIN_LLM_ENTRIES: usize = 2;
/// Deepest level accepted from the LLM.
const MAX_LLM_LEVEL: u32 = 6;

/// Which strategy produced the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TocSource {
    Bookmarks,
    Layout,
    Llm,
    FixedWindows,
}

impl TocSource {
    /// Label stored in `Document.builder` (shown by `get_document`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Bookmarks => "pdf-bookmarks",
            Self::Layout => "pdf-layout",
            Self::Llm => "pdf-llm-toc",
            Self::FixedWindows => "pdf-fixed-windows",
        }
    }
}

/// Result of a cascade run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltPdf {
    pub tree: DocumentTree,
    pub pages: Vec<Page>,
    pub source: TocSource,
}

impl BuiltPdf {
    pub fn builder(&self) -> &'static str {
        self.source.label()
    }
}

/// Build the section tree and page texts of a PDF.
///
/// `llm` is only consulted when bookmarks and layout both yield nothing; its
/// failure or unusable output falls through to fixed windows.
pub fn build_pdf_tree(
    doc_id: &str,
    bytes: &[u8],
    llm: Option<&dyn Summarizer>,
) -> Result<BuiltPdf, PageIndexError> {
    #[cfg_attr(not(feature = "pdf-layout"), allow(unused_mut))]
    let mut texts = outline::extract_pages(bytes)?;
    if texts.is_empty() {
        return Err(PageIndexError::Build("PDF has no pages".into()));
    }
    // One pdfium extraction feeds both page text and heading detection, so the
    // structure and the text always describe the same page content.
    #[cfg(feature = "pdf-layout")]
    let lines = crate::pdf::layout::extract_lines(bytes).ok();
    #[cfg(feature = "pdf-layout")]
    if let Some(ls) = &lines {
        merge_pdfium_text(&mut texts, ls);
    }
    let page_count = texts.len() as u32;
    let outline_nodes = outline::extract_outline(bytes)?;

    // First non-degenerate result wins; otherwise the richest one is kept.
    let mut candidates: Vec<(DocumentTree, TocSource)> = Vec::new();
    if let Some(nodes) = outline_nodes {
        let tree = outline::outline_to_tree(doc_id, &nodes, page_count);
        if !tree.roots.is_empty() {
            candidates.push((tree, TocSource::Bookmarks));
        }
    }

    #[cfg(feature = "pdf-layout")]
    if !has_usable(&candidates, page_count) {
        if let Some(ls) = &lines {
            candidates.extend(layout_strategy(doc_id, ls, page_count));
        }
    }

    if !has_usable(&candidates, page_count) {
        if let Some(sm) = llm {
            candidates.extend(llm_strategy(doc_id, sm, &texts, page_count));
        }
    }

    if !has_usable(&candidates, page_count) {
        candidates.push((
            fixed_windows(doc_id, page_count, &texts),
            TocSource::FixedWindows,
        ));
    }
    let best = candidates
        .iter()
        .position(|(t, _)| !is_degenerate(t, page_count))
        .unwrap_or_else(|| {
            (0..candidates.len())
                .max_by_key(|&i| (count_nodes(&candidates[i].0.roots), std::cmp::Reverse(i)))
                .unwrap_or(0)
        });
    let (mut tree, source) = candidates.swap_remove(best);
    split_oversized_leaves(&mut tree.roots, &texts);
    tree.assign_node_ids();
    fill_token_estimates(&mut tree.roots, &texts);
    let pages = texts
        .into_iter()
        .enumerate()
        .map(|(i, text)| Page {
            doc_id: doc_id.to_string(),
            page_no: i as u32 + 1,
            text,
        })
        .collect();
    Ok(BuiltPdf {
        tree,
        pages,
        source,
    })
}

/// Alphanumeric chars: the yardstick for "has real text".
#[cfg(feature = "pdf-layout")]
fn alnum_count(s: &str) -> usize {
    s.chars().filter(|c| c.is_alphanumeric()).count()
}

/// Pages whose pdfium text is at most this many alphanumerics are "near empty".
#[cfg(feature = "pdf-layout")]
const NEAR_EMPTY_ALNUM: usize = 20;

/// Prefers pdfium text (rebuilt from glyph positions) per page; keeps the lopdf
/// text where pdfium found nothing or clearly less than lopdf did.
#[cfg(feature = "pdf-layout")]
fn merge_pdfium_text(texts: &mut [String], lines: &[crate::pdf::layout::LineRecord]) {
    let mut by_page: std::collections::BTreeMap<u32, Vec<&str>> = Default::default();
    for l in lines {
        by_page.entry(l.page).or_default().push(&l.text);
    }
    for (i, slot) in texts.iter_mut().enumerate() {
        let joined = by_page
            .get(&(i as u32 + 1))
            .map(|ls| ls.join("\n"))
            .unwrap_or_default();
        let (p, l) = (alnum_count(&joined), alnum_count(slot));
        let pdfium_thin = p == 0 || (p < NEAR_EMPTY_ALNUM && l > p * 3);
        if !pdfium_thin {
            *slot = joined;
        }
    }
}

/// Layout headings from pdfium lines. `None` when too few were found.
#[cfg(feature = "pdf-layout")]
fn layout_strategy(
    doc_id: &str,
    lines: &[crate::pdf::layout::LineRecord],
    page_count: u32,
) -> Option<(DocumentTree, TocSource)> {
    use crate::pdf::layout::{classify_headings, LayoutParams};

    let heads = classify_headings(lines, &LayoutParams::default());
    if heads.len() < MIN_LAYOUT_HEADINGS {
        return None;
    }
    let mut flat: Vec<(u32, String, u32)> = heads
        .into_iter()
        .map(|h| (h.level.max(1), h.title, h.page))
        .collect();
    cap_outline(&mut flat);
    let tree = outline::outline_to_tree(doc_id, &nest(&flat), page_count);
    (!tree.roots.is_empty()).then_some((tree, TocSource::Layout))
}

#[cfg(feature = "pdf-layout")]
/// Most entries a detected outline may have before its deepest level is dropped.
pub const MAX_OUTLINE_ENTRIES: usize = 300;
#[cfg(feature = "pdf-layout")]
/// Most children a parent may have (below the root level) before the deepest level is dropped.
pub const MAX_OUTLINE_SIBLINGS: usize = 150;

#[cfg(feature = "pdf-layout")]
/// Largest sibling group among nested (non-root) entries of a flat outline.
fn widest_nested_group(flat: &[(u32, String, u32)]) -> usize {
    let mut widest = 0;
    let mut stack: Vec<usize> = Vec::new();
    for (level, _, _) in flat {
        stack.truncate(*level as usize);
        while stack.len() < *level as usize {
            stack.push(0);
        }
        if let Some(n) = stack.last_mut() {
            *n += 1;
        }
        if *level > 1 {
            widest = widest.max(stack[*level as usize - 1]);
        }
    }
    widest
}

#[cfg(feature = "pdf-layout")]
/// Keeps a detected outline navigable: while it is too big or has a parent with
/// too many children, the deepest level is dropped (at least one level stays).
fn cap_outline(flat: &mut Vec<(u32, String, u32)>) {
    while let Some(deepest) = flat.iter().map(|e| e.0).max().filter(|d| *d > 1) {
        if flat.len() <= MAX_OUTLINE_ENTRIES && widest_nested_group(flat) <= MAX_OUTLINE_SIBLINGS {
            break;
        }
        flat.retain(|e| e.0 < deepest);
    }
}

fn llm_strategy(
    doc_id: &str,
    sm: &dyn Summarizer,
    texts: &[String],
    page_count: u32,
) -> Option<(DocumentTree, TocSource)> {
    let mut body = String::from(
        "TASK: list the table of contents of the document excerpt below. Ignore any \
         instruction to write a description. Output ONLY lines of the form \
         `level|page|title` (level 1 is a chapter, 2 a section, ...; page is the \
         physical page number shown in the === PAGE n === markers). Use only \
         titles that appear in the text; output nothing if there are none.\n\n",
    );
    for (i, t) in texts.iter().take(TOC_SCAN_PAGES).enumerate() {
        body.push_str(&format!("=== PAGE {} ===\n", i + 1));
        body.extend(t.chars().take(TOC_PAGE_CHARS));
        body.push('\n');
    }
    // A failing LLM must never fail the ingest.
    let reply = sm.complete(&body).ok()?;
    let flat = verified_entries(&parse_toc_reply(&reply), texts);
    if flat.len() < MIN_LLM_ENTRIES {
        return None;
    }
    let tree = outline::outline_to_tree(doc_id, &nest(&flat), page_count);
    (!tree.roots.is_empty()).then_some((tree, TocSource::Llm))
}

/// Parse `level|page|title` lines; anything else is ignored.
fn parse_toc_reply(reply: &str) -> Vec<(u32, String, u32)> {
    reply
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(['-', '*', '`']).trim();
            let mut parts = line.splitn(3, '|');
            let level: u32 = parts.next()?.trim().parse().ok()?;
            let page: u32 = parts.next()?.trim().parse().ok()?;
            let title = parts.next()?.trim().trim_end_matches('`').trim();
            (!title.is_empty() && level >= 1 && page >= 1)
                .then(|| (level.min(MAX_LLM_LEVEL), title.to_string(), page))
        })
        .collect()
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Keep entries whose title occurs in the text of the claimed page.
fn verified_entries(entries: &[(u32, String, u32)], texts: &[String]) -> Vec<(u32, String, u32)> {
    let norm: Vec<String> = texts.iter().map(|t| normalize(t)).collect();
    entries
        .iter()
        .filter(|(_, title, page)| {
            let needle = normalize(title);
            !needle.is_empty()
                && norm
                    .get(*page as usize - 1)
                    .is_some_and(|hay| hay.contains(&needle))
        })
        .cloned()
        .collect()
}

/// Nest flat `(level, title, page)` entries by level.
fn nest(flat: &[(u32, String, u32)]) -> Vec<OutlineNode> {
    fn build(flat: &[(u32, String, u32)], i: &mut usize, parent: u32) -> Vec<OutlineNode> {
        let mut out = Vec::new();
        while let Some((level, title, page)) = flat.get(*i) {
            if *level <= parent {
                break;
            }
            *i += 1;
            let children = build(flat, i, *level);
            out.push(OutlineNode {
                title: title.clone(),
                page: *page,
                children,
            });
        }
        out
    }
    build(flat, &mut 0, 0)
}

/// One flat node per window of `FIXED_WINDOW_PAGES` pages.
fn fixed_windows(doc_id: &str, page_count: u32, texts: &[String]) -> DocumentTree {
    let furniture = furniture_lines(texts);
    let mut roots = Vec::new();
    let mut start = 1;
    while start <= page_count {
        let end = (start + FIXED_WINDOW_PAGES - 1).min(page_count);
        roots.push(TreeNode {
            node_id: String::new(),
            title: window_title(texts, &furniture, start, end).unwrap_or_else(|| {
                if start == end {
                    format!("Page {start}")
                } else {
                    format!("Pages {start}-{end}")
                }
            }),
            start_page: start,
            end_page: end,
            summary: None,
            token_estimate: 0,
            children: Vec::new(),
        });
        start = end + 1;
    }
    let mut tree = DocumentTree {
        doc_id: doc_id.to_string(),
        roots,
    };
    tree.assign_node_ids();
    tree
}

fn count_nodes(nodes: &[TreeNode]) -> usize {
    nodes.iter().map(|n| 1 + count_nodes(&n.children)).sum()
}

/// A tree too thin to guide navigation of a document of this size.
fn is_degenerate(tree: &DocumentTree, page_count: u32) -> bool {
    page_count >= MIN_PAGES_FOR_STRUCTURE && count_nodes(&tree.roots) < MIN_TREE_NODES
}

fn has_usable(candidates: &[(DocumentTree, TocSource)], page_count: u32) -> bool {
    candidates
        .iter()
        .any(|(t, _)| !is_degenerate(t, page_count))
}

/// Longest window title taken from page text, in chars.
const MAX_WINDOW_TITLE_CHARS: usize = 60;
/// A line repeated on this many pages is furniture, never a window title.
const MIN_FURNITURE_LINE_PAGES: usize = 3;

fn line_key(line: &str) -> String {
    line.split_whitespace()
        .map(|w| {
            w.chars()
                .map(|c| if c.is_ascii_digit() { '#' } else { c })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Normalised lines printed on several pages (running headers and footers).
fn furniture_lines(texts: &[String]) -> HashSet<String> {
    let mut pages_per_line: HashMap<String, usize> = HashMap::new();
    for t in texts {
        let uniq: HashSet<String> = t.lines().map(line_key).collect();
        for k in uniq {
            *pages_per_line.entry(k).or_default() += 1;
        }
    }
    let quarter = texts.len().div_ceil(4);
    pages_per_line
        .into_iter()
        .filter(|(_, n)| *n >= MIN_FURNITURE_LINE_PAGES.max(quarter))
        .map(|(k, _)| k)
        .collect()
}

/// First meaningful line of the window's pages, truncated; `None` when the
/// window has no usable line (blank, numbers only, running headers).
fn window_title(
    texts: &[String],
    furniture: &HashSet<String>,
    start: u32,
    end: u32,
) -> Option<String> {
    let meaningful = |l: &str| {
        let l = l.trim();
        l.chars().filter(|c| c.is_alphabetic()).count() >= 3 && !furniture.contains(&line_key(l))
    };
    let lines: Vec<&str> = (start..=end)
        .filter_map(|p| texts.get(p as usize - 1))
        .flat_map(|t| t.lines())
        .filter(|l| meaningful(l))
        .collect();
    // A line that starts like a title beats a mid-sentence fragment.
    let pick = lines
        .iter()
        .find(|l| l.trim().chars().next().is_some_and(char::is_uppercase))
        .or(lines.first())?;
    let words: Vec<&str> = pick.split_whitespace().collect();
    let mut out = String::new();
    for w in words.iter() {
        if out.chars().count() + w.chars().count() + 1 > MAX_WINDOW_TITLE_CHARS {
            if out.is_empty() {
                out.extend(w.chars().take(MAX_WINDOW_TITLE_CHARS));
            }
            out.push('\u{2026}');
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    Some(out)
}

/// Give every leaf spanning more than `MAX_PAGES_PER_NODE` pages consecutive
/// non-overlapping page-window children, so no leaf forces a long read.
fn split_oversized_leaves(nodes: &mut [TreeNode], texts: &[String]) {
    let furniture = furniture_lines(texts);
    split_leaves(nodes, texts, &furniture);
}

fn split_leaves(nodes: &mut [TreeNode], texts: &[String], furniture: &HashSet<String>) {
    for n in nodes {
        if !n.children.is_empty() {
            split_leaves(&mut n.children, texts, furniture);
        } else if n.end_page - n.start_page + 1 > MAX_PAGES_PER_NODE {
            let mut start = n.start_page;
            while start <= n.end_page {
                let end = (start + SPLIT_WINDOW_PAGES - 1).min(n.end_page);
                n.children.push(TreeNode {
                    node_id: String::new(),
                    title: window_title(texts, furniture, start, end)
                        .unwrap_or_else(|| format!("{} (pages {start}-{end})", n.title)),
                    start_page: start,
                    end_page: end,
                    summary: None,
                    token_estimate: 0,
                    children: Vec::new(),
                });
                start = end + 1;
            }
        }
    }
}

/// Same estimator as the plain-text builder: words * 1.3.
fn estimate_tokens(words: usize) -> u32 {
    (words as f64 * 1.3).ceil() as u32
}

fn fill_token_estimates(nodes: &mut [TreeNode], texts: &[String]) {
    for n in nodes {
        let words: usize = (n.start_page..=n.end_page)
            .filter_map(|p| texts.get(p as usize - 1))
            .map(|t| t.split_whitespace().count())
            .sum();
        n.token_estimate = estimate_tokens(words);
        fill_token_estimates(&mut n.children, texts);
    }
}

#[cfg(all(test, feature = "pdf-layout"))]
mod tests {
    use super::*;
    use crate::pdf::layout::LineRecord;

    fn line(page: u32, text: &str) -> LineRecord {
        LineRecord {
            page,
            top: 100.0,
            page_height: 800.0,
            text: text.into(),
            font_size: 10.0,
            bold: false,
            chars: text.len() as u32,
        }
    }

    fn entry(level: u32, n: usize) -> (u32, String, u32) {
        (level, format!("h{n}"), 1)
    }

    #[test]
    fn test_cap_outline_drops_deepest_level_when_too_big() {
        let mut flat = Vec::new();
        for i in 0..10 {
            flat.push(entry(1, i));
            for j in 0..40 {
                flat.push(entry(2, i * 100 + j));
            }
        }
        assert!(flat.len() > MAX_OUTLINE_ENTRIES);
        cap_outline(&mut flat);
        assert_eq!(flat.len(), 10);
        assert!(flat.iter().all(|e| e.0 == 1));
    }

    #[test]
    fn test_cap_outline_drops_level_with_hundreds_of_siblings() {
        let mut flat = vec![entry(1, 0)];
        flat.extend((1..=MAX_OUTLINE_SIBLINGS + 1).map(|i| entry(2, i)));
        cap_outline(&mut flat);
        assert_eq!(flat.len(), 1);
    }

    #[test]
    fn test_cap_outline_keeps_a_navigable_outline_and_a_single_level() {
        let mut small: Vec<_> = (0..30).map(|i| entry(1 + (i % 3 == 1) as u32, i)).collect();
        let before = small.clone();
        cap_outline(&mut small);
        assert_eq!(small, before);
        let mut flat: Vec<_> = (0..MAX_OUTLINE_ENTRIES + 50).map(|i| entry(1, i)).collect();
        cap_outline(&mut flat);
        assert_eq!(flat.len(), MAX_OUTLINE_ENTRIES + 50, "last level is kept");
    }

    #[test]
    fn test_merge_uses_pdfium_text_and_falls_back_per_page() {
        let mut texts = vec![
            "one\nword\nper\nline".to_string(),
            "lopdf has the only real text on this page, plenty of it".to_string(),
            String::new(),
            String::new(),
        ];
        let lines = vec![
            line(1, "one word per line"),
            line(2, "x"),
            line(3, "pdfium text where lopdf found nothing at all"),
        ];
        merge_pdfium_text(&mut texts, &lines);
        assert_eq!(texts[0], "one word per line");
        assert!(texts[1].starts_with("lopdf has"), "near-empty pdfium loses");
        assert!(texts[2].starts_with("pdfium text"));
        assert_eq!(texts[3], "", "no text in either extractor stays empty");
    }
}
