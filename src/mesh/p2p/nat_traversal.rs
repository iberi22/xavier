//! STUN/TURN NAT Traversal Engine and ICE Candidate Negotiation for Xavier Mesh.
//!
//! Provides deterministic NAT type discovery, RFC 5389 / RFC 8445 STUN/TURN parsing,
//! ICE candidate gathering, candidate pairing, and UDP/TCP hole punching state machines.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::net::UdpSocket;

/// Magic Cookie constant defined in RFC 5389 (0x2112A442)
pub const STUN_MAGIC_COOKIE: u32 = 0x2112A442;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum NatTraversalError {
    #[error("Packet too short: expected at least {expected} bytes, got {got}")]
    PacketTooShort { expected: usize, got: usize },

    #[error("Invalid STUN magic cookie: expected 0x2112A442, got {0:#010x}")]
    InvalidMagicCookie(u32),

    #[error("Unknown STUN message type: {0:#06x}")]
    UnknownMessageType(u16),

    #[error("Invalid attribute length or format for type {0:#06x}")]
    InvalidAttributeFormat(u16),

    #[error("Socket I/O error: {0}")]
    IoError(String),

    #[error("STUN request timeout after {0:?}")]
    Timeout(Duration),

    #[error("TURN allocation failed: {0}")]
    TurnAllocationFailed(String),

    #[error("No valid candidate pairs found")]
    NoValidCandidatePairs,
}

/// STUN Message Types according to RFC 5389 / RFC 8656 (TURN)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StunMessageType {
    BindingRequest,
    BindingResponse,
    BindingErrorResponse,
    TurnAllocateRequest,
    TurnAllocateResponse,
    TurnSendIndication,
    TurnDataIndication,
    Custom(u16),
}

impl StunMessageType {
    pub fn to_u16(self) -> u16 {
        match self {
            StunMessageType::BindingRequest => 0x0001,
            StunMessageType::BindingResponse => 0x0101,
            StunMessageType::BindingErrorResponse => 0x0111,
            StunMessageType::TurnAllocateRequest => 0x0003,
            StunMessageType::TurnAllocateResponse => 0x0103,
            StunMessageType::TurnSendIndication => 0x0016,
            StunMessageType::TurnDataIndication => 0x0017,
            StunMessageType::Custom(val) => val,
        }
    }

    pub fn from_u16(val: u16) -> Self {
        match val {
            0x0001 => StunMessageType::BindingRequest,
            0x0101 => StunMessageType::BindingResponse,
            0x0111 => StunMessageType::BindingErrorResponse,
            0x0003 => StunMessageType::TurnAllocateRequest,
            0x0103 => StunMessageType::TurnAllocateResponse,
            0x0016 => StunMessageType::TurnSendIndication,
            0x0017 => StunMessageType::TurnDataIndication,
            other => StunMessageType::Custom(other),
        }
    }
}

/// STUN Attributes parsed or constructed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StunAttribute {
    MappedAddress(SocketAddr),
    XorMappedAddress(SocketAddr),
    XorRelayedAddress(SocketAddr),
    XorPeerAddress(SocketAddr),
    Username(String),
    ErrorCode { code: u16, reason: String },
    Unknown { attr_type: u16, value: Vec<u8> },
}

/// Representation of a parsed or crafted STUN Message
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StunMessage {
    pub message_type: StunMessageType,
    pub transaction_id: [u8; 12],
    pub attributes: Vec<StunAttribute>,
}

impl StunMessage {
    /// Creates a new STUN Binding Request with a given 96-bit transaction ID
    pub fn create_binding_request(transaction_id: [u8; 12]) -> Self {
        Self {
            message_type: StunMessageType::BindingRequest,
            transaction_id,
            attributes: Vec::new(),
        }
    }

    /// Encodes the STUN message into binary wire format according to RFC 5389
    pub fn encode(&self) -> Vec<u8> {
        let mut attr_bytes = Vec::new();

        for attr in &self.attributes {
            match attr {
                StunAttribute::MappedAddress(addr) => {
                    encode_mapped_address_attr(&mut attr_bytes, 0x0001, addr);
                }
                StunAttribute::XorMappedAddress(addr) => {
                    encode_xor_mapped_address_attr(
                        &mut attr_bytes,
                        0x0020,
                        addr,
                        &self.transaction_id,
                    );
                }
                StunAttribute::XorRelayedAddress(addr) => {
                    encode_xor_mapped_address_attr(
                        &mut attr_bytes,
                        0x0116,
                        addr,
                        &self.transaction_id,
                    );
                }
                StunAttribute::XorPeerAddress(addr) => {
                    encode_xor_mapped_address_attr(
                        &mut attr_bytes,
                        0x0125,
                        addr,
                        &self.transaction_id,
                    );
                }
                StunAttribute::Username(user) => {
                    let u_bytes = user.as_bytes();
                    let len = u_bytes.len() as u16;
                    attr_bytes.extend_from_slice(&0x0006u16.to_be_bytes());
                    attr_bytes.extend_from_slice(&len.to_be_bytes());
                    attr_bytes.extend_from_slice(u_bytes);
                    // Padding to 4-byte boundary
                    let pad = (4 - (len as usize % 4)) % 4;
                    attr_bytes.extend_from_slice(&vec![0u8; pad]);
                }
                StunAttribute::ErrorCode { code, reason } => {
                    let r_bytes = reason.as_bytes();
                    let len = (4 + r_bytes.len()) as u16;
                    attr_bytes.extend_from_slice(&0x0009u16.to_be_bytes());
                    attr_bytes.extend_from_slice(&len.to_be_bytes());
                    attr_bytes.extend_from_slice(&[
                        0u8,
                        0u8,
                        (code / 100) as u8,
                        (code % 100) as u8,
                    ]);
                    attr_bytes.extend_from_slice(r_bytes);
                    let pad = (4 - (len as usize % 4)) % 4;
                    attr_bytes.extend_from_slice(&vec![0u8; pad]);
                }
                StunAttribute::Unknown { attr_type, value } => {
                    let len = value.len() as u16;
                    attr_bytes.extend_from_slice(&attr_type.to_be_bytes());
                    attr_bytes.extend_from_slice(&len.to_be_bytes());
                    attr_bytes.extend_from_slice(value);
                    let pad = (4 - (len as usize % 4)) % 4;
                    attr_bytes.extend_from_slice(&vec![0u8; pad]);
                }
            }
        }

        let mut header = Vec::with_capacity(20 + attr_bytes.len());
        let msg_type = self.message_type.to_u16();
        let msg_len = attr_bytes.len() as u16;

        header.extend_from_slice(&msg_type.to_be_bytes());
        header.extend_from_slice(&msg_len.to_be_bytes());
        header.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        header.extend_from_slice(&self.transaction_id);
        header.extend_from_slice(&attr_bytes);

        header
    }

    /// Parses a raw binary payload into a `StunMessage`
    pub fn parse(buf: &[u8]) -> Result<Self, NatTraversalError> {
        if buf.len() < 20 {
            return Err(NatTraversalError::PacketTooShort {
                expected: 20,
                got: buf.len(),
            });
        }

        let msg_type_raw = u16::from_be_bytes([buf[0], buf[1]]);
        let msg_len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
        let magic_cookie = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);

        if magic_cookie != STUN_MAGIC_COOKIE {
            return Err(NatTraversalError::InvalidMagicCookie(magic_cookie));
        }

        if buf.len() < 20 + msg_len {
            return Err(NatTraversalError::PacketTooShort {
                expected: 20 + msg_len,
                got: buf.len(),
            });
        }

        let mut transaction_id = [0u8; 12];
        transaction_id.copy_from_slice(&buf[8..20]);

        let message_type = StunMessageType::from_u16(msg_type_raw);
        let mut attributes = Vec::new();
        let mut offset = 20;
        let end = 20 + msg_len;

        while offset < end {
            if offset + 4 > end {
                break;
            }

            let attr_type = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
            let attr_len = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]) as usize;
            offset += 4;

            if offset + attr_len > end {
                return Err(NatTraversalError::InvalidAttributeFormat(attr_type));
            }

            let attr_data = &buf[offset..offset + attr_len];

            let attr = match attr_type {
                0x0001 => {
                    // MAPPED-ADDRESS
                    parse_mapped_address_attr(attr_data)?.map(StunAttribute::MappedAddress)
                }
                0x0020 => {
                    // XOR-MAPPED-ADDRESS
                    parse_xor_mapped_address_attr(attr_data, &transaction_id)?
                        .map(StunAttribute::XorMappedAddress)
                }
                0x0116 => {
                    // XOR-RELAYED-ADDRESS
                    parse_xor_mapped_address_attr(attr_data, &transaction_id)?
                        .map(StunAttribute::XorRelayedAddress)
                }
                0x0125 => {
                    // XOR-PEER-ADDRESS
                    parse_xor_mapped_address_attr(attr_data, &transaction_id)?
                        .map(StunAttribute::XorPeerAddress)
                }
                0x0006 => {
                    // USERNAME
                    let username = String::from_utf8_lossy(attr_data).to_string();
                    Some(StunAttribute::Username(username))
                }
                0x0009 => {
                    // ERROR-CODE
                    if attr_data.len() >= 4 {
                        let class = attr_data[2] & 0x07;
                        let number = attr_data[3];
                        let code = (class as u16) * 100 + (number as u16);
                        let reason = String::from_utf8_lossy(&attr_data[4..]).to_string();
                        Some(StunAttribute::ErrorCode { code, reason })
                    } else {
                        None
                    }
                }
                _ => Some(StunAttribute::Unknown {
                    attr_type,
                    value: attr_data.to_vec(),
                }),
            };

            if let Some(a) = attr {
                attributes.push(a);
            }

            // Padding alignment to 4-byte boundary
            let pad = (4 - (attr_len % 4)) % 4;
            offset += attr_len + pad;
        }

        Ok(Self {
            message_type,
            transaction_id,
            attributes,
        })
    }

    /// Extracts reflexive address if XOR-MAPPED-ADDRESS or MAPPED-ADDRESS is present
    pub fn get_reflexive_address(&self) -> Option<SocketAddr> {
        for attr in &self.attributes {
            match attr {
                StunAttribute::XorMappedAddress(addr) => return Some(*addr),
                StunAttribute::MappedAddress(addr) => return Some(*addr),
                _ => {}
            }
        }
        None
    }
}

fn encode_mapped_address_attr(buf: &mut Vec<u8>, attr_type: u16, addr: &SocketAddr) {
    let (family, addr_bytes): (u8, Vec<u8>) = match addr.ip() {
        IpAddr::V4(v4) => (0x01, v4.octets().to_vec()),
        IpAddr::V6(v6) => (0x02, v6.octets().to_vec()),
    };

    let len = (4 + addr_bytes.len()) as u16;
    buf.extend_from_slice(&attr_type.to_be_bytes());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.push(0x00); // reserved
    buf.push(family);
    buf.extend_from_slice(&addr.port().to_be_bytes());
    buf.extend_from_slice(&addr_bytes);
}

fn encode_xor_mapped_address_attr(
    buf: &mut Vec<u8>,
    attr_type: u16,
    addr: &SocketAddr,
    transaction_id: &[u8; 12],
) {
    let port = addr.port() ^ ((STUN_MAGIC_COOKIE >> 16) as u16);

    let (family, addr_bytes): (u8, Vec<u8>) = match addr.ip() {
        IpAddr::V4(v4) => {
            let x_ip = u32::from_be_bytes(v4.octets()) ^ STUN_MAGIC_COOKIE;
            (0x01, x_ip.to_be_bytes().to_vec())
        }
        IpAddr::V6(v6) => {
            let mut key = Vec::with_capacity(16);
            key.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
            key.extend_from_slice(transaction_id);

            let orig = v6.octets();
            let mut x_ip = [0u8; 16];
            for i in 0..16 {
                x_ip[i] = orig[i] ^ key[i];
            }
            (0x02, x_ip.to_vec())
        }
    };

    let len = (4 + addr_bytes.len()) as u16;
    buf.extend_from_slice(&attr_type.to_be_bytes());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.push(0x00); // reserved
    buf.push(family);
    buf.extend_from_slice(&port.to_be_bytes());
    buf.extend_from_slice(&addr_bytes);
}

fn parse_mapped_address_attr(data: &[u8]) -> Result<Option<SocketAddr>, NatTraversalError> {
    if data.len() < 4 {
        return Ok(None);
    }

    let family = data[1];
    let port = u16::from_be_bytes([data[2], data[3]]);

    if family == 0x01 && data.len() >= 8 {
        let ip = Ipv4Addr::new(data[4], data[5], data[6], data[7]);
        Ok(Some(SocketAddr::new(IpAddr::V4(ip), port)))
    } else if family == 0x02 && data.len() >= 20 {
        let mut v6_bytes = [0u8; 16];
        v6_bytes.copy_from_slice(&data[4..20]);
        let ip = Ipv6Addr::from(v6_bytes);
        Ok(Some(SocketAddr::new(IpAddr::V6(ip), port)))
    } else {
        Ok(None)
    }
}

fn parse_xor_mapped_address_attr(
    data: &[u8],
    transaction_id: &[u8; 12],
) -> Result<Option<SocketAddr>, NatTraversalError> {
    if data.len() < 4 {
        return Ok(None);
    }

    let family = data[1];
    let raw_port = u16::from_be_bytes([data[2], data[3]]);
    let port = raw_port ^ ((STUN_MAGIC_COOKIE >> 16) as u16);

    if family == 0x01 && data.len() >= 8 {
        let raw_ip = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ip_val = raw_ip ^ STUN_MAGIC_COOKIE;
        let ip = Ipv4Addr::from(ip_val);
        Ok(Some(SocketAddr::new(IpAddr::V4(ip), port)))
    } else if family == 0x02 && data.len() >= 20 {
        let mut key = Vec::with_capacity(16);
        key.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        key.extend_from_slice(transaction_id);

        let mut ip_bytes = [0u8; 16];
        for i in 0..16 {
            ip_bytes[i] = data[4 + i] ^ key[i];
        }
        let ip = Ipv6Addr::from(ip_bytes);
        Ok(Some(SocketAddr::new(IpAddr::V6(ip), port)))
    } else {
        Ok(None)
    }
}

/// Transport protocol used by ICE candidates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportProtocol {
    Udp,
    Tcp,
}

/// ICE Candidate Types according to RFC 8445
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IceCandidateType {
    Host,
    ServerReflexive,
    PeerReflexive,
    Relayed,
}

impl IceCandidateType {
    pub fn preference(self) -> u32 {
        match self {
            IceCandidateType::Host => 126,
            IceCandidateType::PeerReflexive => 110,
            IceCandidateType::ServerReflexive => 100,
            IceCandidateType::Relayed => 0,
        }
    }
}

/// Represents an ICE candidate for connection establishment
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IceCandidate {
    pub foundation: String,
    pub component_id: u32,
    pub protocol: TransportProtocol,
    pub priority: u64,
    pub addr: SocketAddr,
    pub candidate_type: IceCandidateType,
    pub rel_addr: Option<SocketAddr>,
}

impl IceCandidate {
    pub fn new(
        foundation: impl Into<String>,
        component_id: u32,
        protocol: TransportProtocol,
        addr: SocketAddr,
        candidate_type: IceCandidateType,
        rel_addr: Option<SocketAddr>,
    ) -> Self {
        let type_pref = candidate_type.preference() as u64;
        let local_pref = 65535u64; // High default preference
        let comp_id = component_id as u64;
        // RFC 8445 priority formula: (2^24)*type_pref + (2^8)*local_pref + (2^0)*(256 - comp_id)
        let priority = (1 << 24) * type_pref + (1 << 8) * local_pref + (256 - comp_id);

        Self {
            foundation: foundation.into(),
            component_id,
            protocol,
            priority,
            addr,
            candidate_type,
            rel_addr,
        }
    }
}

/// Candidate pair state in ICE connectivity checks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidatePairState {
    Waiting,
    InProgress,
    Succeeded,
    Failed,
    Frozen,
}

/// ICE Candidate Pair
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidatePair {
    pub local: IceCandidate,
    pub remote: IceCandidate,
    pub priority: u64,
    pub state: CandidatePairState,
}

impl CandidatePair {
    pub fn new(local: IceCandidate, remote: IceCandidate, controlling: bool) -> Self {
        let g = if controlling {
            local.priority
        } else {
            remote.priority
        };
        let a = if controlling {
            remote.priority
        } else {
            local.priority
        };
        // RFC 8445 priority formula for candidate pair
        let min_ga = std::cmp::min(g, a);
        let max_ga = std::cmp::max(g, a);
        let priority = (1u64 << 32) * min_ga + 2 * max_ga + if g > a { 1 } else { 0 };

        Self {
            local,
            remote,
            priority,
            state: CandidatePairState::Waiting,
        }
    }
}

/// Classification of NAT Type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NatType {
    OpenInternet,
    FullCone,
    RestrictedCone,
    PortRestrictedCone,
    Symmetric,
    Unknown,
}

/// Overall Hole Punching & Candidate Negotiation State Machine
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HolePunchState {
    Idle,
    GatheringCandidates,
    PairingCandidates,
    ConnectivityCheck,
    Connected { active_pair: CandidatePair },
    Failed { reason: String },
}

/// TURN Server Configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnServerConfig {
    pub server_addr: SocketAddr,
    pub username: String,
    pub credential: String,
}

/// Primary NAT Traversal Engine for SWAL Mesh P2P
#[derive(Debug)]
pub struct NatTraversalEngine {
    local_addrs: Vec<SocketAddr>,
    stun_servers: Vec<SocketAddr>,
    turn_servers: Vec<TurnServerConfig>,
    gathered_candidates: Vec<IceCandidate>,
    remote_candidates: Vec<IceCandidate>,
    candidate_pairs: Vec<CandidatePair>,
    state: HolePunchState,
    nat_type: NatType,
}

impl NatTraversalEngine {
    pub fn new(stun_servers: Vec<SocketAddr>, turn_servers: Vec<TurnServerConfig>) -> Self {
        Self {
            local_addrs: Vec::new(),
            stun_servers,
            turn_servers,
            gathered_candidates: Vec::new(),
            remote_candidates: Vec::new(),
            candidate_pairs: Vec::new(),
            state: HolePunchState::Idle,
            nat_type: NatType::Unknown,
        }
    }

    pub fn add_local_address(&mut self, addr: SocketAddr) {
        if !self.local_addrs.contains(&addr) {
            self.local_addrs.push(addr);
        }
    }

    pub fn set_nat_type(&mut self, nat_type: NatType) {
        self.nat_type = nat_type;
    }

    pub fn nat_type(&self) -> NatType {
        self.nat_type
    }

    pub fn state(&self) -> &HolePunchState {
        &self.state
    }

    /// Gather local Host candidates
    pub fn gather_host_candidates(&mut self) -> Vec<IceCandidate> {
        self.state = HolePunchState::GatheringCandidates;
        let mut candidates = Vec::new();

        for (idx, addr) in self.local_addrs.iter().enumerate() {
            let candidate = IceCandidate::new(
                format!("host_{}", idx),
                1,
                TransportProtocol::Udp,
                *addr,
                IceCandidateType::Host,
                None,
            );
            candidates.push(candidate);
        }

        self.gathered_candidates.extend(candidates.clone());
        candidates
    }

    /// Send STUN request over a TokIo UDP socket to discover reflexive candidate
    pub async fn discover_server_reflexive_candidate(
        &mut self,
        socket: &UdpSocket,
        stun_server: SocketAddr,
        timeout: Duration,
    ) -> Result<IceCandidate, NatTraversalError> {
        let transaction_id = rand_transaction_id();
        let req = StunMessage::create_binding_request(transaction_id);
        let bytes = req.encode();

        socket
            .send_to(&bytes, stun_server)
            .await
            .map_err(|e| NatTraversalError::IoError(e.to_string()))?;

        let mut buf = [0u8; 1024];
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err(NatTraversalError::Timeout(timeout));
            }

            let recv_res = tokio::time::timeout(
                timeout.saturating_sub(start.elapsed()),
                socket.recv_from(&mut buf),
            )
            .await;

            match recv_res {
                Ok(Ok((len, _src))) => {
                    if let Ok(msg) = StunMessage::parse(&buf[..len]) {
                        if msg.transaction_id == transaction_id {
                            if let Some(reflexive_addr) = msg.get_reflexive_address() {
                                let local_addr = socket
                                    .local_addr()
                                    .map_err(|e| NatTraversalError::IoError(e.to_string()))?;

                                let candidate = IceCandidate::new(
                                    "srflx_1".to_string(),
                                    1,
                                    TransportProtocol::Udp,
                                    reflexive_addr,
                                    IceCandidateType::ServerReflexive,
                                    Some(local_addr),
                                );

                                self.gathered_candidates.push(candidate.clone());
                                return Ok(candidate);
                            }
                        }
                    }
                }
                Ok(Err(e)) => return Err(NatTraversalError::IoError(e.to_string())),
                Err(_) => return Err(NatTraversalError::Timeout(timeout)),
            }
        }
    }

    /// Add a candidate received from a remote peer
    pub fn add_remote_candidate(&mut self, candidate: IceCandidate) {
        if !self.remote_candidates.contains(&candidate) {
            self.remote_candidates.push(candidate);
        }
    }

    /// Form candidate pairs between gathered local candidates and remote candidates
    pub fn form_candidate_pairs(&mut self, controlling: bool) -> Vec<CandidatePair> {
        self.state = HolePunchState::PairingCandidates;
        let mut pairs = Vec::new();

        for local in &self.gathered_candidates {
            for remote in &self.remote_candidates {
                // Match transport protocol and IP family (IPv4 with IPv4, IPv6 with IPv6)
                if local.protocol == remote.protocol
                    && local.addr.is_ipv4() == remote.addr.is_ipv4()
                {
                    let pair = CandidatePair::new(local.clone(), remote.clone(), controlling);
                    pairs.push(pair);
                }
            }
        }

        // Sort pairs by descending priority
        pairs.sort_by_key(|p| std::cmp::Reverse(p.priority));
        self.candidate_pairs = pairs.clone();
        pairs
    }

    pub fn candidate_pairs(&self) -> &[CandidatePair] {
        &self.candidate_pairs
    }

    pub fn select_best_pair(&self) -> Option<CandidatePair> {
        self.candidate_pairs.first().cloned()
    }

    /// Perform direct UDP hole punching sequence to remote candidate address
    pub async fn perform_hole_punch(
        &mut self,
        socket: &UdpSocket,
        pair: &CandidatePair,
        rounds: usize,
        interval: Duration,
    ) -> Result<CandidatePair, NatTraversalError> {
        self.state = HolePunchState::ConnectivityCheck;
        let remote_addr = pair.remote.addr;
        let transaction_id = rand_transaction_id();
        let ping_msg = StunMessage::create_binding_request(transaction_id);
        let ping_bytes = ping_msg.encode();

        let mut buf = [0u8; 1024];

        for _round in 0..rounds {
            // Send outbound STUN packet to punch NAT entry
            let _ = socket.send_to(&ping_bytes, remote_addr).await;

            // Wait briefly for incoming ping/ack
            let recv_res = tokio::time::timeout(interval, socket.recv_from(&mut buf)).await;

            if let Ok(Ok((len, src))) = recv_res {
                if src == remote_addr {
                    if let Ok(_msg) = StunMessage::parse(&buf[..len]) {
                        let mut succeeded_pair = pair.clone();
                        succeeded_pair.state = CandidatePairState::Succeeded;
                        self.state = HolePunchState::Connected {
                            active_pair: succeeded_pair.clone(),
                        };
                        return Ok(succeeded_pair);
                    }
                }
            }
        }

        self.state = HolePunchState::Failed {
            reason: format!("No response from remote candidate {}", remote_addr),
        };
        Err(NatTraversalError::NoValidCandidatePairs)
    }

    /// Determines NAT type based on reflexive responses across multiple STUN servers
    pub fn determine_nat_type_from_responses(
        local_port: u16,
        reflexive_addrs: &[SocketAddr],
    ) -> NatType {
        if reflexive_addrs.is_empty() {
            return NatType::Unknown;
        }

        let first = reflexive_addrs[0];

        // If reflexive address equals local port & IP (no NAT intervention)
        if first.port() == local_port && is_public_ip(&first.ip()) {
            return NatType::OpenInternet;
        }

        // Check if all STUN servers see the same mapped IP and Port
        let all_same = reflexive_addrs.iter().all(|addr| *addr == first);

        if all_same {
            // Same mapping regardless of destination server -> Cone NAT
            NatType::FullCone
        } else {
            // Different mapped address/port per server -> Symmetric NAT
            NatType::Symmetric
        }
    }
}

fn is_public_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !(v4.is_private() || v4.is_loopback() || v4.is_link_local()),
        IpAddr::V6(v6) => !(v6.is_loopback()),
    }
}

fn rand_transaction_id() -> [u8; 12] {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let mut id = [0u8; 12];
    let nanos_bytes = nanos.to_be_bytes();
    for i in 0..12 {
        let offset = ((i * 37) % 256) as u8;
        id[i] = nanos_bytes[i % nanos_bytes.len()].wrapping_add(offset);
    }
    id
}
