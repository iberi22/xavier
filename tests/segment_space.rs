//! Integration tests for segment space management (WAVE-22.02)

use async_trait::async_trait;
use xavier::domain::memory::{MemoryQueryFilters, MemoryRecord};
use xavier::memory::qmd::types::NavEntry;
use xavier::memory::segments::{document_path, segment_for_path, SegmentSpace};
use xavier::ports::inbound::MemoryQueryPort;

struct TestMemoryPort {
    records: Vec<MemoryRecord>,
}

#[async_trait]
impl MemoryQueryPort for TestMemoryPort {
    async fn search(
        &self,
        _query: &str,
        _limit: usize,
        _filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        Ok(self.records.clone())
    }

    async fn expand_depth(
        &self,
        _results: &[MemoryRecord],
        _depth: usize,
        _filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        Ok(Vec::new())
    }

    async fn add(&self, record: MemoryRecord) -> anyhow::Result<String> {
        Ok(record.id)
    }

    async fn update(&self, _id: &str, record: MemoryRecord) -> anyhow::Result<MemoryRecord> {
        Ok(record)
    }

    async fn delete(&self, _id: &str) -> anyhow::Result<Option<MemoryRecord>> {
        Ok(None)
    }

    async fn get(&self, _id: &str) -> anyhow::Result<Option<MemoryRecord>> {
        Ok(None)
    }

    async fn list(&self, _workspace_id: &str, _limit: usize) -> anyhow::Result<Vec<MemoryRecord>> {
        Ok(self.records.clone())
    }

    async fn export(&self, _public_only: bool) -> anyhow::Result<Vec<MemoryRecord>> {
        Ok(self.records.clone())
    }

    async fn ls(&self, _path: &str) -> anyhow::Result<Vec<NavEntry>> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn test_unknown_segment_rejected() {
    let res_doc_path = document_path("seg-nonexistent", "my-doc.md");
    assert!(res_doc_path.is_err());
    let err_msg = res_doc_path.unwrap_err().to_string();
    assert!(err_msg.contains("Unknown segment ID"));

    let memory = TestMemoryPort { records: vec![] };
    let space = SegmentSpace::new(&memory);
    let res_list = space.list("seg-nonexistent", 10).await;
    assert!(res_list.is_err());
}

#[tokio::test]
async fn test_path_traversal_rejected() {
    assert!(document_path("seg-research", "../secret.txt").is_err());
    assert!(document_path("seg-research", "dir/file.txt").is_err());
    assert!(document_path("seg-research", "dir\\file.txt").is_err());
    assert!(document_path("seg-research", "/etc/passwd").is_err());
    assert!(document_path("seg-research", "").is_err());
    assert!(document_path("seg-research", "  ").is_err());
    assert!(document_path("seg-research", "..").is_err());
}

#[tokio::test]
async fn test_segment_for_path_mapping() {
    assert_eq!(
        segment_for_path("segments/seg-research/paper.md"),
        Some("seg-research")
    );
    assert_eq!(
        segment_for_path("/segments/seg-admin/budget.md"),
        Some("seg-admin")
    );
    assert_eq!(
        segment_for_path("segments/seg-ops/infra.yaml"),
        Some("seg-ops")
    );
    assert_eq!(
        segment_for_path("segments/seg-legal/contract.pdf"),
        Some("seg-legal")
    );
    assert_eq!(segment_for_path("docs/seg-research/paper.md"), None);
    assert_eq!(segment_for_path("segments/unknown-seg/doc.md"), None);
    assert_eq!(segment_for_path("random/path/to/file.md"), None);
}

#[tokio::test]
async fn test_list_returns_only_documents_of_namespace() {
    let records = vec![
        MemoryRecord {
            id: "rec-1".into(),
            path: "segments/seg-research/paper1.md".into(),
            content: "Research Content 1".into(),
            ..Default::default()
        },
        MemoryRecord {
            id: "rec-2".into(),
            path: "segments/seg-research/paper2.md".into(),
            content: "Research Content 2".into(),
            ..Default::default()
        },
        MemoryRecord {
            id: "rec-3".into(),
            path: "segments/seg-ops/runbook.md".into(),
            content: "Ops Content".into(),
            ..Default::default()
        },
        MemoryRecord {
            id: "rec-4".into(),
            path: "unclassified/general.md".into(),
            content: "General Content".into(),
            ..Default::default()
        },
    ];

    let memory = TestMemoryPort { records };
    let space = SegmentSpace::new(&memory);

    let research_docs = space.list("seg-research", 10).await.unwrap();
    assert_eq!(research_docs.len(), 2);
    assert_eq!(research_docs[0]["path"], "segments/seg-research/paper1.md");
    assert_eq!(research_docs[1]["path"], "segments/seg-research/paper2.md");

    let ops_docs = space.list("seg-ops", 10).await.unwrap();
    assert_eq!(ops_docs.len(), 1);
    assert_eq!(ops_docs[0]["path"], "segments/seg-ops/runbook.md");

    let admin_docs = space.list("seg-admin", 10).await.unwrap();
    assert_eq!(admin_docs.len(), 0);
}
