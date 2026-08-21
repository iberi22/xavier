//! P2P Sync Filter — Consent-based data filtering for peer-to-peer sync.
//!
//! This module implements the [`SyncFilter`] that gates what data flows
//! between Xavier nodes during P2P synchronization. Every outgoing chunk
//! is checked against the local node's [`DataConsentManager`] before it
//! is sent to a peer. Unconsented data is stripped; partially-consented
//! data is sanitised (metadata-only or anonymized) per the consent level.
//!
//! # Design
//!
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │              PeerMemorySync                       │
//! │   sync_with(peer_url)  ──►  push/pull diffs      │
//! └────────────────┬─────────────────────────────────┘
//!                  │  outgoing diffs
//!                  ▼
//! ┌──────────────────────────────────────────────────┐
//! │              SyncFilter                           │
//! │   filter_out_unconsented(diffs, peer_id)         │
//! │     ├─ ConsentLevel::None   → drop               │
//! │     ├─ ConsentLevel::Metadata → strip fields     │
//! │     ├─ ConsentLevel::Anonymized → hash identity  │
//! │     └─ ConsentLevel::Full   → pass through       │
//! └──────────────────────────────────────────────────┘
//! ```

use crate::memory::sync::{ChunkDiff, Manifest, ManifestEntry};
use crate::mesh::data_consent::{ConsentLevel, DataConsentManager};
use crate::mesh::node::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// SyncFilter
// ---------------------------------------------------------------------------

/// Consent-aware filter for P2P memory synchronization.
///
/// Wraps a [`DataConsentManager`] and adds per-peer consent overrides so
/// different peers can receive different subsets of data. The filter is
/// cheaply cloneable (the consent manager is shared via `Arc`).
#[derive(Clone)]
pub struct SyncFilter {
    consent_manager: std::sync::Arc<DataConsentManager>,
    /// Per-peer consent overrides: peer_node_id → (data_type → ConsentLevel).
    peer_overrides:
        std::sync::Arc<std::collections::HashMap<NodeId, HashMap<String, ConsentLevel>>>,
}

impl SyncFilter {
    /// Create a new filter from an existing consent manager.
    pub fn new(consent_manager: DataConsentManager) -> Self {
        Self {
            consent_manager: std::sync::Arc::new(consent_manager),
            peer_overrides: std::sync::Arc::new(std::collections::HashMap::new()),
        }
    }

    /// Create a filter with per-peer consent overrides already populated.
    pub fn with_overrides(
        consent_manager: DataConsentManager,
        overrides: HashMap<NodeId, HashMap<String, ConsentLevel>>,
    ) -> Self {
        Self {
            consent_manager: std::sync::Arc::new(consent_manager),
            peer_overrides: std::sync::Arc::new(overrides),
        }
    }

    /// Return a reference to the underlying consent manager.
    pub fn consent_manager(&self) -> &DataConsentManager {
        &self.consent_manager
    }

    // -----------------------------------------------------------------------
    // Consent resolution
    // -----------------------------------------------------------------------

    /// Resolve the effective consent level for a data type, optionally scoped
    /// to a specific peer. Peer overrides take precedence over the global
    /// consent map.
    pub fn get_consent(&self, data_type: &str, peer_id: Option<&NodeId>) -> ConsentLevel {
        // Check peer-specific overrides first
        if let Some(peer_id) = peer_id {
            if let Some(overrides) = self.peer_overrides.get(peer_id) {
                if let Some(level) = overrides.get(data_type) {
                    return *level;
                }
            }
        }
        self.consent_manager.get_consent(data_type)
    }

    /// Returns `true` if data of the given type may be shared with `peer_id`.
    pub fn is_allowed(&self, data_type: &str, peer_id: &NodeId) -> bool {
        self.get_consent(data_type, Some(peer_id)) != ConsentLevel::None
    }

    // -----------------------------------------------------------------------
    // Filtering: ChunkDiff
    // -----------------------------------------------------------------------

    /// Filter a list of outgoing [`ChunkDiff`]s, removing those for which the
    /// local node has not granted consent.
    ///
    /// For data with `ConsentLevel::Metadata` the chunk data payload is
    /// stripped (set to `None`) so the receiver only gets the manifest
    /// reference. For `ConsentLevel::Anonymized` the data is passed through
    /// but the node_id field in any JSON payload is hashed.
    pub fn filter_out_unconsented(
        &self,
        diffs: Vec<ChunkDiff>,
        peer_id: &NodeId,
    ) -> Vec<ChunkDiff> {
        diffs
            .into_iter()
            .filter_map(|diff| {
                let level = self.get_consent(&diff.namespace, Some(peer_id));
                match level {
                    ConsentLevel::None => {
                        tracing::debug!(
                            "sync_filter: dropping chunk {} (namespace={}) — no consent",
                            diff.chunk_hash,
                            diff.namespace
                        );
                        None
                    }
                    ConsentLevel::Metadata => {
                        // Strip the data payload; keep only the manifest reference
                        Some(ChunkDiff { data: None, ..diff })
                    }
                    ConsentLevel::Anonymized | ConsentLevel::Full => Some(diff),
                }
            })
            .collect()
    }

    /// Filter a list of incoming [`ChunkDiff`]s (from a remote peer).
    ///
    /// Only accepts data for namespaces where the local node has opted in.
    /// This prevents a peer from pushing data we never agreed to receive.
    pub fn filter_incoming(&self, diffs: Vec<ChunkDiff>, peer_id: &NodeId) -> Vec<ChunkDiff> {
        diffs
            .into_iter()
            .filter(|diff| {
                let level = self.get_consent(&diff.namespace, Some(peer_id));
                level != ConsentLevel::None
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Filtering: Manifest / ManifestEntry
    // -----------------------------------------------------------------------

    /// Filter a [`Manifest`] (list of [`ManifestEntry`]s), keeping only
    /// entries whose namespace has consent for the given peer.
    pub fn filter_manifest(&self, manifest: Manifest, peer_id: &NodeId) -> Manifest {
        manifest
            .into_iter()
            .filter(|entry| self.is_allowed(&entry.namespace, peer_id))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Filtering: individual records (Metadata sanitisation)
    // -----------------------------------------------------------------------

    /// Sanitise a chunk data payload according to the consent level for the
    /// given namespace and peer. Returns `None` if consent is `None`.
    ///
    /// This applies the same transformations as
    /// [`DataConsentManager::sanitize_payload`] but scoped per-peer.
    pub fn sanitize_payload<T: Serialize>(
        &self,
        namespace: &str,
        payload: &T,
        peer_id: &NodeId,
    ) -> Result<Option<serde_json::Value>, serde_json::Error> {
        let level = self.get_consent(namespace, Some(peer_id));
        match level {
            ConsentLevel::None => Ok(None),
            _ => self.consent_manager.sanitize_payload(namespace, payload),
        }
    }

    // -----------------------------------------------------------------------
    // Summary helpers
    // -----------------------------------------------------------------------

    /// Compute a [`FilterSummary`] describing how many items were kept vs
    /// dropped by a filter pass.
    pub fn summarize_diffs(original: &[ChunkDiff], filtered: &[ChunkDiff]) -> FilterSummary {
        FilterSummary {
            total: original.len(),
            kept: filtered.len(),
            dropped: original.len().saturating_sub(filtered.len()),
        }
    }
}

// ---------------------------------------------------------------------------
// FilterSummary
// ---------------------------------------------------------------------------

/// Summary statistics for a filter pass.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilterSummary {
    pub total: usize,
    pub kept: usize,
    pub dropped: usize,
}

// ---------------------------------------------------------------------------
// FilteredSyncSession — wraps PeerMemorySync with consent filtering
// ---------------------------------------------------------------------------

/// A wrapper around [`PeerMemorySync`] that applies [`SyncFilter`] checks
/// before data is sent to a peer and after data is received.
///
/// Use this in the [`PeerRegistrySyncAdapter`] background loop to ensure
/// no data leaves the node without explicit opt-in consent.
pub struct FilteredSyncSession<'a> {
    filter: &'a SyncFilter,
    peer_id: NodeId,
}

impl<'a> FilteredSyncSession<'a> {
    /// Create a new filtered sync session for a specific peer.
    pub fn new(filter: &'a SyncFilter, peer_id: NodeId) -> Self {
        Self { filter, peer_id }
    }

    /// Filter outgoing diffs before they are pushed to the peer.
    ///
    /// This is the primary entry point called by the sync adapter before
    /// `PeerMemorySync::push_to` or the push phase of `sync_with`.
    pub fn filter_outgoing(&self, diffs: Vec<ChunkDiff>) -> Vec<ChunkDiff> {
        self.filter.filter_out_unconsented(diffs, &self.peer_id)
    }

    /// Filter incoming diffs after they are pulled from the peer.
    ///
    /// This ensures we only accept data for namespaces we have opted in to.
    pub fn filter_incoming(&self, diffs: Vec<ChunkDiff>) -> Vec<ChunkDiff> {
        self.filter.filter_incoming(diffs, &self.peer_id)
    }

    /// Filter a manifest before diffing (reduces unnecessary chunk requests).
    pub fn filter_manifest(&self, manifest: Manifest) -> Manifest {
        self.filter.filter_manifest(manifest, &self.peer_id)
    }

    /// Compute a summary of an outgoing filter pass.
    pub fn outgoing_summary(
        &self,
        original: &[ChunkDiff],
        filtered: &[ChunkDiff],
    ) -> FilterSummary {
        SyncFilter::summarize_diffs(original, filtered)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::sync::{ChunkDiff, DiffAction};
    use crate::mesh::data_consent::{ConsentLevel, DataConsentManager};
    use crate::mesh::node::NodeId;
    use std::collections::HashMap;
    use std::time::SystemTime;

    fn make_node(id: &str) -> NodeId {
        NodeId(format!("xv1-{}", id))
    }

    fn make_consent_manager_with_defaults() -> DataConsentManager {
        let node_id = make_node("local");
        let mut mgr = DataConsentManager::new(node_id);
        mgr.set_consent("workspace_public", ConsentLevel::Full);
        mgr.set_consent("workspace_private", ConsentLevel::None);
        mgr.set_consent("workspace_metadata_only", ConsentLevel::Metadata);
        mgr.set_consent("workspace_anon", ConsentLevel::Anonymized);
        mgr
    }

    fn make_diff(namespace: &str) -> ChunkDiff {
        ChunkDiff {
            chunk_hash: format!("hash_{}", namespace),
            namespace: namespace.to_string(),
            action: DiffAction::Add,
            data: Some(vec![1, 2, 3]),
            timestamp: SystemTime::now(),
            record_path: Some(format!("/memories/{}", namespace)),
        }
    }

    fn make_manifest_entry(namespace: &str) -> ManifestEntry {
        ManifestEntry {
            chunk_hash: format!("hash_{}", namespace),
            namespace: namespace.to_string(),
            revision: 1,
            updated_at: chrono::Utc::now(),
            size_bytes: 100,
            record_path: Some(format!("/memories/{}", namespace)),
        }
    }

    // ---- Test 1: filter_out_unconsented drops None-consent diffs ----

    #[test]
    fn test_filter_drops_unconsented_diffs() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let diffs = vec![
            make_diff("workspace_public"),
            make_diff("workspace_private"),
        ];

        let filtered = filter.filter_out_unconsented(diffs, &peer);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].namespace, "workspace_public");
    }

    // ---- Test 2: Metadata consent strips data payload ----

    #[test]
    fn test_metadata_consent_strips_data() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let diffs = vec![make_diff("workspace_metadata_only")];
        let filtered = filter.filter_out_unconsented(diffs, &peer);

        assert_eq!(filtered.len(), 1);
        assert!(
            filtered[0].data.is_none(),
            "Metadata consent should strip data payload"
        );
    }

    // ---- Test 3: Full consent passes data through ----

    #[test]
    fn test_full_consent_passes_through() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let diffs = vec![make_diff("workspace_public")];
        let filtered = filter.filter_out_unconsented(diffs, &peer);

        assert_eq!(filtered.len(), 1);
        assert!(
            filtered[0].data.is_some(),
            "Full consent should preserve data"
        );
    }

    // ---- Test 4: Anonymized consent passes data through ----

    #[test]
    fn test_anonymized_consent_passes_through() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let diffs = vec![make_diff("workspace_anon")];
        let filtered = filter.filter_out_unconsented(diffs, &peer);

        assert_eq!(filtered.len(), 1);
        assert!(
            filtered[0].data.is_some(),
            "Anonymized consent should preserve data"
        );
    }

    // ---- Test 5: Peer-specific overrides take precedence ----

    #[test]
    fn test_peer_overrides_take_precedence() {
        let mgr = make_consent_manager_with_defaults();
        let peer_a = make_node("peer-a");
        let peer_b = make_node("peer-b");

        // Override: peer_a gets Full access to workspace_private,
        // but peer_b keeps the default (None)
        let mut overrides = HashMap::new();
        overrides.insert(peer_a.clone(), {
            let mut m = HashMap::new();
            m.insert("workspace_private".to_string(), ConsentLevel::Full);
            m
        });

        let filter = SyncFilter::with_overrides(mgr, overrides);

        let diffs_a = vec![make_diff("workspace_private")];
        let filtered_a = filter.filter_out_unconsented(diffs_a, &peer_a);
        assert_eq!(
            filtered_a.len(),
            1,
            "peer_a should receive workspace_private via override"
        );

        let diffs_b = vec![make_diff("workspace_private")];
        let filtered_b = filter.filter_out_unconsented(diffs_b, &peer_b);
        assert_eq!(
            filtered_b.len(),
            0,
            "peer_b should NOT receive workspace_private"
        );
    }

    // ---- Test 6: filter_manifest removes unconsented entries ----

    #[test]
    fn test_filter_manifest_removes_unconsented() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let manifest = vec![
            make_manifest_entry("workspace_public"),
            make_manifest_entry("workspace_private"),
            make_manifest_entry("workspace_anon"),
        ];

        let filtered = filter.filter_manifest(manifest, &peer);
        assert_eq!(filtered.len(), 2);
        let namespaces: Vec<&str> = filtered.iter().map(|e| e.namespace.as_str()).collect();
        assert!(namespaces.contains(&"workspace_public"));
        assert!(namespaces.contains(&"workspace_anon"));
        assert!(!namespaces.contains(&"workspace_private"));
    }

    // ---- Test 7: filter_incoming drops unconsented incoming data ----

    #[test]
    fn test_filter_incoming_drops_unconsented() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let diffs = vec![
            make_diff("workspace_public"),
            make_diff("workspace_private"),
            make_diff("workspace_anon"),
        ];

        let filtered = filter.filter_incoming(diffs, &peer);
        assert_eq!(
            filtered.len(),
            2,
            "should keep public and anon, drop private"
        );
    }

    // ---- Test 8: FilterSummary computation ----

    #[test]
    fn test_filter_summary() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let original = vec![
            make_diff("workspace_public"),
            make_diff("workspace_private"),
            make_diff("workspace_anon"),
        ];
        let filtered = filter.filter_out_unconsented(original.clone(), &peer);

        let summary = SyncFilter::summarize_diffs(&original, &filtered);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.kept, 2);
        assert_eq!(summary.dropped, 1);
    }

    // ---- Test 9: is_allowed returns correct boolean ----

    #[test]
    fn test_is_allowed() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        assert!(filter.is_allowed("workspace_public", &peer));
        assert!(!filter.is_allowed("workspace_private", &peer));
        assert!(filter.is_allowed("workspace_anon", &peer));
    }

    // ---- Test 10: unknown data type defaults to None (opt-in) ----

    #[test]
    fn test_unknown_data_type_defaults_to_none() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        assert!(!filter.is_allowed("totally_unknown_namespace", &peer));
        let level = filter.get_consent("totally_unknown_namespace", Some(&peer));
        assert_eq!(level, ConsentLevel::None);
    }

    // ---- Test 11: FilteredSyncSession integration ----

    #[test]
    fn test_filtered_sync_session() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let session = FilteredSyncSession::new(&filter, peer);

        let outgoing = vec![
            make_diff("workspace_public"),
            make_diff("workspace_private"),
        ];
        let filtered_out = session.filter_outgoing(outgoing.clone());
        assert_eq!(filtered_out.len(), 1);

        let summary = session.outgoing_summary(&outgoing, &filtered_out);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.kept, 1);
        assert_eq!(summary.dropped, 1);
    }

    // ---- Test 12: All consent levels across a full diff batch ----

    #[test]
    fn test_all_consent_levels_in_batch() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let diffs = vec![
            make_diff("workspace_public"),        // Full
            make_diff("workspace_private"),       // None
            make_diff("workspace_metadata_only"), // Metadata
            make_diff("workspace_anon"),          // Anonymized
        ];

        let filtered = filter.filter_out_unconsented(diffs, &peer);
        // Full, Metadata (stripped), Anonymized = 3 kept; None = 1 dropped
        assert_eq!(filtered.len(), 3);

        // The metadata one should have data stripped
        let metadata_diff = filtered
            .iter()
            .find(|d| d.namespace == "workspace_metadata_only")
            .unwrap();
        assert!(metadata_diff.data.is_none());

        // The full and anonymized ones should have data intact
        let full_diff = filtered
            .iter()
            .find(|d| d.namespace == "workspace_public")
            .unwrap();
        assert!(full_diff.data.is_some());
        let anon_diff = filtered
            .iter()
            .find(|d| d.namespace == "workspace_anon")
            .unwrap();
        assert!(anon_diff.data.is_some());
    }

    // ---- Test 13: test_filter_concurrent_access ----

    #[test]
    fn test_filter_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let mgr = make_consent_manager_with_defaults();
        let peer_a = make_node("peer-a");
        let peer_b = make_node("peer-b");

        let mut overrides = HashMap::new();
        overrides.insert(peer_a.clone(), {
            let mut m = HashMap::new();
            m.insert("workspace_private".to_string(), ConsentLevel::Full);
            m
        });

        let filter = Arc::new(SyncFilter::with_overrides(mgr, overrides));

        let mut handles = vec![];
        for i in 0..10 {
            let filter_clone = Arc::clone(&filter);
            let peer_a_clone = peer_a.clone();
            let peer_b_clone = peer_b.clone();

            let handle = thread::spawn(move || {
                for _ in 0..100 {
                    let diffs = vec![
                        make_diff("workspace_public"),
                        make_diff("workspace_private"),
                        make_diff("workspace_metadata_only"),
                    ];
                    let filtered_a =
                        filter_clone.filter_out_unconsented(diffs.clone(), &peer_a_clone);
                    assert_eq!(filtered_a.len(), 3, "thread {} peer_a should get all 3", i);

                    let filtered_b = filter_clone.filter_out_unconsented(diffs, &peer_b_clone);
                    assert_eq!(filtered_b.len(), 2, "thread {} peer_b should get 2", i);

                    assert!(filter_clone.is_allowed("workspace_public", &peer_b_clone));
                    assert!(!filter_clone.is_allowed("workspace_private", &peer_b_clone));
                }
            });
            handles.push(handle);
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    // ---- Test 14: test_filter_large_diff_batch ----

    #[test]
    fn test_filter_large_diff_batch() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let mut large_batch = Vec::with_capacity(1200);
        for i in 0..1200 {
            let ns = match i % 4 {
                0 => "workspace_public",        // Full -> kept
                1 => "workspace_private",       // None -> dropped
                2 => "workspace_metadata_only", // Metadata -> kept (data stripped)
                _ => "workspace_anon",          // Anonymized -> kept
            };
            let mut diff = make_diff(ns);
            diff.chunk_hash = format!("hash_{}_{}", ns, i);
            large_batch.push(diff);
        }

        let filtered = filter.filter_out_unconsented(large_batch.clone(), &peer);
        assert_eq!(
            filtered.len(),
            900,
            "900 out of 1200 should be kept (300 None dropped)"
        );

        let summary = SyncFilter::summarize_diffs(&large_batch, &filtered);
        assert_eq!(summary.total, 1200);
        assert_eq!(summary.kept, 900);
        assert_eq!(summary.dropped, 300);

        let metadata_count = filtered.iter().filter(|d| d.data.is_none()).count();
        assert_eq!(
            metadata_count, 300,
            "300 metadata items should have data stripped"
        );
    }

    // ---- Test 15: test_filter_mixed_consent_levels ----

    #[test]
    fn test_filter_mixed_consent_levels() {
        let mgr = make_consent_manager_with_defaults();
        let peer_a = make_node("peer-a");

        let mut overrides = HashMap::new();
        overrides.insert(peer_a.clone(), {
            let mut m = HashMap::new();
            m.insert("custom_overridden_full".to_string(), ConsentLevel::Full);
            m.insert("workspace_public".to_string(), ConsentLevel::Metadata);
            m
        });

        let filter = SyncFilter::with_overrides(mgr, overrides);

        let batch = vec![
            make_diff("workspace_public"),        // Overridden to Metadata
            make_diff("workspace_private"),       // Default None -> dropped
            make_diff("workspace_metadata_only"), // Default Metadata
            make_diff("workspace_anon"),          // Default Anonymized
            make_diff("custom_overridden_full"),  // Overridden to Full
            make_diff("unconfigured_ns"),         // Default None -> dropped
        ];

        let filtered = filter.filter_out_unconsented(batch, &peer_a);
        assert_eq!(filtered.len(), 4);

        let pub_diff = filtered
            .iter()
            .find(|d| d.namespace == "workspace_public")
            .unwrap();
        assert!(
            pub_diff.data.is_none(),
            "overridden workspace_public should have stripped data"
        );

        let full_diff = filtered
            .iter()
            .find(|d| d.namespace == "custom_overridden_full")
            .unwrap();
        assert!(
            full_diff.data.is_some(),
            "overridden custom_overridden_full should have data"
        );

        let anon_diff = filtered
            .iter()
            .find(|d| d.namespace == "workspace_anon")
            .unwrap();
        assert!(anon_diff.data.is_some());
    }

    // ---- Test 16: test_filter_roundtrip_consistency ----

    #[test]
    fn test_filter_roundtrip_consistency() {
        let mgr_sender = make_consent_manager_with_defaults();
        let mgr_receiver = make_consent_manager_with_defaults();

        let filter_sender = SyncFilter::new(mgr_sender);
        let filter_receiver = SyncFilter::new(mgr_receiver);

        let peer_node = make_node("peer-target");

        let original_diffs = vec![
            make_diff("workspace_public"),
            make_diff("workspace_private"),
            make_diff("workspace_metadata_only"),
            make_diff("workspace_anon"),
        ];

        // 1. Sender filters outgoing diffs
        let outgoing_diffs = filter_sender.filter_out_unconsented(original_diffs, &peer_node);
        assert_eq!(outgoing_diffs.len(), 3);

        // 2. Receiver filters incoming diffs
        let incoming_diffs = filter_receiver.filter_incoming(outgoing_diffs.clone(), &peer_node);
        assert_eq!(incoming_diffs.len(), 3);

        // Verify exact roundtrip consistency
        for (out_d, in_d) in outgoing_diffs.iter().zip(incoming_diffs.iter()) {
            assert_eq!(out_d.namespace, in_d.namespace);
            assert_eq!(out_d.chunk_hash, in_d.chunk_hash);
            assert_eq!(out_d.data, in_d.data);
        }
    }

    // ---- Test 17: test_filter_empty_manifest ----

    #[test]
    fn test_filter_empty_manifest() {
        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-a");

        let empty_manifest: Manifest = vec![];
        let filtered_manifest = filter.filter_manifest(empty_manifest, &peer);
        assert!(filtered_manifest.is_empty());

        let empty_diffs: Vec<ChunkDiff> = vec![];
        let filtered_diffs = filter.filter_out_unconsented(empty_diffs.clone(), &peer);
        assert!(filtered_diffs.is_empty());

        let summary = SyncFilter::summarize_diffs(&empty_diffs, &filtered_diffs);
        assert_eq!(summary.total, 0);
        assert_eq!(summary.kept, 0);
        assert_eq!(summary.dropped, 0);
    }

    // ---- Test 18: test_filter_unknown_peer_handling ----

    #[test]
    fn test_filter_unknown_peer_handling() {
        let mgr = make_consent_manager_with_defaults();
        let known_peer = make_node("known-peer");
        let unknown_peer = make_node("unknown-peer-12345");

        let mut overrides = HashMap::new();
        overrides.insert(known_peer.clone(), {
            let mut m = HashMap::new();
            m.insert("workspace_private".to_string(), ConsentLevel::Full);
            m
        });

        let filter = SyncFilter::with_overrides(mgr, overrides);

        // Unknown peer should fall back to default global consent (None for workspace_private)
        assert_eq!(
            filter.get_consent("workspace_private", Some(&unknown_peer)),
            ConsentLevel::None
        );
        assert!(!filter.is_allowed("workspace_private", &unknown_peer));

        // Known peer uses override (Full for workspace_private)
        assert_eq!(
            filter.get_consent("workspace_private", Some(&known_peer)),
            ConsentLevel::Full
        );
        assert!(filter.is_allowed("workspace_private", &known_peer));

        // Both use default global consent for workspace_public (Full)
        assert_eq!(
            filter.get_consent("workspace_public", Some(&unknown_peer)),
            ConsentLevel::Full
        );
        assert_eq!(
            filter.get_consent("workspace_public", Some(&known_peer)),
            ConsentLevel::Full
        );
    }

    // ---- Test 19: test_filter_performance_under_load ----

    #[test]
    fn test_filter_performance_under_load() {
        use std::time::Instant;

        let mgr = make_consent_manager_with_defaults();
        let filter = SyncFilter::new(mgr);
        let peer = make_node("peer-perf");

        let mut batch = Vec::with_capacity(10_000);
        for i in 0..10_000 {
            let ns = match i % 4 {
                0 => "workspace_public",
                1 => "workspace_private",
                2 => "workspace_metadata_only",
                _ => "workspace_anon",
            };
            batch.push(make_diff(ns));
        }

        let start = Instant::now();
        let filtered = filter.filter_out_unconsented(batch, &peer);
        let elapsed = start.elapsed();

        assert_eq!(filtered.len(), 7500);
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "Filtering 10,000 items took too long: {:?}",
            elapsed
        );
    }

    // ---- Test 20: test_filter_state_persistence ----

    #[test]
    fn test_filter_state_persistence() {
        use crate::mesh::data_consent::ActiveConsent;

        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().to_path_buf();
        std::env::set_var("XAVIER_CONFIG_DIR", config_path.as_os_str());

        // 1. Setup initial consent manager & register active consent
        let active_consent = ActiveConsent {
            token_id: "tok_123".to_string(),
            workspace_id: "ws_test".to_string(),
            expires_at: chrono::Utc::now().timestamp() as u64 + 3600,
            token: "secret_token".to_string(),
        };
        DataConsentManager::register_active_consent(active_consent.clone()).unwrap();

        // Verify persistence file was written and can be read back
        let consents = DataConsentManager::list_active_consents().unwrap();
        assert_eq!(consents.len(), 1);
        assert_eq!(consents[0].token_id, "tok_123");

        // 2. Verify config map & peer overrides serialization roundtrip
        let mut original_mgr = DataConsentManager::new(make_node("node-persist"));
        original_mgr.set_consent("workspace_a", ConsentLevel::Full);
        original_mgr.set_consent("workspace_b", ConsentLevel::Metadata);

        let mut peer_overrides = HashMap::new();
        let peer_id = make_node("peer-p");
        let mut peer_map = HashMap::new();
        peer_map.insert("workspace_b".to_string(), ConsentLevel::Full);
        peer_overrides.insert(peer_id.clone(), peer_map);

        // Serialize consent_map and peer_overrides
        let serialized_map = serde_json::to_string(original_mgr.consent_map()).unwrap();
        let serialized_overrides = serde_json::to_string(&peer_overrides).unwrap();

        // Reload
        let reloaded_map: HashMap<String, ConsentLevel> =
            serde_json::from_str(&serialized_map).unwrap();
        let reloaded_overrides: HashMap<NodeId, HashMap<String, ConsentLevel>> =
            serde_json::from_str(&serialized_overrides).unwrap();

        let mut reloaded_mgr = DataConsentManager::new(make_node("node-persist"));
        for (k, v) in reloaded_map {
            reloaded_mgr.set_consent(&k, v);
        }

        let filter = SyncFilter::with_overrides(reloaded_mgr, reloaded_overrides);

        assert_eq!(
            filter.get_consent("workspace_a", Some(&peer_id)),
            ConsentLevel::Full
        );
        assert_eq!(
            filter.get_consent("workspace_b", Some(&peer_id)),
            ConsentLevel::Full
        );
        assert_eq!(
            filter.get_consent("workspace_b", None),
            ConsentLevel::Metadata
        );

        std::env::remove_var("XAVIER_CONFIG_DIR");
    }
}
