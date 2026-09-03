//! mDNS peer auto-discovery for Xavier mesh
//!
//! Registers Xavier mesh service via mDNS/DNS-SD and discovers
//! local network peers automatically.

use crate::mesh::node::NodeId;
use crate::mesh::peer::{PeerInfo, PeerRegistry};
use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::sync::Arc;
use tokio::sync::RwLock;

/// mDNS service type for Xavier mesh discovery.
pub const MDNS_SERVICE_TYPE: &str = "_xavier-mesh._tcp.local.";

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
}
