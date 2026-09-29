//! PAGEINDEX.13: hybrid retrieval arm over PageIndex trees.
//!
//! Every tree node is scored directly with BM25 over its title, summary and
//! own page text (no top-down navigation: issue 14 measured that lexical
//! descent loses to chunk BM25). The best nodes come back with their
//! breadcrumb, page range and node id, so a caller can expand them through
//! `get_document_structure` / `get_pages`. The arm is one extra ranked list
//! for the gating RRF and is inert unless `XAVIER_PAGEINDEX_ARM_ENABLED`.
//!
//! The corpus is derived from the tree store and refreshed when a document's
//! `doc_id` changes, so re-ingesting replaces stale nodes. No node embeddings
//! exist in the store, so there is no vector leg; a vector source can join the
//! same RRF later.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use xavier_pageindex::eval::tokenize;
use xavier_pageindex::store::SqliteStore;
use xavier_pageindex::{BrowseQuery, PageIndex, PageIndexError, StructureNode, StructureOpts};

use crate::pageindex_glue::state::shared_state;
use crate::pageindex_glue::PageIndexState;
use crate::search::rrf::ScoredResult;

/// Nodes the arm contributes to the fusion.
pub const ARM_TOP_K: usize = 10;
const PAGES_PER_FETCH: u32 = 20;
const BROWSE_PAGE: usize = 50;
const BIG: usize = 50_000_000;
const LEAD_CHARS: usize = 300;
const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;
pub const ARM_SOURCE: &str = "pageindex";

/// One scored tree node with everything needed to expand it.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeHit {
    pub doc: String,
    pub node_id: String,
    /// Ancestor titles from the root down to the node itself.
    pub breadcrumb: Vec<String>,
    pub start_page: u32,
    pub end_page: u32,
    pub score: f64,
    pub lead: String,
}

impl NodeHit {
    fn to_scored(&self) -> ScoredResult {
        ScoredResult {
            id: format!("{ARM_SOURCE}/{}/{}", self.doc, self.node_id),
            content: format!(
                "{} [doc {}, node {}, pages {}-{}] {}",
                self.breadcrumb.join(" > "),
                self.doc,
                self.node_id,
                self.start_page,
                self.end_page,
                self.lead
            ),
            score: self.score as f32,
            source: ARM_SOURCE.to_string(),
            path: format!("{ARM_SOURCE}/{}#{}", self.doc, self.node_id),
            updated_at: None,
            zone: None,
        }
    }
}

struct NodeEntry {
    doc: String,
    node_id: String,
    breadcrumb: Vec<String>,
    start_page: u32,
    end_page: u32,
    lead: String,
    tf: HashMap<String, u32>,
    len: u32,
}

impl NodeEntry {
    /// Store tags of the node record: `pageindex`, `doc:<name>`, `node:<id>`.
    pub fn tags(&self) -> Vec<String> {
        vec![
            ARM_SOURCE.to_string(),
            format!("doc:{}", self.doc),
            format!("node:{}", self.node_id),
        ]
    }
}

#[derive(Default)]
struct Corpus {
    /// doc name -> (doc_id, node entries)
    docs: HashMap<String, (String, Vec<NodeEntry>)>,
    df: HashMap<String, usize>,
    n: usize,
    avg_len: f64,
}

impl Corpus {
    fn recompute(&mut self) {
        self.df.clear();
        let (mut n, mut total) = (0usize, 0u64);
        for (_, entries) in self.docs.values() {
            for e in entries {
                n += 1;
                total += u64::from(e.len);
                for w in e.tf.keys() {
                    *self.df.entry(w.clone()).or_default() += 1;
                }
            }
        }
        self.n = n;
        self.avg_len = (total as f64 / n.max(1) as f64).max(1.0);
    }

    fn score(&self, query: &[String], e: &NodeEntry) -> f64 {
        let mut s = 0.0;
        for q in query {
            let Some(&tf) = e.tf.get(q) else { continue };
            let df = *self.df.get(q).unwrap_or(&0) as f64;
            let idf = (1.0 + (self.n as f64 - df + 0.5) / (df + 0.5)).ln();
            let norm = 1.0 - BM25_B + BM25_B * f64::from(e.len) / self.avg_len;
            let tf = f64::from(tf);
            s += idf * tf * (BM25_K1 + 1.0) / (tf + BM25_K1 * norm);
        }
        s
    }
}

pub struct PageIndexArm {
    state: Arc<PageIndexState>,
    workspace: String,
    enabled: bool,
    weight: f32,
    corpus: Mutex<Corpus>,
}

impl std::fmt::Debug for PageIndexArm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageIndexArm")
            .field("workspace", &self.workspace)
            .field("enabled", &self.enabled)
            .field("weight", &self.weight)
            .finish()
    }
}

impl PageIndexArm {
    /// Enabled/weight come from the state's settings.
    pub fn new(state: Arc<PageIndexState>, workspace: impl Into<String>) -> Self {
        let (enabled, weight) = (state.settings.arm_enabled, state.settings.arm_weight);
        Self {
            state,
            workspace: workspace.into(),
            enabled,
            weight,
            corpus: Mutex::new(Corpus::default()),
        }
    }

    /// Arm over the process-wide index; `None` unless the env enables it.
    pub fn from_env(workspace: &str) -> Option<Arc<Self>> {
        let arm = Self::new(shared_state(), workspace);
        arm.enabled.then(|| Arc::new(arm))
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn weight(&self) -> f32 {
        self.weight
    }

    /// (Re)build the node records of one document, replacing its stale nodes.
    pub fn embed_document(&self, doc: &str) -> Result<usize, PageIndexError> {
        let idx = self.state.index()?;
        let doc_id = idx.get_document(&self.workspace, doc)?.doc_id;
        let entries = build_entries(&idx, &self.workspace, doc)?;
        let count = entries.len();
        let mut c = self.lock();
        c.docs.insert(doc.to_string(), (doc_id, entries));
        c.recompute();
        Ok(count)
    }

    /// Tags of every stored node record of `doc` (one list per node).
    pub fn node_tags(&self, doc: &str) -> Vec<Vec<String>> {
        let c = self.lock();
        c.docs
            .get(doc)
            .map(|(_, es)| es.iter().map(NodeEntry::tags).collect())
            .unwrap_or_default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Corpus> {
        self.corpus.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Bring the corpus in line with the store: new or re-ingested documents
    /// are rebuilt, deleted ones dropped.
    fn sync(&self, idx: &PageIndex<SqliteStore>) -> Result<(), PageIndexError> {
        let mut names = Vec::new();
        let mut offset = 0;
        loop {
            let page = idx.browse(
                &self.workspace,
                BrowseQuery {
                    query: None,
                    offset,
                    limit: BROWSE_PAGE,
                },
            )?;
            names.extend(page.documents.into_iter().map(|d| d.name));
            match page.next_offset {
                Some(o) => offset = o,
                None => break,
            }
        }
        let mut changed = false;
        let mut c = self.lock();
        let before = c.docs.len();
        c.docs.retain(|name, _| names.contains(name));
        changed |= c.docs.len() != before;
        for name in names {
            let Ok(info) = idx.get_document(&self.workspace, &name) else {
                continue;
            };
            if c.docs.get(&name).is_some_and(|(id, _)| *id == info.doc_id) {
                continue;
            }
            match build_entries(idx, &self.workspace, &name) {
                Ok(entries) => {
                    c.docs.insert(name, (info.doc_id, entries));
                    changed = true;
                }
                Err(e) => tracing::debug!("pageindex arm: skip '{name}': {e}"),
            }
        }
        if changed {
            c.recompute();
        }
        Ok(())
    }

    /// Top `k` nodes by BM25 over title + summary + own text. Blocking.
    pub fn search_nodes(&self, query: &str, k: usize) -> Result<Vec<NodeHit>, PageIndexError> {
        let idx = self.state.index()?;
        self.sync(&idx)?;
        let mut q = tokenize(query);
        q.sort();
        q.dedup();
        let c = self.lock();
        let mut hits: Vec<NodeHit> = c
            .docs
            .values()
            .flat_map(|(_, es)| es.iter())
            .filter_map(|e| {
                let score = c.score(&q, e);
                (score > 0.0).then(|| NodeHit {
                    doc: e.doc.clone(),
                    node_id: e.node_id.clone(),
                    breadcrumb: e.breadcrumb.clone(),
                    start_page: e.start_page,
                    end_page: e.end_page,
                    score,
                    lead: e.lead.clone(),
                })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.doc.cmp(&b.doc))
                .then_with(|| a.node_id.cmp(&b.node_id))
        });
        hits.truncate(k);
        Ok(hits)
    }

    /// Ranked gating candidates. Blocking.
    pub fn search(&self, query: &str, k: usize) -> Result<Vec<ScoredResult>, PageIndexError> {
        Ok(self
            .search_nodes(query, k)?
            .iter()
            .map(NodeHit::to_scored)
            .collect())
    }

    /// The gating hook: ranked list and weight, or `None` when the arm is off,
    /// finds nothing or fails (errors are logged, never propagated).
    pub async fn ranked_source(self: &Arc<Self>, query: &str) -> Option<(Vec<ScoredResult>, f32)> {
        if !self.enabled {
            return None;
        }
        let (arm, q) = (Arc::clone(self), query.to_string());
        match tokio::task::spawn_blocking(move || arm.search(&q, ARM_TOP_K)).await {
            Ok(Ok(hits)) if !hits.is_empty() => Some((hits, self.weight)),
            Ok(Ok(_)) => None,
            Ok(Err(e)) => {
                tracing::warn!("pageindex arm failed, ignoring: {e}");
                None
            }
            Err(e) => {
                tracing::warn!("pageindex arm task failed, ignoring: {e}");
                None
            }
        }
    }
}

fn build_entries(
    idx: &PageIndex<SqliteStore>,
    ws: &str,
    doc: &str,
) -> Result<Vec<NodeEntry>, PageIndexError> {
    let view = idx.get_structure(
        ws,
        doc,
        StructureOpts {
            char_budget: Some(BIG),
            ..Default::default()
        },
    )?;
    let mut pages: Vec<String> = Vec::with_capacity(view.page_count as usize);
    let mut lo = 1;
    while lo <= view.page_count {
        let hi = (lo + PAGES_PER_FETCH - 1).min(view.page_count);
        let got = idx.get_pages(ws, doc, &format!("{lo}-{hi}"), BIG)?;
        pages.extend(got.pages.into_iter().map(|p| p.text));
        lo = hi + 1;
    }
    let mut out = Vec::new();
    walk(&view.nodes, &mut Vec::new(), doc, &pages, &mut out);
    Ok(out)
}

/// Text of a node's own pages: everything for a leaf, only the pages before
/// the first child for an inner node (children carry their own text).
fn own_text(n: &StructureNode, pages: &[String]) -> String {
    let end = match n.children.first() {
        None => n.end_page,
        Some(c) if c.start_page > n.start_page => c.start_page - 1,
        Some(_) => return String::new(),
    };
    pages
        .get((n.start_page.saturating_sub(1)) as usize..(end as usize).min(pages.len()))
        .map(|p| p.join("\n"))
        .unwrap_or_default()
}

fn walk(
    nodes: &[StructureNode],
    crumbs: &mut Vec<String>,
    doc: &str,
    pages: &[String],
    out: &mut Vec<NodeEntry>,
) {
    for n in nodes {
        crumbs.push(n.title.clone());
        let text = own_text(n, pages);
        let summary = n.summary.clone().unwrap_or_default();
        let mut tf: HashMap<String, u32> = HashMap::new();
        let mut len = 0;
        for w in tokenize(&format!("{} {} {}", n.title, summary, text)) {
            *tf.entry(w).or_default() += 1;
            len += 1;
        }
        let lead_src = if summary.is_empty() { &text } else { &summary };
        out.push(NodeEntry {
            doc: doc.to_string(),
            node_id: n.node_id.clone(),
            breadcrumb: crumbs.clone(),
            start_page: n.start_page,
            end_page: n.end_page,
            lead: lead_src.chars().take(LEAD_CHARS).collect(),
            tf,
            len,
        });
        walk(&n.children, crumbs, doc, pages, out);
        crumbs.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pageindex_glue::PageIndexSettings;
    use crate::retrieval::gating::{AdaptiveGating, GatingConfig};
    use xavier_pageindex::{IngestOptions, Source};

    const MANUAL: &str = "# Manual\n\nIntro text about the widget.\n\n## Battery\n\nThe battery lasts twelve hours and charges through the usb port.\n\n## Cleaning\n\nWipe the lens with a microfiber cloth only.\n";
    const MANUAL_V2: &str = "# Manual\n\nIntro text.\n\n## Warranty\n\nThe warranty covers manufacturing defects for two years.\n";

    fn state() -> Arc<PageIndexState> {
        Arc::new(PageIndexState::with_store(
            PageIndexSettings::default(),
            SqliteStore::open_in_memory().unwrap(),
        ))
    }

    fn ingest(state: &PageIndexState, ws: &str, doc: &str, text: &str) {
        let opts = IngestOptions {
            lines_per_page: 4,
            ..Default::default()
        };
        state
            .index()
            .unwrap()
            .ingest(ws, doc, Source::Markdown(text), opts)
            .unwrap();
    }

    fn arm(state: &Arc<PageIndexState>, ws: &str) -> PageIndexArm {
        PageIndexArm::new(Arc::clone(state), ws).with_enabled(true)
    }

    #[test]
    fn test_arm_embeds_node_summaries_with_doc_and_node_tags() {
        let st = state();
        ingest(&st, "w", "manual.md", MANUAL);
        let a = arm(&st, "w");
        let n = a.embed_document("manual.md").unwrap();
        let tags = a.node_tags("manual.md");
        assert_eq!(tags.len(), n);
        assert!(n >= 3);
        for t in &tags {
            assert_eq!(t[0], "pageindex");
            assert_eq!(t[1], "doc:manual.md");
            assert!(t[2].starts_with("node:"));
        }
    }

    #[test]
    fn test_arm_reingest_replaces_stale_embeddings() {
        let st = state();
        ingest(&st, "w", "manual.md", MANUAL);
        let a = arm(&st, "w");
        assert!(!a.search_nodes("battery usb", 3).unwrap().is_empty());
        ingest(&st, "w", "manual.md", MANUAL_V2);
        assert!(a.search_nodes("battery usb", 3).unwrap().is_empty());
        let hits = a.search_nodes("warranty defects", 3).unwrap();
        assert_eq!(hits[0].breadcrumb.last().unwrap(), "Warranty");
    }

    #[test]
    fn test_arm_results_carry_doc_node_page_range() {
        let st = state();
        ingest(&st, "w", "manual.md", MANUAL);
        let a = arm(&st, "w");
        let hit = &a.search_nodes("microfiber cloth lens", 3).unwrap()[0];
        assert_eq!(hit.doc, "manual.md");
        assert_eq!(hit.breadcrumb, vec!["Manual", "Cleaning"]);
        assert!(hit.start_page >= 1 && hit.end_page >= hit.start_page);
        let scored = &a.search("microfiber cloth lens", 3).unwrap()[0];
        assert_eq!(scored.source, "pageindex");
        assert!(scored.content.contains(&format!("node {}", hit.node_id)));
        assert!(scored.content.contains("Manual > Cleaning"));
        let pages = format!("pages {}-{}", hit.start_page, hit.end_page);
        assert!(scored.content.contains(&pages));
    }

    #[test]
    fn test_arm_workspace_scoped() {
        let st = state();
        ingest(&st, "a", "manual.md", MANUAL);
        ingest(&st, "b", "other.md", MANUAL_V2);
        let arm_a = arm(&st, "a");
        assert!(arm_a
            .search_nodes("warranty defects", 3)
            .unwrap()
            .is_empty());
        let hits = arm_a.search_nodes("battery usb", 3).unwrap();
        assert!(hits.iter().all(|h| h.doc == "manual.md"));
        assert!(!hits.is_empty());
    }

    fn cfg() -> GatingConfig {
        GatingConfig {
            relevance_threshold: 0.0,
            grounding_enabled: false,
            ..Default::default()
        }
    }

    async fn run(g: &AdaptiveGating, q: &str) -> Vec<ScoredResult> {
        g.retrieve(&[], &[], &[], q, None).await
    }

    #[tokio::test]
    async fn test_gating_arm_disabled_by_default_no_behavior_change() {
        let st = state();
        ingest(&st, "w", "manual.md", MANUAL);
        let off = Arc::new(PageIndexArm::new(Arc::clone(&st), "w"));
        assert!(!off.is_enabled());
        let plain = AdaptiveGating::new(cfg());
        let with_off = AdaptiveGating::new(cfg()).with_pageindex_arm(Some(off));
        let (a, b) = (
            run(&plain, "battery").await,
            run(&with_off, "battery").await,
        );
        assert_eq!(a, b);
        assert!(a.is_empty());
    }

    #[tokio::test]
    async fn test_gating_rrf_fuses_arm_with_layers() {
        let st = state();
        ingest(&st, "w", "manual.md", MANUAL);
        let on = Arc::new(arm(&st, "w").with_weight(2.0));
        let g = AdaptiveGating::new(cfg()).with_pageindex_arm(Some(on));
        let res = run(&g, "battery usb charges").await;
        assert!(!res.is_empty());
        assert!(res[0].path.starts_with("pageindex/manual.md#"));
        assert!(res[0].content.contains("Battery"));
        // weight 2 / (k + rank 1)
        let expect = 2.0 / (crate::retrieval::config::DEFAULT_RRF_K as f32 + 1.0);
        assert!((res[0].score - expect).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_gating_arm_failure_is_fail_open() {
        let dir = tempfile::tempdir().unwrap();
        // A directory as db path: opening the store fails.
        let settings = PageIndexSettings {
            db_path: dir.path().to_path_buf(),
            ..Default::default()
        };
        let broken = Arc::new(PageIndexState::new(settings));
        let bad = Arc::new(PageIndexArm::new(broken, "w").with_enabled(true));
        assert!(bad.search("battery", 3).is_err());
        let plain = AdaptiveGating::new(cfg());
        let g = AdaptiveGating::new(cfg()).with_pageindex_arm(Some(bad));
        assert_eq!(run(&plain, "battery").await, run(&g, "battery").await);
    }
}
