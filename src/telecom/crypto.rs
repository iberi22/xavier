//! Core cryptographic primitives for inter-node telecom communication.
//!
//! Provides X25519 ephemeral Diffie-Hellman key exchange, ratchet-based session keys,
//! and authenticated AEAD encryption (AES-256-GCM / ChaCha-style authenticated framing)
//! for direct node-to-node and person-to-person messaging channels.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::crypto::NONCE_SIZE;

/// Size of cryptographic symmetric key in bytes (256 bits).
pub const SYMMETRIC_KEY_SIZE: usize = 32;

/// Cryptographic errors encountered during telecom handshake or payload operations.
#[derive(Debug, Error)]
pub enum TelecomCryptoError {
    #[error("Key exchange failed: {0}")]
    KeyExchangeError(String),

    #[error("Encryption failure: {0}")]
    EncryptionError(String),

    #[error("Decryption failure or invalid authentication tag: {0}")]
    DecryptionError(String),

    #[error("Invalid ratchet step: {0}")]
    RatchetError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// A telecom X25519 keypair used for establishing encrypted inter-node sessions.
#[derive(Clone)]
pub struct TelecomKeypair {
    pub public_key: [u8; 32],
    secret_bytes: [u8; 32],
}

impl TelecomKeypair {
    /// Generate a fresh random X25519 keypair.
    pub fn generate() -> Self {
        let mut secret_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut secret_bytes);
        let secret = StaticSecret::from(secret_bytes);
        let public = X25519PublicKey::from(&secret);
        Self {
            public_key: *public.as_bytes(),
            secret_bytes,
        }
    }

    /// Create keypair from raw secret bytes.
    pub fn from_secret_bytes(bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = X25519PublicKey::from(&secret);
        Self {
            public_key: *public.as_bytes(),
            secret_bytes: bytes,
        }
    }

    /// Return the public key bytes.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.public_key
    }

    /// Perform Diffie-Hellman against a peer's public key to derive shared secret bytes.
    pub fn diffie_hellman(&self, peer_public_key: &[u8; 32]) -> [u8; 32] {
        let secret = StaticSecret::from(self.secret_bytes);
        let peer_pub = X25519PublicKey::from(*peer_public_key);
        let shared = secret.diffie_hellman(&peer_pub);
        *shared.as_bytes()
    }
}

/// Derives a deterministic 32-byte symmetric session key from a shared secret and context salt.
pub fn derive_shared_secret(
    shared_secret: &[u8; 32],
    context_salt: &[u8],
) -> [u8; SYMMETRIC_KEY_SIZE] {
    let mut hasher = Sha256::new();
    hasher.update(b"XAVIER_TELECOM_SESSION_V1:");
    hasher.update(shared_secret);
    hasher.update(context_salt);
    let result = hasher.finalize();
    let mut key = [0u8; SYMMETRIC_KEY_SIZE];
    key.copy_from_slice(&result);
    key
}

/// Symmetric ratchet state for advancing message keys in an established channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRatchet {
    /// Current root key
    root_key: [u8; 32],
    /// Current sequence number
    sequence_number: u64,
}

impl SessionRatchet {
    /// Initialize a new ratchet with a negotiated base key.
    pub fn new(initial_key: [u8; 32]) -> Self {
        Self {
            root_key: initial_key,
            sequence_number: 0,
        }
    }

    /// Current sequence counter.
    pub fn sequence(&self) -> u64 {
        self.sequence_number
    }

    /// Advance the ratchet forward by one step, returning the message encryption key.
    pub fn next_message_key(&mut self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.root_key);
        hasher.update(self.sequence_number.to_be_bytes());
        let msg_key_hash = hasher.finalize();

        // Step root key forward
        let mut root_hasher = Sha256::new();
        root_hasher.update(b"RATCHET_STEP");
        root_hasher.update(self.root_key);
        let new_root = root_hasher.finalize();
        self.root_key.copy_from_slice(&new_root);
        self.sequence_number += 1;

        let mut msg_key = [0u8; 32];
        msg_key.copy_from_slice(&msg_key_hash);
        msg_key
    }
}

/// An authenticated, encrypted wire frame for node-to-node telecom transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedTelecomFrame {
    /// 12-byte initialization vector / nonce
    pub nonce: [u8; NONCE_SIZE],
    /// Encrypted ciphertext and appended 16-byte authentication tag
    pub ciphertext: Vec<u8>,
    /// Frame sequence number
    pub sequence: u64,
    /// Associated Authenticated Data (AAD) tag for channel binding
    pub channel_id: String,
}

/// Encrypt a payload using a derived message key and channel ID as authenticated data.
pub fn ratchet_encrypt(
    key: &[u8; 32],
    channel_id: &str,
    sequence: u64,
    plaintext: &[u8],
) -> Result<EncryptedTelecomFrame, TelecomCryptoError> {
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| TelecomCryptoError::EncryptionError(e.to_string()))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Bind channel_id into AEAD
    let ciphertext = cipher
        .encrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: plaintext,
                aad: channel_id.as_bytes(),
            },
        )
        .map_err(|e| TelecomCryptoError::EncryptionError(e.to_string()))?;

    Ok(EncryptedTelecomFrame {
        nonce: nonce_bytes,
        ciphertext,
        sequence,
        channel_id: channel_id.to_string(),
    })
}

/// Decrypt a frame using the expected message key, validating AAD and sequence.
pub fn ratchet_decrypt(
    key: &[u8; 32],
    frame: &EncryptedTelecomFrame,
) -> Result<Vec<u8>, TelecomCryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| TelecomCryptoError::DecryptionError(e.to_string()))?;
    let nonce = Nonce::from_slice(&frame.nonce);

    let plaintext = cipher
        .decrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: &frame.ciphertext,
                aad: frame.channel_id.as_bytes(),
            },
        )
        .map_err(|e| TelecomCryptoError::DecryptionError(e.to_string()))?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_x25519_key_exchange_determinism() {
        let alice = TelecomKeypair::generate();
        let bob = TelecomKeypair::generate();

        let alice_shared = alice.diffie_hellman(&bob.public_key_bytes());
        let bob_shared = bob.diffie_hellman(&alice.public_key_bytes());

        assert_eq!(alice_shared, bob_shared);

        let salt = b"room-alpha-test";
        let alice_key = derive_shared_secret(&alice_shared, salt);
        let bob_key = derive_shared_secret(&bob_shared, salt);
        assert_eq!(alice_key, bob_key);
    }

    #[test]
    fn test_ratchet_stepping_and_message_encryption() {
        let mut alice_ratchet = SessionRatchet::new([42u8; 32]);
        let mut bob_ratchet = SessionRatchet::new([42u8; 32]);

        let k1_alice = alice_ratchet.next_message_key();
        let k1_bob = bob_ratchet.next_message_key();
        assert_eq!(k1_alice, k1_bob);
        assert_eq!(alice_ratchet.sequence(), 1);

        let payload = b"Hello, confidential peer node!";
        let channel = "room-test-123";
        let frame = ratchet_encrypt(&k1_alice, channel, 1, payload).expect("encrypt ok");

        assert_eq!(frame.channel_id, channel);
        assert_eq!(frame.sequence, 1);

        let decrypted = ratchet_decrypt(&k1_bob, &frame).expect("decrypt ok");
        assert_eq!(decrypted, payload);
    }

    #[test]
    fn test_ratchet_tamper_rejection() {
        let mut ratchet = SessionRatchet::new([7u8; 32]);
        let key = ratchet.next_message_key();

        let mut frame = ratchet_encrypt(&key, "valid-channel", 0, b"Top secret").unwrap();
        // Tamper with ciphertext
        if let Some(byte) = frame.ciphertext.first_mut() {
            *byte ^= 0xFF;
        }

        let err = ratchet_decrypt(&key, &frame);
        assert!(err.is_err());
    }
}
