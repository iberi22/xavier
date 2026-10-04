//! PAGEINDEX.14: page-range retrieval eval on committed fixtures.
//!
//! CI columns are LLM-free: DocBot BM25 chunks vs a lexical tree navigator.
//! The real-LLM navigator is `#[ignore]`d and env-gated (see the spec, sec. 10).
#![cfg(feature = "pageindex")]

use std::path::PathBuf;
use std::sync::Arc;

use xavier::collections::indexer::{ingest_document, ChunkConfig};
use xavier::collections::schema::DocCollection;
use xavier::collections::store::CollectionStore;
use xavier::pageindex_glue::{PageIndexSettings, PageIndexState};
use xavier::retrieval::gating::{AdaptiveGating, GatingConfig};
use xavier::retrieval::pageindex_arm::PageIndexArm;
use xavier_pageindex::eval::{
    format_report, navigate, parse_gold_jsonl, title_summary_text, ColumnStats, GoldQuery, NavDoc,
    Span,
};
use xavier_pageindex::store::SqliteStore;
use xavier_pageindex::{IngestOptions, PageIndex, Source, StructureNode, StructureOpts, TreeNode};

const WS: &str = "eval";
const LINES_PER_PAGE: u32 = 8;
const DOCS: [(&str, Kind); 3] = [
    ("manual.md", Kind::Markdown),
    ("contract.txt", Kind::Legal),
    ("handbook.txt", Kind::Plain),
];

#[derive(Clone, Copy)]
enum Kind {
    Markdown,
    Legal,
    Plain,
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pageindex")
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_dir().join(name)).expect("fixture readable")
}

fn gold() -> Vec<GoldQuery> {
    parse_gold_jsonl(&read_fixture("gold.jsonl")).expect("gold parses")
}

fn build_index() -> PageIndex<SqliteStore> {
    let pi = PageIndex::new(SqliteStore::open_in_memory().unwrap());
    ingest_fixtures(&pi);
    pi
}

fn ingest_fixtures(pi: &PageIndex<SqliteStore>) {
    let opts = IngestOptions {
        lines_per_page: LINES_PER_PAGE,
        ..Default::default()
    };
    for (name, kind) in DOCS {
        let text = read_fixture(name);
        let src = match kind {
            Kind::Markdown => Source::Markdown(&text),
            Kind::Legal => Source::Legal(&text),
            Kind::Plain => Source::PlainText(&text),
        };
        pi.ingest(WS, name, src, opts.clone()).unwrap();
    }
}

/// Words kept as a deterministic extractive stand-in for an LLM summary.
const LEAD_WORDS: usize = 40;

fn to_tree(pi: &PageIndex<SqliteStore>, doc: &str, n: &StructureNode, lead: bool) -> TreeNode {
    let summary = lead.then(|| {
        let page = pi
            .get_pages(WS, doc, &n.start_page.to_string(), 0)
            .unwrap()
            .pages
            .remove(0)
            .text;
        page.split_whitespace()
            .take(LEAD_WORDS)
            .collect::<Vec<_>>()
            .join(" ")
    });
    TreeNode {
        node_id: n.node_id.clone(),
        title: n.title.clone(),
        start_page: n.start_page,
        end_page: n.end_page,
        summary,
        token_estimate: n.token_estimate,
        children: n
            .children
            .iter()
            .map(|c| to_tree(pi, doc, c, lead))
            .collect(),
    }
}

fn forest(pi: &PageIndex<SqliteStore>, lead: bool) -> Vec<(String, Vec<TreeNode>)> {
    DOCS.iter()
        .map(|(name, _)| {
            let view = pi
                .get_structure(WS, name, StructureOpts::default())
                .unwrap();
            let roots = view
                .nodes
                .iter()
                .map(|n| to_tree(pi, name, n, lead))
                .collect();
            (name.to_string(), roots)
        })
        .collect()
}

fn tree_column(name: &str, forest: &[(String, Vec<TreeNode>)], qs: &[GoldQuery]) -> ColumnStats {
    let docs: Vec<NavDoc<'_>> = forest
        .iter()
        .map(|(n, r)| NavDoc { name: n, roots: r })
        .collect();
    let mut col = ColumnStats::new(name);
    for q in qs {
        let ranked: Vec<Span> = navigate(&docs, &q.query, title_summary_text, 3)
            .into_iter()
            .map(|c| c.span)
            .collect();
        col.add(&ranked, &q.gold_span());
    }
    col
}

/// Pages (1-based) whose text contains a line of `chunk`; None when nothing matches.
fn chunk_span(pages: &[(u32, String)], doc: &str, chunk: &str) -> Option<Span> {
    let mut hit: Vec<u32> = Vec::new();
    for line in chunk.lines().map(str::trim).filter(|l| l.len() >= 15) {
        for (no, text) in pages {
            if text.lines().any(|l| l.trim() == line) {
                hit.push(*no);
            }
        }
    }
    let (lo, hi) = (hit.iter().min()?, hit.iter().max()?);
    Some(Span::new(doc, *lo, *hi))
}

async fn docbot_column(
    name: &str,
    pi: &PageIndex<SqliteStore>,
    qs: &[GoldQuery],
    chunk: &ChunkConfig,
) -> ColumnStats {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CollectionStore::open(&dir.path().join("docbot.db")).unwrap());
    let col = DocCollection::new("eval");
    store.create_collection(&col).unwrap();
    let mut by_doc_id = std::collections::HashMap::new();
    let mut pages: std::collections::HashMap<String, Vec<(u32, String)>> = Default::default();
    for (doc, _) in DOCS {
        let text = read_fixture(doc);
        let mime = if doc.ends_with(".md") {
            "text/markdown"
        } else {
            "text/plain"
        };
        let res = ingest_document(
            store.clone(),
            None,
            None,
            &col.id,
            doc,
            Some(doc),
            mime,
            text.as_bytes(),
            chunk,
        )
        .await
        .unwrap();
        assert!(res.chunk_count > 0, "{doc} produced no chunks");
        by_doc_id.insert(res.doc_id, doc.to_string());
        let n = pi.get_document(WS, doc).unwrap().page_count;
        let content = pi.get_pages(WS, doc, &format!("1-{n}"), 0).unwrap();
        pages.insert(
            doc.to_string(),
            content
                .pages
                .into_iter()
                .map(|p| (p.page_no, p.text))
                .collect(),
        );
    }
    let mut stats = ColumnStats::new(name);
    for q in qs {
        let hits = store.fts_search(&q.query, Some(&col.id), 3).unwrap();
        let ranked: Vec<Span> = hits
            .iter()
            .filter_map(|h| {
                let doc = by_doc_id.get(&h.doc_id)?;
                chunk_span(&pages[doc], doc, &h.content)
            })
            .collect();
        stats.add(&ranked, &q.gold_span());
    }
    stats
}

/// Hybrid column: the gating pipeline with only the PAGEINDEX.13 tree-node
/// arm contributing (memory layers are empty on the fixtures), so it measures
/// node-level BM25 fused through the production RRF path.
async fn hybrid_column(qs: &[GoldQuery]) -> ColumnStats {
    let settings = PageIndexSettings {
        arm_enabled: true,
        ..Default::default()
    };
    let state = Arc::new(PageIndexState::with_store(
        settings,
        SqliteStore::open_in_memory().unwrap(),
    ));
    let pi = state.index().unwrap();
    ingest_fixtures(&pi);
    let mut ranges: std::collections::HashMap<(String, String), (u32, u32)> = Default::default();
    for (doc, _) in DOCS {
        let view = pi.get_structure(WS, doc, StructureOpts::default()).unwrap();
        fn walk(
            doc: &str,
            n: &StructureNode,
            out: &mut std::collections::HashMap<(String, String), (u32, u32)>,
        ) {
            out.insert(
                (doc.to_string(), n.node_id.clone()),
                (n.start_page, n.end_page),
            );
            n.children.iter().for_each(|c| walk(doc, c, out));
        }
        view.nodes.iter().for_each(|n| walk(doc, n, &mut ranges));
    }
    let arm = Arc::new(PageIndexArm::new(state, WS));
    let gating = AdaptiveGating::new(GatingConfig {
        relevance_threshold: 0.0,
        grounding_enabled: false,
        ..Default::default()
    })
    .with_pageindex_arm(Some(arm));
    let mut col = ColumnStats::new("hybrid (gating + tree arm)");
    for q in qs {
        let ranked: Vec<Span> = gating
            .retrieve(&[], &[], &[], &q.query, None)
            .await
            .iter()
            .take(3)
            .filter_map(|r| {
                let (doc, node) = r.path.strip_prefix("pageindex/")?.split_once('#')?;
                let (lo, hi) = ranges.get(&(doc.to_string(), node.to_string()))?;
                Some(Span::new(doc, *lo, *hi))
            })
            .collect();
        col.add(&ranked, &q.gold_span());
    }
    col
}

#[test]
fn test_eval_fixture_set_has_gold_ranges() {
    let pi = build_index();
    let qs = gold();
    assert!(qs.len() >= 30, "need >= 30 queries, got {}", qs.len());
    let mut per_doc = std::collections::HashMap::new();
    for q in &qs {
        *per_doc.entry(q.doc.clone()).or_insert(0) += 1;
        let info = pi.get_document(WS, &q.doc).expect("gold doc is a fixture");
        assert!(
            q.end_page <= info.page_count,
            "{}: range past page_count",
            q.id
        );
        let ev = q.evidence.as_deref().expect("every gold row has evidence");
        let text = pi
            .get_pages(WS, &q.doc, &format!("{}-{}", q.start_page, q.end_page), 0)
            .unwrap()
            .pages
            .iter()
            .map(|p| p.text.replace('\n', " ").to_lowercase())
            .collect::<String>();
        assert!(
            text.contains(&ev.to_lowercase()),
            "{}: evidence {ev:?} not inside pages {}-{}",
            q.id,
            q.start_page,
            q.end_page
        );
    }
    assert_eq!(per_doc.len(), DOCS.len());
    assert!(per_doc.values().all(|n| *n >= 10));
}

#[tokio::test]
async fn test_eval_report_compares_docbot_tree_hybrid() {
    let pi = build_index();
    let qs = gold();
    let plain = forest(&pi, false);
    let lead = forest(&pi, true);
    let small = ChunkConfig {
        target_tokens: 64,
        overlap_tokens: 8,
        min_chars: 40,
    };
    let mut cols = vec![
        docbot_column(
            "docbot-bm25 (512 tok chunks)",
            &pi,
            &qs,
            &ChunkConfig::default(),
        )
        .await,
        docbot_column("docbot-bm25 (64 tok chunks)", &pi, &qs, &small).await,
        tree_column("tree-lexical (titles)", &plain, &qs),
        tree_column("tree-lexical (titles+lead)", &lead, &qs),
    ];
    cols.push(hybrid_column(&qs).await);
    println!("queries: {}\n{}", qs.len(), format_report(&cols));
    for c in &cols {
        for v in [c.hit1.value(), c.hit3.value(), c.mrr.value()] {
            assert!((0.0..=1.0).contains(&v), "{}: metric out of range", c.name);
        }
        assert!(c.hit1.value() <= c.hit3.value());
    }
    // Determinism: a second tree run yields identical numbers.
    let again = tree_column("tree-lexical (titles+lead)", &lead, &qs);
    assert_eq!(again.hit3.value(), cols[3].hit3.value());
    assert_eq!(again.mrr.value(), cols[3].mrr.value());
    // Regression floors, set below the measured values (spec sec. 10).
    assert!(
        cols[0].hit3.value() >= TREE_FLOOR_DOCBOT,
        "docbot regressed"
    );
    assert!(
        cols[3].hit3.value() >= TREE_FLOOR_TREE,
        "tree navigator regressed"
    );
    assert!(cols[4].hit3.value() >= HYBRID_FLOOR, "hybrid arm regressed");
}

// Measured 0.917 and 0.222 on the 36 fixture queries (spec sec. 10).
const TREE_FLOOR_DOCBOT: f64 = 0.8;
const TREE_FLOOR_TREE: f64 = 0.15;
// Measured 0.778 for the hybrid arm (spec sec. 10).
const HYBRID_FLOOR: f64 = 0.7;

#[tokio::test]
async fn test_eval_hybrid_column_after_issue_13() {
    let col = hybrid_column(&gold()).await;
    println!("{}", format_report(std::slice::from_ref(&col)));
    assert!(col.hit3.value() > 0.0, "hybrid arm found nothing");
    assert!(col.hit1.value() <= col.hit3.value());
}

/// Real-LLM navigator. Never runs in CI:
/// `DOCBOT_LLM_BACKEND=... cargo test -p xavier --features ci-safe --test pageindex_eval -- --ignored --nocapture llm`
/// Optional `PAGEINDEX_OSS_BENCHMARK_DIR` adds `<dir>/questions.jsonl` rows whose
/// `doc` is a `.md`/`.txt` file under `<dir>/documents/` (PDFs need `pageindex-pdf`).
#[tokio::test]
#[ignore = "needs a live LLM backend (DOCBOT_LLM_*); never in CI"]
async fn test_eval_llm_navigator_optional() {
    use xavier::rag::llm_adapter::{LlmAdapter, LlmAdapterTrait};
    let pi = build_index();
    let mut qs = gold();
    if let Ok(dir) = std::env::var("PAGEINDEX_OSS_BENCHMARK_DIR") {
        let dir = PathBuf::from(dir);
        if let Some(extra) = xavier_pageindex::eval::load_benchmark_jsonl(&dir).unwrap() {
            for q in extra {
                let path = dir.join("documents").join(&q.doc);
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let src = if q.doc.ends_with(".md") {
                    Source::Markdown(&text)
                } else {
                    Source::PlainText(&text)
                };
                let opts = IngestOptions {
                    lines_per_page: LINES_PER_PAGE,
                    ..Default::default()
                };
                if pi.ingest(WS, &q.doc, src, opts).is_ok() {
                    qs.push(q);
                }
            }
        }
    }
    let llm = LlmAdapter::from_env();
    let mut col = ColumnStats::new(format!("tree-llm ({})", llm.model_name()));
    for q in &qs {
        let view = pi
            .get_structure(WS, &q.doc, StructureOpts::default())
            .unwrap();
        let mut outline = String::new();
        fn walk(n: &StructureNode, d: usize, out: &mut String) {
            out.push_str(&format!(
                "{}[{}] {} (pages {}-{})\n",
                "  ".repeat(d),
                n.node_id,
                n.title,
                n.start_page,
                n.end_page
            ));
            n.children.iter().for_each(|c| walk(c, d + 1, out));
        }
        view.nodes.iter().for_each(|n| walk(n, 0, &mut outline));
        let prompt = format!(
            "Document outline:\n{outline}\nQuestion: {}\nReply with only the id of the most specific section that answers it, like 0007.",
            q.query
        );
        let reply = llm.complete(&prompt).await.unwrap_or_default();
        let id = reply
            .split(|c: char| !c.is_ascii_digit())
            .find(|t| t.len() == 4)
            .unwrap_or("");
        let mut ranked = Vec::new();
        fn find<'a>(ns: &'a [StructureNode], id: &str) -> Option<&'a StructureNode> {
            ns.iter().find_map(|n| {
                if n.node_id == id {
                    Some(n)
                } else {
                    find(&n.children, id)
                }
            })
        }
        if let Some(n) = find(&view.nodes, id) {
            ranked.push(Span::new(q.doc.clone(), n.start_page, n.end_page));
        }
        col.add(&ranked, &q.gold_span());
    }
    println!("queries: {}\n{}", qs.len(), format_report(&[col]));
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
