//! Inbound port for memory operations
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::domain::memory::{MemoryQueryFilters, MemoryRecord};
use async_trait::async_trait;

#[async_trait]
pub trait MemoryQueryPort: Send + Sync {
    async fn search(
        &self,
        query: &str,
        limit: usize,
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>>;
    async fn expand_depth(
        &self,
        results: &[MemoryRecord],
        depth: usize,
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>>;
    async fn add(&self, record: MemoryRecord) -> anyhow::Result<String>;
    async fn update(&self, id: &str, record: MemoryRecord) -> anyhow::Result<MemoryRecord>;
    async fn delete(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>>;
    async fn get(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>>;
    async fn list(&self, workspace_id: &str, limit: usize) -> anyhow::Result<Vec<MemoryRecord>>;
    async fn export(&self, public_only: bool) -> anyhow::Result<Vec<MemoryRecord>>;
    async fn ls(&self, path: &str) -> anyhow::Result<Vec<crate::memory::qmd::types::NavEntry>>;
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Default, Clone)]
pub struct MockMemoryQueryPort;

#[cfg(any(test, feature = "test-utils"))]
#[async_trait]
impl MemoryQueryPort for MockMemoryQueryPort {
    async fn search(
        &self,
        _query: &str,
        _limit: usize,
        _filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        Ok(Vec::new())
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
        Ok(Vec::new())
    }

    async fn export(&self, _public_only: bool) -> anyhow::Result<Vec<MemoryRecord>> {
        Ok(Vec::new())
    }

    async fn ls(&self, _path: &str) -> anyhow::Result<Vec<crate::memory::qmd::types::NavEntry>> {
        Ok(Vec::new())
    }
}
