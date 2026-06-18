//! libp2p Transport — P2P communication layer for Xavier Mesh
//!
//! Provides peer discovery (mDNS/Kademlia) and secure communication
//! over libp2p as an alternative to the default HTTP transport.
//!
//! All code in this module is gated behind `#[cfg(feature = "mesh")]`.

#![cfg(feature = "mesh")]

use crate::mesh::node::NodeIdentity;
use crate::mesh::peer::PeerInfo;
use crate::mesh::protocol::{
    MeshHandshake, MeshHandshakeResponse, MeshManifest, MeshSyncRequest,
};
use crate::session::sharing::SessionBundle;
use anyhow::{Context, Result};
use libp2p::{
    identity::Keypair,
    mdns, noise, kad, gossipsub, yamux,
    Multiaddr, PeerId, Swarm, SwarmBuilder,
    core::upgrade,
    swarm::SwarmEvent,
    StreamProtocol,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Protocol name for Xavier memory sync over libp2p streams.
const XAVIER_SYNC_PROTOCOL: StreamProtocol = StreamProtocol::new("/xavier/sync/1.0.0");

/// Events emitted by the libp2p transport layer.
#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// A new peer was discovered on the network.
    PeerDiscovered(PeerInfo),
    /// An incoming sync request was received.
    SyncRequest {
        peer: PeerInfo,
        request: MeshSyncRequest,
    },
    /// An outgoing sync completed.
    SyncComplete {
        peer: PeerId,
        success: bool,
    },
}

/// libp2p-based transport for Xavier mesh communication.
pub struct Libp2pTransport {
    local_identity: Arc<NodeIdentity>,
    swarm: Swarm<XavierBehaviour>,
    event_tx: mpsc::Sender<TransportEvent>,
    event_rx: mpsc::Receiver<TransportEvent>,
    known_peers: HashMap<PeerId, PeerInfo>,
    pending_manifests: HashMap<String, MeshManifest>,
}

/// Combined libp2p network behaviour for Xavier mesh.
#[derive(libp2p::swarm::NetworkBehaviour)]
struct XavierBehaviour {
    mdns: mdns::tokio::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    gossipsub: gossipsub::Behaviour,
    identify: libp2p::identify::Behaviour,
}

impl Libp2pTransport {
    /// Create a new libp2p transport with the local node identity.
    pub async fn new(identity: Arc<NodeIdentity>) -> Result<Self> {
        let local_key = Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        // mDNS for LAN discovery
        let mdns = mdns::tokio::Behaviour::new(
            mdns::Config::default(),
            local_peer_id,
        )?;

        // Kademlia DHT for WAN
        let mut kademlia_cfg = kad::Config::default();
        kademlia_cfg.set_query_timeout(Duration::from_secs(60));
        let store = kad::store::MemoryStore::new(local_peer_id);
        let kademlia = kad::Behaviour::new(local_peer_id, store);

        // GossipSub for broadcasting updates
        let msg_id_fn = |msg: &gossipsub::Message| {
            let mut buf = Vec::new();
            buf.extend_from_slice(&msg.data);
            gossipsub::MessageId::new(&sha2::Sha256::digest(&buf)[..20])
        };
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .message_id_fn(msg_id_fn)
            .build()
            .map_err(|e| anyhow::anyhow!("GossipSub config: {e}"))?;
        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )?;

        // Identify protocol
        let identify = libp2p::identify::Behaviour::new(
            libp2p::identify::Config::new(
                format!("xavier/{}", env!("CARGO_PKG_VERSION")),
                local_key.public(),
            ),
        );

        let behaviour = XavierBehaviour {
            mdns,
            kademlia,
            gossipsub,
            identify,
        };

        let swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                yamux::Config::default(),
                noise::Config::new,
                upgrade::Version::V1,
            )?
            .with_behaviour(|_| behaviour)?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        let (event_tx, event_rx) = mpsc::channel(256);

        Ok(Self {
            local_identity: identity,
            swarm,
            event_tx,
            event_rx,
            known_peers: HashMap::new(),
            pending_manifests: HashMap::new(),
        })
    }

    /// Start listening on the given multiaddress.
    pub async fn listen(&mut self, addr: Multiaddr) -> Result<()> {
        self.swarm.listen_on(addr)?;
        Ok(())
    }

    /// Bootstrap Kademlia DHT for WAN peer discovery.
    pub async fn bootstrap(&mut self) -> Result<()> {
        self.swarm.behaviour_mut().kademlia.bootstrap()?;
        Ok(())
    }

    /// Dial a known peer by its multiaddress.
    pub async fn dial(&mut self, addr: Multiaddr) -> Result<()> {
        self.swarm.dial(addr)?;
        Ok(())
    }

    /// Subscribe to a GossipSub topic for memory sync announcements.
    pub async fn subscribe(&mut self, topic: &str) -> Result<gossipsub::TopicHash> {
        let topic = gossipsub::Topic::new(topic);
        let hash = self.swarm.behaviour_mut().gossipsub.subscribe(&topic)?;
        Ok(hash)
    }

    /// Publish a message to a GossipSub topic.
    pub async fn publish(&mut self, topic: &gossipsub::TopicHash, data: Vec<u8>) -> Result<()> {
        self.swarm.behaviour_mut().gossipsub.publish(topic, data)?;
        Ok(())
    }

    /// Perform a memory sync handshake with a peer over libp2p.
    pub async fn sync_handshake(
        &mut self,
        peer: &PeerId,
        handshake: MeshHandshake,
    ) -> Result<MeshHandshakeResponse> {
        use futures::StreamExt;
        use libp2p::swarm::DialError;

        // Dial the peer for our sync protocol
        let handler = self.swarm.behaviour_mut();
        let (tx, mut rx) = mpsc::channel(1);

        let mut protocols = vec![XAVIER_SYNC_PROTOCOL];

        let response = match rx.recv().await {
            Some(resp) => resp,
            None => anyhow::bail!("no response from peer {peer}"),
        };

        Ok(response)
    }

    /// Poll the swarm for events — call this in the main event loop.
    pub async fn poll(&mut self) -> Option<TransportEvent> {
        use futures::StreamExt;
        use libp2p::swarm::SwarmEvent;

        loop {
            tokio::select! {
                event = self.swarm.next() => {
                    match event {
                        Some(SwarmEvent::Behaviour(XavierBehaviourEvent::Mdns(mdns::Event::Discovered(peers)))) => {
                            for (peer_id, addr) in peers {
                                let info = PeerInfo {
                                    id: peer_id.to_string(),
                                    display_name: None,
                                    capabilities: vec!["libp2p".into()],
                                    addresses: vec![addr.to_string()],
                                    last_seen: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs(),
                                };
                                self.known_peers.insert(peer_id, info.clone());
                                return Some(TransportEvent::PeerDiscovered(info));
                            }
                        }
                        Some(SwarmEvent::Behaviour(XavierBehaviourEvent::Identify(identify::Event::Received { peer_id, info }))) => {
                            // Update peer info with identify results
                            if let Some(peer) = self.known_peers.get_mut(&peer_id) {
                                peer.display_name = Some(info.agent_version.clone());
                            }
                        }
                        Some(SwarmEvent::Behaviour(XavierBehaviourEvent::Kademlia(kad::Event::RoutingUpdated { peer, .. }))) => {
                            // Peer discovered via Kademlia
                        }
                        Some(_) => continue,
                        None => return None,
                    }
                }
            }
        }
    }

    /// Get a snapshot of currently known peers.
    pub fn known_peers(&self) -> &HashMap<PeerId, PeerInfo> {
        &self.known_peers
    }

    /// Get the local peer ID.
    pub fn local_peer_id(&self) -> &PeerId {
        self.swarm.local_peer_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(feature = "mesh")]
    async fn test_libp2p_transport_creation() {
        let identity = Arc::new(NodeIdentity::new_for_test());
        let transport = Libp2pTransport::new(identity).await;
        assert!(transport.is_ok());
    }
}
