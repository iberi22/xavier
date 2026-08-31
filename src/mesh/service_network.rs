//! Service Network layer (Capa 2) for internal work routing in the Xavier Mesh.
//!
//! This module implements service discovery, a service registry for nodes to
//! register their capabilities (e.g., memory, search, code-graph), and
//! health-aware service routing to direct requests to healthy nodes.

use crate::mesh::node::NodeId;
use crate::mesh::peer::PeerRegistry;
use crate::security::clearance::ClearanceLevel;
use crate::security::redaction::{RedactionEngine, RedactionRule};
use serde::{Deserialize, Serialize};

/// Telemetry sanitizer stripping file paths, IP subnets, hostnames, and PII before telemetry broadcast.
#[derive(Debug, Clone)]
pub struct TelemetrySanitizer {
    engine: RedactionEngine,
}

impl Default for TelemetrySanitizer {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetrySanitizer {
    /// Create a new `TelemetrySanitizer` configured with telemetry privacy rules.
    pub fn new() -> Self {
        let mut rules = vec![
            RedactionRule {
                name: "ip_subnet".to_string(),
                pattern: r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)/(?:[0-9]|[1-2][0-9]|3[0-2])\b".to_string(),
                mask: "[IP_SUBNET]".to_string(),
            },
            RedactionRule {
                name: "win_path".to_string(),
                pattern: r"(?i)\b[a-z]:\\(?:[^\\\s:]+\\)+[^\\\s:]*".to_string(),
                mask: "[PATH]".to_string(),
            },
            RedactionRule {
                name: "unix_path".to_string(),
                pattern: r"(?:/(?:home|Users|var|tmp|etc|opt|usr|root|app|mnt|Volumes|workspace|projects)/[a-zA-Z0-9_.-]+[a-zA-Z0-9_/.-]*|/[a-zA-Z0-9_.-]+(?:/[a-zA-Z0-9_.-]+)+\.(?:rs|ts|js|py|json|db|log|txt|toml|md|sh|yaml|yml))\b".to_string(),
                mask: "[PATH]".to_string(),
            },
            RedactionRule {
                name: "hostname_kv".to_string(),
                pattern: r"\b(?:hostname|host|machine_name|node_host)\s*[:=]\s*[a-zA-Z0-9_.-]+\b".to_string(),
                mask: "[HOSTNAME]".to_string(),
            },
            RedactionRule {
                name: "local_hostname".to_string(),
                pattern: r"\b[a-zA-Z0-9_-]+\.(?:local|lan|internal|domain)\b".to_string(),
                mask: "[HOSTNAME]".to_string(),
            },
        ];

        let default_engine = RedactionEngine::default();
        rules.extend(default_engine.rules);

        Self {
            engine: RedactionEngine::new(rules),
        }
    }

    /// Sanitize input text by scrubbing file paths, IP subnets, hostnames, and PII.
    pub fn sanitize(&self, input: &str) -> String {
        self.engine.redact(input)
    }
}
use std::collections::HashMap;

/// Telemetry sample published across the service network.
/// Must always be classified as INTERNAL and contain no personal data (PII).
/// INTERNAL|Telemetry classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelemetrySample {
    pub node_id: NodeId,
    pub kind: ServiceKind,
    pub payload: String,
    pub ts: i64,
    pub classification: String,
}

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
    /// Collected telemetry samples from nodes in the service network.
    pub telemetry: Vec<TelemetrySample>,
}

impl ServiceRegistry {
    /// Create a new empty service registry.
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            telemetry: Vec::new(),
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

    /// Publish a telemetry sample to the service network.
    /// INTERNAL|Telemetry classification enforcement
    ///
    /// The payload is automatically scrubbed for sensitive workspace paths, IP subnets,
    /// hostnames, and PII via `TelemetrySanitizer`, and `classification` is enforced to be `ClearanceLevel::Internal` ("INTERNAL").
    pub fn publish_telemetry(&mut self, mut sample: TelemetrySample) -> TelemetrySample {
        let sanitizer = TelemetrySanitizer::default();
        sample.payload = sanitizer.sanitize(&sample.payload);
        sample.classification = ClearanceLevel::Internal.as_str().to_uppercase();
        if sample.ts == 0 {
            sample.ts = chrono::Utc::now().timestamp();
        }
        self.telemetry.push(sample.clone());
        sample
    }

    /// Consume telemetry samples recorded since the given timestamp (`since`).
    /// INTERNAL|Telemetry consume handler
    pub fn consume_telemetry(&self, since: i64) -> Vec<TelemetrySample> {
        self.telemetry
            .iter()
            .filter(|s| s.ts >= since)
            .cloned()
            .collect()
    }

    /// Helper to collect system health info as a telemetry snapshot sample.
    pub fn collect_health_telemetry(
        &mut self,
        node_id: NodeId,
        health: &crate::observability::health::HealthStatus,
    ) -> TelemetrySample {
        let payload = format!(
            "health status={:?} cpu={:.1}% ram={:.1}% active_peers={}",
            health.status,
            health.system.cpu_usage,
            health.system.ram_usage_percent,
            health.mesh.active_peers
        );
        let sample = TelemetrySample {
            node_id,
            kind: ServiceKind::Custom("health_snapshot".to_string()),
            payload,
            ts: health.timestamp.timestamp(),
            classification: ClearanceLevel::Internal.as_str().to_uppercase(),
        };
        self.publish_telemetry(sample)
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
    fn test_service_network_telemetry_sanitization() {
        let mut registry = ServiceRegistry::new();
        let node_id = NodeId("xv1-sanitizer-node".to_string());

        let raw_sample = TelemetrySample {
            node_id: node_id.clone(),
            kind: ServiceKind::Memory,
            payload: "Error on hostname=dev-box-01 (ip 192.168.1.0/24) path /home/user/project/src/main.rs or C:\\Users\\admin\\config.json contact john.doe@example.com".to_string(),
            ts: 2000,
            classification: "PUBLIC".to_string(),
        };

        let published = registry.publish_telemetry(raw_sample);

        // Classification forced to INTERNAL
        assert_eq!(published.classification, "INTERNAL");

        // Paths scrubbed
        assert!(!published.payload.contains("/home/user/project/src/main.rs"));
        assert!(!published.payload.contains("C:\\Users\\admin\\config.json"));
        assert!(published.payload.contains("[PATH]"));

        // IP subnets scrubbed
        assert!(!published.payload.contains("192.168.1.0/24"));
        assert!(published.payload.contains("[IP_SUBNET]"));

        // Hostnames scrubbed
        assert!(!published.payload.contains("hostname=dev-box-01"));
        assert!(published.payload.contains("[HOSTNAME]"));

        // PII scrubbed
        assert!(!published.payload.contains("john.doe@example.com"));
        assert!(published.payload.contains("[EMAIL]"));
    }

    /// Test personal data exclusion guarantee asserting PII and sensitive fields are redacted.
    /// personal exclusion|PII
    #[test]
    fn test_personal_data_exclusion() {
        let mut registry = ServiceRegistry::new();
        let node_id = NodeId("xv1-pii-exclusion-node".to_string());

        let raw_sample = TelemetrySample {
            node_id: node_id.clone(),
            kind: ServiceKind::Memory,
            payload: "User personal data exclusion check: contact user@domain.com or call +1-800-555-0199, path /home/user/secret.txt, subnet 10.0.0.0/16, host=node1.local".to_string(),
            ts: 12345,
            classification: "CONFIDENTIAL".to_string(),
        };

        let published = registry.publish_telemetry(raw_sample);

        // Assert INTERNAL clearance level classification
        assert_eq!(
            published.classification,
            ClearanceLevel::Internal.as_str().to_uppercase()
        );

        // Assert personal data (PII) and sensitive data exclusion
        assert!(!published.payload.contains("user@domain.com"));
        assert!(!published.payload.contains("+1-800-555-0199"));
        assert!(!published.payload.contains("/home/user/secret.txt"));
        assert!(!published.payload.contains("10.0.0.0/16"));
        assert!(!published.payload.contains("node1.local"));

        assert!(published.payload.contains("[EMAIL]"));
        assert!(published.payload.contains("[PHONE]"));
        assert!(published.payload.contains("[PATH]"));
        assert!(published.payload.contains("[IP_SUBNET]"));
        assert!(published.payload.contains("[HOSTNAME]"));
    }

    #[test]
    fn test_telemetry_publish_classification_and_pii_redaction() {
        let mut registry = ServiceRegistry::new();
        let node_id = NodeId("xv1-telemetry-node".to_string());

        let raw_sample = TelemetrySample {
            node_id: node_id.clone(),
            kind: ServiceKind::Memory,
            payload: "Node error when contacting user john.doe@example.com or calling +1-555-123-4567 during bench".to_string(),
            ts: 1000,
            classification: "PUBLIC".to_string(), // Attempting to publish non-INTERNAL
        };

        let published = registry.publish_telemetry(raw_sample);

        // 1. Classification forced to INTERNAL
        assert_eq!(published.classification, "INTERNAL");

        // 2. PII excluded (email and phone redacted)
        assert!(!published.payload.contains("john.doe@example.com"));
        assert!(!published.payload.contains("+1-555-123-4567"));
        assert!(published.payload.contains("[EMAIL]"));
        assert!(published.payload.contains("[PHONE]"));

        // 3. Telemetry stored and consumable
        let consumed = registry.consume_telemetry(500);
        assert_eq!(consumed.len(), 1);
        assert_eq!(consumed[0], published);
    }

    #[test]
    fn test_telemetry_consume_since() {
        let mut registry = ServiceRegistry::new();
        let node_id = NodeId("xv1-node".to_string());

        let s1 = TelemetrySample {
            node_id: node_id.clone(),
            kind: ServiceKind::Search,
            payload: "sample 1".to_string(),
            ts: 100,
            classification: "INTERNAL".to_string(),
        };
        let s2 = TelemetrySample {
            node_id: node_id.clone(),
            kind: ServiceKind::Search,
            payload: "sample 2".to_string(),
            ts: 200,
            classification: "INTERNAL".to_string(),
        };

        registry.publish_telemetry(s1);
        registry.publish_telemetry(s2);

        let consumed = registry.consume_telemetry(150);
        assert_eq!(consumed.len(), 1);
        assert_eq!(consumed[0].payload, "sample 2");
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
            capabilities: Vec::new(),
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
            capabilities: Vec::new(),
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
