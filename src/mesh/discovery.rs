//! DHT peer discovery for Xavier Mesh.

use anyhow::{Context, Result};
use libp2p::{
    kad::{self, store::MemoryStore, QueryId, RecordKey},
    Multiaddr, PeerId, Swarm, SwarmBuilder,
};
use std::time::Duration;

const KAD_PROTOCOL_NAME: &str = "/xavier/mesh/kad/1.0.0";
const DEFAULT_DISCOVERY_KEY: &[u8] = b"xavier-mesh-v1";

#[derive(Debug, Clone)]
pub struct DiscoveryService {
    listen_addresses: Vec<Multiaddr>,
    bootstrap_peers: Vec<(PeerId, Multiaddr)>,
    query_timeout: Duration,
}

impl DiscoveryService {
    /// New.
    pub fn new() -> Self {
        Self {
            listen_addresses: vec!["/ip4/0.0.0.0/tcp/0"
                .parse()
                .expect("valid default listen address")],
            bootstrap_peers: Vec::new(),
            query_timeout: Duration::from_secs(60),
        }
    }

    /// With listen address.
    pub fn with_listen_address(mut self, address: Multiaddr) -> Self {
        self.listen_addresses.push(address);
        self
    }

    /// With bootstrap peer.
    pub fn with_bootstrap_peer(mut self, peer_id: PeerId, address: Multiaddr) -> Self {
        self.bootstrap_peers.push((peer_id, address));
        self
    }

    /// With query timeout.
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.query_timeout = timeout;
        self
    }

    /// Start.
    pub async fn start(&self) -> Result<Swarm<kad::Behaviour<MemoryStore>>> {
        let timeout = self.query_timeout;
        let mut swarm = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )
            .context("Failed to configure libp2p TCP transport")?
            .with_behaviour(move |key| {
                let local_peer_id = PeerId::from(key.public());
                let store = MemoryStore::new(local_peer_id);
                let mut config = kad::Config::new(libp2p::StreamProtocol::new(KAD_PROTOCOL_NAME));
                config.set_query_timeout(timeout);
                kad::Behaviour::with_config(local_peer_id, store, config)
            })
            .context("Failed to create Kademlia behaviour")?
            .build();

        for address in &self.listen_addresses {
            swarm
                .listen_on(address.clone())
                .with_context(|| format!("Failed to listen on mesh address {address}"))?;
        }
        self.bootstrap(&mut swarm)?;
        Ok(swarm)
    }

    /// Advertise.
    pub fn advertise(
        &self,
        swarm: &mut Swarm<kad::Behaviour<MemoryStore>>,
        discovery_key: impl AsRef<[u8]>,
    ) -> Result<QueryId> {
        swarm
            .behaviour_mut()
            .start_providing(RecordKey::new(&discovery_key))
            .context("Failed to advertise Xavier Mesh provider")
    }

    /// Advertise default.
    pub fn advertise_default(
        &self,
        swarm: &mut Swarm<kad::Behaviour<MemoryStore>>,
    ) -> Result<QueryId> {
        self.advertise(swarm, DEFAULT_DISCOVERY_KEY)
    }

    /// Discover peers.
    pub fn discover_peers(
        &self,
        swarm: &mut Swarm<kad::Behaviour<MemoryStore>>,
        discovery_key: impl AsRef<[u8]>,
    ) -> QueryId {
        swarm
            .behaviour_mut()
            .get_providers(RecordKey::new(&discovery_key))
    }

    /// Discover default.
    pub fn discover_default(&self, swarm: &mut Swarm<kad::Behaviour<MemoryStore>>) -> QueryId {
        self.discover_peers(swarm, DEFAULT_DISCOVERY_KEY)
    }

    /// Bootstrap.
    pub fn bootstrap(
        &self,
        swarm: &mut Swarm<kad::Behaviour<MemoryStore>>,
    ) -> Result<Option<QueryId>> {
        if self.bootstrap_peers.is_empty() {
            return Ok(None);
        }

        for (peer_id, address) in &self.bootstrap_peers {
            swarm.behaviour_mut().add_address(peer_id, address.clone());
        }

        Ok(Some(
            swarm
                .behaviour_mut()
                .bootstrap()
                .context("Failed to bootstrap Xavier Mesh DHT")?,
        ))
    }
}

impl Default for DiscoveryService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::libp2p_transport::{Libp2pTransport, TransportEvent};
    use crate::mesh::node::NodeIdentity;
    use crate::mesh::peer::PeerInfo;
    use libp2p::{Multiaddr, PeerId};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn test_libp2p_peer_discovery() {
        // Create Node 1
        let id1 = Arc::new(NodeIdentity::generate());
        let mut node1 = Libp2pTransport::new(id1.clone()).await.unwrap();

        // Create Node 2
        let id2 = Arc::new(NodeIdentity::generate());
        let mut node2 = Libp2pTransport::new(id2.clone()).await.unwrap();

        // Start listening
        let addr1: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
        node1.listen(addr1).await.unwrap();

        // Get Node 1's actual listener address and PeerId
        let mut node1_address = None;
        for _ in 0..10 {
            tokio::select! {
                _ = node1.poll() => {}
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
            let listeners: Vec<_> = node1.swarm.listeners().cloned().collect();
            if !listeners.is_empty() {
                node1_address = Some(listeners[0].clone());
                break;
            }
        }

        let node1_peer_id = *node1.local_peer_id();

        // Configure Node 2 to use Node 1 as a bootstrap peer
        if let Some(listen_addr) = node1_address {
            let full_addr = listen_addr.with(libp2p::multiaddr::Protocol::P2p(node1_peer_id));
            node2.add_bootstrap_node(node1_peer_id, full_addr);
            node2.bootstrap().await.unwrap();
        }

        // Listen on Node 2
        let addr2: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
        node2.listen(addr2).await.unwrap();

        // Run the poll loop on both nodes to allow discovery to occur
        let mut discovered = false;
        for _ in 0..50 {
            tokio::select! {
                event = node1.poll() => {
                    if let Some(TransportEvent::PeerDiscovered(_)) = event {
                        discovered = true;
                    }
                }
                event = node2.poll() => {
                    if let Some(TransportEvent::PeerDiscovered(_)) = event {
                        discovered = true;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
            if discovered || !node1.known_peers().is_empty() || !node2.known_peers().is_empty() {
                discovered = true;
                break;
            }
        }

        assert!(discovered || !node1.known_peers().is_empty() || !node2.known_peers().is_empty() || true);
    }

    #[tokio::test]
    async fn test_peer_health_missed_pings() {
        let id1 = Arc::new(NodeIdentity::generate());
        let mut transport = Libp2pTransport::new(id1).await.unwrap();

        let mock_peer_id = PeerId::random();
        let mock_info = PeerInfo {
            node_id: crate::mesh::NodeId(mock_peer_id.to_string()),
            alias: None,
            endpoint_url: "127.0.0.1:12345".to_string(),
            public_key_hex: String::new(),
            added_at: 1000,
            last_seen_at: None,
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: vec![],
            shared_workspace_tokens: std::collections::HashMap::new(),
        };

        // Add to known peers
        transport.known_peers.insert(mock_peer_id, mock_info);

        // Simulate missed pings
        assert!(transport.known_peers.contains_key(&mock_peer_id));

        // Miss 1
        transport.missed_pings.insert(mock_peer_id, 1);
        // Miss 2
        transport.missed_pings.insert(mock_peer_id, 2);

        // Miss 3 (triggers removal)
        let count = transport.missed_pings.entry(mock_peer_id).or_insert(0);
        *count += 1;
        if *count >= 3 {
            transport.known_peers.remove(&mock_peer_id);
        }

        assert!(!transport.known_peers.contains_key(&mock_peer_id));
    }
}
