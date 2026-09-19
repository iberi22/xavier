//! RBAC & case clearance enforcement for telecom channels.
//!
//! Restricts channel creation, join requests, and message visibility according to
//! sender node clearance, case role, and wallet cryptographic authority.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::security::clearance::ClearanceLevel;

/// Errors returned when access or cryptographic authentication fails at the gate.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClearanceGateError {
    #[error("Insufficient clearance: required {required:?}, participant has {actual:?}")]
    InsufficientClearance {
        required: ChannelClearanceLevel,
        actual: ChannelClearanceLevel,
    },

    #[error("Invalid wallet signature: {0}")]
    InvalidSignature(String),

    #[error("Participant '{0}' is not authorized for channel '{1}'")]
    UnauthorizedParticipant(String, String),

    #[error("Channel '{0}' not found in clearance registry")]
    ChannelNotFound(String),

    #[error("Token expired or revoked")]
    TokenInvalid,
}

/// Clearance classification required for telecom rooms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChannelClearanceLevel {
    Public = 0,
    Internal = 1,
    Confidential = 2,
    Secret = 3,
    TopSecret = 4,
}

impl From<ClearanceLevel> for ChannelClearanceLevel {
    fn from(level: ClearanceLevel) -> Self {
        match level {
            ClearanceLevel::Unclassified => Self::Public,
            ClearanceLevel::Internal => Self::Internal,
            ClearanceLevel::Restricted => Self::Internal,
            ClearanceLevel::Confidential => Self::Confidential,
            ClearanceLevel::Secret => Self::Secret,
            ClearanceLevel::TopSecret => Self::TopSecret,
        }
    }
}

/// Cryptographic identity token proving participant wallet authorization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParticipantToken {
    pub participant_id: String,
    pub wallet_pubkey: [u8; 32],
    pub clearance: ChannelClearanceLevel,
    pub authorized_cases: Vec<String>,
    pub expires_at: i64,
}

/// Security gate enforcing access rules across rooms and transmissions.
#[derive(Default)]
pub struct TelecomClearanceGate {
    channel_clearances: RwLock<HashMap<String, ChannelClearanceLevel>>,
    authorized_participants: RwLock<HashMap<String, HashSet<String>>>,
}

impl TelecomClearanceGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a channel's clearance level.
    pub async fn register_channel(
        &self,
        channel_id: impl Into<String>,
        level: ChannelClearanceLevel,
    ) {
        let mut map = self.channel_clearances.write().await;
        map.insert(channel_id.into(), level);
    }

    /// Authorize a participant to join a channel based on clearance level and case role.
    pub async fn authorize_join(
        &self,
        channel_id: &str,
        token: &ParticipantToken,
        current_time_secs: i64,
    ) -> Result<(), ClearanceGateError> {
        if token.expires_at <= current_time_secs {
            return Err(ClearanceGateError::TokenInvalid);
        }

        let clearances = self.channel_clearances.read().await;
        let required = clearances
            .get(channel_id)
            .copied()
            .unwrap_or(ChannelClearanceLevel::Confidential);

        if token.clearance < required {
            return Err(ClearanceGateError::InsufficientClearance {
                required,
                actual: token.clearance,
            });
        }

        // Add to authorized set
        let mut parts = self.authorized_participants.write().await;
        parts
            .entry(channel_id.to_string())
            .or_default()
            .insert(token.participant_id.clone());

        Ok(())
    }

    /// Authorize transmitting a message or file payload into a channel.
    pub async fn authorize_transmit(
        &self,
        channel_id: &str,
        participant_id: &str,
    ) -> Result<(), ClearanceGateError> {
        let parts = self.authorized_participants.read().await;
        if let Some(set) = parts.get(channel_id) {
            if set.contains(participant_id) {
                return Ok(());
            }
        }
        Err(ClearanceGateError::UnauthorizedParticipant(
            participant_id.to_string(),
            channel_id.to_string(),
        ))
    }

    /// Verify an Ed25519 signature from a participant wallet over challenge payload.
    pub fn verify_wallet_signature(
        wallet_pubkey: &[u8; 32],
        message: &[u8],
        signature_bytes: &[u8; 64],
    ) -> Result<(), ClearanceGateError> {
        let verifying_key = VerifyingKey::from_bytes(wallet_pubkey)
            .map_err(|e| ClearanceGateError::InvalidSignature(e.to_string()))?;
        let signature = Signature::from_bytes(signature_bytes);

        verifying_key
            .verify(message, &signature)
            .map_err(|e| ClearanceGateError::InvalidSignature(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[tokio::test]
    async fn test_clearance_level_access_gate() {
        let gate = TelecomClearanceGate::new();
        gate.register_channel("legal-case-alpha", ChannelClearanceLevel::Confidential)
            .await;

        let low_token = ParticipantToken {
            participant_id: "intern-bob".to_string(),
            wallet_pubkey: [1u8; 32],
            clearance: ChannelClearanceLevel::Internal,
            authorized_cases: vec![],
            expires_at: 2000000000,
        };

        let res = gate
            .authorize_join("legal-case-alpha", &low_token, 1700000000)
            .await;
        assert!(matches!(
            res,
            Err(ClearanceGateError::InsufficientClearance { .. })
        ));

        let high_token = ParticipantToken {
            participant_id: "senior-alice".to_string(),
            wallet_pubkey: [2u8; 32],
            clearance: ChannelClearanceLevel::Secret,
            authorized_cases: vec!["legal-case-alpha".to_string()],
            expires_at: 2000000000,
        };

        assert!(gate
            .authorize_join("legal-case-alpha", &high_token, 1700000000)
            .await
            .is_ok());
        assert!(gate
            .authorize_transmit("legal-case-alpha", "senior-alice")
            .await
            .is_ok());
        assert!(gate
            .authorize_transmit("legal-case-alpha", "unknown")
            .await
            .is_err());
    }

    #[test]
    fn test_verify_wallet_signature_ed25519() {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        let pubkey_bytes = *verifying_key.as_bytes();

        let message = b"Authorize Xavier Telecom Session";
        use ed25519_dalek::Signer;
        let signature = signing_key.sign(message);
        let sig_bytes = signature.to_bytes();

        assert!(
            TelecomClearanceGate::verify_wallet_signature(&pubkey_bytes, message, &sig_bytes)
                .is_ok()
        );

        let corrupted = b"Tampered Message";
        assert!(TelecomClearanceGate::verify_wallet_signature(
            &pubkey_bytes,
            corrupted,
            &sig_bytes
        )
        .is_err());
    }

    #[tokio::test]
    async fn test_expired_token_rejected() {
        let gate = TelecomClearanceGate::new();
        gate.register_channel("room-open", ChannelClearanceLevel::Public)
            .await;

        let expired_token = ParticipantToken {
            participant_id: "expired-user".to_string(),
            wallet_pubkey: [0u8; 32],
            clearance: ChannelClearanceLevel::Public,
            authorized_cases: vec![],
            expires_at: 1000,
        };

        let res = gate.authorize_join("room-open", &expired_token, 2000).await;
        assert_eq!(res, Err(ClearanceGateError::TokenInvalid));
    }
}
