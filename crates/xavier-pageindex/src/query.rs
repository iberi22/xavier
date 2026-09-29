//! Read-side views for LLM navigation: cheap structure without page text,
//! size-capped page content, page-range parsing and unknown-name hints.

use std::collections::HashSet;

use serde::Serialize;

use crate::error::PageIndexError;
use crate::model::{DocStatus, Document, PageUnit, SourceKind, TreeNode};

/// Documents above this many pages should be read structure-first.
pub const STRUCTURE_FIRST_PAGE_THRESHOLD: u32 = 20;
/// Most distinct pages a single `get_pages` call may request.
pub const MAX_PAGES_PER_CALL: usize = 20;
/// Default char budget of a structure response.
pub const DEFAULT_STRUCTURE_CHAR_BUDGET: usize = 95_000;
/// Default char cap of a page-content response.
pub const DEFAULT_RESPONSE_CHAR_CAP: usize = 95_000;
/// Default and maximum documents per browse page.
pub const DEFAULT_BROWSE_LIMIT: usize = 10;
pub const MAX_BROWSE_LIMIT: usize = 50;

const SIMILAR_NAMES_LIMIT: usize = 3;
const SIMILAR_NAMES_CUTOFF: f64 = 0.5;

/// Tunables of the facade.
#[derive(Debug, Clone)]
pub struct PageIndexConfig {
    pub structure_char_budget: usize,
    pub response_char_cap: usize,
}

impl Default for PageIndexConfig {
    fn default() -> Self {
        Self {
            structure_char_budget: DEFAULT_STRUCTURE_CHAR_BUDGET,
            response_char_cap: DEFAULT_RESPONSE_CHAR_CAP,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BrowseQuery {
    /// Case-insensitive match on document name or any node title.
    pub query: Option<String>,
    pub offset: usize,
    /// Clamped to `1..=MAX_BROWSE_LIMIT`; 0 means the default.
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentSummary {
    pub name: String,
    pub status: String,
    pub source_kind: SourceKind,
    pub page_count: u32,
    pub page_unit: PageUnit,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowsePage {
    pub documents: Vec<DocumentSummary>,
    pub total: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub has_more: bool,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentInfo {
    pub name: String,
    pub doc_id: String,
    pub status: String,
    /// Failure reason when `status` is `failed`.
    pub error: Option<String>,
    pub source_kind: SourceKind,
    pub page_count: u32,
    pub page_unit: PageUnit,
    /// True when the document is long enough that the structure must be read first.
    pub structure_first: bool,
    pub node_count: usize,
    pub builder: String,
    pub created_at: i64,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StructureOpts {
    /// Levels to include below the starting point (1 = only its top level).
    pub max_depth: Option<u32>,
    /// Return only the subtree rooted at this node.
    pub node_id: Option<String>,
    /// Overrides the configured char budget.
    pub char_budget: Option<usize>,
}

/// A tree node without page text.
#[derive(Debug, Clone, Serialize)]
pub struct StructureNode {
    pub node_id: String,
    pub title: String,
    pub start_page: u32,
    pub end_page: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub token_estimate: u32,
    /// Children left out by `max_depth` or the char budget.
    #[serde(skip_serializing_if = "is_zero")]
    pub hidden_children: usize,
    /// Titles of the first hidden descendants, so the caller sees what is inside.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub descendant_titles_sample: Vec<String>,
    /// Page span above the per-call page limit: drill down or search instead.
    #[serde(skip_serializing_if = "is_false")]
    pub too_large_for_one_call: bool,
    pub children: Vec<StructureNode>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Most descendant titles listed per node, and their length.
const TITLE_SAMPLE_LEN: usize = 4;
const TITLE_SAMPLE_CHARS: usize = 60;

fn short_title(t: &str) -> String {
    cut_chars(t, TITLE_SAMPLE_CHARS).to_string()
}

fn sample_tree(nodes: &[TreeNode], out: &mut Vec<String>) {
    for n in nodes {
        if out.len() >= TITLE_SAMPLE_LEN {
            return;
        }
        out.push(short_title(&n.title));
        sample_tree(&n.children, out);
    }
}

fn sample_structure(nodes: &[StructureNode], out: &mut Vec<String>) {
    for n in nodes {
        if out.len() >= TITLE_SAMPLE_LEN {
            return;
        }
        out.push(short_title(&n.title));
        sample_structure(&n.children, out);
    }
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Serialize)]
pub struct StructureView {
    pub doc_name: String,
    pub page_count: u32,
    pub page_unit: PageUnit,
    pub structure_first: bool,
    pub nodes: Vec<StructureNode>,
    /// True when the char budget cut nodes (deepest levels go first).
    pub truncated: bool,
    pub omitted_nodes: usize,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageText {
    pub page_no: u32,
    pub text: String,
    /// True when this page's text was cut at the response cap.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageContent {
    pub doc_name: String,
    pub page_unit: PageUnit,
    pub pages: Vec<PageText>,
    pub truncated: bool,
    /// Requested pages not returned because of the cap, as a page spec.
    pub remaining_pages: Option<String>,
    pub next_steps: Vec<String>,
}

pub(crate) fn status_str(s: &DocStatus) -> &'static str {
    match s {
        DocStatus::Processing => "processing",
        DocStatus::Completed => "completed",
        DocStatus::Failed(_) => "failed",
    }
}

pub(crate) fn summarize_doc(d: &Document) -> DocumentSummary {
    DocumentSummary {
        name: d.name.clone(),
        status: status_str(&d.status).into(),
        source_kind: d.source_kind,
        page_count: d.page_count,
        page_unit: d.page_unit,
        created_at: d.created_at,
    }
}

pub(crate) fn count_nodes(nodes: &[TreeNode]) -> usize {
    nodes.iter().map(|n| 1 + count_nodes(&n.children)).sum()
}

pub(crate) fn find_node<'a>(nodes: &'a [TreeNode], id: &str) -> Option<&'a TreeNode> {
    nodes.iter().find_map(|n| {
        if n.node_id == id {
            Some(n)
        } else {
            find_node(&n.children, id)
        }
    })
}

/// Parse `"5-7,12"` into sorted distinct 1-based pages within `page_count`.
pub fn parse_page_spec(spec: &str, page_count: u32) -> Result<Vec<u32>, PageIndexError> {
    let bad = |m: String| PageIndexError::InvalidRange(m);
    let too_many = || {
        bad(format!(
            "'{spec}' requests more than {MAX_PAGES_PER_CALL} pages; request a narrower range"
        ))
    };
    let mut out: Vec<u32> = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        let num = |s: &str| {
            s.trim()
                .parse::<u32>()
                .map_err(|_| bad(format!("invalid page specification '{spec}'")))
        };
        let (start, end) = match part.split_once('-') {
            Some((a, b)) => (num(a)?, num(b)?),
            None => {
                let n = num(part)?;
                (n, n)
            }
        };
        if start < 1 {
            return Err(bad("page numbers start at 1".into()));
        }
        if start > end {
            return Err(bad(format!("invalid range '{part}': start must be <= end")));
        }
        if end > page_count {
            return Err(bad(format!(
                "page {end} is out of range: the document has {page_count} pages"
            )));
        }
        // Bound arithmetically before expanding.
        if (end - start) as usize + 1 > MAX_PAGES_PER_CALL {
            return Err(too_many());
        }
        out.extend(start..=end);
        out.sort_unstable();
        out.dedup();
        if out.len() > MAX_PAGES_PER_CALL {
            return Err(too_many());
        }
    }
    Ok(out)
}

/// Compress `[1,2,3,5]` into `"1-3,5"`.
pub fn format_page_spec(pages: &[u32]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < pages.len() {
        let mut j = i;
        while j + 1 < pages.len() && pages[j + 1] == pages[j] + 1 {
            j += 1;
        }
        parts.push(if i == j {
            pages[i].to_string()
        } else {
            format!("{}-{}", pages[i], pages[j])
        });
        i = j + 1;
    }
    parts.join(",")
}

/// Cut `text` to at most `max_chars` chars on a char boundary.
pub(crate) fn cut_chars(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

/// Assemble page content under a total char cap.
pub(crate) fn cap_pages(
    doc: &Document,
    wanted: &[u32],
    fetched: Vec<crate::model::Page>,
    cap: usize,
) -> PageContent {
    let mut pages: Vec<PageText> = Vec::new();
    let mut used = 0usize;
    let mut delivered: HashSet<u32> = HashSet::new();
    let mut truncated = false;
    for p in fetched {
        let len = p.text.chars().count();
        if used + len <= cap {
            used += len;
            delivered.insert(p.page_no);
            pages.push(PageText {
                page_no: p.page_no,
                text: p.text,
                truncated: false,
            });
        } else {
            truncated = true;
            if pages.is_empty() {
                // A single page above the cap still yields its head.
                delivered.insert(p.page_no);
                pages.push(PageText {
                    page_no: p.page_no,
                    text: cut_chars(&p.text, cap).to_string(),
                    truncated: true,
                });
            }
            break;
        }
    }
    let rest: Vec<u32> = wanted
        .iter()
        .copied()
        .filter(|n| !delivered.contains(n))
        .collect();
    let remaining_pages = (!rest.is_empty()).then(|| format_page_spec(&rest));
    let mut next_steps = Vec::new();
    if truncated {
        next_steps.push(match &remaining_pages {
            Some(r) => format!("Response hit the size cap; request the remaining pages: {r}"),
            None => "A page was cut at the size cap; request narrower content".into(),
        });
    }
    next_steps.push("Answer from these pages, or fetch a tighter range from the structure".into());
    PageContent {
        doc_name: doc.name.clone(),
        page_unit: doc.page_unit,
        pages,
        truncated,
        remaining_pages,
        next_steps,
    }
}

/// Project a node without text, `depth` levels deep (`None` = unlimited).
fn project(n: &TreeNode, depth: Option<u32>) -> StructureNode {
    let mut sample = Vec::new();
    let (children, hidden) = match depth {
        Some(0) => {
            sample_tree(&n.children, &mut sample);
            (Vec::new(), n.children.len())
        }
        _ => (
            n.children
                .iter()
                .map(|c| project(c, depth.map(|d| d - 1)))
                .collect(),
            0,
        ),
    };
    StructureNode {
        node_id: n.node_id.clone(),
        title: n.title.clone(),
        start_page: n.start_page,
        end_page: n.end_page,
        summary: n.summary.clone(),
        token_estimate: n.token_estimate,
        hidden_children: hidden,
        descendant_titles_sample: sample,
        too_large_for_one_call: (n.end_page - n.start_page) as usize + 1 > MAX_PAGES_PER_CALL,
        children,
    }
}

fn node_cost(n: &StructureNode) -> usize {
    // Serialized size of the node without its children, plus separator.
    let mut shell = n.clone();
    shell.children = Vec::new();
    serde_json::to_string(&shell).map_or(0, |s| s.len()) + 1
}

/// Prune breadth-first to fit `budget`. Returns the pruned roots and the
/// number of omitted nodes.
fn prune_to_budget(roots: Vec<StructureNode>, budget: usize) -> (Vec<StructureNode>, usize) {
    fn total(nodes: &[StructureNode]) -> usize {
        nodes.iter().map(|n| 1 + total(&n.children)).sum()
    }
    // Breadth-first order of node ids; a node is kept only if its parent is.
    let mut keep: HashSet<String> = HashSet::new();
    let mut used = 0usize;
    let mut level: Vec<&StructureNode> = roots.iter().collect();
    'outer: while !level.is_empty() {
        let mut next = Vec::new();
        for n in level {
            let cost = node_cost(n);
            if used + cost > budget && !keep.is_empty() {
                break 'outer;
            }
            used += cost;
            keep.insert(n.node_id.clone());
            next.extend(n.children.iter());
        }
        level = next;
    }
    fn filter(nodes: Vec<StructureNode>, keep: &HashSet<String>) -> Vec<StructureNode> {
        nodes
            .into_iter()
            .filter(|n| keep.contains(&n.node_id))
            .map(|mut n| {
                let before = std::mem::take(&mut n.children);
                let (kept, dropped): (Vec<_>, Vec<_>) =
                    before.into_iter().partition(|c| keep.contains(&c.node_id));
                sample_structure(&dropped, &mut n.descendant_titles_sample);
                n.hidden_children += dropped.len();
                n.children = filter(kept, keep);
                n
            })
            .collect()
    }
    let before = total(&roots);
    let kept = filter(roots, &keep);
    let omitted = before - total(&kept);
    (kept, omitted)
}

/// Build the text-free structure view.
pub(crate) fn structure_view(
    doc: &Document,
    roots: &[TreeNode],
    opts: &StructureOpts,
    default_budget: usize,
) -> Result<StructureView, PageIndexError> {
    let start: Vec<&TreeNode> = match &opts.node_id {
        Some(id) => vec![find_node(roots, id).ok_or_else(|| {
            PageIndexError::NotFound(format!(
                "node_id '{id}' not found in '{}'; copy node_id values from get_document_structure()",
                doc.name
            ))
        })?],
        None => roots.iter().collect(),
    };
    // max_depth counts levels: 1 = only the starting nodes.
    let depth = opts.max_depth.map(|d| d.max(1) - 1);
    let projected: Vec<StructureNode> = start.into_iter().map(|n| project(n, depth)).collect();
    let budget = opts.char_budget.unwrap_or(default_budget);
    let (nodes, omitted_nodes) = prune_to_budget(projected, budget);
    let truncated = omitted_nodes > 0;
    let structure_first = doc.page_count > STRUCTURE_FIRST_PAGE_THRESHOLD;
    let mut next_steps = Vec::new();
    if truncated {
        next_steps.push(
            "Structure was truncated to the size budget; pass node_id to expand a subtree \
             or max_depth to see fewer levels"
                .into(),
        );
    } else if nodes.iter().any(has_hidden) {
        next_steps.push("Some children are hidden; pass node_id to expand a subtree".into());
    }
    if nodes.iter().any(has_too_large) {
        next_steps.push(format!(
            "Nodes with too_large_for_one_call span over {MAX_PAGES_PER_CALL} pages: pass their \
             node_id to see their children, or use search() to locate the exact pages"
        ));
    }
    next_steps.push(
        "Pick the relevant sections, then call get_page_content() with their page ranges \
         (or node_id) — never the whole document"
            .into(),
    );
    Ok(StructureView {
        doc_name: doc.name.clone(),
        page_count: doc.page_count,
        page_unit: doc.page_unit,
        structure_first,
        nodes,
        truncated,
        omitted_nodes,
        next_steps,
    })
}

fn has_too_large(n: &StructureNode) -> bool {
    n.too_large_for_one_call || n.children.iter().any(has_too_large)
}

fn has_hidden(n: &StructureNode) -> bool {
    n.hidden_children > 0 || n.children.iter().any(has_hidden)
}

/// Levenshtein similarity ratio in `0.0..=1.0` (case-insensitive).
fn similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let longest = a.len().max(b.len());
    if longest == 0 {
        return 1.0;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    1.0 - prev[b.len()] as f64 / longest as f64
}

/// Top 3 names closest to `query` by edit-distance ratio.
pub fn similar_names<'a>(names: impl IntoIterator<Item = &'a str>, query: &str) -> Vec<String> {
    let mut scored: Vec<(f64, &str)> = names
        .into_iter()
        .map(|n| (similarity(n, query), n))
        .filter(|(s, _)| *s >= SIMILAR_NAMES_CUTOFF)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(SIMILAR_NAMES_LIMIT)
        .map(|(_, n)| n.to_string())
        .collect()
}

pub(crate) fn not_found_message(name: &str, similar: &[String]) -> String {
    if similar.is_empty() {
        format!("document '{name}' not found; use browse_documents() to list documents")
    } else {
        let list: Vec<String> = similar.iter().map(|s| format!("\"{s}\"")).collect();
        format!(
            "document '{name}' not found. Did you mean: {}?",
            list.join(", ")
        )
    }
}
