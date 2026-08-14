//! Service Network layer (Capa 2) for internal work routing in the Xavier Mesh.
//!
//! This module implements service discovery, a service registry for nodes to
//! register their capabilities (e.g., memory, search, code-graph), and
//! health-aware service routing to direct requests to healthy nodes.

use crate::mesh::node::NodeId;
use crate::mesh::peer::PeerRegistry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Types of services available in the Xavier Mesh.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServiceKind {
    Memory,
    Search,
    CodeGraph,
    Custom(String),
}

impl std::fmt::Display for ServiceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memory => write!(f, "memory"),
            Self::Search => write!(f, "search"),
            Self::CodeGraph => write!(f, "code-graph"),
            Self::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Information about a registered service instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceInfo {
    pub node_id: NodeId,
    pub kind: ServiceKind,
    pub capabilities: Vec<String>,
    pub endpoint_url: String,
    pub version: String,
}

/// A registry of services offered by nodes within the mesh network.
#[derive(Debug, Default, Clone)]
pub struct ServiceRegistry {
    /// Maps service kind to a list of node-hosted service instances.
    pub services: HashMap<ServiceKind, Vec<ServiceInfo>>,
}

impl ServiceRegistry {
    /// Create a new empty service registry.
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    /// Register a service in the registry.
    pub fn register_service(&mut self, info: ServiceInfo) {
        let entry = self.services.entry(info.kind.clone()).or_default();
        // Avoid duplicate registrations for the same node and service kind.
        if let Some(existing) = entry.iter_mut().find(|s| s.node_id == info.node_id) {
            *existing = info;
        } else {
            entry.push(info);
        }
    }

    /// Deregister a service for a specific node.
    pub fn deregister_service(&mut self, node_id: &NodeId, kind: &ServiceKind) {
        if let Some(instances) = self.services.get_mut(kind) {
            instances.retain(|info| info.node_id != *node_id);
        }
    }

    /// Discover nodes providing a specific service kind.
    pub fn discover_service(&self, kind: &ServiceKind) -> Vec<ServiceInfo> {
        self.services.get(kind).cloned().unwrap_or_default()
    }

    /// Route a request for a service kind to an available node.
    /// Skip any unhealthy nodes based on the provided health-checking function.
    pub fn route_service<F>(&self, kind: &ServiceKind, is_healthy: F) -> Option<&ServiceInfo>
    where
        F: Fn(&NodeId) -> bool,
    {
        let instances = self.services.get(kind)?;
        // Find the first healthy instance.
        instances.iter().find(|info| is_healthy(&info.node_id))
    }

    /// Route a request using a PeerRegistry to check node health.
    pub fn route_service_with_registry(
        &self,
        kind: &ServiceKind,
        registry: &PeerRegistry,
    ) -> Option<&ServiceInfo> {
        self.route_service(kind, |node_id| {
            registry
                .get_peer(node_id)
                .map(|p| p.is_healthy())
                .unwrap_or(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::peer::PeerInfo;
    use tempfile::tempdir;

    #[test]
    fn test_mesh_service_registration_and_discovery() {
        let mut registry = ServiceRegistry::new();
        let node_id = NodeId("xv1-node1".to_string());

        let info = ServiceInfo {
            node_id: node_id.clone(),
            kind: ServiceKind::Memory,
            capabilities: vec!["sqlite".to_string(), "read-write".to_string()],
            endpoint_url: "http://localhost:8000".to_string(),
            version: "0.12.0".to_string(),
        };

        registry.register_service(info.clone());

        let discovered = registry.discover_service(&ServiceKind::Memory);
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0], info);

        // Update the registration
        let mut updated_info = info.clone();
        updated_info.capabilities.push("compression".to_string());
        registry.register_service(updated_info.clone());

        let discovered_updated = registry.discover_service(&ServiceKind::Memory);
        assert_eq!(discovered_updated.len(), 1);
        assert_eq!(discovered_updated[0].capabilities.len(), 3);
    }

    #[test]
    fn test_mesh_service_deregistration() {
        let mut registry = ServiceRegistry::new();
        let node_id = NodeId("xv1-node1".to_string());

        let info = ServiceInfo {
            node_id: node_id.clone(),
            kind: ServiceKind::Search,
            capabilities: vec!["vector-search".to_string()],
            endpoint_url: "http://localhost:8001".to_string(),
            version: "0.12.0".to_string(),
        };

        registry.register_service(info);
        assert_eq!(registry.discover_service(&ServiceKind::Search).len(), 1);

        registry.deregister_service(&node_id, &ServiceKind::Search);
        assert_eq!(registry.discover_service(&ServiceKind::Search).len(), 0);
    }

    #[test]
    fn test_mesh_service_health_aware_routing() {
        let mut registry = ServiceRegistry::new();
        let node1 = NodeId("xv1-node1".to_string());
        let node2 = NodeId("xv1-node2".to_string());

        let s1 = ServiceInfo {
            node_id: node1.clone(),
            kind: ServiceKind::CodeGraph,
            capabilities: vec!["rust-parser".to_string()],
            endpoint_url: "http://localhost:8002".to_string(),
            version: "0.12.0".to_string(),
        };

        let s2 = ServiceInfo {
            node_id: node2.clone(),
            kind: ServiceKind::CodeGraph,
            capabilities: vec!["rust-parser".to_string()],
            endpoint_url: "http://localhost:8003".to_string(),
            version: "0.12.0".to_string(),
        };

        registry.register_service(s1);
        registry.register_service(s2);

        // Case 1: All healthy -> should pick the first registered (node1)
        let routed_healthy = registry.route_service(&ServiceKind::CodeGraph, |_| true);
        assert_eq!(routed_healthy.map(|s| &s.node_id), Some(&node1));

        // Case 2: Only node2 is healthy -> should skip node1 and route to node2
        let routed_filtered = registry.route_service(&ServiceKind::CodeGraph, |id| id == &node2);
        assert_eq!(routed_filtered.map(|s| &s.node_id), Some(&node2));

        // Case 3: None healthy -> should return None
        let routed_none = registry.route_service(&ServiceKind::CodeGraph, |_| false);
        assert!(routed_none.is_none());
    }

    #[test]
    fn test_mesh_service_routing_with_peer_registry() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("peers.json");
        let mut peer_registry = PeerRegistry::load_from(storage_path).unwrap();

        let node1 = NodeId("xv1-node1".to_string());
        let node2 = NodeId("xv1-node2".to_string());

        // node1: healthy (last_seen_at is recent)
        let peer1 = PeerInfo {
            node_id: node1.clone(),
            alias: Some("Node 1".to_string()),
            endpoint_url: "http://localhost:8000".to_string(),
            public_key_hex: "aabbcc".to_string(),
            added_at: 1000,
            last_seen_at: Some(chrono::Utc::now().timestamp() - 10), // 10 seconds ago (healthy)
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: HashMap::new(),
        };

        // node2: unhealthy (last_seen_at is None or old)
        let peer2 = PeerInfo {
            node_id: node2.clone(),
            alias: Some("Node 2".to_string()),
            endpoint_url: "http://localhost:8001".to_string(),
            public_key_hex: "ddeeff".to_string(),
            added_at: 1000,
            last_seen_at: Some(chrono::Utc::now().timestamp() - 200), // 200 seconds ago (unhealthy)
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: HashMap::new(),
        };

        peer_registry.add_peer(peer1).unwrap();
        peer_registry.add_peer(peer2).unwrap();

        let mut service_registry = ServiceRegistry::new();

        let s1 = ServiceInfo {
            node_id: node1.clone(),
            kind: ServiceKind::Memory,
            capabilities: vec![],
            endpoint_url: "http://localhost:8000".to_string(),
            version: "0.12.0".to_string(),
        };

        let s2 = ServiceInfo {
            node_id: node2.clone(),
            kind: ServiceKind::Memory,
            capabilities: vec![],
            endpoint_url: "http://localhost:8001".to_string(),
            version: "0.12.0".to_string(),
        };

        // If we register s2 (unhealthy) first, then s1 (healthy)
        service_registry.register_service(s2);
        service_registry.register_service(s1);

        // Routing with registry should skip node2 (which is unhealthy, even though registered first) and route to node1
        let routed =
            service_registry.route_service_with_registry(&ServiceKind::Memory, &peer_registry);
        assert_eq!(routed.map(|s| &s.node_id), Some(&node1));
    }
}
