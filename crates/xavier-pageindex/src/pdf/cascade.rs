//! PDF strategy cascade: bookmarks, then layout headings (pdfium), then an
//! optional LLM-generated table of contents, then fixed page windows.
//!
//! Every strategy that cannot produce a usable tree falls through to the next
//! one; only unreadable input (malformed, encrypted) is an error.

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
        candidates.extend(layout_strategy(doc_id, bytes, &mut texts, page_count));
    }

    if !has_usable(&candidates, page_count) {
        if let Some(sm) = llm {
            candidates.extend(llm_strategy(doc_id, sm, &texts, page_count));
        }
    }

    if !has_usable(&candidates, page_count) {
        candidates.push((fixed_windows(doc_id, page_count), TocSource::FixedWindows));
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
    split_oversized_leaves(&mut tree.roots);
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

/// Layout headings via pdfium. As a side effect prefers pdfium page text over
/// lopdf text for pages where pdfium found any. Skips silently when pdfium is
/// unavailable or the document cannot be read by it.
#[cfg(feature = "pdf-layout")]
fn layout_strategy(
    doc_id: &str,
    bytes: &[u8],
    texts: &mut [String],
    page_count: u32,
) -> Option<(DocumentTree, TocSource)> {
    use crate::pdf::layout::{classify_headings, extract_lines, LayoutParams};

    let lines = extract_lines(bytes).ok()?;
    let mut by_page: std::collections::BTreeMap<u32, Vec<&str>> = Default::default();
    for l in &lines {
        by_page.entry(l.page).or_default().push(&l.text);
    }
    for (page, ls) in by_page {
        if let Some(slot) = texts.get_mut(page as usize - 1) {
            let joined = ls.join("\n");
            if !joined.trim().is_empty() {
                *slot = joined;
            }
        }
    }
    let heads = classify_headings(&lines, &LayoutParams::default());
    if heads.len() < MIN_LAYOUT_HEADINGS {
        return None;
    }
    let flat: Vec<(u32, String, u32)> = heads
        .into_iter()
        .map(|h| (h.level.max(1), h.title, h.page))
        .collect();
    let tree = outline::outline_to_tree(doc_id, &nest(&flat), page_count);
    (!tree.roots.is_empty()).then_some((tree, TocSource::Layout))
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
fn fixed_windows(doc_id: &str, page_count: u32) -> DocumentTree {
    let mut roots = Vec::new();
    let mut start = 1;
    while start <= page_count {
        let end = (start + FIXED_WINDOW_PAGES - 1).min(page_count);
        roots.push(TreeNode {
            node_id: String::new(),
            title: if start == end {
                format!("Page {start}")
            } else {
                format!("Pages {start}-{end}")
            },
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

/// Give every leaf spanning more than `MAX_PAGES_PER_NODE` pages consecutive
/// non-overlapping page-window children, so no leaf forces a long read.
fn split_oversized_leaves(nodes: &mut [TreeNode]) {
    for n in nodes {
        if !n.children.is_empty() {
            split_oversized_leaves(&mut n.children);
        } else if n.end_page - n.start_page + 1 > MAX_PAGES_PER_NODE {
            let mut start = n.start_page;
            while start <= n.end_page {
                let end = (start + SPLIT_WINDOW_PAGES - 1).min(n.end_page);
                n.children.push(TreeNode {
                    node_id: String::new(),
                    title: format!("{} (pages {start}-{end})", n.title),
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
