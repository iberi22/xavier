//! Segment space management for lab 10% segments
//!
//! Provides the `SegmentSpace` API and path resolution utilities for documents
//! stored under segment namespaces (`segments/<seg-id>/...`).

use anyhow::{anyhow, bail, Result};
use std::path::{Component, Path};

use crate::ports::inbound::MemoryQueryPort;
use crate::security::groups::LAB_SEGMENTS;

/// Segment space manager wrapping a `MemoryQueryPort` implementation.
pub struct SegmentSpace<'a> {
    memory: &'a dyn MemoryQueryPort,
}

impl<'a> SegmentSpace<'a> {
    /// Create a new `SegmentSpace` instance with the given memory port adapter.
    pub fn new(memory: &'a dyn MemoryQueryPort) -> Self {
        Self { memory }
    }

    /// List documents under the given segment namespace up to `limit`.
    ///
    /// Returns an error if `segment_id` is not one of `LAB_SEGMENTS`.
    pub async fn list(&self, segment_id: &str, limit: usize) -> Result<Vec<serde_json::Value>> {
        if !LAB_SEGMENTS.iter().any(|(id, _, _)| *id == segment_id) {
            bail!("Unknown segment ID: {}", segment_id);
        }

        let all_records = self.memory.export(false).await?;
        let effective_limit = if limit == 0 { usize::MAX } else { limit };

        let filtered: Vec<serde_json::Value> = all_records
            .into_iter()
            .filter(|rec| segment_for_path(&rec.path) == Some(segment_id))
            .take(effective_limit)
            .map(|rec| serde_json::to_value(rec).unwrap_or(serde_json::Value::Null))
            .collect();

        Ok(filtered)
    }
}

/// Returns the segment ID for a given path if it falls under a valid segment namespace.
///
/// Example: `segments/seg-research/doc1.md` -> `Some("seg-research")`.
pub fn segment_for_path(path: &str) -> Option<&'static str> {
    let rest = path
        .strip_prefix("segments/")
        .or_else(|| path.strip_prefix("/segments/"))?;
    let id = rest.split('/').next()?;
    LAB_SEGMENTS
        .iter()
        .find(|(segment_id, _, _)| *segment_id == id)
        .map(|(segment_id, _, _)| *segment_id)
}

/// Constructs a relative document path for a segment document, validating
/// segment existence and preventing path traversal attacks.
///
/// Rejects `..`, slashes inside slug, absolute paths, empty slugs, and unknown `segment_id`.
pub fn document_path(segment_id: &str, slug: &str) -> Result<String> {
    if !LAB_SEGMENTS.iter().any(|(id, _, _)| *id == segment_id) {
        bail!("Unknown segment ID: {}", segment_id);
    }

    let trimmed = slug.trim();
    if trimmed.is_empty() {
        bail!("Slug cannot be empty");
    }

    if trimmed.contains('/') || trimmed.contains('\\') {
        bail!("Slug cannot contain directory separators");
    }

    if trimmed.contains("..") {
        bail!("Path traversal attempt detected in slug");
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        bail!("Slug cannot be an absolute path");
    }

    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!("Invalid path component in slug"),
        }
    }

    Ok(format!("segments/{}/{}", segment_id, trimmed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::MemoryRecord;
    use crate::ports::inbound::memory_port::MockMemoryQueryPort;
    use async_trait::async_trait;

    #[test]
    fn test_segment_for_path_valid() {
        assert_eq!(
            segment_for_path("segments/seg-research/doc1.md"),
            Some("seg-research")
        );
        assert_eq!(
            segment_for_path("/segments/seg-admin/plan.md"),
            Some("seg-admin")
        );
        assert_eq!(
            segment_for_path("segments/seg-ops/infra/notes.txt"),
            Some("seg-ops")
        );
        assert_eq!(segment_for_path("segments/seg-legal"), Some("seg-legal"));
    }

    #[test]
    fn test_segment_for_path_invalid() {
        assert_eq!(segment_for_path("docs/seg-research/doc1.md"), None);
        assert_eq!(segment_for_path("segments/seg-unknown/doc1.md"), None);
        assert_eq!(segment_for_path("random/path"), None);
        assert_eq!(segment_for_path("segments/"), None);
    }

    #[test]
    fn test_document_path_valid() {
        let path = document_path("seg-research", "paper.md").unwrap();
        assert_eq!(path, "segments/seg-research/paper.md");

        let path_ops = document_path("seg-ops", "runbook.txt").unwrap();
        assert_eq!(path_ops, "segments/seg-ops/runbook.txt");
    }

    #[test]
    fn test_document_path_unknown_segment() {
        let res = document_path("seg-invalid", "paper.md");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Unknown segment ID"));
    }

    #[test]
    fn test_document_path_traversal_and_slashes() {
        assert!(document_path("seg-research", "../etc/passwd").is_err());
        assert!(document_path("seg-research", "foo/bar").is_err());
        assert!(document_path("seg-research", "foo\\bar").is_err());
        assert!(document_path("seg-research", "/etc/passwd").is_err());
        assert!(document_path("seg-research", "").is_err());
        assert!(document_path("seg-research", "   ").is_err());
        assert!(document_path("seg-research", "..").is_err());
        assert!(document_path("seg-research", "sub/..").is_err());
    }

    struct CustomMockMemory {
        records: Vec<MemoryRecord>,
    }

    #[async_trait]
    impl MemoryQueryPort for CustomMockMemory {
        async fn search(
            &self,
            _query: &str,
            _limit: usize,
            _filters: Option<crate::domain::memory::MemoryQueryFilters>,
        ) -> Result<Vec<MemoryRecord>> {
            Ok(self.records.clone())
        }

        async fn expand_depth(
            &self,
            _results: &[MemoryRecord],
            _depth: usize,
            _filters: Option<crate::domain::memory::MemoryQueryFilters>,
        ) -> Result<Vec<MemoryRecord>> {
            Ok(Vec::new())
        }

        async fn add(&self, record: MemoryRecord) -> Result<String> {
            Ok(record.id)
        }

        async fn update(&self, _id: &str, record: MemoryRecord) -> Result<MemoryRecord> {
            Ok(record)
        }

        async fn delete(&self, _id: &str) -> Result<Option<MemoryRecord>> {
            Ok(None)
        }

        async fn get(&self, _id: &str) -> Result<Option<MemoryRecord>> {
            Ok(None)
        }

        async fn list(&self, _workspace_id: &str, _limit: usize) -> Result<Vec<MemoryRecord>> {
            Ok(self.records.clone())
        }

        async fn export(&self, _public_only: bool) -> Result<Vec<MemoryRecord>> {
            Ok(self.records.clone())
        }

        async fn ls(&self, _path: &str) -> Result<Vec<crate::memory::qmd::types::NavEntry>> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn test_segment_space_list_filters_by_segment() {
        let records = vec![
            MemoryRecord {
                id: "1".into(),
                path: "segments/seg-research/doc1.md".into(),
                content: "Research doc 1".into(),
                ..Default::default()
            },
            MemoryRecord {
                id: "2".into(),
                path: "segments/seg-research/doc2.md".into(),
                content: "Research doc 2".into(),
                ..Default::default()
            },
            MemoryRecord {
                id: "3".into(),
                path: "segments/seg-ops/doc3.md".into(),
                content: "Ops doc".into(),
                ..Default::default()
            },
            MemoryRecord {
                id: "4".into(),
                path: "docs/general.md".into(),
                content: "General doc".into(),
                ..Default::default()
            },
        ];

        let mock = CustomMockMemory { records };
        let space = SegmentSpace::new(&mock);

        let research_docs = space.list("seg-research", 10).await.unwrap_or_default();
        assert_eq!(research_docs.len(), 2);
        assert_eq!(research_docs[0]["path"], "segments/seg-research/doc1.md");
        assert_eq!(research_docs[1]["path"], "segments/seg-research/doc2.md");

        let ops_docs = space.list("seg-ops", 10).await.unwrap_or_default();
        assert_eq!(ops_docs.len(), 1);
        assert_eq!(ops_docs[0]["path"], "segments/seg-ops/doc3.md");

        let unknown_res = space.list("seg-invalid", 10).await;
        assert!(unknown_res.is_err());
    }

    #[tokio::test]
    async fn test_segment_space_list_respects_limit() {
        let records = vec![
            MemoryRecord {
                id: "1".into(),
                path: "segments/seg-research/doc1.md".into(),
                ..Default::default()
            },
            MemoryRecord {
                id: "2".into(),
                path: "segments/seg-research/doc2.md".into(),
                ..Default::default()
            },
        ];

        let mock = CustomMockMemory { records };
        let space = SegmentSpace::new(&mock);

        let limited = space.list("seg-research", 1).await.unwrap();
        assert_eq!(limited.len(), 1);
    }
}
