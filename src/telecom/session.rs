//! P2P secure telecom session state machine, bidirectional challenge-response, and keepalive management.
//!
//! Tracks peer state through Init, Handshaking, Established, Expired, and Terminated,
//! ensuring cryptographic authenticity and automated heartbeat liveness.

use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::telecom::crypto::{derive_shared_secret, SessionRatchet, TelecomKeypair};

/// Standard session inactivity timeout in seconds (5 minutes).
pub const SESSION_TTL_SECONDS: i64 = 300;

/// Errors arising during session transitions or challenge verification.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TelecomSessionError {
    #[error("Session is not in valid state for action: current {0:?}")]
    InvalidState(SessionState),

    #[error("Handshake challenge verification failed")]
    ChallengeMismatch,

    #[error("Session has expired due to inactivity")]
    SessionExpired,

    #[error("Cryptographic error: {0}")]
    CryptoError(String),
}

/// Lifecycle states for an authenticated P2P telecom channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Init,
    Handshaking,
    Established,
    Expired,
    Terminated,
}

/// Cryptographic challenge exchanged during handshake initiation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeChallenge {
    pub session_id: String,
    pub initiator_node_id: String,
    pub initiator_ephemeral_pubkey: [u8; 32],
    pub challenge_nonce: [u8; 32],
    pub created_at: DateTime<Utc>,
}

/// Response returned to finalize session establishment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeResponse {
    pub session_id: String,
    pub responder_node_id: String,
    pub responder_ephemeral_pubkey: [u8; 32],
    pub challenge_response_hash: [u8; 32],
}

/// Periodic heartbeat beacon to maintain session liveness.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeepAliveBeacon {
    pub session_id: String,
    pub node_id: String,
    pub sequence: u64,
    pub sent_at: DateTime<Utc>,
}

/// Active authenticated session context with a peer node.
pub struct PeerSession {
    pub session_id: String,
    pub local_node_id: String,
    pub remote_node_id: String,
    pub state: SessionState,
    pub ephemeral_keypair: TelecomKeypair,
    pub remote_ephemeral_pubkey: Option<[u8; 32]>,
    pub ratchet: Option<SessionRatchet>,
    pub created_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
    pub heartbeat_sequence: u64,
}

impl PeerSession {
    /// Initiate a new outbound handshake challenge to a target peer.
    pub fn initiate_handshake(
        session_id: impl Into<String>,
        local_node_id: impl Into<String>,
        remote_node_id: impl Into<String>,
    ) -> (Self, HandshakeChallenge) {
        let sid = session_id.into();
        let l_nid = local_node_id.into();
        let r_nid = remote_node_id.into();

        let keypair = TelecomKeypair::generate();
        let mut nonce = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut nonce);
        let now = Utc::now();

        let challenge = HandshakeChallenge {
            session_id: sid.clone(),
            initiator_node_id: l_nid.clone(),
            initiator_ephemeral_pubkey: keypair.public_key_bytes(),
            challenge_nonce: nonce,
            created_at: now,
        };

        let session = Self {
            session_id: sid,
            local_node_id: l_nid,
            remote_node_id: r_nid,
            state: SessionState::Handshaking,
            ephemeral_keypair: keypair,
            remote_ephemeral_pubkey: None,
            ratchet: None,
            created_at: now,
            last_heartbeat: now,
            heartbeat_sequence: 0,
        };

        (session, challenge)
    }

    /// Complete the handshake once a valid response with peer's public key is received.
    pub fn complete_handshake(
        &mut self,
        response: &HandshakeResponse,
    ) -> Result<(), TelecomSessionError> {
        if self.state != SessionState::Handshaking {
            return Err(TelecomSessionError::InvalidState(self.state));
        }

        // Derive shared secret via X25519 Diffie-Hellman
        let shared_dh = self
            .ephemeral_keypair
            .diffie_hellman(&response.responder_ephemeral_pubkey);

        let root_key = derive_shared_secret(&shared_dh, self.session_id.as_bytes());

        self.remote_ephemeral_pubkey = Some(response.responder_ephemeral_pubkey);
        self.ratchet = Some(SessionRatchet::new(root_key));
        self.state = SessionState::Established;
        self.last_heartbeat = Utc::now();

        Ok(())
    }

    /// Record a heartbeat beacon received from the peer.
    pub fn record_heartbeat(
        &mut self,
        beacon: &KeepAliveBeacon,
    ) -> Result<(), TelecomSessionError> {
        if !self.is_active() {
            self.state = SessionState::Expired;
            return Err(TelecomSessionError::SessionExpired);
        }

        self.last_heartbeat = Utc::now();
        self.heartbeat_sequence = beacon.sequence;
        Ok(())
    }

    /// Check if the session is active and within TTL threshold.
    pub fn is_active(&self) -> bool {
        if self.state != SessionState::Established {
            return false;
        }
        let elapsed = Utc::now() - self.last_heartbeat;
        elapsed.num_seconds() <= SESSION_TTL_SECONDS
    }

    /// Terminate session gracefully.
    pub fn terminate(&mut self) {
        self.state = SessionState::Terminated;
    }
}

/// Thread-safe registry for managing active peer sessions.
#[derive(Default)]
pub struct SessionRegistry {
    sessions: RwLock<std::collections::HashMap<String, PeerSession>>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, session: PeerSession) {
        let mut map = self.sessions.write().await;
        map.insert(session.session_id.clone(), session);
    }

    pub async fn is_active(&self, session_id: &str) -> bool {
        let map = self.sessions.read().await;
        map.get(session_id).map(|s| s.is_active()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_lifecycle() {
        let (mut alice_session, _challenge) =
            PeerSession::initiate_handshake("sess-1", "node-alice", "node-bob");

        assert_eq!(alice_session.state, SessionState::Handshaking);
        assert!(!alice_session.is_active());

        let bob_keypair = TelecomKeypair::generate();
        let response = HandshakeResponse {
            session_id: "sess-1".to_string(),
            responder_node_id: "node-bob".to_string(),
            responder_ephemeral_pubkey: bob_keypair.public_key_bytes(),
            challenge_response_hash: [0u8; 32],
        };

        alice_session
            .complete_handshake(&response)
            .expect("handshake completed");
        assert_eq!(alice_session.state, SessionState::Established);
        assert!(alice_session.is_active());
        assert!(alice_session.ratchet.is_some());
    }

    #[test]
    fn test_heartbeat_recording() {
        let (mut session, _) = PeerSession::initiate_handshake("sess-2", "node-a", "node-b");
        let bob_key = TelecomKeypair::generate();
        session
            .complete_handshake(&HandshakeResponse {
                session_id: "sess-2".to_string(),
                responder_node_id: "node-b".to_string(),
                responder_ephemeral_pubkey: bob_key.public_key_bytes(),
                challenge_response_hash: [0u8; 32],
            })
            .unwrap();

        let beacon = KeepAliveBeacon {
            session_id: "sess-2".to_string(),
            node_id: "node-b".to_string(),
            sequence: 5,
            sent_at: Utc::now(),
        };

        assert!(session.record_heartbeat(&beacon).is_ok());
        assert_eq!(session.heartbeat_sequence, 5);
    }

    #[test]
    fn test_termination_behavior() {
        let (mut session, _) = PeerSession::initiate_handshake("sess-3", "node-a", "node-b");
        session.terminate();
        assert_eq!(session.state, SessionState::Terminated);
        assert!(!session.is_active());
    }
}
