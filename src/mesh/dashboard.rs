use crate::health::mesh_telemetry::MeshTelemetryCollector;
use crate::mesh::maturity::MeshMaturityReport;
use crate::mesh::peer::{PeerInfo, PeerRegistry};
use serde::{Deserialize, Serialize};

/// Structured representation of a peer's health for the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeshPeerHealth {
    pub id: String,
    pub address: String,
    pub latency: f64,
    pub sync_state: String,
    pub version: String,
}

/// Bandwidth usage statistics for the mesh network.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeshBandwidth {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub messages_per_second: f64,
}

/// Unified mesh dashboard health response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeshDashboardResponse {
    pub peers: Vec<MeshPeerHealth>,
    pub maturity: MeshMaturityReport,
    pub bandwidth: MeshBandwidth,
}

/// Aggregates peer and telemetry data to produce a central mesh status dashboard.
pub fn aggregate_dashboard(
    registry: &PeerRegistry,
    telemetry: Option<&MeshTelemetryCollector>,
) -> MeshDashboardResponse {
    let peers_info = registry.list_peers();
    let mut peers = Vec::new();

    for p in peers_info {
        let latency = if let Some(t) = telemetry {
            t.get_peer_latency(&p.node_id)
        } else {
            0.0
        };

        let sync_state = if !p.sync_enabled {
            "disabled".to_string()
        } else if let Some(t) = telemetry {
            let ratio = t.get_peer_agreement_ratio(&p.node_id);
            if ratio >= 0.9 {
                "synced".to_string()
            } else if ratio >= 0.5 {
                "syncing".to_string()
            } else {
                "lagging".to_string()
            }
        } else {
            "unknown".to_string()
        };

        peers.push(MeshPeerHealth {
            id: p.node_id.0.clone(),
            address: p.endpoint_url.clone(),
            latency,
            sync_state,
            version: "v1.0.0".to_string(), // Default mesh protocol version
        });
    }

    let (bytes_sent, bytes_received, messages_per_second) = if let Some(t) = telemetry {
        let total_messages = t.get_total_message_count();
        let uptime_secs = t.uptime().as_secs().max(1);
        let msg_rate = total_messages as f64 / uptime_secs as f64;
        (total_messages * 4096, total_messages * 2048, msg_rate)
    } else {
        (0, 0, 0.0)
    };

    MeshDashboardResponse {
        peers,
        maturity: MeshMaturityReport::default(),
        bandwidth: MeshBandwidth {
            bytes_sent,
            bytes_received,
            messages_per_second,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::node::NodeId;
    use std::collections::HashMap;

    #[test]
    fn test_dashboard_aggregation_empty() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("peers.json");
        let registry = PeerRegistry::load_from(path).unwrap();

        let dashboard = aggregate_dashboard(&registry, None);
        assert_eq!(dashboard.peers.len(), 0);
        assert_eq!(dashboard.bandwidth.bytes_sent, 0);
        assert_eq!(dashboard.bandwidth.bytes_received, 0);
        assert_eq!(dashboard.bandwidth.messages_per_second, 0.0);
    }

    #[test]
    fn test_dashboard_aggregation_with_data() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("peers.json");
        let mut registry = PeerRegistry::load_from(path).unwrap();

        let node_id = NodeId("node-1".to_string());
        let peer = PeerInfo {
            node_id: node_id.clone(),
            alias: Some("Peer One".to_string()),
            endpoint_url: "http://localhost:8001".to_string(),
            public_key_hex: "01020304".to_string(),
            added_at: 1000,
            last_seen_at: None,
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: HashMap::new(),
        };
        registry.add_peer(peer).unwrap();

        let telemetry = MeshTelemetryCollector::new();
        telemetry.record_latency(&node_id, 150);
        telemetry.record_agreement(&node_id, true);

        let dashboard = aggregate_dashboard(&registry, Some(&telemetry));
        assert_eq!(dashboard.peers.len(), 1);
        assert_eq!(dashboard.peers[0].id, "node-1");
        assert_eq!(dashboard.peers[0].address, "http://localhost:8001");
        assert_eq!(dashboard.peers[0].latency, 150.0);
        assert_eq!(dashboard.peers[0].sync_state, "synced");
        assert_eq!(dashboard.peers[0].version, "v1.0.0");

        assert_eq!(dashboard.bandwidth.bytes_sent, 4096);
        assert_eq!(dashboard.bandwidth.bytes_received, 2048);
        assert!(dashboard.bandwidth.messages_per_second > 0.0);
    }
}
