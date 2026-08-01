//! libp2p Transport — P2P communication layer for Xavier Mesh
//!
//! Provides peer discovery (mDNS/Kademlia) and secure communication
//! over libp2p as an alternative to the default HTTP transport.
//!
//! All code in this module is gated behind `#[cfg(feature = "mesh")]`.

#![cfg(feature = "mesh")]

pub mod discovery;

use crate::mesh::node::NodeIdentity;
use crate::mesh::peer::PeerInfo;
use crate::mesh::protocol::{MeshHandshake, MeshHandshakeResponse, MeshManifest};
use anyhow::{Context, Result};
use chrono::Utc;
use futures_util::StreamExt;
use libp2p::{
    gossipsub, identify, kad, mdns, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, PeerId, Swarm, SwarmBuilder, Transport,
};
use sha2::Digest;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Events emitted by the libp2p transport layer.
#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// A new peer was discovered on the network.
    PeerDiscovered(PeerInfo),
    /// Outgoing sync completed.
    SyncComplete { peer: PeerId, success: bool },
}

/// Combined libp2p network behaviour for resilient Xavier mesh.
#[derive(NetworkBehaviour)]
pub struct XavierBehaviour {
    pub discovery: crate::mesh::transport::libp2p::discovery::DiscoveryBehaviour,
    pub gossipsub: gossipsub::Behaviour,
    pub identify: identify::Behaviour,
    pub ping: ping::Behaviour,
    pub relay_client: relay::client::Behaviour,
}

/// libp2p-based transport for resilient Xavier mesh communication.
pub struct Libp2pTransport {
    pub local_identity: Arc<NodeIdentity>,
    pub swarm: Swarm<XavierBehaviour>,
    pub event_tx: mpsc::Sender<TransportEvent>,
    pub event_rx: mpsc::Receiver<TransportEvent>,
    pub known_peers: HashMap<PeerId, PeerInfo>,
    pub pending_manifests: HashMap<String, MeshManifest>,
    pub missed_pings: HashMap<PeerId, u32>,

    // Metrics tracking
    pub active_connections: std::sync::atomic::AtomicUsize,
    pub latency_ms: std::sync::atomic::AtomicU64,
    pub bytes_sent: std::sync::atomic::AtomicU64,
    pub bytes_received: std::sync::atomic::AtomicU64,

    // Peer address tracking for reconnection
    pub peer_addresses:
        std::sync::Arc<parking_lot::Mutex<std::collections::HashMap<PeerId, Multiaddr>>>,

    // Reconnection tracking
    pub reconnect_attempts: std::sync::Arc<
        parking_lot::Mutex<std::collections::HashMap<PeerId, (u32, std::time::Instant)>>,
    >,
    pub dial_queue_tx: mpsc::Sender<(PeerId, Multiaddr)>,
    pub dial_queue_rx: mpsc::Receiver<(PeerId, Multiaddr)>,
}

impl Libp2pTransport {
    /// Create a new upgraded and resilient libp2p transport layer.
    pub async fn new(identity: Arc<NodeIdentity>) -> Result<Self> {
        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        // mDNS for LAN discovery
        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?;

        // Kademlia DHT for WAN peer discovery
        let store = kad::store::MemoryStore::new(local_peer_id);
        let mut kademlia_cfg = kad::Config::default();
        kademlia_cfg.set_query_timeout(Duration::from_secs(60));
        let kademlia = kad::Behaviour::with_config(local_peer_id, store, kademlia_cfg);

        let discovery =
            crate::mesh::transport::libp2p::discovery::DiscoveryBehaviour { mdns, kademlia };

        // Gossipsub
        let msg_id_fn = |msg: &gossipsub::Message| {
            let mut buf = Vec::new();
            buf.extend_from_slice(&msg.data);
            gossipsub::MessageId::new(&sha2::Sha256::digest(&buf)[..20])
        };
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .message_id_fn(msg_id_fn)
            .build()
            .map_err(|e| anyhow::anyhow!("Gossipsub config: {e}"))?;
        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )
        .map_err(|e| anyhow::anyhow!(e))?;

        // Identify
        let identify = identify::Behaviour::new(identify::Config::new(
            format!("xavier/{}", env!("CARGO_PKG_VERSION")),
            local_key.public(),
        ));

        // Ping for latency metrics, configured to ping every 30 seconds
        let ping_cfg = ping::Config::default().with_interval(Duration::from_secs(30));
        let ping = ping::Behaviour::new(ping_cfg);

        // Relay Client for NAT traversal
        let (relay_transport, relay_client) = relay::client::new(local_peer_id);

        let behaviour = XavierBehaviour {
            discovery,
            gossipsub,
            identify,
            ping,
            relay_client,
        };

        // TCP + Relay Transport Setup
        let tcp_transport = libp2p::tcp::tokio::Transport::default();
        let transport = tcp_transport
            .or_transport(relay_transport)
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(libp2p::noise::Config::new(&local_key)?)
            .multiplex(libp2p::yamux::Config::default())
            .boxed();

        let swarm_config = libp2p::swarm::Config::with_tokio_executor()
            .with_idle_connection_timeout(Duration::from_secs(60));

        let swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_other_transport(|_key| Ok(transport))?
            .with_behaviour(|_| behaviour)?
            .with_swarm_config(|_| swarm_config)
            .build();

        let (event_tx, event_rx) = mpsc::channel(256);
        let (dial_queue_tx, dial_queue_rx) = mpsc::channel(256);

        Ok(Self {
            local_identity: identity,
            swarm,
            event_tx,
            event_rx,
            known_peers: HashMap::new(),
            pending_manifests: HashMap::new(),
            missed_pings: HashMap::new(),
            active_connections: std::sync::atomic::AtomicUsize::new(0),
            latency_ms: std::sync::atomic::AtomicU64::new(0),
            bytes_sent: std::sync::atomic::AtomicU64::new(0),
            bytes_received: std::sync::atomic::AtomicU64::new(0),
            peer_addresses: std::sync::Arc::new(parking_lot::Mutex::new(
                std::collections::HashMap::new(),
            )),
            reconnect_attempts: std::sync::Arc::new(parking_lot::Mutex::new(
                std::collections::HashMap::new(),
            )),
            dial_queue_tx,
            dial_queue_rx,
        })
    }

    /// Start listening on the given multiaddress.
    pub async fn listen(&mut self, addr: Multiaddr) -> Result<()> {
        self.swarm.listen_on(addr)?;
        Ok(())
    }

    /// Bootstrap Kademlia DHT for WAN peer discovery.
    pub async fn bootstrap(&mut self) -> Result<()> {
        self.swarm.behaviour_mut().discovery.kademlia.bootstrap()?;
        Ok(())
    }

    /// Add a bootstrap peer to the Kademlia routing table.
    pub fn add_bootstrap_node(&mut self, peer_id: PeerId, addr: Multiaddr) {
        self.swarm
            .behaviour_mut()
            .discovery
            .kademlia
            .add_address(&peer_id, addr);
    }

    /// Dial a known peer by its multiaddress.
    pub async fn dial(&mut self, addr: Multiaddr) -> Result<()> {
        if let Some(peer_id) = addr.iter().find_map(|p| match p {
            libp2p::multiaddr::Protocol::P2p(peer_id) => Some(peer_id),
            _ => None,
        }) {
            let mut map = self.peer_addresses.lock();
            map.insert(peer_id, addr.clone());
        }
        self.swarm.dial(addr)?;
        Ok(())
    }

    /// Subscribe to a GossipSub topic for memory sync announcements.
    pub async fn subscribe(&mut self, topic: &str) -> Result<gossipsub::TopicHash> {
        let topic = gossipsub::IdentTopic::new(topic);
        let topic_hash = topic.hash();
        let _ = self.swarm.behaviour_mut().gossipsub.subscribe(&topic)?;
        Ok(topic_hash)
    }

    /// Publish a message to a GossipSub topic.
    pub async fn publish(&mut self, topic: &gossipsub::TopicHash, data: Vec<u8>) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(topic.clone(), data)?;
        Ok(())
    }

    /// Perform a memory sync handshake with a peer over libp2p.
    pub async fn sync_handshake(
        &mut self,
        _peer: &PeerId,
        _handshake: MeshHandshake,
    ) -> Result<MeshHandshakeResponse> {
        Err(anyhow::anyhow!("sync_handshake protocol not implemented"))
    }

    /// Poll the swarm for events — call this in the main event loop.
    pub async fn poll(&mut self) -> Option<TransportEvent> {
        loop {
            tokio::select! {
                dial_req = self.dial_queue_rx.recv() => {
                    if let Some((_peer_id, addr)) = dial_req {
                        let _ = self.swarm.dial(addr);
                    }
                }
                event = self.swarm.next() => {
                    match event {
                        Some(SwarmEvent::ConnectionEstablished { peer_id, .. }) => {
                            self.active_connections.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            let mut attempts = self.reconnect_attempts.lock();
                            attempts.remove(&peer_id);
                        }
                        Some(SwarmEvent::ConnectionClosed { .. }) => {
                            let val = self.active_connections.load(std::sync::atomic::Ordering::SeqCst);
                            if val > 0 {
                                self.active_connections.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                            }
                        }
                        Some(SwarmEvent::OutgoingConnectionError { peer_id, .. }) => {
                            if let Some(peer_id) = peer_id {
                                let attempts = {
                                    let mut attempts_map = self.reconnect_attempts.lock();
                                    let entry = attempts_map.entry(peer_id).or_insert((0, std::time::Instant::now()));
                                    entry.0 += 1;
                                    entry.1 = std::time::Instant::now();
                                    entry.0
                                };

                                if attempts <= 3 {
                                    let base_delay = Duration::from_millis(50);
                                    let delay = base_delay * 2u32.pow(attempts - 1);
                                    let tx = self.dial_queue_tx.clone();

                                    let addr_opt = {
                                        let map = self.peer_addresses.lock();
                                        map.get(&peer_id).cloned()
                                    };

                                    if let Some(addr) = addr_opt {
                                        tokio::spawn(async move {
                                            tokio::time::sleep(delay).await;
                                            let _ = tx.send((peer_id, addr)).await;
                                        });
                                    }
                                }
                            }
                        }
                        Some(SwarmEvent::Behaviour(XavierBehaviourEvent::Discovery(discovery_event))) => {
                            match discovery_event {
                                crate::mesh::transport::libp2p::discovery::DiscoveryBehaviourEvent::Mdns(mdns::Event::Discovered(peers)) => {
                                    for (peer_id, addr) in peers {
                                        // Auto-add to Kademlia so they can discover each other on WAN/DHT
                                        self.swarm.behaviour_mut().discovery.kademlia.add_address(&peer_id, addr.clone());

                                        let info = PeerInfo {
                                            node_id: crate::mesh::NodeId(peer_id.to_string()),
                                            alias: None,
                                            endpoint_url: addr.to_string(),
                                            public_key_hex: String::new(),
                                            added_at: Utc::now().timestamp(),
                                            last_seen_at: Some(Utc::now().timestamp()),
                                            sync_enabled: true,
                                            is_cloud: false,
                                            iroh_addr: None,
                                            shared_workspace_ids: vec![],
                                            shared_workspace_tokens: HashMap::new(),
                                        };
                                        self.known_peers.insert(peer_id, info.clone());
                                        return Some(TransportEvent::PeerDiscovered(info));
                                    }
                                }
                                crate::mesh::transport::libp2p::discovery::DiscoveryBehaviourEvent::Kademlia(kad::Event::RoutingUpdated { peer, addresses, .. }) => {
                                    let addr_str = addresses.iter().next().map(|a| a.to_string()).unwrap_or_default();
                                    let info = PeerInfo {
                                        node_id: crate::mesh::NodeId(peer.to_string()),
                                        alias: None,
                                        endpoint_url: addr_str,
                                        public_key_hex: String::new(),
                                        added_at: Utc::now().timestamp(),
                                        last_seen_at: Some(Utc::now().timestamp()),
                                        sync_enabled: true,
                                        is_cloud: false,
                                        iroh_addr: None,
                                        shared_workspace_ids: vec![],
                                        shared_workspace_tokens: HashMap::new(),
                                    };
                                    self.known_peers.insert(peer, info.clone());
                                    return Some(TransportEvent::PeerDiscovered(info));
                                }
                                _ => {}
                            }
                        }
                        Some(SwarmEvent::Behaviour(XavierBehaviourEvent::Identify(libp2p::identify::Event::Received { peer_id, info, .. }))) => {
                            if let Some(peer) = self.known_peers.get_mut(&peer_id) {
                                peer.alias = Some(info.agent_version.clone());
                            }
                        }
                        Some(SwarmEvent::Behaviour(XavierBehaviourEvent::Ping(ping::Event { peer, result, .. }))) => {
                            match result {
                                Ok(rtt) => {
                                    self.latency_ms.store(rtt.as_millis() as u64, std::sync::atomic::Ordering::SeqCst);
                                    self.missed_pings.insert(peer, 0);
                                    if let Some(peer_info) = self.known_peers.get_mut(&peer) {
                                        peer_info.last_seen_at = Some(Utc::now().timestamp());
                                    }
                                }
                                Err(_) => {
                                    let count = self.missed_pings.entry(peer).or_insert(0);
                                    *count += 1;
                                    if *count >= 3 {
                                        self.known_peers.remove(&peer);
                                        self.swarm.behaviour_mut().discovery.kademlia.remove_peer(&peer);
                                        tracing::warn!(peer = %peer, "Peer marked unhealthy and removed after 3 missed pings");
                                    }
                                }
                            }
                        }
                        Some(_) => continue,
                        None => return None,
                    }
                }
            }
        }
    }

    /// Retrieve number of active connections.
    pub fn active_connections(&self) -> usize {
        self.active_connections
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Retrieve recorded ping latency (RTT) in milliseconds.
    pub fn latency_ms(&self) -> u64 {
        self.latency_ms.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Retrieve cumulative throughput (bytes sent, bytes received).
    pub fn throughput(&self) -> (u64, u64) {
        (
            self.bytes_sent.load(std::sync::atomic::Ordering::SeqCst),
            self.bytes_received
                .load(std::sync::atomic::Ordering::SeqCst),
        )
    }

    /// Record bytes sent.
    pub fn record_sent_bytes(&self, bytes: u64) {
        self.bytes_sent
            .fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }

    /// Record bytes received.
    pub fn record_received_bytes(&self, bytes: u64) {
        self.bytes_received
            .fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
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
