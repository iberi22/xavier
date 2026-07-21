// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Outbound port for threat detection
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use async_trait::async_trait;

#[async_trait]
pub trait ThreatDetectionPort: Send + Sync {
    /// Scans the given text for security threats and logs them to the audit chain.
    /// Returns true if the content is clean, false if a threat was detected.
    async fn scan_and_log(&self, text: &str, component: &str) -> anyhow::Result<bool>;

    /// Checks if an action requires Human-in-the-Loop approval
    async fn requires_hitl(&self, action: &str, target: &str) -> anyhow::Result<bool> {
        // Default implementation: assume no HITL required
        let _ = action;
        let _ = target;
        Ok(false)
    }
}
