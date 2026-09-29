use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use xavier_pageindex::optimize::{merge_tiny, optimize, split_huge, OptimizeOpts};
use xavier_pageindex::summarize::{summarize_tree, Summarizer, SummaryCache};
use xavier_pageindex::{DocumentTree, Page, PageIndex, PageIndexError, TreeNode};

fn node(title: &str, s: u32, e: u32, tokens: u32, children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        node_id: String::new(),
        title: title.into(),
        start_page: s,
        end_page: e,
        summary: None,
        token_estimate: tokens,
        children,
    }
}

fn tree(roots: Vec<TreeNode>) -> DocumentTree {
    let mut t = DocumentTree {
        doc_id: "d".into(),
        roots,
    };
    t.assign_node_ids();
    t
}

fn pages(n: u32) -> Vec<Page> {
    (1..=n)
        .map(|i| Page {
            doc_id: "d".into(),
            page_no: i,
            text: format!("text of page {i}"),
        })
        .collect()
}

fn covered(nodes: &[TreeNode], out: &mut Vec<bool>) {
    for n in nodes {
        if n.children.is_empty() {
            for p in n.start_page..=n.end_page {
                out[p as usize - 1] = true;
            }
        }
        covered(&n.children, out);
    }
}

#[test]
fn test_optimize_merges_tiny_leaf_into_prev_sibling() {
    let mut t = tree(vec![
        node("A", 1, 2, 500, vec![]),
        node("tiny", 3, 3, 10, vec![]),
        node("B", 3, 5, 500, vec![]),
    ]);
    merge_tiny(&mut t, 200, 6000);
    assert_eq!(t.roots.len(), 2);
    assert_eq!((t.roots[0].start_page, t.roots[0].end_page), (1, 3));
    assert_eq!(t.roots[0].token_estimate, 510);
    assert_eq!(t.roots[1].node_id, "0001");
    t.validate(5).unwrap();
}

#[test]
fn test_optimize_does_not_merge_tiny_leaf_into_parent_with_children() {
    let mut t = tree(vec![
        node(
            "prev",
            1,
            3,
            400,
            vec![node("c1", 1, 2, 200, vec![]), node("c2", 2, 3, 200, vec![])],
        ),
        node("tiny", 3, 4, 10, vec![]),
    ]);
    merge_tiny(&mut t, 250, 6000);
    t.validate(4).unwrap();
    assert_eq!(t.roots.len(), 2);
    let mut cov = vec![false; 4];
    covered(&t.roots, &mut cov);
    assert!(cov.iter().all(|c| *c), "page 4 must stay covered by a leaf");
    assert_eq!(t.roots[1].end_page, 4);
}

#[test]
fn test_optimize_splits_node_over_token_limit_on_page_boundaries() {
    let mut t = tree(vec![node("Big", 1, 6, 6000, vec![])]);
    split_huge(&mut t, &pages(6), 2000);
    let kids = &t.roots[0].children;
    assert!(kids.len() >= 3);
    assert_eq!(kids[0].start_page, 1);
    assert_eq!(kids.last().unwrap().end_page, 6);
    for w in kids.windows(2) {
        assert_eq!(w[0].end_page + 1, w[1].start_page);
    }
    assert!(kids.iter().all(|k| k.token_estimate <= 2000));
    t.validate(6).unwrap();
}

#[test]
fn test_optimize_preserves_page_coverage() {
    let mut t = tree(vec![
        node("A", 1, 1, 5, vec![]),
        node("B", 1, 3, 20, vec![]),
        node("Big", 3, 8, 9000, vec![]),
        node("C", 8, 9, 5, vec![]),
        node("D", 9, 10, 5, vec![]),
    ]);
    optimize(
        &mut t,
        &pages(10),
        OptimizeOpts {
            min_node_tokens: 100,
            max_node_tokens: 3000,
        },
    );
    t.validate(10).unwrap();
    let mut seen = vec![false; 10];
    covered(&t.roots, &mut seen);
    assert!(seen.iter().all(|c| *c), "{seen:?}");
}

#[test]
fn test_optimize_is_idempotent() {
    let opts = OptimizeOpts {
        min_node_tokens: 100,
        max_node_tokens: 3000,
    };
    let mut t = tree(vec![
        node("A", 1, 2, 40, vec![]),
        node("B", 2, 3, 30, vec![]),
        node("Big", 3, 9, 8000, vec![]),
        node("E", 9, 10, 20, vec![]),
        node("F", 10, 10, 20, vec![]),
    ]);
    optimize(&mut t, &pages(10), opts);
    let once = t.clone();
    optimize(&mut t, &pages(10), opts);
    assert_eq!(t, once);
}

struct Fake {
    order: Mutex<Vec<String>>,
    calls: AtomicUsize,
    fail_on: Option<&'static str>,
}

impl Fake {
    fn new(fail_on: Option<&'static str>) -> Self {
        Self {
            order: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
            fail_on,
        }
    }
}

impl Summarizer for Fake {
    fn model(&self) -> &str {
        "fake"
    }
    fn summarize(&self, title: &str, _text: &str) -> Result<String, PageIndexError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.order.lock().unwrap().push(title.to_string());
        if self.fail_on == Some(title) {
            return Err(PageIndexError::Build("llm down".into()));
        }
        Ok(format!("sum {title}"))
    }
    fn complete(&self, prompt: &str) -> Result<String, PageIndexError> {
        Ok(prompt.to_string())
    }
}

fn nested() -> DocumentTree {
    tree(vec![node(
        "Root",
        1,
        4,
        0,
        vec![
            node("Left", 1, 2, 0, vec![node("Deep", 1, 1, 0, vec![])]),
            node("Right", 3, 4, 0, vec![]),
        ],
    )])
}

#[test]
fn test_summarize_bottom_up_order() {
    let mut t = nested();
    let f = Fake::new(None);
    let r = summarize_tree(&mut t, &pages(4), &f, &SummaryCache::new(), 2);
    assert_eq!(r.summarized, 4);
    let order = f.order.lock().unwrap().clone();
    let pos = |n: &str| order.iter().position(|t| t == n).unwrap();
    assert!(pos("Deep") < pos("Left"));
    assert!(pos("Left") < pos("Root"));
    assert!(pos("Right") < pos("Root"));
    assert_eq!(t.roots[0].summary.as_deref(), Some("sum Root"));
}

#[test]
fn test_summarize_cache_hit_skips_llm() {
    let cache = SummaryCache::new();
    let f = Fake::new(None);
    summarize_tree(&mut nested(), &pages(4), &f, &cache, 1);
    assert_eq!(f.calls.load(Ordering::SeqCst), 4);
    let mut again = nested();
    let r = summarize_tree(&mut again, &pages(4), &f, &cache, 1);
    assert_eq!(f.calls.load(Ordering::SeqCst), 4);
    assert_eq!((r.cached, r.summarized), (4, 0));
    assert!(again.roots[0].summary.is_some());
}

#[test]
fn test_summarize_failure_leaves_summary_none_and_tree_valid() {
    let mut t = nested();
    let f = Fake::new(Some("Left"));
    let r = summarize_tree(&mut t, &pages(4), &f, &SummaryCache::new(), 2);
    assert_eq!(r.failed, 1);
    assert!(t.roots[0].children[0].summary.is_none());
    // Parent still summarized from the surviving child.
    assert!(t.roots[0].summary.is_some());
    t.validate(4).unwrap();
}

#[cfg(all(feature = "sqlite", feature = "markdown"))]
mod facade {
    use super::*;
    use xavier_pageindex::store::SqliteStore;
    use xavier_pageindex::{IngestOptions, Source, StructureOpts};

    const MD: &str = "# One\n\nalpha\n\n# Two\n\nbeta\n";

    #[test]
    fn test_tree_builds_with_no_llm_configured() {
        let pi = PageIndex::new(SqliteStore::open_in_memory().unwrap());
        let opts = IngestOptions {
            summarize: true,
            optimize: true,
            ..Default::default()
        };
        pi.ingest("ws", "a.md", Source::Markdown(MD), opts).unwrap();
        let v = pi
            .get_structure("ws", "a.md", StructureOpts::default())
            .unwrap();
        assert!(!v.nodes.is_empty());
    }

    #[test]
    fn test_ingest_summarizes_with_injected_summarizer() {
        let f = Arc::new(Fake::new(None));
        let pi = PageIndex::new(SqliteStore::open_in_memory().unwrap()).with_summarizer(f.clone());
        let opts = IngestOptions {
            summarize: true,
            ..Default::default()
        };
        pi.ingest("ws", "a.md", Source::Markdown(MD), opts).unwrap();
        assert!(f.calls.load(Ordering::SeqCst) >= 2);
        let off = IngestOptions::default();
        let before = f.calls.load(Ordering::SeqCst);
        pi.ingest("ws", "b.md", Source::Markdown(MD), off).unwrap();
        assert_eq!(f.calls.load(Ordering::SeqCst), before);
    }

    struct CountingStore {
        inner: SqliteStore,
        puts: Arc<AtomicUsize>,
    }

    impl xavier_pageindex::store::Store for CountingStore {
        fn put_document(
            &self,
            d: &xavier_pageindex::Document,
            t: &DocumentTree,
            p: &[Page],
        ) -> Result<xavier_pageindex::store::PutOutcome, PageIndexError> {
            self.puts.fetch_add(1, Ordering::SeqCst);
            self.inner.put_document(d, t, p)
        }
        fn get_document(
            &self,
            w: &str,
            d: &str,
        ) -> Result<Option<xavier_pageindex::Document>, PageIndexError> {
            self.inner.get_document(w, d)
        }
        fn find_by_name(
            &self,
            w: &str,
            n: &str,
        ) -> Result<Option<xavier_pageindex::Document>, PageIndexError> {
            self.inner.find_by_name(w, n)
        }
        fn get_tree(&self, w: &str, d: &str) -> Result<Option<DocumentTree>, PageIndexError> {
            self.inner.get_tree(w, d)
        }
        fn get_pages(&self, w: &str, d: &str, s: u32, e: u32) -> Result<Vec<Page>, PageIndexError> {
            self.inner.get_pages(w, d, s, e)
        }
        fn list_documents(
            &self,
            w: &str,
        ) -> Result<Vec<xavier_pageindex::Document>, PageIndexError> {
            self.inner.list_documents(w)
        }
        fn search_by_title(
            &self,
            w: &str,
            q: &str,
        ) -> Result<Vec<xavier_pageindex::Document>, PageIndexError> {
            self.inner.search_by_title(w, q)
        }
        fn delete_document(&self, w: &str, d: &str) -> Result<bool, PageIndexError> {
            self.inner.delete_document(w, d)
        }
    }

    #[test]
    fn test_reingest_identical_content_skips_build() {
        let f = Arc::new(Fake::new(None));
        let puts = Arc::new(AtomicUsize::new(0));
        let store = CountingStore {
            inner: SqliteStore::open_in_memory().unwrap(),
            puts: puts.clone(),
        };
        let pi = PageIndex::new(store).with_summarizer(f.clone());
        let opts = IngestOptions {
            summarize: true,
            ..Default::default()
        };
        let first = pi
            .ingest("ws", "a.md", Source::Markdown(MD), opts.clone())
            .unwrap();
        let calls = f.calls.load(Ordering::SeqCst);
        let second = pi.ingest("ws", "a.md", Source::Markdown(MD), opts).unwrap();
        assert_eq!(first, second);
        assert_eq!(f.calls.load(Ordering::SeqCst), calls);
        assert_eq!(puts.load(Ordering::SeqCst), 1);
    }
}
