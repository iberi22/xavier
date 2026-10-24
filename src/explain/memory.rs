//! Memory and decision evidence for the fragment being explained.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::future::BoxFuture;
use tokio::runtime::Handle;

use crate::memory::qmd::{query_with_embedding_filtered, QmdMemory};
use crate::memory::schema::{resolve_metadata, MemoryKind, MemoryQueryFilters};

use super::context::{Evidence, EvidenceKind, EvidenceSource, ExplainTarget};

/// Multiplier for decision hits, so they rank above plain memories at equal score.
const DECISION_WEIGHT: f32 = 1.0;
/// Multiplier for plain memory hits.
const MEMORY_WEIGHT: f32 = 0.9;

/// One memory returned by a search.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryHit {
    pub id: String,
    pub text: String,
    pub kind: Option<MemoryKind>,
    /// Raw relevance score as reported by the search backend.
    pub score: f32,
}

/// Async search port over Xavier memory.
pub trait MemorySearch: Send + Sync {
    fn search<'a>(
        &'a self,
        query: &'a str,
        filters: MemoryQueryFilters,
        limit: usize,
    ) -> BoxFuture<'a, Result<Vec<MemoryHit>>>;
}

/// Adapts the embedding search of a `QmdMemory` to [`MemorySearch`].
pub struct QmdMemorySearch {
    memory: Arc<QmdMemory>,
}

impl QmdMemorySearch {
    pub fn new(memory: Arc<QmdMemory>) -> Self {
        Self { memory }
    }
}

impl MemorySearch for QmdMemorySearch {
    fn search<'a>(
        &'a self,
        query: &'a str,
        filters: MemoryQueryFilters,
        limit: usize,
    ) -> BoxFuture<'a, Result<Vec<MemoryHit>>> {
        Box::pin(async move {
            let result =
                query_with_embedding_filtered(&self.memory, query, limit, Some(&filters)).await?;
            let workspace = self.memory.workspace_id();
            let hits: Vec<MemoryHit> = result
                .documents
                .into_iter()
                .map(|doc| {
                    let id = doc.id.clone().unwrap_or_else(|| doc.path.clone());
                    let kind = resolve_metadata(&doc.path, &doc.metadata, workspace, None)
                        .ok()
                        .map(|meta| meta.kind);
                    MemoryHit {
                        id,
                        text: doc.content,
                        kind,
                        score: doc.score,
                    }
                })
                .collect();
            Ok(hits)
        })
    }
}

/// Evidence source adding the memories that mention the target.
///
/// The trait is sync, so `collect` blocks on `handle`. Call it from
/// `tokio::task::spawn_blocking`, never from a runtime worker, or the
/// `block_on` panics.
pub struct MemorySource {
    search: Arc<dyn MemorySearch>,
    handle: Handle,
}

impl MemorySource {
    pub fn new(search: Arc<dyn MemorySearch>, handle: Handle) -> Self {
        Self { search, handle }
    }

    async fn gather(&self, target: &ExplainTarget, limit: usize) -> Result<Vec<Evidence>> {
        let file = target.file.to_string_lossy().into_owned();
        let query = match &target.symbol {
            Some(symbol) => format!("{file} {symbol}"),
            None => file.clone(),
        };

        let decision_filters = MemoryQueryFilters {
            kinds: Some(vec![MemoryKind::Decision]),
            file_path: Some(file),
            symbol: target.symbol.clone(),
            ..Default::default()
        };
        let decisions = self
            .search
            .search(&query, decision_filters, limit)
            .await
            .context("searching decisions")?;
        let memories = self
            .search
            .search(&query, MemoryQueryFilters::default(), limit)
            .await
            .context("searching memories")?;

        // A memory returned by both queries keeps the decision entry, which comes first.
        let mut seen = HashSet::new();
        let mut evidence = Vec::new();
        let groups = [
            (decisions, EvidenceKind::Decision, DECISION_WEIGHT),
            (memories, EvidenceKind::Memory, MEMORY_WEIGHT),
        ];
        for (hits, kind, weight) in groups {
            for hit in hits {
                if !seen.insert(hit.id.clone()) {
                    continue;
                }
                evidence.push(Evidence {
                    kind,
                    source_ref: format!("memory:{}", hit.id),
                    text: hit.text,
                    score: scale(hit.score) * weight,
                });
            }
        }
        evidence.sort_by(|a, b| b.score.total_cmp(&a.score));
        evidence.truncate(limit);
        Ok(evidence)
    }
}

/// Maps a raw score into [0, 1), so it stays below the code fragment and
/// comparable with the other sources.
fn scale(raw: f32) -> f32 {
    let s = raw.max(0.0);
    if s.is_finite() {
        s / (1.0 + s)
    } else {
        1.0
    }
}

impl EvidenceSource for MemorySource {
    fn name(&self) -> &str {
        "memory"
    }

    fn collect(&self, target: &ExplainTarget, limit: usize) -> Result<Vec<Evidence>> {
        self.handle.block_on(self.gather(target, limit))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use anyhow::bail;
    use tokio::sync::RwLock;

    use super::*;

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
    fn decision_query_filters_by_kind_and_file() {
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
    fn decisions_outrank_memories_at_equal_score() {
        let rt = runtime();
        let fake = Arc::new(FakeSearch {
            decisions: vec![hit("d1", "use sqlite", MemoryKind::Decision, 0.5)],
            memories: vec![hit("m1", "chat about lib", MemoryKind::Episodic, 0.5)],
            ..Default::default()
        });
        let source = MemorySource::new(fake, rt.handle().clone());

        let evidence = source.collect(&target(), 5).unwrap();

        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0].kind, EvidenceKind::Decision);
        assert_eq!(evidence[0].source_ref, "memory:d1");
        assert_eq!(evidence[1].kind, EvidenceKind::Memory);
        assert!(evidence[0].score > evidence[1].score);
    }

    #[test]
    fn duplicate_ids_across_queries_appear_once() {
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

    #[test]
    fn search_error_surfaces_as_err() {
        let rt = runtime();
        let fake = Arc::new(FakeSearch {
            fail: true,
            ..Default::default()
        });
        let source = MemorySource::new(fake, rt.handle().clone());

        let err = source.collect(&target(), 5).unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("searching decisions"));
        assert!(message.contains("memory store unavailable"));
    }

    #[test]
    fn empty_result_is_ok_and_empty() {
        let rt = runtime();
        let source = MemorySource::new(Arc::new(FakeSearch::default()), rt.handle().clone());

        let evidence = source.collect(&target(), 5).unwrap();

        assert!(evidence.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn collect_works_inside_spawn_blocking() {
        let fake = Arc::new(FakeSearch {
            decisions: vec![hit("d1", "why", MemoryKind::Decision, 0.5)],
            ..Default::default()
        });
        let source = MemorySource::new(fake, Handle::current());

        let evidence = tokio::task::spawn_blocking(move || source.collect(&target(), 5))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].source_ref, "memory:d1");
    }

    #[tokio::test]
    async fn qmd_search_adapter_returns_no_hits_for_empty_store() {
        // In-memory QmdMemory: construction does no disk I/O, so no tempdir is needed.
        let docs = Arc::new(RwLock::new(Vec::new()));
        let memory = Arc::new(QmdMemory::new_with_workspace(docs, "explain-test"));
        let search = QmdMemorySearch::new(memory);

        let hits = search
            .search("src/lib.rs demo", MemoryQueryFilters::default(), 5)
            .await
            .unwrap();

        assert!(hits.is_empty());
    }
}
