//! P2P Network Data Firewall and Synchronization Filter
//!
//! Enforces Data Node opt-in consent for peer-to-peer data synchronization in SWAL Mesh.
//! When `opt_in` is `false`, Xavier blocks all outbound data replication, chunk pushes,
//! manifest exports, and peer session shares, while permitting local-only database
//! operations and queries.

use crate::mesh::data_consent::DataConsentManager;
use crate::mesh::protocol::MeshManifest;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Decision outcome when evaluating an outbound or local sync request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncFilterDecision {
    /// Outbound synchronization allowed.
    Allowed,
    /// Outbound synchronization blocked because Data Node opt-in consent is false.
    BlockedOptInRequired,
    /// Outbound synchronization blocked because synchronization is explicitly disabled.
    BlockedDisabled,
    /// Outbound synchronization blocked due to payload sanitization failure or rejection.
    BlockedSanitizationFailed(String),
}

impl SyncFilterDecision {
    /// Returns `true` if outbound sync is allowed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, SyncFilterDecision::Allowed)
    }

    /// Returns `true` if outbound sync is blocked.
    pub fn is_blocked(&self) -> bool {
        !self.is_allowed()
    }
}

/// Errors returned by the P2P synchronization firewall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncFilterError {
    /// Outbound replication blocked: Data Node opt-in consent is required.
    OptInRequired,
    /// Outbound replication blocked: synchronization is disabled.
    SyncDisabled(String),
    /// Outbound replication blocked: payload sanitization failed.
    SanitizationFailed(String),
    /// Custom filter policy violation.
    PolicyViolation(String),
}

impl fmt::Display for SyncFilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncFilterError::OptInRequired => write!(
                f,
                "Outbound sync blocked: Data Node opt-in consent is required (opt_in == false)"
            ),
            SyncFilterError::SyncDisabled(reason) => {
                write!(f, "Outbound sync blocked: sync disabled ({})", reason)
            }
            SyncFilterError::SanitizationFailed(err) => {
                write!(f, "Outbound sync blocked: sanitization failed ({})", err)
            }
            SyncFilterError::PolicyViolation(msg) => {
                write!(f, "Outbound sync blocked by policy: {}", msg)
            }
        }
    }
}

impl std::error::Error for SyncFilterError {}

/// Audit statistics for the P2P SyncFilter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncFilterStats {
    /// Current opt-in consent status.
    pub opt_in: bool,
    /// Total outbound sync requests blocked.
    pub blocked_outbound_count: u64,
    /// Total outbound sync requests allowed.
    pub allowed_outbound_count: u64,
    /// Total local-only operations permitted.
    pub local_allowed_count: u64,
}

/// Configuration options for initializing a `SyncFilter`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFilterConfig {
    /// Whether Data Node opt-in consent is granted for outbound P2P sync.
    pub opt_in: bool,
    /// Optional master toggle for outbound sync.
    pub sync_enabled: bool,
    /// Strict mode requiring consent manager rules for metadata filtering.
    pub strict_sanitization: bool,
}

impl Default for SyncFilterConfig {
    fn default() -> Self {
        Self {
            opt_in: false,
            sync_enabled: true,
            strict_sanitization: false,
        }
    }
}

/// P2P Synchronization Firewall
///
/// Controls outbound replication from local Xavier SQLite databases to the broader SWAL Mesh,
/// ensuring strict adherence to Data Node opt-in consent.
#[derive(Debug)]
pub struct SyncFilter {
    opt_in: AtomicBool,
    sync_enabled: AtomicBool,
    strict_sanitization: AtomicBool,
    blocked_outbound_count: AtomicU64,
    allowed_outbound_count: AtomicU64,
    local_allowed_count: AtomicU64,
}

impl SyncFilter {
    /// Create a new `SyncFilter` with the given opt-in status.
    pub fn new(opt_in: bool) -> Self {
        Self {
            opt_in: AtomicBool::new(opt_in),
            sync_enabled: AtomicBool::new(true),
            strict_sanitization: AtomicBool::new(false),
            blocked_outbound_count: AtomicU64::new(0),
            allowed_outbound_count: AtomicU64::new(0),
            local_allowed_count: AtomicU64::new(0),
        }
    }

    /// Create a `SyncFilter` from a `SyncFilterConfig`.
    pub fn from_config(config: SyncFilterConfig) -> Self {
        Self {
            opt_in: AtomicBool::new(config.opt_in),
            sync_enabled: AtomicBool::new(config.sync_enabled),
            strict_sanitization: AtomicBool::new(config.strict_sanitization),
            blocked_outbound_count: AtomicU64::new(0),
            allowed_outbound_count: AtomicU64::new(0),
            local_allowed_count: AtomicU64::new(0),
        }
    }

    /// Get current opt-in consent status.
    pub fn opt_in(&self) -> bool {
        self.opt_in.load(Ordering::Relaxed)
    }

    /// Dynamically update opt-in consent status.
    pub fn set_opt_in(&self, enabled: bool) {
        self.opt_in.store(enabled, Ordering::Relaxed);
    }

    /// Get current sync enabled state.
    pub fn is_sync_enabled(&self) -> bool {
        self.sync_enabled.load(Ordering::Relaxed)
    }

    /// Set sync enabled state.
    pub fn set_sync_enabled(&self, enabled: bool) {
        self.sync_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Get strict sanitization setting.
    pub fn is_strict_sanitization(&self) -> bool {
        self.strict_sanitization.load(Ordering::Relaxed)
    }

    /// Set strict sanitization mode.
    pub fn set_strict_sanitization(&self, enabled: bool) {
        self.strict_sanitization.store(enabled, Ordering::Relaxed);
    }

    /// Always returns `true`: local-only database queries and operations are never blocked.
    pub fn is_local_usage_allowed(&self) -> bool {
        self.local_allowed_count.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Returns `true` if outbound sync is currently permitted (i.e. `opt_in` is true and `sync_enabled` is true).
    pub fn is_outbound_allowed(&self) -> bool {
        self.opt_in.load(Ordering::Relaxed) && self.sync_enabled.load(Ordering::Relaxed)
    }

    /// Evaluates an outbound synchronization request.
    pub fn evaluate_outbound(&self) -> SyncFilterDecision {
        if !self.sync_enabled.load(Ordering::Relaxed) {
            self.blocked_outbound_count.fetch_add(1, Ordering::Relaxed);
            SyncFilterDecision::BlockedDisabled
        } else if !self.opt_in.load(Ordering::Relaxed) {
            self.blocked_outbound_count.fetch_add(1, Ordering::Relaxed);
            SyncFilterDecision::BlockedOptInRequired
        } else {
            self.allowed_outbound_count.fetch_add(1, Ordering::Relaxed);
            SyncFilterDecision::Allowed
        }
    }

    /// Filter outbound payload data for memory replication.
    /// Returns `Err(SyncFilterError::OptInRequired)` if `opt_in` is `false`.
    pub fn filter_outbound_replication<T: serde::Serialize>(
        &self,
        payload: &T,
    ) -> Result<serde_json::Value, SyncFilterError> {
        let decision = self.evaluate_outbound();
        match decision {
            SyncFilterDecision::Allowed => serde_json::to_value(payload)
                .map_err(|e| SyncFilterError::SanitizationFailed(e.to_string())),
            SyncFilterDecision::BlockedOptInRequired => Err(SyncFilterError::OptInRequired),
            SyncFilterDecision::BlockedDisabled => {
                Err(SyncFilterError::SyncDisabled("P2P sync toggle is off".into()))
            }
            SyncFilterDecision::BlockedSanitizationFailed(reason) => {
                Err(SyncFilterError::SanitizationFailed(reason))
            }
        }
    }

    /// Filter outbound chunk sync attempt.
    pub fn filter_outbound_chunk(
        &self,
        chunk_hash: &str,
        data: &[u8],
    ) -> Result<(), SyncFilterError> {
        if chunk_hash.is_empty() || data.is_empty() {
            return Err(SyncFilterError::SanitizationFailed(
                "Empty chunk hash or payload".into(),
            ));
        }
        match self.evaluate_outbound() {
            SyncFilterDecision::Allowed => Ok(()),
            SyncFilterDecision::BlockedOptInRequired => Err(SyncFilterError::OptInRequired),
            SyncFilterDecision::BlockedDisabled => {
                Err(SyncFilterError::SyncDisabled("P2P sync disabled".into()))
            }
            SyncFilterDecision::BlockedSanitizationFailed(r) => {
                Err(SyncFilterError::SanitizationFailed(r))
            }
        }
    }

    /// Filter outbound manifest sync attempt.
    pub fn filter_outbound_manifest(
        &self,
        manifest: &MeshManifest,
    ) -> Result<MeshManifest, SyncFilterError> {
        match self.evaluate_outbound() {
            SyncFilterDecision::Allowed => Ok(manifest.clone()),
            SyncFilterDecision::BlockedOptInRequired => Err(SyncFilterError::OptInRequired),
            SyncFilterDecision::BlockedDisabled => {
                Err(SyncFilterError::SyncDisabled("P2P sync disabled".into()))
            }
            SyncFilterDecision::BlockedSanitizationFailed(r) => {
                Err(SyncFilterError::SanitizationFailed(r))
            }
        }
    }

    /// Filter outbound memory entry.
    pub fn filter_outbound_memory(
        &self,
        memory_id: &str,
        content: &str,
    ) -> Result<String, SyncFilterError> {
        if memory_id.is_empty() {
            return Err(SyncFilterError::SanitizationFailed(
                "Memory ID cannot be empty".into(),
            ));
        }
        match self.evaluate_outbound() {
            SyncFilterDecision::Allowed => Ok(content.to_string()),
            SyncFilterDecision::BlockedOptInRequired => Err(SyncFilterError::OptInRequired),
            SyncFilterDecision::BlockedDisabled => {
                Err(SyncFilterError::SyncDisabled("P2P sync disabled".into()))
            }
            SyncFilterDecision::BlockedSanitizationFailed(r) => {
                Err(SyncFilterError::SanitizationFailed(r))
            }
        }
    }

    /// Evaluate outbound sync combined with `DataConsentManager` payload sanitization.
    pub fn filter_with_consent_manager<T: serde::Serialize>(
        &self,
        manager: &DataConsentManager,
        data_type: &str,
        payload: &T,
    ) -> Result<Option<serde_json::Value>, SyncFilterError> {
        if !self.opt_in() {
            self.blocked_outbound_count.fetch_add(1, Ordering::Relaxed);
            return Err(SyncFilterError::OptInRequired);
        }
        if !self.is_sync_enabled() {
            self.blocked_outbound_count.fetch_add(1, Ordering::Relaxed);
            return Err(SyncFilterError::SyncDisabled("P2P sync disabled".into()));
        }

        match manager.sanitize_payload(data_type, payload) {
            Ok(Some(sanitized)) => {
                self.allowed_outbound_count.fetch_add(1, Ordering::Relaxed);
                Ok(Some(sanitized))
            }
            Ok(None) => {
                self.blocked_outbound_count.fetch_add(1, Ordering::Relaxed);
                Ok(None)
            }
            Err(e) => {
                self.blocked_outbound_count.fetch_add(1, Ordering::Relaxed);
                Err(SyncFilterError::SanitizationFailed(e.to_string()))
            }
        }
    }

    /// Retrieve audit statistics.
    pub fn stats(&self) -> SyncFilterStats {
        SyncFilterStats {
            opt_in: self.opt_in.load(Ordering::Relaxed),
            blocked_outbound_count: self.blocked_outbound_count.load(Ordering::Relaxed),
            allowed_outbound_count: self.allowed_outbound_count.load(Ordering::Relaxed),
            local_allowed_count: self.local_allowed_count.load(Ordering::Relaxed),
        }
    }

    /// Reset audit counters to zero.
    pub fn reset_stats(&self) {
        self.blocked_outbound_count.store(0, Ordering::Relaxed);
        self.allowed_outbound_count.store(0, Ordering::Relaxed);
        self.local_allowed_count.store(0, Ordering::Relaxed);
    }
}

impl Default for SyncFilter {
    fn default() -> Self {
        Self::new(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::node::NodeId;
    use crate::mesh::protocol::ChunkRef;

    #[test]
    fn test_sync_filter_default_blocks_outbound() {
        let filter = SyncFilter::default();
        assert!(!filter.opt_in());
        assert!(filter.is_sync_enabled());
        assert!(!filter.is_outbound_allowed());
        assert!(filter.is_local_usage_allowed());

        let decision = filter.evaluate_outbound();
        assert_eq!(decision, SyncFilterDecision::BlockedOptInRequired);
        assert!(decision.is_blocked());
        assert!(!decision.is_allowed());
    }

    #[test]
    fn test_sync_filter_opt_in_allows_outbound() {
        let filter = SyncFilter::new(true);
        assert!(filter.opt_in());
        assert!(filter.is_outbound_allowed());

        let decision = filter.evaluate_outbound();
        assert_eq!(decision, SyncFilterDecision::Allowed);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_sync_filter_dynamic_toggle() {
        let filter = SyncFilter::new(false);
        assert_eq!(
            filter.evaluate_outbound(),
            SyncFilterDecision::BlockedOptInRequired
        );

        filter.set_opt_in(true);
        assert!(filter.opt_in());
        assert_eq!(filter.evaluate_outbound(), SyncFilterDecision::Allowed);

        filter.set_opt_in(false);
        assert!(!filter.opt_in());
        assert_eq!(
            filter.evaluate_outbound(),
            SyncFilterDecision::BlockedOptInRequired
        );
    }

    #[test]
    fn test_filter_outbound_replication_and_memory() {
        let filter = SyncFilter::new(false);
        let sample = serde_json::json!({"key": "value"});

        assert_eq!(
            filter.filter_outbound_replication(&sample),
            Err(SyncFilterError::OptInRequired)
        );
        assert_eq!(
            filter.filter_outbound_memory("m1", "secret content"),
            Err(SyncFilterError::OptInRequired)
        );

        filter.set_opt_in(true);
        assert_eq!(
            filter.filter_outbound_replication(&sample).unwrap(),
            sample
        );
        assert_eq!(
            filter.filter_outbound_memory("m1", "secret content").unwrap(),
            "secret content"
        );
    }

    #[test]
    fn test_filter_outbound_chunk_and_manifest() {
        let filter = SyncFilter::new(false);
        let manifest = MeshManifest {
            node_id: NodeId("node-1".to_string()),
            chunks: vec![ChunkRef {
                hash: "h1".to_string(),
                document_count: 1,
                created_at: 1000,
            }],
            generated_at: 1000,
        };

        assert_eq!(
            filter.filter_outbound_chunk("h1", b"data"),
            Err(SyncFilterError::OptInRequired)
        );
        assert_eq!(
            filter.filter_outbound_manifest(&manifest).unwrap_err(),
            SyncFilterError::OptInRequired
        );

        filter.set_opt_in(true);
        assert!(filter.filter_outbound_chunk("h1", b"data").is_ok());
        assert_eq!(
            filter.filter_outbound_manifest(&manifest).unwrap().node_id,
            NodeId("node-1".to_string())
        );
    }

    #[test]
    fn test_sync_filter_error_display() {
        assert_eq!(
            SyncFilterError::OptInRequired.to_string(),
            "Outbound sync blocked: Data Node opt-in consent is required (opt_in == false)"
        );
        assert_eq!(
            SyncFilterError::SyncDisabled("testing".into()).to_string(),
            "Outbound sync blocked: sync disabled (testing)"
        );
        assert_eq!(
            SyncFilterError::SanitizationFailed("bad input".into()).to_string(),
            "Outbound sync blocked: sanitization failed (bad input)"
        );
        assert_eq!(
            SyncFilterError::PolicyViolation("custom rule".into()).to_string(),
            "Outbound sync blocked by policy: custom rule"
        );
    }
}
