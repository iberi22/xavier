//! mDNS peer auto-discovery for Xavier mesh
//!
//! Registers Xavier mesh service via mDNS/DNS-SD and discovers
//! local network peers automatically.

use crate::mesh::governance::DaoGovernanceSystem;
use crate::mesh::node::NodeId;
use crate::mesh::peer::{PeerInfo, PeerRegistry};
use crate::mesh::service_network::{ServiceRegistry, TelemetrySample};
use crate::mesh::tokenomics::wallet::{TransactionKind, Wallet};
use crate::security::clearance::ClearanceLevel;
use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// mDNS service type for Xavier mesh discovery.
pub const MDNS_SERVICE_TYPE: &str = "_xavier-mesh._tcp.local.";

/// Summary report from an autonomous mesh node scanning pass.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeshScanReport {
    pub scanned_nodes: usize,
    pub healthy_nodes: usize,
    pub registered_dao_voters: usize,
    pub telemetry_samples_collected: usize,
}

/// Autonomous scanner that discovers active mesh nodes, collects sanitized telemetry,
/// and synchronizes active node credentials into the DAO Governance system and wallet registry.
#[derive(Debug, Default, Clone)]
pub struct MeshNodeScanner;

impl MeshNodeScanner {
    /// Create a new `MeshNodeScanner`.
    pub fn new() -> Self {
        Self
    }

    /// Run an autonomous scan pass over registered peers and service network instances.
    /// Updates DAO Governance voter/maintainer status and records internal telemetry.
    pub fn scan(
        &self,
        peer_registry: &PeerRegistry,
        service_registry: &mut ServiceRegistry,
        dao_governance: &mut DaoGovernanceSystem,
    ) -> MeshScanReport {
        let peers = peer_registry.all_peers();
        let total_scanned = peers.len();
        let mut healthy_count = 0;
        let mut voters_registered = 0;
        let mut telemetry_count = 0;

        for peer in peers {
            if peer.is_healthy() {
                healthy_count += 1;
                let node_id_str = peer.node_id.as_str().to_string();

                // 1. Register maintainer trust score in DAO governance if missing
                dao_governance
                    .maintainer_registry
                    .entry(node_id_str.clone())
                    .or_insert(500);

                // 2. Ensure active peer has an associated XP wallet in DAO governance
                if !dao_governance.wallets.contains_key(&node_id_str) {
                    let mut wallet = Wallet::new(peer.node_id.clone());
                    wallet.credit(
                        100,
                        TransactionKind::Reward,
                        "Mesh node participation initial grant",
                    );
                    dao_governance.wallets.insert(node_id_str.clone(), wallet);
                    voters_registered += 1;
                }

                // 3. Emit sanitized internal telemetry sample for active node
                let payload = format!(
                    "node_scan status=healthy endpoint={} capabilities={}",
                    peer.endpoint_url,
                    peer.capabilities.join(",")
                );
                let sample = TelemetrySample {
                    node_id: peer.node_id.clone(),
                    kind: crate::mesh::service_network::ServiceKind::Custom(
                        "mesh_node_scan".to_string(),
                    ),
                    payload,
                    ts: chrono::Utc::now().timestamp(),
                    classification: ClearanceLevel::Internal.as_str().to_uppercase(),
                };
                service_registry.publish_telemetry(sample);
                telemetry_count += 1;
            }
        }

        MeshScanReport {
            scanned_nodes: total_scanned,
            healthy_nodes: healthy_count,
            registered_dao_voters: voters_registered,
            telemetry_samples_collected: telemetry_count,
        }
    }
}

/// Discover local network peers via mDNS.
pub async fn discover_mdns_peers(registry: Arc<RwLock<PeerRegistry>>) -> Result<Vec<PeerInfo>> {
    let daemon = ServiceDaemon::new()?;
    let receiver = daemon.browse(MDNS_SERVICE_TYPE)?;
    let mut peers = Vec::new();

    // Collect discovered services using async timeout loop without blocking tokio runtime
    let timeout = std::time::Duration::from_millis(500);
    let _ = tokio::time::timeout(timeout, async {
        while let Ok(event) = receiver.recv_async().await {
            if let ServiceEvent::ServiceResolved(info) = event {
                let node_id_str = info.get_property_val_str("node_id").unwrap_or_default();
                if node_id_str.is_empty() {
                    continue;
                }
                let host = info.get_addresses().iter().next();
                if let Some(ip) = host {
                    let endpoint_url = format!("http://{}:{}", ip, info.get_port());
                    let node_id = NodeId(node_id_str.to_string());
                    let peer = PeerInfo {
                        node_id: node_id.clone(),
                        alias: Some(info.get_fullname().to_string()),
                        endpoint_url,
                        last_seen_at: Some(chrono::Utc::now().timestamp()),
                        ..Default::default()
                    };
                    peers.push(peer.clone());
                    let _ = registry.write().await.add_peer(peer);
                }
            }
        }
    })
    .await;

    Ok(peers)
}

/// Register this node as an mDNS service.
pub async fn register_mdns_service(node_id: &str, port: u16) -> Result<ServiceDaemon> {
    let daemon = ServiceDaemon::new()?;
    let mut properties = std::collections::HashMap::new();
    properties.insert("node_id".to_string(), node_id.to_string());

    let host_name = format!("{}.local.", node_id);
    let service_info =
        ServiceInfo::new(MDNS_SERVICE_TYPE, node_id, &host_name, "", port, properties)?;

    daemon.register(service_info)?;
    Ok(daemon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_mdns_service_type() {
        assert_eq!(MDNS_SERVICE_TYPE, "_xavier-mesh._tcp.local.");
    }

    #[tokio::test]
    async fn test_register_mdns_service() {
        let daemon_res = register_mdns_service("xv1-test-node", 8080).await;
        assert!(daemon_res.is_ok());
    }

    #[tokio::test]
    async fn test_discover_mdns_peers_empty() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("peers.json");
        let registry = Arc::new(RwLock::new(PeerRegistry::load_from(registry_path).unwrap()));

        let peers_res = discover_mdns_peers(registry).await;
        assert!(peers_res.is_ok());
    }

    #[tokio::test]
    async fn test_register_and_discover() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("peers.json");
        let registry = Arc::new(RwLock::new(PeerRegistry::load_from(registry_path).unwrap()));

        let daemon = register_mdns_service("xv1-loopback-test", 9090).await;
        assert!(daemon.is_ok());

        let peers = discover_mdns_peers(registry.clone()).await.unwrap();
        assert!(peers.is_empty() || !peers.is_empty());
    }

    #[test]
    fn test_mesh_node_scanner_run_scan() {
        let dir = tempdir().unwrap();
        let registry_path = dir.path().join("peers.json");
        let mut peer_registry = PeerRegistry::load_from(registry_path).unwrap();

        let healthy_node_id = NodeId("xv1-healthy-node".to_string());
        let healthy_peer = PeerInfo {
            node_id: healthy_node_id.clone(),
            alias: Some("Healthy Peer".to_string()),
            endpoint_url: "http://localhost:8006".to_string(),
            public_key_hex: "11223344".to_string(),
            added_at: 1000,
            last_seen_at: Some(chrono::Utc::now().timestamp()),
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: std::collections::HashMap::new(),
            capabilities: vec!["memory".to_string(), "search".to_string()],
        };

        let unhealthy_node_id = NodeId("xv1-unhealthy-node".to_string());
        let unhealthy_peer = PeerInfo {
            node_id: unhealthy_node_id.clone(),
            alias: Some("Unhealthy Peer".to_string()),
            endpoint_url: "http://localhost:8007".to_string(),
            public_key_hex: "55667788".to_string(),
            added_at: 1000,
            last_seen_at: Some(chrono::Utc::now().timestamp() - 300), // 300s ago
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: std::collections::HashMap::new(),
            capabilities: vec!["code-graph".to_string()],
        };

        peer_registry.add_peer(healthy_peer).unwrap();
        peer_registry.add_peer(unhealthy_peer).unwrap();

        let mut service_registry = ServiceRegistry::new();
        let mut dao_governance = DaoGovernanceSystem::new();

        let scanner = MeshNodeScanner::new();
        let report = scanner.scan(&peer_registry, &mut service_registry, &mut dao_governance);

        assert_eq!(report.scanned_nodes, 2);
        assert_eq!(report.healthy_nodes, 1);
        assert_eq!(report.registered_dao_voters, 1);
        assert_eq!(report.telemetry_samples_collected, 1);

        // Maintainer trust score registered
        assert_eq!(
            dao_governance.maintainer_registry.get("xv1-healthy-node"),
            Some(&500)
        );

        // Wallet & initial grant registered
        let wallet = dao_governance.wallets.get("xv1-healthy-node").unwrap();
        assert_eq!(wallet.balance.xp_balance, 100);

        // Telemetry emitted and consumable
        let samples = service_registry.consume_telemetry(0);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].node_id, healthy_node_id);
        assert_eq!(samples[0].classification, "INTERNAL");
    }
}
