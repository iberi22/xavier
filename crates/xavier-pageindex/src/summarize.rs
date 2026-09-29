//! Optional bottom-up node summaries. The crate stays LLM-free: callers plug
//! in a [`Summarizer`]; a failing node keeps `summary = None`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::error::PageIndexError;
use crate::model::{DocumentTree, Page, TreeNode};

/// Longest text (chars) handed to a summarizer for one node.
pub const MAX_INPUT_CHARS: usize = 24_000;

/// Blocking text summarizer. Async callers run the whole ingest inside
/// `spawn_blocking`; implementations may bridge to async internally.
pub trait Summarizer: Send + Sync {
    /// Model identifier; part of the cache key.
    fn model(&self) -> &str;
    /// Summarize `text` (page text for leaves, child summaries for parents).
    fn summarize(&self, title: &str, text: &str) -> Result<String, PageIndexError>;
    /// Send `prompt` to the model verbatim, with no summary framing. Used for
    /// structured requests (e.g. the PDF LLM table of contents).
    fn complete(&self, prompt: &str) -> Result<String, PageIndexError>;
}

/// In-memory summary cache keyed by sha256(title + text + model).
#[derive(Default)]
pub struct SummaryCache(Mutex<HashMap<String, String>>);

impl SummaryCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.0.lock().map_or(0, |m| m.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().ok()?.get(key).cloned()
    }

    fn put(&self, key: String, value: String) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(key, value);
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SummarizeReport {
    pub summarized: usize,
    pub cached: usize,
    pub failed: usize,
}

/// Outcome of one node plus whether it was a cache hit.
type JobResult = (Result<String, PageIndexError>, bool);

fn cache_key(title: &str, text: &str, model: &str) -> String {
    let mut h = Sha256::new();
    for part in [title, text, model] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn node_at<'a>(roots: &'a [TreeNode], path: &[usize]) -> &'a TreeNode {
    let mut n = &roots[path[0]];
    for &i in &path[1..] {
        n = &n.children[i];
    }
    n
}

fn node_at_mut<'a>(roots: &'a mut [TreeNode], path: &[usize]) -> &'a mut TreeNode {
    let mut n = &mut roots[path[0]];
    for &i in &path[1..] {
        n = &mut n.children[i];
    }
    n
}

fn collect(nodes: &[TreeNode], prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    for (i, n) in nodes.iter().enumerate() {
        prefix.push(i);
        out.push(prefix.clone());
        collect(&n.children, prefix, out);
        prefix.pop();
    }
}

fn page_text(n: &TreeNode, pages: &[Page]) -> String {
    let mut s = String::new();
    for p in pages
        .iter()
        .filter(|p| p.page_no >= n.start_page && p.page_no <= n.end_page)
    {
        s.push_str(&p.text);
        s.push('\n');
    }
    s.chars().take(MAX_INPUT_CHARS).collect()
}

/// Input of one node: page text for leaves, child summaries for parents
/// (page text when no child has a summary).
fn node_input(n: &TreeNode, pages: &[Page]) -> String {
    let lines: Vec<String> = n
        .children
        .iter()
        .filter_map(|c| {
            c.summary
                .as_deref()
                .map(|s| format!("- {}: {}", c.title, s))
        })
        .collect();
    if lines.is_empty() {
        page_text(n, pages)
    } else {
        lines.join("\n").chars().take(MAX_INPUT_CHARS).collect()
    }
}

/// Fill `summary` bottom-up. Nodes at the same depth are independent and run
/// on up to `concurrency` threads; a parent starts only after all deeper
/// levels are done. Failures leave the node's summary `None`.
pub fn summarize_tree(
    tree: &mut DocumentTree,
    pages: &[Page],
    summarizer: &dyn Summarizer,
    cache: &SummaryCache,
    concurrency: usize,
) -> SummarizeReport {
    let mut paths = Vec::new();
    collect(&tree.roots, &mut Vec::new(), &mut paths);
    let max_depth = paths.iter().map(Vec::len).max().unwrap_or(0);
    let workers = concurrency.max(1);
    let mut report = SummarizeReport::default();
    for depth in (1..=max_depth).rev() {
        let level: Vec<&Vec<usize>> = paths.iter().filter(|p| p.len() == depth).collect();
        let jobs: Vec<(String, String)> = level
            .iter()
            .map(|p| {
                let n = node_at(&tree.roots, p);
                (n.title.clone(), node_input(n, pages))
            })
            .collect();
        let results: Vec<Mutex<Option<JobResult>>> =
            jobs.iter().map(|_| Mutex::new(None)).collect();
        let next = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..workers.min(jobs.len()) {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some((title, text)) = jobs.get(i) else {
                        break;
                    };
                    let key = cache_key(title, text, summarizer.model());
                    let out = match cache.get(&key) {
                        Some(hit) => (Ok(hit), true),
                        None => {
                            let r = summarizer.summarize(title, text);
                            if let Ok(v) = &r {
                                cache.put(key, v.clone());
                            }
                            (r, false)
                        }
                    };
                    if let Ok(mut slot) = results[i].lock() {
                        *slot = Some(out);
                    }
                });
            }
        });
        for (p, slot) in level.iter().zip(results) {
            let out = slot.into_inner().ok().flatten();
            match out {
                Some((Ok(text), hit)) if !text.trim().is_empty() => {
                    node_at_mut(&mut tree.roots, p).summary = Some(text.trim().to_string());
                    if hit {
                        report.cached += 1;
                    } else {
                        report.summarized += 1;
                    }
                }
                _ => report.failed += 1,
            }
        }
    }
    report
}
