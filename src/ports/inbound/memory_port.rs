//! Inbound port for memory operations
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
pub use crate::domain::cycle_breaks::w30_09::MemoryQueryPort;

#[cfg(any(test, feature = "test-utils"))]
use crate::domain::memory::{MemoryQueryFilters, MemoryRecord};
#[cfg(any(test, feature = "test-utils"))]
use async_trait::async_trait;

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
