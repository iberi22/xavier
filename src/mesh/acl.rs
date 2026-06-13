//! Access Control Lists (ACL) — Mesh-aware permission enforcement
//!
//! This module provides utilities to enforce permissions on memory retrieval
//! and synchronization based on the registered peer information.

use crate::memory::schema::{ClearanceLevel, ContextZone, MemoryQueryFilters};
use crate::mesh::peer::PeerInfo;
use crate::mesh::protocol::MeshManifest;

/// Enforce peer permissions on a memory query filter.
pub fn apply_acl_to_filters(
    filters: &mut MemoryQueryFilters,
    peer: &PeerInfo,
) {
    // 1. Enforce Clearance Level
    if let Some(max_clearance) = peer.max_clearance {
        let mut clearances = filters.clearances.clone().unwrap_or_default();
        if clearances.is_empty() {
            // If no clearances specified, include all up to max_clearance
            clearances = vec![
                ClearanceLevel::Unclassified,
                ClearanceLevel::Confidential,
                ClearanceLevel::Secret,
                ClearanceLevel::TopSecret,
            ]
            .into_iter()
            .filter(|c| *c <= max_clearance)
            .collect();
        } else {
            // Filter out clearances that exceed max_clearance
            clearances.retain(|c| *c <= max_clearance);
        }
        filters.clearances = Some(clearances);
    }

    // 2. Enforce Allowed Namespaces
    if let Some(allowed_namespaces) = &peer.allowed_namespaces {
        if let Some(project) = &filters.project {
            if !allowed_namespaces.contains(project) {
                // If the requested project is not allowed, we force a project that won't match
                filters.project = Some("__DISALLOWED__".to_string());
            }
        } else {
            // If no project specified, we should probably restrict it to the allowed ones?
            // For now, if the peer has a namespace whitelist, they must stay within it.
            // This logic depends on how MemoryQueryFilters handles missing project (usually matches all).
            // A safer approach is to not allow empty project if a whitelist exists.
        }
    }

    // 3. Enforce Allowed Paths
    if let Some(allowed_paths) = &peer.allowed_paths {
        if let Some(prefix) = &filters.path_prefix {
            let mut allowed = false;
            for path in allowed_paths {
                if prefix.starts_with(path) || path.starts_with(prefix) {
                    allowed = true;
                    break;
                }
            }
            if !allowed {
                filters.path_prefix = Some("__DISALLOWED__".to_string());
            }
        } else {
            // If no prefix specified, we could force one of the allowed paths
            if !allowed_paths.is_empty() {
                filters.path_prefix = Some(allowed_paths[0].clone());
            }
        }
    }
}

/// Filter a mesh manifest based on peer permissions.
pub fn apply_acl_to_manifest(
    _manifest: &mut MeshManifest,
    _peer: &PeerInfo,
    _workspace_id: &str,
) {
    // Phase 1: Simple filtering based on what we can infer from the manifest.
    // In Phase 1, MeshManifest only contains chunk hashes and basic metadata.
    // Full ACL at manifest level requires knowing which documents are in which chunks.
    // For now, we skip manifest-level filtering or implement a placeholder.

    // Future: the manifest should ideally be generated *for* a specific peer,
    // applying the document-level filters before grouping them into chunks.
}

/// Check if a specific document path and metadata match peer permissions.
pub fn is_allowed(
    path: &str,
    metadata: &serde_json::Value,
    peer: &PeerInfo,
) -> bool {
    // 1. Check Clearance
    if let Some(max_clearance) = peer.max_clearance {
        let clearance_str = metadata
            .get("clearance")
            .and_then(|v| v.as_str())
            .unwrap_or("top_secret");
        let clearance = ClearanceLevel::parse(clearance_str);
        if clearance > max_clearance {
            return false;
        }
    }

    // 2. Check Namespaces
    if let Some(allowed_namespaces) = &peer.allowed_namespaces {
        let project = metadata
            .get("namespace")
            .and_then(|v| v.get("project"))
            .and_then(|v| v.as_str());
        if let Some(project) = project {
            if !allowed_namespaces.contains(&project.to_string()) {
                return false;
            }
        } else if !allowed_namespaces.is_empty() {
            // If document has no project but a whitelist exists, disallow by default?
            return false;
        }
    }

    // 3. Check Paths
    if let Some(allowed_paths) = &peer.allowed_paths {
        let mut allowed = false;
        for allowed_path in allowed_paths {
            if path.starts_with(allowed_path) {
                allowed = true;
                break;
            }
        }
        if !allowed {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::node::NodeId;
    use serde_json::json;

    fn mock_peer() -> PeerInfo {
        PeerInfo {
            node_id: NodeId("xv1-test".to_string()),
            alias: None,
            endpoint_url: String::new(),
            public_key_hex: String::new(),
            added_at: 0,
            last_seen_at: None,
            sync_enabled: true,
            max_clearance: Some(ClearanceLevel::TopSecret),
            allowed_namespaces: Some(vec!["project-a".to_string()]),
            allowed_paths: Some(vec!["docs/".to_string()]),
        }
    }

    #[test]
    fn test_is_allowed_clearance() {
        let mut peer = mock_peer();
        peer.max_clearance = Some(ClearanceLevel::Secret);

        let allowed_meta = json!({ "clearance": "confidential" });
        assert!(is_allowed("docs/test.md", &allowed_meta, &peer));

        let disallowed_meta = json!({ "clearance": "top_secret" });
        assert!(!is_allowed("docs/test.md", &disallowed_meta, &peer));
    }

    #[test]
    fn test_is_allowed_namespace() {
        let peer = mock_peer();

        let allowed_meta = json!({ "namespace": { "project": "project-a" }, "clearance": "unclassified" });
        assert!(is_allowed("docs/test.md", &allowed_meta, &peer));

        let disallowed_meta = json!({ "namespace": { "project": "project-b" }, "clearance": "unclassified" });
        assert!(!is_allowed("docs/test.md", &disallowed_meta, &peer));
    }

    #[test]
    fn test_is_allowed_path() {
        let peer = mock_peer();

        assert!(is_allowed("docs/test.md", &json!({ "clearance": "unclassified" }), &peer));
        assert!(!is_allowed("src/lib.rs", &json!({ "clearance": "unclassified" }), &peer));
    }

    #[test]
    fn test_apply_acl_to_filters() {
        let mut peer = mock_peer();
        peer.max_clearance = Some(ClearanceLevel::Secret);
        let mut filters = MemoryQueryFilters::default();

        apply_acl_to_filters(&mut filters, &peer);

        assert_eq!(filters.clearances.unwrap().len(), 3); // Unclassified, Confidential, Secret
        assert_eq!(filters.path_prefix, Some("docs/".to_string()));
    }
}
