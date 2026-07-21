// SPDX-License-Identifier: MIT OR LICENSE-MESH
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
