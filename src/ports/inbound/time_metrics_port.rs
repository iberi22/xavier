// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Inbound port for metrics collection
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::domain::memory::TimeMetric;
use async_trait::async_trait;

/// Port for time metrics operations (inbound)
#[async_trait]
pub trait TimeMetricsPort: Send + Sync {
    /// Save a time metric record
    async fn save_time_metric(&self, metric: &TimeMetric, workspace_id: &str)
        -> Result<(), String>;
}
