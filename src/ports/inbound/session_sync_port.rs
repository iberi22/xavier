// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Inbound port for session synchronization
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::tasks::session_sync_task::SyncCheckResult;
use async_trait::async_trait;

#[async_trait]
pub trait SessionSyncPort: Send + Sync {
    async fn check(&self) -> anyhow::Result<SyncCheckResult>;
    async fn last_result(&self) -> SyncCheckResult;
}
