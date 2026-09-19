//! Distributed routing mesh for relaying encrypted packets between NAT-traversed nodes.
//!
//! Maintains peer endpoints, calculates direct vs relay hops, and authorizes multi-hop
//! message relaying using cryptographic tickets.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use thiserror::Error;
use tokio::sync::RwLock;

use crate::telecom::protocol::TelecomPacket;

/// Maximum number of relay hops permitted to prevent routing loops.
pub const MAX_RELAY_HOPS: u8 = 5;

/// Errors arising during peer route resolution or relay dispatch.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TelecomRoutingError {
    #[error("No route found to destination node: '{0}'")]
    RouteNotFound(String),

    #[error("Exceeded maximum relay hops ({MAX_RELAY_HOPS})")]
    MaxHopsExceeded,

    #[error("Relay ticket expired or unauthorized")]
    InvalidRelayTicket,

    #[error("Endpoint unavailable: {0}")]
    EndpointUnavailable(String),
}

/// Known network transport address for a node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerEndpoint {
    pub node_id: String,
    pub socket_addr: Option<SocketAddr>,
    pub iroh_peer_id: Option<String>,
    pub last_seen: DateTime<Utc>,
    pub is_relay_node: bool,
}

/// Next routing hop evaluated for packet dispatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RouteHop {
    Direct(PeerEndpoint),
    Relay {
        relay_node: PeerEndpoint,
        destination_node_id: String,
        hop_count: u8,
    },
}

/// Authorization proof to allow intermediate node packet relay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayTicket {
    pub ticket_id: String,
    pub origin_node_id: String,
    pub target_node_id: String,
    pub authorized_relay_id: String,
    pub expires_at: i64,
}

/// In-memory routing table with peer discovery and multi-hop path resolution.
#[derive(Default)]
pub struct TelecomRoutingTable {
    peers: RwLock<HashMap<String, PeerEndpoint>>,
    relay_tickets: RwLock<HashMap<String, RelayTicket>>,
}

impl TelecomRoutingTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register or refresh a known peer endpoint.
    pub async fn register_peer(&self, endpoint: PeerEndpoint) {
        let mut map = self.peers.write().await;
        map.insert(endpoint.node_id.clone(), endpoint);
    }

    /// Issue an authorization ticket for packet relay.
    pub async fn issue_relay_ticket(&self, ticket: RelayTicket) {
        let mut map = self.relay_tickets.write().await;
        map.insert(ticket.ticket_id.clone(), ticket);
    }

    /// Resolve the best route hop (direct or relay) to reach a target node.
    pub async fn resolve_route(
        &self,
        destination_node_id: &str,
    ) -> Result<RouteHop, TelecomRoutingError> {
        let peers = self.peers.read().await;

        // 1. Direct route if peer is known and has an active address
        if let Some(target) = peers.get(destination_node_id) {
            if target.socket_addr.is_some() || target.iroh_peer_id.is_some() {
                return Ok(RouteHop::Direct(target.clone()));
            }
        }

        // 2. Relay route via any active relay node
        for peer in peers.values() {
            if peer.is_relay_node && peer.node_id != destination_node_id {
                return Ok(RouteHop::Relay {
                    relay_node: peer.clone(),
                    destination_node_id: destination_node_id.to_string(),
                    hop_count: 1,
                });
            }
        }

        Err(TelecomRoutingError::RouteNotFound(
            destination_node_id.to_string(),
        ))
    }

    /// Validate and forward a relayed packet towards destination.
    pub async fn relay_packet(
        &self,
        ticket_id: &str,
        packet: &TelecomPacket,
        current_hop: u8,
        now_secs: i64,
    ) -> Result<RouteHop, TelecomRoutingError> {
        if current_hop >= MAX_RELAY_HOPS {
            return Err(TelecomRoutingError::MaxHopsExceeded);
        }

        let tickets = self.relay_tickets.read().await;
        let ticket = tickets
            .get(ticket_id)
            .ok_or(TelecomRoutingError::InvalidRelayTicket)?;

        if ticket.expires_at <= now_secs || ticket.target_node_id != packet.header.recipient_id {
            return Err(TelecomRoutingError::InvalidRelayTicket);
        }

        self.resolve_route(&packet.header.recipient_id).await
    }

    /// Prune endpoints inactive for longer than max_age_seconds.
    pub async fn prune_stale_routes(&self, max_age_seconds: i64) -> usize {
        let mut map = self.peers.write().await;
        let now = Utc::now();
        let before_len = map.len();
        map.retain(|_, ep| (now - ep.last_seen).num_seconds() <= max_age_seconds);
        before_len - map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn make_test_endpoint(node_id: &str, is_relay: bool) -> PeerEndpoint {
        PeerEndpoint {
            node_id: node_id.to_string(),
            socket_addr: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8080,
            )),
            iroh_peer_id: None,
            last_seen: Utc::now(),
            is_relay_node: is_relay,
        }
    }

    #[tokio::test]
    async fn test_direct_route_resolution() {
        let table = TelecomRoutingTable::new();
        let target = make_test_endpoint("node-target", false);
        table.register_peer(target.clone()).await;

        let hop = table
            .resolve_route("node-target")
            .await
            .expect("route resolved");
        assert_eq!(hop, RouteHop::Direct(target));
    }

    #[tokio::test]
    async fn test_fallback_relay_route() {
        let table = TelecomRoutingTable::new();
        let relay = make_test_endpoint("relay-hub-1", true);
        table.register_peer(relay.clone()).await;

        // Destination is not directly registered
        let hop = table
            .resolve_route("node-hidden-nat")
            .await
            .expect("relay hop resolved");
        match hop {
            RouteHop::Relay {
                relay_node,
                destination_node_id,
                hop_count,
            } => {
                assert_eq!(relay_node.node_id, "relay-hub-1");
                assert_eq!(destination_node_id, "node-hidden-nat");
                assert_eq!(hop_count, 1);
            }
            _ => panic!("Expected relay hop"),
        }
    }

    #[tokio::test]
    async fn test_route_pruning() {
        let table = TelecomRoutingTable::new();
        let mut stale = make_test_endpoint("stale-node", false);
        stale.last_seen = Utc::now() - chrono::Duration::seconds(400);
        table.register_peer(stale).await;

        let pruned = table.prune_stale_routes(300).await;
        assert_eq!(pruned, 1);
        assert!(table.resolve_route("stale-node").await.is_err());
    }
}
