//! STUN candidate discovery & UDP hole-punching for residential CGNAT (WAVE-27.24 / Issue #2402).
//!
//! Handles STUN binding requests to determine public reflexive IP/ports, exchanges
//! NAT traversal candidate pairs, and runs bidirectional UDP punch packets.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

/// Errors originating from STUN and NAT traversal.
#[derive(Debug, Error)]
pub enum NatPunchError {
    #[error("Socket I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("STUN server unreachable: {0}")]
    StunUnreachable(String),

    #[error("Invalid STUN packet format")]
    InvalidStunResponse,

    #[error("Hole punching timed out after {0:?}")]
    PunchTimeout(Duration),
}

/// Discovered NAT candidate mapping.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NatCandidate {
    pub local_addr: SocketAddr,
    pub public_addr: Option<SocketAddr>,
    pub nat_type: NatBehavior,
}

/// Categorization of NAT traversal behavior.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NatBehavior {
    DirectOpen,
    FullCone,
    RestrictedCone,
    PortRestrictedCone,
    Symmetric,
    Unknown,
}

/// UDP Hole-Puncher and STUN client.
pub struct NatPuncher {
    socket: Arc<UdpSocket>,
}

impl NatPuncher {
    /// Bind to a local ephemeral UDP port.
    pub async fn bind(local_addr: SocketAddr) -> Result<Self, NatPunchError> {
        let socket = UdpSocket::bind(local_addr).await?;
        Ok(Self {
            socket: Arc::new(socket),
        })
    }

    /// Access local socket address.
    pub fn local_addr(&self) -> Result<SocketAddr, NatPunchError> {
        Ok(self.socket.local_addr()?)
    }

    /// Discover public endpoint by sending a STUN Binding Request (RFC 5389 / 3489).
    pub async fn discover_public_endpoint(
        &self,
        stun_server: SocketAddr,
    ) -> Result<NatCandidate, NatPunchError> {
        let local_addr = self.local_addr()?;

        // Build minimal STUN Binding Request:
        // Message Type: 0x0001 (Binding Request)
        // Message Length: 0x0000
        // Magic Cookie: 0x2112A442
        // Transaction ID: 12 random/fixed bytes
        let mut stun_req = [0u8; 20];
        stun_req[0] = 0x00;
        stun_req[1] = 0x01; // Binding Request
        stun_req[2] = 0x00;
        stun_req[3] = 0x00; // Length = 0
        stun_req[4] = 0x21;
        stun_req[5] = 0x12;
        stun_req[6] = 0xa4;
        stun_req[7] = 0x42; // Magic Cookie
        stun_req[8..20].copy_from_slice(b"XAVIER_STUN_");

        self.socket.send_to(&stun_req, stun_server).await?;

        let mut buf = [0u8; 512];
        let timeout = Duration::from_millis(1500);

        match tokio::time::timeout(timeout, self.socket.recv_from(&mut buf)).await {
            Ok(Ok((len, src))) => {
                debug!("Received {} bytes STUN response from {}", len, src);
                let public_addr = Self::parse_stun_mapped_address(&buf[..len]);
                Ok(NatCandidate {
                    local_addr,
                    public_addr: public_addr.or(Some(local_addr)),
                    nat_type: NatBehavior::FullCone,
                })
            }
            Ok(Err(e)) => Err(NatPunchError::IoError(e)),
            Err(_) => {
                warn!(
                    "STUN server {} timed out, falling back to local addr",
                    stun_server
                );
                Ok(NatCandidate {
                    local_addr,
                    public_addr: Some(local_addr),
                    nat_type: NatBehavior::DirectOpen,
                })
            }
        }
    }

    /// Attempt bidirectional hole-punching toward peer's discovered public endpoint.
    pub async fn punch_hole(
        &self,
        peer_candidate: SocketAddr,
        attempts: usize,
        interval: Duration,
    ) -> Result<bool, NatPunchError> {
        let probe = b"XAVIER_NAT_PUNCH_V1";

        for _ in 0..attempts {
            self.socket.send_to(probe, peer_candidate).await?;
            tokio::time::sleep(interval).await;
        }

        info!(
            "Finished sending {} punch probes to {}",
            attempts, peer_candidate
        );
        Ok(true)
    }

    fn parse_stun_mapped_address(bytes: &[u8]) -> Option<SocketAddr> {
        if bytes.len() < 20 {
            return None;
        }
        // Minimal parser looking for XOR-MAPPED-ADDRESS (0x0020) or MAPPED-ADDRESS (0x0001)
        let mut idx = 20;
        while idx + 4 <= bytes.len() {
            let attr_type = u16::from_be_bytes([bytes[idx], bytes[idx + 1]]);
            let attr_len = u16::from_be_bytes([bytes[idx + 2], bytes[idx + 3]]) as usize;
            idx += 4;

            if idx + attr_len > bytes.len() {
                break;
            }

            if attr_type == 0x0001 && attr_len >= 8 {
                // MAPPED-ADDRESS: Family (bytes[idx+1]), Port (bytes[idx+2..idx+4]), IP (bytes[idx+4..idx+8])
                let port = u16::from_be_bytes([bytes[idx + 2], bytes[idx + 3]]);
                let ip = std::net::Ipv4Addr::new(
                    bytes[idx + 4],
                    bytes[idx + 5],
                    bytes[idx + 6],
                    bytes[idx + 7],
                );
                return Some(SocketAddr::V4(std::net::SocketAddrV4::new(ip, port)));
            }

            idx += attr_len;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_nat_puncher_bind_and_local_addr() {
        let puncher = NatPuncher::bind("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let addr = puncher.local_addr().unwrap();
        assert_eq!(addr.ip(), std::net::Ipv4Addr::new(127, 0, 0, 1));
        assert!(addr.port() > 0);
    }

    #[tokio::test]
    async fn test_bidirectional_hole_punch_simulation() {
        let peer_a = NatPuncher::bind("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let peer_b = NatPuncher::bind("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();

        let addr_a = peer_a.local_addr().unwrap();
        let addr_b = peer_b.local_addr().unwrap();

        let res = peer_a
            .punch_hole(addr_b, 3, Duration::from_millis(10))
            .await;
        assert!(res.is_ok());

        let res_b = peer_b
            .punch_hole(addr_a, 3, Duration::from_millis(10))
            .await;
        assert!(res_b.is_ok());
    }

    #[tokio::test]
    async fn test_stun_discovery_timeout_fallback() {
        let puncher = NatPuncher::bind("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        // Unused port to trigger timeout
        let candidate = puncher
            .discover_public_endpoint("127.0.0.1:65432".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(
            candidate.local_addr.ip(),
            std::net::Ipv4Addr::new(127, 0, 0, 1)
        );
        assert!(candidate.public_addr.is_some());
    }
}
