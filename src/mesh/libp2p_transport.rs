//! libp2p Transport — P2P communication layer for Xavier Mesh
//!
//! Provides peer discovery (mDNS/Kademlia) and secure communication
//! over libp2p as an alternative to the default HTTP transport.
//!
//! All code in this module is gated behind `#[cfg(feature = "mesh")]`.

#![cfg(feature = "mesh")]

pub use crate::mesh::transport::libp2p::{
    Libp2pTransport, XavierBehaviour, XavierBehaviourEvent, TransportEvent,
};
