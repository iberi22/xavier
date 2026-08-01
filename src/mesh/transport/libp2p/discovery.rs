//! Combined peer discovery behaviour using mDNS for LAN and Kademlia DHT for WAN.
//!
//! Gated behind the `#[cfg(feature = "mesh")]` compiler flag.

#![cfg(feature = "mesh")]

use libp2p::{kad, mdns, swarm::NetworkBehaviour};

/// Combined peer discovery behaviour combining local mDNS and WAN Kademlia DHT.
#[derive(NetworkBehaviour)]
pub struct DiscoveryBehaviour {
    pub mdns: mdns::tokio::Behaviour,
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
}
