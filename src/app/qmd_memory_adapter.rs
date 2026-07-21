//! QmdMemory adapter that implements MemoryQueryPort.
//! Wraps QmdMemory (the domain) behind the inbound port interface.
/// NOTE: HexArch improvement — depends on concrete crate::memory::qmd_memory, should use a port abstraction
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::schema::MemoryQueryFilters;
use crate::memory::store::MemoryRecord;
use crate::ports::inbound::MemoryQueryPort;
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Clone)]
pub struct QmdMemoryAdapter {
    inner: Arc<QmdMemory>,
}

impl QmdMemoryAdapter {
    /// New.
    pub fn new(inner: Arc<QmdMemory>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl MemoryQueryPort for QmdMemoryAdapter {
    async fn search(
        &self,
        query: &str,
        limit: usize,
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        let results = self
            .inner
            .search_filtered(query, limit, filters.as_ref())
            .await?;

        let workspace_id = self.inner.workspace_id();
        Ok(results
            .into_iter()
            .map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None))
            .collect())
    }

    async fn add(&self, record: MemoryRecord) -> anyhow::Result<String> {
        let doc = record.to_document();
        self.inner
            .add_document(doc.path, doc.content, doc.metadata)
            .await
    }

    async fn update(&self, id: &str, record: MemoryRecord) -> anyhow::Result<MemoryRecord> {
        let mut doc = record.to_document();
        doc.id = Some(id.to_string());
        self.inner.update(doc).await?;
        let workspace_id = self.inner.workspace_id();
        let result = self.inner.get(id).await?;
        result
            .map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None))
            .ok_or_else(|| anyhow::anyhow!("Record not found after update"))
    }

    async fn delete(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>> {
        let workspace_id = self.inner.workspace_id();
        let result = self.inner.delete(id).await?;
        Ok(result.map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None)))
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>> {
        let workspace_id = self.inner.workspace_id();
        let result = self.inner.get(id).await?;
        Ok(result.map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None)))
    }

    async fn list(&self, workspace_id: &str, limit: usize) -> anyhow::Result<Vec<MemoryRecord>> {
        let limit = limit.clamp(1, 100);
        let results = self.inner.all_documents().await;

        Ok(results
            .into_iter()
            .take(limit)
            .map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None))
            .collect())
    }

    async fn export(&self, public_only: bool) -> anyhow::Result<Vec<MemoryRecord>> {
        let workspace_id = self.inner.workspace_id();
        let results = self.inner.export(public_only).await?;

        Ok(results
            .into_iter()
            .map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None))
            .collect())
    }

    async fn ls(&self, path: &str) -> anyhow::Result<Vec<crate::memory::qmd::types::NavEntry>> {
        self.inner.ls(path).await
    }

    async fn expand_depth(
        &self,
        results: &[MemoryRecord],
        depth: usize,
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        let docs: Vec<_> = results.iter().map(|r| r.to_document()).collect();
        let expanded = self
            .inner
            .expand_depth(&docs, depth, filters.as_ref())
            .await?;
        let workspace_id = self.inner.workspace_id();
        Ok(expanded
            .into_iter()
            .map(|doc| MemoryRecord::from_document(workspace_id, &doc, true, None))
            .collect())
    }
}
