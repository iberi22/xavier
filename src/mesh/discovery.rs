// SPDX-License-Identifier: MIT OR LICENSE-MESH
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
    pub fn new() -> Self {
        Self {
            listen_addresses: vec!["/ip4/0.0.0.0/tcp/0"
                .parse()
                .expect("valid default listen address")],
            bootstrap_peers: Vec::new(),
            query_timeout: Duration::from_secs(60),
        }
    }

    pub fn with_listen_address(mut self, address: Multiaddr) -> Self {
        self.listen_addresses.push(address);
        self
    }

    pub fn with_bootstrap_peer(mut self, peer_id: PeerId, address: Multiaddr) -> Self {
        self.bootstrap_peers.push((peer_id, address));
        self
    }

    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.query_timeout = timeout;
        self
    }

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

    pub fn advertise_default(
        &self,
        swarm: &mut Swarm<kad::Behaviour<MemoryStore>>,
    ) -> Result<QueryId> {
        self.advertise(swarm, DEFAULT_DISCOVERY_KEY)
    }

    pub fn discover_peers(
        &self,
        swarm: &mut Swarm<kad::Behaviour<MemoryStore>>,
        discovery_key: impl AsRef<[u8]>,
    ) -> QueryId {
        swarm
            .behaviour_mut()
            .get_providers(RecordKey::new(&discovery_key))
    }

    pub fn discover_default(&self, swarm: &mut Swarm<kad::Behaviour<MemoryStore>>) -> QueryId {
        self.discover_peers(swarm, DEFAULT_DISCOVERY_KEY)
    }

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
