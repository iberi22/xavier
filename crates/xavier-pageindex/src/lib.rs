//! Standalone PageIndex-style tree retrieval. Must not depend on `xavier`.

pub mod builders;
pub mod error;
pub mod eval;
pub mod model;
pub mod optimize;
#[cfg(any(feature = "pdf-outline", feature = "pdf-layout"))]
pub mod pdf;
pub mod query;
pub mod search;
pub mod store;
pub mod summarize;

pub use error::PageIndexError;
pub use model::{DocStatus, Document, DocumentTree, Page, PageUnit, SourceKind, TreeNode};

pub use query::{
    BrowsePage, BrowseQuery, DocumentInfo, DocumentSummary, PageContent, PageIndexConfig, PageText,
    StructureNode, StructureOpts, StructureView,
};
pub use search::{SearchHit, SearchResults};

use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::builders::plain::{self, DEFAULT_LINES_PER_PAGE};
use crate::query::{
    cap_pages, count_nodes, find_node, not_found_message, parse_page_spec, similar_names,
    status_str, structure_view, summarize_doc, DEFAULT_BROWSE_LIMIT, MAX_BROWSE_LIMIT,
    STRUCTURE_FIRST_PAGE_THRESHOLD,
};
use crate::store::Store;
use crate::summarize::{summarize_tree, Summarizer, SummaryCache};

pub use crate::optimize::OptimizeOpts;

/// Raw document handed to [`PageIndex::ingest`].
pub enum Source<'a> {
    Markdown(&'a str),
    PlainText(&'a str),
    Legal(&'a str),
    Pdf(&'a [u8]),
}

#[derive(Debug, Clone)]
pub struct IngestOptions {
    /// Summarize nodes; a no-op unless a summarizer is set on the facade.
    pub summarize: bool,
    /// Merge tiny and split huge nodes (deterministic, no LLM).
    pub optimize: bool,
    /// Virtual page size for text sources; 0 means the default.
    pub lines_per_page: u32,
}

impl Default for IngestOptions {
    fn default() -> Self {
        Self {
            summarize: false,
            optimize: false,
            lines_per_page: DEFAULT_LINES_PER_PAGE as u32,
        }
    }
}

/// Facade over a store. Synchronous; async callers wrap it in `spawn_blocking`.
pub struct PageIndex<S> {
    store: S,
    cfg: PageIndexConfig,
    optimize: OptimizeOpts,
    summarizer: Option<Arc<dyn Summarizer>>,
    summaries: SummaryCache,
}

/// Threads used for summarizing nodes of one tree level.
const SUMMARY_CONCURRENCY: usize = 4;

impl<S> PageIndex<S> {
    pub fn new(store: S) -> Self {
        Self::with_config(store, PageIndexConfig::default())
    }

    pub fn with_config(store: S, cfg: PageIndexConfig) -> Self {
        Self {
            store,
            cfg,
            optimize: OptimizeOpts::default(),
            summarizer: None,
            summaries: SummaryCache::new(),
        }
    }

    /// Thresholds used when `IngestOptions::optimize` is set.
    pub fn with_optimize_opts(mut self, opts: OptimizeOpts) -> Self {
        self.optimize = opts;
        self
    }

    /// Summarizer used when `IngestOptions::summarize` is set.
    pub fn with_summarizer(mut self, summarizer: Arc<dyn Summarizer>) -> Self {
        self.summarizer = Some(summarizer);
        self
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl<S: Store> PageIndex<S> {
    /// Build and store a document. Idempotent on `(workspace, name, content_hash)`.
    pub fn ingest(
        &self,
        ws: &str,
        name: &str,
        src: Source<'_>,
        opts: IngestOptions,
    ) -> Result<Document, PageIndexError> {
        let lpp = match opts.lines_per_page as usize {
            0 => DEFAULT_LINES_PER_PAGE,
            n => n,
        };
        let content_hash = match &src {
            Source::Markdown(t) | Source::PlainText(t) | Source::Legal(t) => {
                hex(&Sha256::digest(t.as_bytes()))
            }
            #[cfg(feature = "pdf-outline")]
            Source::Pdf(b) => hex(&Sha256::digest(b)),
            #[cfg(not(feature = "pdf-outline"))]
            Source::Pdf(_) => return Err(PageIndexError::FeatureDisabled("pdf")),
        };
        let doc_id = format!(
            "pi_{}",
            &hex(&Sha256::digest(format!("{ws}\0{name}\0{content_hash}")))[..16]
        );
        let (source_kind, page_unit, builder, mut tree, mut pages) = match src {
            #[cfg(feature = "markdown")]
            Source::Markdown(t) => {
                let (tree, pages) = builders::markdown::build(t, lpp);
                (
                    SourceKind::Markdown,
                    PageUnit::VirtualPage,
                    "markdown",
                    tree,
                    pages,
                )
            }
            #[cfg(not(feature = "markdown"))]
            Source::Markdown(_) => return Err(PageIndexError::FeatureDisabled("markdown")),
            Source::PlainText(t) => {
                let b = plain::build(&doc_id, t, lpp)?;
                (
                    SourceKind::PlainText,
                    PageUnit::VirtualPage,
                    b.builder,
                    b.tree,
                    b.pages,
                )
            }
            Source::Legal(t) => {
                let b = builders::legal::build(&doc_id, t, lpp)?;
                (
                    SourceKind::Legal,
                    PageUnit::VirtualPage,
                    b.builder,
                    b.tree,
                    b.pages,
                )
            }
            #[cfg(feature = "pdf-outline")]
            Source::Pdf(b) => {
                // The LLM table of contents is only tried when summaries are requested.
                let llm = self.summarizer.as_deref().filter(|_| opts.summarize);
                let b = pdf::cascade::build_pdf_tree(&doc_id, b, llm)?;
                let label = b.builder();
                (SourceKind::Pdf, PageUnit::Page, label, b.tree, b.pages)
            }
            #[cfg(not(feature = "pdf-outline"))]
            Source::Pdf(_) => return Err(PageIndexError::FeatureDisabled("pdf")),
        };
        tree.doc_id = doc_id.clone();
        for p in &mut pages {
            p.doc_id = doc_id.clone();
        }
        let page_count = pages.len() as u32;
        if opts.optimize {
            optimize::optimize(&mut tree, &pages, self.optimize);
        }
        tree.validate(page_count)?;
        if let (true, Some(sm)) = (opts.summarize, &self.summarizer) {
            // Failed nodes keep `summary = None`; the tree stays valid.
            summarize_tree(
                &mut tree,
                &pages,
                sm.as_ref(),
                &self.summaries,
                SUMMARY_CONCURRENCY,
            );
        }
        let doc = Document {
            doc_id,
            workspace: ws.to_string(),
            name: name.to_string(),
            source_kind,
            content_hash,
            page_count,
            page_unit,
            status: DocStatus::Completed,
            builder: builder.to_string(),
            created_at: now_secs(),
        };
        let outcome = self.store.put_document(&doc, &tree, &pages)?;
        self.store
            .get_document(ws, &outcome.doc_id)?
            .ok_or_else(|| PageIndexError::Store("document vanished after insert".into()))
    }

    /// List documents newest first, optionally filtered by name or node title.
    pub fn browse(&self, ws: &str, q: BrowseQuery) -> Result<BrowsePage, PageIndexError> {
        let mut docs = match q.query.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(text) => self.store.search_by_title(ws, text)?,
            None => self.store.list_documents(ws)?,
        };
        docs.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.name.cmp(&b.name))
        });
        let limit = match q.limit {
            0 => DEFAULT_BROWSE_LIMIT,
            n => n.min(MAX_BROWSE_LIMIT),
        };
        let total = docs.len();
        let end = q.offset.saturating_add(limit).min(total);
        let documents: Vec<_> = docs
            .iter()
            .skip(q.offset)
            .take(limit)
            .map(summarize_doc)
            .collect();
        let has_more = end < total;
        let mut next_steps = Vec::new();
        if has_more {
            next_steps.push(format!("More documents available: pass offset={end}"));
        }
        if documents.is_empty() {
            next_steps
                .push("No documents matched; try a broader query or ingest a document".into());
        } else {
            next_steps.push(
                "Call get_document() for status, then get_document_structure() to locate sections"
                    .into(),
            );
        }
        Ok(BrowsePage {
            documents,
            total,
            offset: q.offset,
            next_offset: has_more.then_some(end),
            has_more,
            next_steps,
        })
    }

    /// Resolve a name, or fail with similar-name hints.
    fn resolve(&self, ws: &str, name: &str) -> Result<Document, PageIndexError> {
        if let Some(d) = self.store.find_by_name(ws, name)? {
            return Ok(d);
        }
        let docs = self.store.list_documents(ws)?;
        let similar = similar_names(docs.iter().map(|d| d.name.as_str()), name);
        Err(PageIndexError::NotFound(not_found_message(name, &similar)))
    }

    /// Like `resolve`, but the document must be ready to read.
    fn resolve_ready(&self, ws: &str, name: &str) -> Result<Document, PageIndexError> {
        let doc = self.resolve(ws, name)?;
        match &doc.status {
            DocStatus::Completed => Ok(doc),
            DocStatus::Processing => Err(PageIndexError::Build(format!(
                "document '{name}' is still processing; check get_document() later"
            ))),
            DocStatus::Failed(why) => Err(PageIndexError::Build(format!(
                "document '{name}' failed to process: {why}"
            ))),
        }
    }

    /// Names of stored documents closest to `name` (top 3).
    pub fn similar_document_names(
        &self,
        ws: &str,
        name: &str,
    ) -> Result<Vec<String>, PageIndexError> {
        let docs = self.store.list_documents(ws)?;
        Ok(similar_names(docs.iter().map(|d| d.name.as_str()), name))
    }

    pub fn get_document(&self, ws: &str, name: &str) -> Result<DocumentInfo, PageIndexError> {
        let doc = self.resolve(ws, name)?;
        let node_count = self
            .store
            .get_tree(ws, &doc.doc_id)?
            .map_or(0, |t| count_nodes(&t.roots));
        let structure_first = doc.page_count > STRUCTURE_FIRST_PAGE_THRESHOLD;
        let next_steps = match &doc.status {
            DocStatus::Completed if structure_first => vec![format!(
                "Document has {} pages: call get_document_structure() first, then \
                 get_page_content() with tight ranges",
                doc.page_count
            )],
            DocStatus::Completed => vec![
                "Call get_document_structure() to locate sections, or get_page_content() \
                 directly for this short document"
                    .into(),
            ],
            DocStatus::Processing => vec!["Still processing; check again later".into()],
            DocStatus::Failed(_) => vec!["Processing failed; ingest the document again".into()],
        };
        Ok(DocumentInfo {
            name: doc.name.clone(),
            doc_id: doc.doc_id.clone(),
            status: status_str(&doc.status).into(),
            error: match &doc.status {
                DocStatus::Failed(why) => Some(why.clone()),
                _ => None,
            },
            source_kind: doc.source_kind,
            page_count: doc.page_count,
            page_unit: doc.page_unit,
            structure_first,
            node_count,
            builder: doc.builder.clone(),
            created_at: doc.created_at,
            next_steps,
        })
    }

    /// Outline without page text, optionally depth-limited or rooted at a node.
    pub fn get_structure(
        &self,
        ws: &str,
        name: &str,
        opts: StructureOpts,
    ) -> Result<StructureView, PageIndexError> {
        let doc = self.resolve_ready(ws, name)?;
        let tree = self
            .store
            .get_tree(ws, &doc.doc_id)?
            .ok_or_else(|| PageIndexError::NotFound(format!("tree of '{name}' is missing")))?;
        structure_view(&doc, &tree.roots, &opts, self.cfg.structure_char_budget)
    }

    /// Text of the pages in `pages` (`"5-7,12"`, 1-based, max 20 pages),
    /// truncated to `cap` chars in total (0 means the configured cap).
    pub fn get_pages(
        &self,
        ws: &str,
        name: &str,
        pages: &str,
        cap: usize,
    ) -> Result<PageContent, PageIndexError> {
        let doc = self.resolve_ready(ws, name)?;
        let wanted = parse_page_spec(pages, doc.page_count)?;
        self.fetch_pages(ws, &doc, &wanted, cap)
    }

    /// Text of the pages covered by a tree node, under the same cap.
    pub fn get_node_pages(
        &self,
        ws: &str,
        name: &str,
        node_id: &str,
        cap: usize,
    ) -> Result<PageContent, PageIndexError> {
        let doc = self.resolve_ready(ws, name)?;
        let tree = self
            .store
            .get_tree(ws, &doc.doc_id)?
            .ok_or_else(|| PageIndexError::NotFound(format!("tree of '{name}' is missing")))?;
        let node = find_node(&tree.roots, node_id).ok_or_else(|| {
            PageIndexError::NotFound(format!("node_id '{node_id}' not found in '{name}'"))
        })?;
        let wanted: Vec<u32> = (node.start_page..=node.end_page).collect();
        if wanted.len() > query::MAX_PAGES_PER_CALL {
            return Err(PageIndexError::InvalidRange(format!(
                "node {node_id} spans pages {}-{} (more than {} pages); expand its children \
                 with get_document_structure(node_id) and read them one by one",
                node.start_page,
                node.end_page,
                query::MAX_PAGES_PER_CALL
            )));
        }
        self.fetch_pages(ws, &doc, &wanted, cap)
    }

    fn fetch_pages(
        &self,
        ws: &str,
        doc: &Document,
        wanted: &[u32],
        cap: usize,
    ) -> Result<PageContent, PageIndexError> {
        let cap = if cap == 0 {
            self.cfg.response_char_cap
        } else {
            cap
        };
        let (lo, hi) = (wanted[0], wanted[wanted.len() - 1]);
        let fetched: Vec<Page> = self
            .store
            .get_pages(ws, &doc.doc_id, lo, hi)?
            .into_iter()
            .filter(|p| wanted.binary_search(&p.page_no).is_ok())
            .collect();
        Ok(cap_pages(doc, wanted, fetched, cap))
    }

    /// BM25 search inside one document: ranked pages with breadcrumb and
    /// snippet. `limit` 0 means the default; it is clamped to the maximum.
    pub fn search(
        &self,
        ws: &str,
        name: &str,
        query: &str,
        limit: usize,
    ) -> Result<SearchResults, PageIndexError> {
        let doc = self.resolve_ready(ws, name)?;
        let tree = self
            .store
            .get_tree(ws, &doc.doc_id)?
            .ok_or_else(|| PageIndexError::NotFound(format!("tree of '{name}' is missing")))?;
        let pages = self.store.get_pages(ws, &doc.doc_id, 1, doc.page_count)?;
        let limit = match limit {
            0 => search::DEFAULT_SEARCH_LIMIT,
            n => n.min(search::MAX_SEARCH_LIMIT),
        };
        search::search_document(
            &doc,
            &tree.roots,
            &pages,
            query,
            limit,
            self.cfg.response_char_cap,
        )
    }

    pub fn delete(&self, ws: &str, name: &str) -> Result<(), PageIndexError> {
        let doc = self.resolve(ws, name)?;
        self.store.delete_document(ws, &doc.doc_id)?;
        Ok(())
    }
}
