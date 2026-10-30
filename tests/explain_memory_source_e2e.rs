//! feat-explain-memory-source: explain memory source e2e tests
use anyhow::{bail, Result};
use futures_util::future::BoxFuture;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use xavier::explain::{
    EvidenceKind, EvidenceSource, ExplainTarget, MemoryHit, MemorySearch, MemorySource,
};
use xavier::memory::schema::{MemoryKind, MemoryQueryFilters};

xavier::isolate_test_process!();

struct Call {
    query: String,
    filters: MemoryQueryFilters,
    limit: usize,
}

#[derive(Default)]
struct FakeSearch {
    decisions: Vec<MemoryHit>,
    memories: Vec<MemoryHit>,
    fail: bool,
    calls: Mutex<Vec<Call>>,
}

impl MemorySearch for FakeSearch {
    fn search<'a>(
        &'a self,
        query: &'a str,
        filters: MemoryQueryFilters,
        limit: usize,
    ) -> BoxFuture<'a, Result<Vec<MemoryHit>>> {
        let is_decision_query = filters.kinds.as_deref() == Some(&[MemoryKind::Decision][..]);
        self.calls.lock().unwrap().push(Call {
            query: query.to_string(),
            filters,
            limit,
        });
        Box::pin(async move {
            if self.fail {
                bail!("memory store unavailable");
            }
            let hits = if is_decision_query {
                self.decisions.clone()
            } else {
                self.memories.clone()
            };
            Ok(hits)
        })
    }
}

fn hit(id: &str, text: &str, kind: MemoryKind, score: f32) -> MemoryHit {
    MemoryHit {
        id: id.to_string(),
        text: text.to_string(),
        kind: Some(kind),
        score,
    }
}

fn target() -> ExplainTarget {
    ExplainTarget {
        repo_root: PathBuf::from("/nonexistent-repo"),
        file: PathBuf::from("src/lib.rs"),
        start_line: 2,
        end_line: 3,
        symbol: Some("demo".to_string()),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn e2e_memory_source_decision_query_filters_by_kind_and_file() {
    let rt = runtime();
    let fake = Arc::new(FakeSearch::default());
    let source = MemorySource::new(fake.clone(), rt.handle().clone());

    source.collect(&target(), 5).unwrap();

    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    let decision = &calls[0];
    assert_eq!(decision.filters.kinds, Some(vec![MemoryKind::Decision]));
    assert_eq!(decision.filters.file_path.as_deref(), Some("src/lib.rs"));
    assert_eq!(decision.filters.symbol.as_deref(), Some("demo"));
    assert_eq!(decision.query, "src/lib.rs demo");
    assert_eq!(decision.limit, 5);
    assert_eq!(calls[1].filters.kinds, None);
}

#[test]
fn e2e_memory_source_duplicate_ids_across_queries_appear_once() {
    let rt = runtime();
    let fake = Arc::new(FakeSearch {
        decisions: vec![hit("shared", "decision text", MemoryKind::Decision, 0.5)],
        memories: vec![
            hit("shared", "memory copy", MemoryKind::Decision, 0.9),
            hit("other", "other memory", MemoryKind::Episodic, 0.4),
        ],
        ..Default::default()
    });
    let source = MemorySource::new(fake, rt.handle().clone());

    let evidence = source.collect(&target(), 5).unwrap();

    let shared: Vec<_> = evidence
        .iter()
        .filter(|e| e.source_ref == "memory:shared")
        .collect();
    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0].kind, EvidenceKind::Decision);
    assert_eq!(shared[0].text, "decision text");
    assert_eq!(evidence.len(), 2);
    assert!(evidence.iter().any(|e| e.source_ref == "memory:other"));
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn e2e_memory_source_real_qmd_search() {
    use tokio::sync::RwLock;
    use xavier::explain::QmdMemorySearch;
    use xavier::memory::qmd::types::MemoryDocument;
    use xavier::memory::qmd_memory::QmdMemory;

    let rt = runtime();

    let docs = Arc::new(RwLock::new(vec![
        MemoryDocument {
            id: Some("doc-match".to_string()),
            path: "src/lib.rs".to_string(),
            content: "decision text match".to_string(),
            metadata: serde_json::json!({
                "kind": "decision",
                "symbol": "demo"
            }),
            embedding: vec![],
            ..Default::default()
        },
        MemoryDocument {
            id: Some("doc-exclude".to_string()),
            path: "src/other.rs".to_string(),
            content: "decision text exclude".to_string(),
            metadata: serde_json::json!({
                "kind": "decision",
                "symbol": "other"
            }),
            embedding: vec![],
            ..Default::default()
        },
    ]));

    let qmd = Arc::new(QmdMemory::new_with_workspace(docs, "explain-test"));
    let search = Arc::new(QmdMemorySearch::new(qmd));

    let source = MemorySource::new(search, rt.handle().clone());

    let evidence = tokio::task::spawn_blocking(move || source.collect(&target(), 5))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].source_ref, "memory:doc-match");
    assert_eq!(evidence[0].kind, EvidenceKind::Decision);
}
