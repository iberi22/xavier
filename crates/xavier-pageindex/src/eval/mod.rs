//! Evaluation harness: gold queries, an LLM-free tree navigator and a report.
//!
//! The navigator is a deterministic lexical policy (BM25 over node text,
//! beam descent). It is a lower bound for what an LLM agent walking the same
//! tree can do, and it needs no network, so CI can run it.

pub mod metrics;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::PageIndexError;
use crate::model::TreeNode;
pub use metrics::{first_hit_rank, page_range_overlap, HitAtK, Mrr, Span};

/// One question with its gold page range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldQuery {
    #[serde(default)]
    pub id: String,
    /// Document name as ingested.
    #[serde(alias = "document")]
    pub doc: String,
    #[serde(alias = "question")]
    pub query: String,
    #[serde(alias = "start")]
    pub start_page: u32,
    #[serde(alias = "end")]
    pub end_page: u32,
    /// Optional phrase that must appear inside the gold pages.
    #[serde(default)]
    pub evidence: Option<String>,
}

impl GoldQuery {
    pub fn gold_span(&self) -> Span {
        Span::new(self.doc.clone(), self.start_page, self.end_page)
    }
}

/// Parse JSON-lines gold queries; blank lines and `#` comments are skipped.
pub fn parse_gold_jsonl(text: &str) -> Result<Vec<GoldQuery>, PageIndexError> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut q: GoldQuery = serde_json::from_str(line)
            .map_err(|e| PageIndexError::Build(format!("gold line {}: {e}", i + 1)))?;
        if q.id.is_empty() {
            q.id = format!("q{}", out.len() + 1);
        }
        if q.start_page < 1 || q.start_page > q.end_page {
            return Err(PageIndexError::InvalidRange(format!(
                "gold line {}: bad page range {}-{}",
                i + 1,
                q.start_page,
                q.end_page
            )));
        }
        out.push(q);
    }
    Ok(out)
}

/// Load an external benchmark exported as `<dir>/questions.jsonl` in the gold
/// format above. Returns `Ok(None)` when the directory has no such file, so
/// callers can skip. Nothing is ever downloaded.
pub fn load_benchmark_jsonl(
    dir: &std::path::Path,
) -> Result<Option<Vec<GoldQuery>>, PageIndexError> {
    let path = dir.join("questions.jsonl");
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| PageIndexError::Build(format!("read {}: {e}", path.display())))?;
    parse_gold_jsonl(&text).map(Some)
}

/// One document as the navigator sees it.
pub struct NavDoc<'a> {
    pub name: &'a str,
    pub roots: &'a [TreeNode],
}

/// A ranked navigator answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub span: Span,
    pub node_id: String,
    pub score: f64,
}

/// Node text used for scoring: title plus summary when present.
pub fn title_summary_text(n: &TreeNode) -> String {
    match &n.summary {
        Some(s) => format!("{} {}", n.title, s),
        None => n.title.clone(),
    }
}

/// Lowercase alphanumeric tokens of length >= 2.
pub fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= 2)
        .map(str::to_lowercase)
        .collect()
}

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

/// BM25 scorer whose idf comes from all node texts of the corpus.
struct Bm25 {
    df: HashMap<String, usize>,
    n: usize,
    avg_len: f64,
}

impl Bm25 {
    fn new(texts: &[Vec<String>]) -> Self {
        let mut df: HashMap<String, usize> = HashMap::new();
        for t in texts {
            let mut seen: Vec<&String> = t.iter().collect();
            seen.sort();
            seen.dedup();
            for w in seen {
                *df.entry(w.clone()).or_default() += 1;
            }
        }
        let total: usize = texts.iter().map(Vec::len).sum();
        Self {
            df,
            n: texts.len(),
            avg_len: (total as f64 / texts.len().max(1) as f64).max(1.0),
        }
    }

    fn score(&self, query: &[String], doc: &[String]) -> f64 {
        let mut s = 0.0;
        for q in query {
            let tf = doc.iter().filter(|w| *w == q).count() as f64;
            if tf == 0.0 {
                continue;
            }
            let df = *self.df.get(q).unwrap_or(&0) as f64;
            let idf = (1.0 + (self.n as f64 - df + 0.5) / (df + 0.5)).ln();
            let norm = 1.0 - BM25_B + BM25_B * doc.len() as f64 / self.avg_len;
            s += idf * tf * (BM25_K1 + 1.0) / (tf + BM25_K1 * norm);
        }
        s
    }
}

fn collect_texts<F: Fn(&TreeNode) -> String>(
    nodes: &[TreeNode],
    f: &F,
    out: &mut Vec<Vec<String>>,
) {
    for n in nodes {
        out.push(tokenize(&f(n)));
        collect_texts(&n.children, f, out);
    }
}

/// Beam-descent over a forest of documents.
///
/// A document is scored by its name plus its top-level titles; then, level by
/// level, the `k` best frontier entries are expanded to their children and
/// rescored (path score = sum of node scores). A node stops descending when
/// it is a leaf or no child matches the query. Returns at most `k` terminal
/// nodes, best first; ties break by document order, so output is deterministic.
pub fn navigate<F: Fn(&TreeNode) -> String>(
    docs: &[NavDoc<'_>],
    query: &str,
    text_of: F,
    k: usize,
) -> Vec<Candidate> {
    let q = tokenize(query);
    let mut corpus = Vec::new();
    for d in docs {
        corpus.push(tokenize(&doc_text(d)));
        collect_texts(d.roots, &text_of, &mut corpus);
    }
    let bm = Bm25::new(&corpus);

    struct Item<'a> {
        doc: &'a str,
        node: &'a TreeNode,
        score: f64,
    }
    let mut frontier: Vec<Item<'_>> = Vec::new();
    let mut ranked_docs: Vec<(usize, f64)> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| (i, bm.score(&q, &tokenize(&doc_text(d)))))
        .collect();
    ranked_docs.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut done: Vec<Item<'_>> = Vec::new();
    for (i, ds) in ranked_docs.into_iter().take(k) {
        if ds <= 0.0 {
            continue;
        }
        for r in docs[i].roots {
            let s = ds + bm.score(&q, &tokenize(&text_of(r)));
            frontier.push(Item {
                doc: docs[i].name,
                node: r,
                score: s,
            });
        }
    }
    let by_score = |a: &Item<'_>, b: &Item<'_>| b.score.total_cmp(&a.score);
    frontier.sort_by(by_score);
    frontier.truncate(k);
    while !frontier.is_empty() {
        let mut next: Vec<Item<'_>> = Vec::new();
        for it in frontier {
            let mut kids: Vec<Item<'_>> = it
                .node
                .children
                .iter()
                .map(|c| Item {
                    doc: it.doc,
                    node: c,
                    score: it.score + bm.score(&q, &tokenize(&text_of(c))),
                })
                .filter(|c| c.score > it.score)
                .collect();
            if kids.is_empty() {
                done.push(it);
            } else {
                next.append(&mut kids);
            }
        }
        next.sort_by(by_score);
        next.truncate(k);
        frontier = next;
    }
    done.sort_by(by_score);
    done.into_iter()
        .take(k)
        .map(|i| Candidate {
            span: Span::new(i.doc, i.node.start_page, i.node.end_page),
            node_id: i.node.node_id.clone(),
            score: i.score,
        })
        .collect()
}

fn doc_text(d: &NavDoc<'_>) -> String {
    let mut s = d.name.to_string();
    for r in d.roots {
        s.push(' ');
        s.push_str(&r.title);
    }
    s
}

/// Aggregated metrics of one retrieval column.
#[derive(Debug, Clone)]
pub struct ColumnStats {
    pub name: String,
    pub hit1: HitAtK,
    pub hit3: HitAtK,
    pub mrr: Mrr,
    pages_at1: u64,
    pages_at3: u64,
    queries: usize,
}

impl ColumnStats {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            hit1: HitAtK::new(1),
            hit3: HitAtK::new(3),
            mrr: Mrr::default(),
            pages_at1: 0,
            pages_at3: 0,
            queries: 0,
        }
    }

    /// Score one query. Cost proxy: distinct pages an agent would read for the
    /// top 1 and top 3 spans.
    pub fn add(&mut self, ranked: &[Span], gold: &Span) {
        let rank = first_hit_rank(ranked, gold);
        self.hit1.add(rank);
        self.hit3.add(rank);
        self.mrr.add(rank);
        self.pages_at1 += distinct_pages(&ranked[..ranked.len().min(1)]);
        self.pages_at3 += distinct_pages(&ranked[..ranked.len().min(3)]);
        self.queries += 1;
    }

    pub fn avg_pages_at1(&self) -> f64 {
        self.pages_at1 as f64 / self.queries.max(1) as f64
    }

    pub fn avg_pages_at3(&self) -> f64 {
        self.pages_at3 as f64 / self.queries.max(1) as f64
    }
}

fn distinct_pages(spans: &[Span]) -> u64 {
    let mut seen = std::collections::HashSet::new();
    for s in spans {
        for p in s.start_page..=s.end_page {
            seen.insert((s.doc.as_str(), p));
        }
    }
    seen.len() as u64
}

/// Render columns as a fixed-width table.
pub fn format_report(cols: &[ColumnStats]) -> String {
    let mut s = format!(
        "{:<28} {:>6} {:>6} {:>6} {:>9} {:>9}\n",
        "column", "hit@1", "hit@3", "MRR", "pages@1", "pages@3"
    );
    for c in cols {
        s.push_str(&format!(
            "{:<28} {:>6.3} {:>6.3} {:>6.3} {:>9.2} {:>9.2}\n",
            c.name,
            c.hit1.value(),
            c.hit3.value(),
            c.mrr.value(),
            c.avg_pages_at1(),
            c.avg_pages_at3()
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, title: &str, s: u32, e: u32, kids: Vec<TreeNode>) -> TreeNode {
        TreeNode {
            node_id: id.into(),
            title: title.into(),
            start_page: s,
            end_page: e,
            summary: None,
            token_estimate: 0,
            children: kids,
        }
    }

    fn sample() -> Vec<TreeNode> {
        vec![
            node(
                "0000",
                "Billing",
                1,
                4,
                vec![
                    node("0001", "Invoices", 1, 2, vec![]),
                    node("0002", "Refund policy", 3, 4, vec![]),
                ],
            ),
            node("0003", "Security", 5, 6, vec![]),
        ]
    }

    #[test]
    fn test_eval_page_range_hit_metric() {
        let g = Span::new("a", 3, 5);
        assert!(page_range_overlap(&Span::new("a", 5, 9), &g));
        assert!(page_range_overlap(&Span::new("a", 1, 3), &g));
        assert!(page_range_overlap(&Span::new("a", 4, 4), &g));
        assert!(!page_range_overlap(&Span::new("a", 6, 9), &g));
        assert!(!page_range_overlap(&Span::new("b", 3, 5), &g));
    }

    #[test]
    fn test_eval_hit_at_k_and_mrr() {
        let (mut h1, mut h3, mut m) = (HitAtK::new(1), HitAtK::new(3), Mrr::default());
        for r in [Some(1), Some(2), None, Some(5)] {
            h1.add(r);
            h3.add(r);
            m.add(r);
        }
        assert_eq!(h1.value(), 0.25);
        assert_eq!(h3.value(), 0.5);
        assert!((m.value() - (1.0 + 0.5 + 0.0 + 0.2) / 4.0).abs() < 1e-12);
        let ranked = [Span::new("a", 1, 1), Span::new("a", 4, 4)];
        assert_eq!(first_hit_rank(&ranked, &Span::new("a", 3, 4)), Some(2));
    }

    #[test]
    fn test_eval_loads_benchmark_jsonl() {
        let text = "# comment\n\
            {\"doc\":\"a.pdf\",\"question\":\"what?\",\"start\":2,\"end\":3}\n\
            \n\
            {\"id\":\"x\",\"doc\":\"b\",\"query\":\"why\",\"start_page\":1,\"end_page\":1,\"evidence\":\"e\"}\n";
        let qs = parse_gold_jsonl(text).unwrap();
        assert_eq!(qs.len(), 2);
        assert_eq!(qs[0].id, "q1");
        assert_eq!(qs[0].gold_span(), Span::new("a.pdf", 2, 3));
        assert_eq!(qs[1].evidence.as_deref(), Some("e"));
        assert!(parse_gold_jsonl(
            "{\"doc\":\"a\",\"query\":\"q\",\"start_page\":3,\"end_page\":2}"
        )
        .is_err());
        assert!(parse_gold_jsonl("not json").is_err());
        let dir = std::env::temp_dir().join("xavier_pi_eval_no_such_dir");
        assert!(load_benchmark_jsonl(&dir).unwrap().is_none());
    }

    #[test]
    fn test_eval_tree_navigator_lexical_baseline_deterministic() {
        let roots = sample();
        let docs = [NavDoc {
            name: "manual",
            roots: &roots,
        }];
        let a = navigate(&docs, "refund policy for billing", title_summary_text, 3);
        let b = navigate(&docs, "refund policy for billing", title_summary_text, 3);
        assert_eq!(a, b);
        assert_eq!(a[0].node_id, "0002");
        assert_eq!(a[0].span, Span::new("manual", 3, 4));
        // No lexical match anywhere: no candidates rather than a guess.
        assert!(navigate(&docs, "zebra", title_summary_text, 3).is_empty());
        let rep = format_report(&[ColumnStats::new("t")]);
        assert!(rep.contains("hit@1"));
    }
}
