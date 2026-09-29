//! In-document search: BM25 over the pages of one document, boosted by the
//! headings (title + summary) of the nodes that start on a page. Hits carry a
//! breadcrumb and a short snippet, never a full page.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::error::PageIndexError;
use crate::eval::tokenize;
use crate::model::{Document, Page, TreeNode};

pub const DEFAULT_SEARCH_LIMIT: usize = 10;
pub const MAX_SEARCH_LIMIT: usize = 30;
const SNIPPET_BEFORE: usize = 60;
const SNIPPET_CHARS: usize = 220;
const HEADING_WEIGHT: f64 = 1.0;
const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

/// One BM25 term contribution; shared by every scorer in the workspace.
pub fn bm25_term_score(tf: f64, len: f64, df: f64, n: f64, avg_len: f64) -> f64 {
    let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
    let norm = 1.0 - BM25_B + BM25_B * len / avg_len;
    idf * tf * (BM25_K1 + 1.0) / (tf + BM25_K1 * norm)
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub page_no: u32,
    /// Deepest node covering the page.
    pub node_id: Option<String>,
    /// Ancestor titles from the root down to that node.
    pub breadcrumb: Vec<String>,
    pub score: f64,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResults {
    pub doc_name: String,
    pub query: String,
    pub hits: Vec<SearchHit>,
    /// Pages that matched, before `limit`.
    pub total_matches: usize,
    pub truncated: bool,
    pub next_steps: Vec<String>,
}

struct Terms {
    tf: HashMap<String, u32>,
    len: u32,
}

impl Terms {
    fn of(text: &str) -> Self {
        let mut tf: HashMap<String, u32> = HashMap::new();
        let mut len = 0;
        for w in tokenize(text) {
            *tf.entry(w).or_default() += 1;
            len += 1;
        }
        Self { tf, len }
    }
}

/// Corpus statistics of one field (pages or headings).
struct Stats {
    df: HashMap<String, usize>,
    n: usize,
    avg_len: f64,
}

impl Stats {
    fn new(docs: &[Terms]) -> Self {
        let mut df: HashMap<String, usize> = HashMap::new();
        for d in docs {
            for w in d.tf.keys() {
                *df.entry(w.clone()).or_default() += 1;
            }
        }
        let total: u64 = docs.iter().map(|d| u64::from(d.len)).sum();
        Self {
            df,
            n: docs.len(),
            avg_len: (total as f64 / docs.len().max(1) as f64).max(1.0),
        }
    }

    fn score(&self, query: &[String], d: &Terms) -> f64 {
        query
            .iter()
            .filter_map(|q| {
                let tf = f64::from(*d.tf.get(q)?);
                let df = *self.df.get(q).unwrap_or(&0) as f64;
                Some(bm25_term_score(
                    tf,
                    f64::from(d.len),
                    df,
                    self.n as f64,
                    self.avg_len,
                ))
            })
            .sum()
    }
}

struct Flat<'a> {
    node: &'a TreeNode,
    parent: Option<usize>,
}

fn flatten<'a>(nodes: &'a [TreeNode], parent: Option<usize>, out: &mut Vec<Flat<'a>>) {
    for n in nodes {
        let me = out.len();
        out.push(Flat { node: n, parent });
        flatten(&n.children, Some(me), out);
    }
}

/// Window of `SNIPPET_CHARS` around the first query-term match, whitespace
/// collapsed; the page head when nothing matches literally.
fn snippet(text: &str, query: &HashSet<&str>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut first = None;
    let mut i = 0;
    while i < chars.len() && first.is_none() {
        if !chars[i].is_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_alphanumeric() {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect::<String>().to_lowercase();
        if query.contains(word.as_str()) {
            first = Some(start);
        }
    }
    let from = first.map_or(0, |s| s.saturating_sub(SNIPPET_BEFORE));
    let to = (from + SNIPPET_CHARS).min(chars.len());
    let body = chars[from..to]
        .iter()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{}{body}{}",
        if from > 0 { "…" } else { "" },
        if to < chars.len() { "…" } else { "" }
    )
}

pub(crate) fn search_document(
    doc: &Document,
    roots: &[TreeNode],
    pages: &[Page],
    query: &str,
    limit: usize,
    cap: usize,
) -> Result<SearchResults, PageIndexError> {
    let mut q = tokenize(query);
    q.sort();
    q.dedup();
    if q.is_empty() {
        return Err(PageIndexError::InvalidRange(
            "query has no searchable terms; use words of 2+ letters or digits".into(),
        ));
    }
    let mut flat = Vec::new();
    flatten(roots, None, &mut flat);
    // Preorder fill: deeper and later nodes overwrite, so each page ends up
    // owned by its deepest (latest on shared boundary pages) node.
    let mut owner: Vec<Option<usize>> = vec![None; doc.page_count as usize + 1];
    for (i, f) in flat.iter().enumerate() {
        for p in f.node.start_page..=f.node.end_page.min(doc.page_count) {
            owner[p as usize] = Some(i);
        }
    }
    let mut texts: Vec<Option<&str>> = vec![None; doc.page_count as usize + 1];
    for p in pages {
        if let Some(slot) = texts.get_mut(p.page_no as usize) {
            *slot = Some(p.text.as_str());
        }
    }
    let page_terms: Vec<Terms> = texts
        .iter()
        .map(|t| Terms::of(t.unwrap_or_default()))
        .collect();
    let page_stats = Stats::new(&page_terms[1..]);
    let head_terms: Vec<Terms> = flat
        .iter()
        .map(|f| {
            let n = f.node;
            Terms::of(&format!(
                "{} {}",
                n.title,
                n.summary.as_deref().unwrap_or("")
            ))
        })
        .collect();
    let head_stats = Stats::new(&head_terms);
    // Heading score per page: best node that starts there.
    let mut heading: HashMap<u32, f64> = HashMap::new();
    for (f, t) in flat.iter().zip(&head_terms) {
        let s = head_stats.score(&q, t);
        if s > 0.0 {
            let e = heading.entry(f.node.start_page).or_default();
            *e = e.max(s);
        }
    }
    let mut scored: Vec<(u32, f64)> = (1..=doc.page_count)
        .filter_map(|p| {
            let s = page_stats.score(&q, &page_terms[p as usize])
                + HEADING_WEIGHT * heading.get(&p).copied().unwrap_or(0.0);
            (s > 0.0).then_some((p, s))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let total_matches = scored.len();
    let qset: HashSet<&str> = q.iter().map(String::as_str).collect();
    let mut hits = Vec::new();
    let mut used = 0usize;
    let mut truncated = total_matches > limit;
    for (p, s) in scored.into_iter().take(limit) {
        let mut breadcrumb = Vec::new();
        let mut cur = owner[p as usize];
        while let Some(i) = cur {
            breadcrumb.push(flat[i].node.title.clone());
            cur = flat[i].parent;
        }
        breadcrumb.reverse();
        let snip = snippet(texts[p as usize].unwrap_or_default(), &qset);
        used += snip.chars().count() + breadcrumb.iter().map(|b| b.chars().count()).sum::<usize>();
        if used > cap && !hits.is_empty() {
            truncated = true;
            break;
        }
        hits.push(SearchHit {
            page_no: p,
            node_id: owner[p as usize].map(|i| flat[i].node.node_id.clone()),
            breadcrumb,
            score: (s * 1000.0).round() / 1000.0,
            snippet: snip,
        });
    }
    let next_steps = if hits.is_empty() {
        vec!["No page matched; try other or fewer terms, or read get_document_structure()".into()]
    } else {
        vec!["Fetch the top pages with get_page_content() before answering".into()]
    };
    Ok(SearchResults {
        doc_name: doc.name.clone(),
        query: query.trim().to_string(),
        hits,
        total_matches,
        truncated,
        next_steps,
    })
}
