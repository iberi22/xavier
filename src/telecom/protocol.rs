//! Binary and structured wire protocol framing for the Xavier Telecom mesh.
//!
//! Provides multiplexing across four channel types (Control, DirectChat, Group, DataStream),
//! packet headers with versioning, sequence ordering, payload typing, and cryptographic checksum validation.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::telecom::crypto::EncryptedTelecomFrame;

/// Current wire protocol specification version.
pub const TELECOM_PROTOCOL_VERSION: u8 = 1;

/// Channel types supported by the telecom wire envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ChannelType {
    Control = 0x01,
    DirectChat = 0x02,
    Group = 0x03,
    DataStream = 0x04,
}

impl ChannelType {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x01 => Some(Self::Control),
            0x02 => Some(Self::DirectChat),
            0x03 => Some(Self::Group),
            0x04 => Some(Self::DataStream),
            _ => None,
        }
    }
}

/// Errors occurring during packet framing, serialization or validation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TelecomProtocolError {
    #[error("Invalid protocol version: expected {expected}, got {found}")]
    InvalidVersion { expected: u8, found: u8 },

    #[error("Corrupted packet checksum: calculated {calculated:08x}, found {found:08x}")]
    ChecksumMismatch { calculated: u32, found: u32 },

    #[error("Malformed packet envelope: {0}")]
    MalformedEnvelope(String),

    #[error("Payload decode error: {0}")]
    DecodeError(String),
}

/// Packet header containing routing, version, and integrity metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PacketHeader {
    /// Protocol format version
    pub version: u8,
    /// Channel multiplexing category
    pub channel_type: ChannelType,
    /// Unique message/packet sequence counter
    pub sequence: u64,
    /// Timestamp (UTC milliseconds)
    pub timestamp_ms: u64,
    /// Sender Node identifier (hex or DID)
    pub sender_node_id: String,
    /// Recipient Node or Group identifier
    pub recipient_id: String,
    /// Precomputed CRC32 checksum over the payload
    pub payload_checksum: u32,
}

/// Dynamic payload types supported over telecom channels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PacketPayload {
    /// Raw control signal (ping, pong, ack, close)
    ControlSignal {
        action: String,
        metadata: Option<String>,
    },
    /// Encrypted frame containing end-to-end ciphertext
    EncryptedFrame(EncryptedTelecomFrame),
    /// Streaming data chunk (e.g. document transfer)
    DataChunk {
        chunk_index: u32,
        total_chunks: u32,
        bytes: Vec<u8>,
    },
    /// Ephemeral heartbeat / status beacon
    Heartbeat { load_metric: u8 },
}

/// Top-level wire packet envelope for transmission over TCP, WebSockets, or Iroh QUIC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelecomPacket {
    pub header: PacketHeader,
    pub payload: PacketPayload,
}

impl TelecomPacket {
    /// Construct a new direct chat packet with automatic checksum calculation.
    pub fn new_chat_packet(
        sender_node_id: impl Into<String>,
        recipient_id: impl Into<String>,
        sequence: u64,
        timestamp_ms: u64,
        frame: EncryptedTelecomFrame,
    ) -> Result<Self, TelecomProtocolError> {
        let payload = PacketPayload::EncryptedFrame(frame);
        let payload_bytes = serde_json::to_vec(&payload)
            .map_err(|e| TelecomProtocolError::MalformedEnvelope(e.to_string()))?;
        let checksum = compute_checksum(&payload_bytes);

        Ok(Self {
            header: PacketHeader {
                version: TELECOM_PROTOCOL_VERSION,
                channel_type: ChannelType::DirectChat,
                sequence,
                timestamp_ms,
                sender_node_id: sender_node_id.into(),
                recipient_id: recipient_id.into(),
                payload_checksum: checksum,
            },
            payload,
        })
    }
}

/// Compute a fast 32-bit checksum (derived from SHA-256 prefix for determinism and security).
pub fn compute_checksum(data: &[u8]) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]])
}

/// Validate that a packet's computed checksum matches the header declaration.
pub fn validate_checksum(packet: &TelecomPacket) -> Result<(), TelecomProtocolError> {
    let payload_bytes = serde_json::to_vec(&packet.payload)
        .map_err(|e| TelecomProtocolError::MalformedEnvelope(e.to_string()))?;
    let calculated = compute_checksum(&payload_bytes);
    if calculated != packet.header.payload_checksum {
        return Err(TelecomProtocolError::ChecksumMismatch {
            calculated,
            found: packet.header.payload_checksum,
        });
    }
    Ok(())
}

/// Serialize a packet to compact JSON bytes (or wire buffer).
pub fn encode_packet(packet: &TelecomPacket) -> Result<Vec<u8>, TelecomProtocolError> {
    serde_json::to_vec(packet).map_err(|e| TelecomProtocolError::MalformedEnvelope(e.to_string()))
}

/// Deserialize and validate a packet from wire bytes.
pub fn decode_packet(bytes: &[u8]) -> Result<TelecomPacket, TelecomProtocolError> {
    let packet: TelecomPacket = serde_json::from_slice(bytes)
        .map_err(|e| TelecomProtocolError::DecodeError(e.to_string()))?;

    if packet.header.version != TELECOM_PROTOCOL_VERSION {
        return Err(TelecomProtocolError::InvalidVersion {
            expected: TELECOM_PROTOCOL_VERSION,
            found: packet.header.version,
        });
    }

    validate_checksum(&packet)?;

    Ok(packet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::NONCE_SIZE;

    fn sample_frame() -> EncryptedTelecomFrame {
        EncryptedTelecomFrame {
            nonce: [1u8; NONCE_SIZE],
            ciphertext: vec![10, 20, 30, 40],
            sequence: 42,
            channel_id: "test-room".to_string(),
        }
    }

    #[test]
    fn test_packet_encode_decode_roundtrip() {
        let frame = sample_frame();
        let packet = TelecomPacket::new_chat_packet(
            "node-alpha",
            "node-beta",
            101,
            1710000000,
            frame.clone(),
        )
        .expect("packet creation ok");

        let encoded = encode_packet(&packet).expect("encode ok");
        let decoded = decode_packet(&encoded).expect("decode ok");

        assert_eq!(decoded.header.version, TELECOM_PROTOCOL_VERSION);
        assert_eq!(decoded.header.channel_type, ChannelType::DirectChat);
        assert_eq!(decoded.header.sender_node_id, "node-alpha");
        assert_eq!(decoded.header.recipient_id, "node-beta");
        assert_eq!(decoded.payload, PacketPayload::EncryptedFrame(frame));
    }

    #[test]
    fn test_corrupted_checksum_rejection() {
        let frame = sample_frame();
        let mut packet =
            TelecomPacket::new_chat_packet("node-alpha", "node-beta", 1, 1710000000, frame)
                .unwrap();

        // Corrupt checksum
        packet.header.payload_checksum ^= 0xFFFFFFFF;
        let encoded = encode_packet(&packet).unwrap();

        let err = decode_packet(&encoded);
        assert!(matches!(
            err,
            Err(TelecomProtocolError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn test_channel_type_u8_mapping() {
        assert_eq!(ChannelType::from_u8(0x01), Some(ChannelType::Control));
        assert_eq!(ChannelType::from_u8(0x02), Some(ChannelType::DirectChat));
        assert_eq!(ChannelType::from_u8(0x03), Some(ChannelType::Group));
        assert_eq!(ChannelType::from_u8(0x04), Some(ChannelType::DataStream));
        assert_eq!(ChannelType::from_u8(0xFF), None);
    }
}
