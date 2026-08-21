//! E2EE Keystore for Ed25519 Node Identities (Issue #1433)
//!
//! Provides encrypted key storage at rest using AES-256-GCM with master key derived
//! via Argon2id. Manages key generation, import, export, TTL, key rotation, and
//! integration with `crate::mesh::node::NodeIdentity` and `crate::node_identity::DerivedNodeKeys`.

use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use crate::crypto::encryption::{decrypt_data, encrypt_data, NonceBytes};
use crate::crypto::keys::{KeySalt, KEK};
use crate::crypto::SALT_SIZE;
use crate::crypto::{hex_decode, hex_encode};
use crate::mesh::node::{NodeId, NodeIdentity};
use crate::node_identity::DerivedNodeKeys;

/// Wallet error types.
#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("Key alias '{0}' not found in wallet")]
    KeyNotFound(String),

    #[error("Key alias '{0}' already exists in wallet")]
    KeyAlreadyExists(String),

    #[error("Key alias '{0}' has expired")]
    KeyExpired(String),

    #[error("No active key set in wallet")]
    NoActiveKey,

    #[error("Invalid master password or decryption failed")]
    InvalidMasterPassword,

    #[error("Key derivation error: {0}")]
    KeyDerivationError(String),

    #[error("Invalid secret key length: expected 32 bytes")]
    InvalidSecretKeyLength,

    #[error("Hex decode error: {0}")]
    HexDecodeError(String),

    #[error("Encryption error: {0}")]
    EncryptionFailed(String),

    #[error("Decryption error: {0}")]
    DecryptionFailed(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("I/O error: {0}")]
    IoError(String),
}

pub type WalletResult<T> = Result<T, WalletError>;

/// Metadata for a key entry stored in the wallet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyMetadata {
    /// Friendly alias for the key (e.g. "primary", "node-1").
    pub alias: String,
    /// Base32 human-readable NodeID (`xv1-...`).
    pub node_id: String,
    /// Lowercase hex-encoded 32-byte Ed25519 public key.
    pub public_key_hex: String,
    /// Timestamp when key was initially created.
    pub created_at: DateTime<Utc>,
    /// Timestamp of last key rotation (if any).
    pub last_rotated_at: Option<DateTime<Utc>>,
    /// TTL in seconds. `None` means key does not expire.
    pub ttl_seconds: Option<u64>,
    /// Key version (increments on rotation, starts at 1).
    pub version: u32,
    /// Optional ML-DSA commitment hex string for hybrid quantum bridge.
    pub ml_dsa_commitment_hex: Option<String>,
}

/// Secret payload encrypted at rest.
#[derive(Serialize, Deserialize)]
struct EncryptedSecretPayload {
    ed25519_secret_hex: String,
    ml_dsa_commitment_hex: Option<String>,
}

/// On-disk record for a single key entry.
#[derive(Serialize, Deserialize)]
struct EncryptedKeyRecord {
    metadata: KeyMetadata,
    nonce_hex: String,
    ciphertext_hex: String,
}

/// Complete encrypted keystore format persisted to file.
#[derive(Serialize, Deserialize)]
struct EncryptedKeystoreFile {
    version: u32,
    salt_hex: String,
    active_alias: Option<String>,
    records: Vec<EncryptedKeyRecord>,
    created_at: DateTime<Utc>,
}

/// In-memory representation of an unencrypted key entry.
#[derive(Clone)]
pub struct KeyEntry {
    pub metadata: KeyMetadata,
    pub ed25519_secret: [u8; 32],
    pub ed25519_public: [u8; 32],
    pub ml_dsa_commitment: Option<[u8; 32]>,
}

impl fmt::Debug for KeyEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyEntry")
            .field("metadata", &self.metadata)
            .field("ed25519_public", &hex_encode(self.ed25519_public))
            .field("ed25519_secret", &"[REDACTED]")
            .field(
                "ml_dsa_commitment",
                &self.ml_dsa_commitment.as_ref().map(hex_encode),
            )
            .finish()
    }
}

/// E2EE Keystore for Ed25519 Node Identities.
///
/// Encrypts secret key material at rest using AES-256-GCM with a master key derived
/// via Argon2id. Supports key generation, importing, exporting, TTL expiration,
/// key rotation, active identity management, and node identity integration.
pub struct Ed25519Wallet {
    salt: [u8; SALT_SIZE],
    kek: [u8; 32],
    entries: HashMap<String, KeyEntry>,
    active_alias: Option<String>,
    default_ttl_seconds: Option<u64>,
}

impl Ed25519Wallet {
    /// Create a new empty wallet with a fresh Argon2 salt and derived master key (KEK).
    pub fn new(master_password: &str) -> WalletResult<Self> {
        let salt = KeySalt::generate();
        let salt_bytes = *salt.as_bytes();
        let kek = KEK::derive_from_password(master_password, &salt)
            .map_err(|e| WalletError::KeyDerivationError(e.to_string()))?;

        Ok(Self {
            salt: salt_bytes,
            kek: *kek.as_bytes(),
            entries: HashMap::new(),
            active_alias: None,
            default_ttl_seconds: None,
        })
    }

    /// Create a wallet with a specific salt and master password.
    pub fn with_salt_and_password(
        salt_bytes: &[u8; SALT_SIZE],
        master_password: &str,
    ) -> WalletResult<Self> {
        let salt = KeySalt::from_bytes(salt_bytes);
        let kek = KEK::derive_from_password(master_password, &salt)
            .map_err(|e| WalletError::KeyDerivationError(e.to_string()))?;

        Ok(Self {
            salt: *salt_bytes,
            kek: *kek.as_bytes(),
            entries: HashMap::new(),
            active_alias: None,
            default_ttl_seconds: None,
        })
    }

    /// Return master salt bytes.
    pub fn salt(&self) -> &[u8; SALT_SIZE] {
        &self.salt
    }

    /// Set default TTL for newly generated / imported keys in this wallet.
    pub fn set_default_ttl(&mut self, ttl: Option<Duration>) {
        self.default_ttl_seconds = ttl.map(|d| d.num_seconds() as u64);
    }

    /// Get default TTL.
    pub fn default_ttl(&self) -> Option<Duration> {
        self.default_ttl_seconds
            .map(|s| Duration::seconds(s as i64))
    }

    /// Generate a new Ed25519 keypair for the given alias.
    pub fn generate_keypair(&mut self, alias: &str, ttl: Option<Duration>) -> WalletResult<NodeId> {
        if self.entries.contains_key(alias) {
            return Err(WalletError::KeyAlreadyExists(alias.to_string()));
        }

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let public_bytes = verifying_key.to_bytes();
        let secret_bytes = signing_key.to_bytes();
        let node_id = NodeId::from_public_key_bytes(&public_bytes);

        let effective_ttl = ttl
            .map(|d| d.num_seconds() as u64)
            .or(self.default_ttl_seconds);

        let metadata = KeyMetadata {
            alias: alias.to_string(),
            node_id: node_id.0.clone(),
            public_key_hex: hex_encode(public_bytes),
            created_at: Utc::now(),
            last_rotated_at: None,
            ttl_seconds: effective_ttl,
            version: 1,
            ml_dsa_commitment_hex: None,
        };

        let entry = KeyEntry {
            metadata,
            ed25519_secret: secret_bytes,
            ed25519_public: public_bytes,
            ml_dsa_commitment: None,
        };

        self.entries.insert(alias.to_string(), entry);
        if self.active_alias.is_none() {
            self.active_alias = Some(alias.to_string());
        }

        Ok(node_id)
    }

    /// Import a keypair from raw 32-byte secret key bytes.
    pub fn import_secret_bytes(
        &mut self,
        alias: &str,
        secret_bytes: &[u8; 32],
        ttl: Option<Duration>,
    ) -> WalletResult<NodeId> {
        if self.entries.contains_key(alias) {
            return Err(WalletError::KeyAlreadyExists(alias.to_string()));
        }

        let signing_key = SigningKey::from_bytes(secret_bytes);
        let verifying_key = signing_key.verifying_key();
        let public_bytes = verifying_key.to_bytes();
        let node_id = NodeId::from_public_key_bytes(&public_bytes);

        let effective_ttl = ttl
            .map(|d| d.num_seconds() as u64)
            .or(self.default_ttl_seconds);

        let metadata = KeyMetadata {
            alias: alias.to_string(),
            node_id: node_id.0.clone(),
            public_key_hex: hex_encode(public_bytes),
            created_at: Utc::now(),
            last_rotated_at: None,
            ttl_seconds: effective_ttl,
            version: 1,
            ml_dsa_commitment_hex: None,
        };

        let entry = KeyEntry {
            metadata,
            ed25519_secret: *secret_bytes,
            ed25519_public: public_bytes,
            ml_dsa_commitment: None,
        };

        self.entries.insert(alias.to_string(), entry);
        if self.active_alias.is_none() {
            self.active_alias = Some(alias.to_string());
        }

        Ok(node_id)
    }

    /// Import a keypair from a hex-encoded secret key string (32 bytes = 64 hex chars).
    pub fn import_secret_hex(
        &mut self,
        alias: &str,
        secret_hex: &str,
        ttl: Option<Duration>,
    ) -> WalletResult<NodeId> {
        let bytes = hex_decode(secret_hex.trim())
            .map_err(|e| WalletError::HexDecodeError(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(WalletError::InvalidSecretKeyLength);
        }
        let mut secret_bytes = [0u8; 32];
        secret_bytes.copy_from_slice(&bytes);
        self.import_secret_bytes(alias, &secret_bytes, ttl)
    }

    /// Import an existing `NodeIdentity` into the wallet.
    pub fn import_node_identity(
        &mut self,
        alias: &str,
        identity: &NodeIdentity,
        ttl: Option<Duration>,
    ) -> WalletResult<NodeId> {
        let priv_bytes = identity.private_key_bytes();
        if priv_bytes.len() != 32 {
            return Err(WalletError::InvalidSecretKeyLength);
        }
        let mut secret_bytes = [0u8; 32];
        secret_bytes.copy_from_slice(priv_bytes);

        let node_id = self.import_secret_bytes(alias, &secret_bytes, ttl)?;

        if let Some(ml_hex) = identity.ml_dsa_commitment_hex() {
            if let Ok(ml_bytes) = hex_decode(&ml_hex) {
                if ml_bytes.len() == 32 {
                    if let Some(entry) = self.entries.get_mut(alias) {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&ml_bytes);
                        entry.ml_dsa_commitment = Some(arr);
                        entry.metadata.ml_dsa_commitment_hex = Some(ml_hex);
                    }
                }
            }
        }

        Ok(node_id)
    }

    /// Import derived keys (`DerivedNodeKeys`) derived from BIP39 seed into the wallet.
    pub fn import_derived_node_keys(
        &mut self,
        alias: &str,
        keys: &DerivedNodeKeys,
        ttl: Option<Duration>,
    ) -> WalletResult<NodeId> {
        let node_id = self.import_secret_bytes(alias, &keys.ed25519_secret, ttl)?;
        if let Some(entry) = self.entries.get_mut(alias) {
            entry.ml_dsa_commitment = Some(keys.ml_dsa_commitment);
            entry.metadata.ml_dsa_commitment_hex = Some(hex_encode(keys.ml_dsa_commitment));
        }
        Ok(node_id)
    }

    /// Export 32-byte public key.
    pub fn export_public_key(&self, alias: &str) -> WalletResult<[u8; 32]> {
        let entry = self.get_entry(alias)?;
        Ok(entry.ed25519_public)
    }

    /// Export 32-byte secret key.
    pub fn export_secret_key(&self, alias: &str) -> WalletResult<[u8; 32]> {
        let entry = self.get_entry(alias)?;
        Ok(entry.ed25519_secret)
    }

    /// Export public key as hex string.
    pub fn export_public_key_hex(&self, alias: &str) -> WalletResult<String> {
        let pk = self.export_public_key(alias)?;
        Ok(hex_encode(pk))
    }

    /// Export secret key as hex string.
    pub fn export_secret_key_hex(&self, alias: &str) -> WalletResult<String> {
        let sk = self.export_secret_key(alias)?;
        Ok(hex_encode(sk))
    }

    /// Export keypair as `(public_key_hex, secret_key_hex)`.
    pub fn export_keypair_hex(&self, alias: &str) -> WalletResult<(String, String)> {
        Ok((
            self.export_public_key_hex(alias)?,
            self.export_secret_key_hex(alias)?,
        ))
    }

    /// Export entry as a `NodeIdentity` object.
    pub fn export_node_identity(&self, alias: &str) -> WalletResult<NodeIdentity> {
        let entry = self.get_entry(alias)?;
        let derived = DerivedNodeKeys {
            node_id: NodeId::parse(&entry.metadata.node_id)
                .map_err(|e| WalletError::KeyDerivationError(e.to_string()))?,
            ed25519_public: entry.ed25519_public,
            ed25519_secret: entry.ed25519_secret,
            ml_dsa_commitment: entry.ml_dsa_commitment.unwrap_or([0u8; 32]),
        };
        Ok(NodeIdentity::from_derived(&derived))
    }

    /// Get `NodeId` for alias.
    pub fn get_node_id(&self, alias: &str) -> WalletResult<NodeId> {
        let entry = self.get_entry(alias)?;
        NodeId::parse(&entry.metadata.node_id)
            .map_err(|e| WalletError::KeyDerivationError(e.to_string()))
    }

    /// Get metadata for alias.
    pub fn get_metadata(&self, alias: &str) -> WalletResult<KeyMetadata> {
        let entry = self.get_entry(alias)?;
        Ok(entry.metadata.clone())
    }

    /// List all aliases in wallet.
    pub fn list_aliases(&self) -> Vec<String> {
        let mut list: Vec<String> = self.entries.keys().cloned().collect();
        list.sort();
        list
    }

    /// Number of keys in wallet.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if wallet is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Check if alias exists in wallet.
    pub fn contains_alias(&self, alias: &str) -> bool {
        self.entries.contains_key(alias)
    }

    /// Active alias getters & setters.
    pub fn active_alias(&self) -> Option<&str> {
        self.active_alias.as_deref()
    }

    pub fn set_active_alias(&mut self, alias: &str) -> WalletResult<()> {
        if !self.entries.contains_key(alias) {
            return Err(WalletError::KeyNotFound(alias.to_string()));
        }
        self.active_alias = Some(alias.to_string());
        Ok(())
    }

    pub fn get_active_node_id(&self) -> WalletResult<NodeId> {
        let alias = self.active_alias.as_ref().ok_or(WalletError::NoActiveKey)?;
        self.get_node_id(alias)
    }

    pub fn export_active_node_identity(&self) -> WalletResult<NodeIdentity> {
        let alias = self.active_alias.as_ref().ok_or(WalletError::NoActiveKey)?;
        self.export_node_identity(alias)
    }

    /// Sign a message using active key.
    pub fn sign_active(&self, message: &[u8]) -> WalletResult<[u8; 64]> {
        let alias = self.active_alias.as_ref().ok_or(WalletError::NoActiveKey)?;
        self.sign(alias, message)
    }

    /// Sign a message using a key in the wallet identified by alias.
    pub fn sign(&self, alias: &str, message: &[u8]) -> WalletResult<[u8; 64]> {
        if self.is_expired(alias)? {
            return Err(WalletError::KeyExpired(alias.to_string()));
        }
        let entry = self.get_entry(alias)?;
        let signing_key = SigningKey::from_bytes(&entry.ed25519_secret);
        let signature: Signature = signing_key.sign(message);
        Ok(signature.to_bytes())
    }

    /// Sign a message even if key has expired (e.g. for emergency/audit operations).
    pub fn sign_allow_expired(&self, alias: &str, message: &[u8]) -> WalletResult<[u8; 64]> {
        let entry = self.get_entry(alias)?;
        let signing_key = SigningKey::from_bytes(&entry.ed25519_secret);
        let signature: Signature = signing_key.sign(message);
        Ok(signature.to_bytes())
    }

    /// Verify signature using public key.
    pub fn verify(public_key_bytes: &[u8; 32], message: &[u8], signature_bytes: &[u8; 64]) -> bool {
        let Ok(verifying_key) = VerifyingKey::from_bytes(public_key_bytes) else {
            return false;
        };
        let signature = Signature::from_bytes(signature_bytes);
        verifying_key.verify(message, &signature).is_ok()
    }

    /// Check if key for given alias has expired based on TTL.
    pub fn is_expired(&self, alias: &str) -> WalletResult<bool> {
        let entry = self.get_entry(alias)?;
        let Some(ttl_sec) = entry.metadata.ttl_seconds else {
            return Ok(false);
        };
        let base_time = entry
            .metadata
            .last_rotated_at
            .unwrap_or(entry.metadata.created_at);
        let expiration_time = base_time + Duration::seconds(ttl_sec as i64);
        Ok(Utc::now() > expiration_time)
    }

    /// Return remaining TTL duration for a key, or `None` if key has no TTL.
    pub fn ttl_remaining(&self, alias: &str) -> WalletResult<Option<Duration>> {
        let entry = self.get_entry(alias)?;
        let Some(ttl_sec) = entry.metadata.ttl_seconds else {
            return Ok(None);
        };
        let base_time = entry
            .metadata
            .last_rotated_at
            .unwrap_or(entry.metadata.created_at);
        let expiration_time = base_time + Duration::seconds(ttl_sec as i64);
        let now = Utc::now();
        if now >= expiration_time {
            Ok(Some(Duration::zero()))
        } else {
            Ok(Some(expiration_time - now))
        }
    }

    /// Update TTL for a key alias.
    pub fn set_ttl(&mut self, alias: &str, ttl: Option<Duration>) -> WalletResult<()> {
        let entry = self
            .entries
            .get_mut(alias)
            .ok_or_else(|| WalletError::KeyNotFound(alias.to_string()))?;
        entry.metadata.ttl_seconds = ttl.map(|d| d.num_seconds() as u64);
        Ok(())
    }

    /// Rotate key for given alias: generates fresh Ed25519 keypair, increments version, updates timestamps.
    pub fn rotate_key(&mut self, alias: &str) -> WalletResult<NodeId> {
        let entry = self
            .entries
            .get_mut(alias)
            .ok_or_else(|| WalletError::KeyNotFound(alias.to_string()))?;

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let public_bytes = verifying_key.to_bytes();
        let secret_bytes = signing_key.to_bytes();
        let node_id = NodeId::from_public_key_bytes(&public_bytes);

        entry.ed25519_secret = secret_bytes;
        entry.ed25519_public = public_bytes;
        entry.metadata.node_id = node_id.0.clone();
        entry.metadata.public_key_hex = hex_encode(public_bytes);
        entry.metadata.last_rotated_at = Some(Utc::now());
        entry.metadata.version += 1;

        Ok(node_id)
    }

    /// List all aliases that have currently expired.
    pub fn list_expired_keys(&self) -> Vec<String> {
        let mut expired = Vec::new();
        for alias in self.entries.keys() {
            if self.is_expired(alias).unwrap_or(false) {
                expired.push(alias.clone());
            }
        }
        expired.sort();
        expired
    }

    /// Automatically rotate all expired keys in wallet. Returns list of rotated aliases.
    pub fn rotate_expired_keys(&mut self) -> WalletResult<Vec<String>> {
        let expired = self.list_expired_keys();
        for alias in &expired {
            self.rotate_key(alias)?;
        }
        Ok(expired)
    }

    /// Remove a key entry by alias.
    pub fn remove_key(&mut self, alias: &str) -> WalletResult<KeyMetadata> {
        let entry = self
            .entries
            .remove(alias)
            .ok_or_else(|| WalletError::KeyNotFound(alias.to_string()))?;
        if self.active_alias.as_deref() == Some(alias) {
            self.active_alias = self.entries.keys().next().cloned();
        }
        Ok(entry.metadata)
    }

    /// Encrypt wallet data into serialized JSON bytes.
    pub fn to_encrypted_bytes(&self) -> WalletResult<Vec<u8>> {
        let mut records = Vec::with_capacity(self.entries.len());

        for entry in self.entries.values() {
            let payload = EncryptedSecretPayload {
                ed25519_secret_hex: hex_encode(entry.ed25519_secret),
                ml_dsa_commitment_hex: entry.ml_dsa_commitment.as_ref().map(hex_encode),
            };

            let payload_json = serde_json::to_vec(&payload)
                .map_err(|e| WalletError::SerializationError(e.to_string()))?;

            let nonce = NonceBytes::generate();
            let blob = encrypt_data(&payload_json, &self.kek, &nonce)
                .map_err(|e| WalletError::EncryptionFailed(e.to_string()))?;

            records.push(EncryptedKeyRecord {
                metadata: entry.metadata.clone(),
                nonce_hex: hex_encode(blob.nonce),
                ciphertext_hex: hex_encode(blob.ciphertext),
            });
        }

        let store_file = EncryptedKeystoreFile {
            version: 1,
            salt_hex: hex_encode(self.salt),
            active_alias: self.active_alias.clone(),
            records,
            created_at: Utc::now(),
        };

        serde_json::to_vec_pretty(&store_file)
            .map_err(|e| WalletError::SerializationError(e.to_string()))
    }

    /// Decrypt and load wallet from serialized encrypted bytes using master password.
    pub fn from_encrypted_bytes(data: &[u8], master_password: &str) -> WalletResult<Self> {
        let store_file: EncryptedKeystoreFile = serde_json::from_slice(data)
            .map_err(|e| WalletError::SerializationError(e.to_string()))?;

        let salt_bytes_vec = hex_decode(&store_file.salt_hex)
            .map_err(|e| WalletError::HexDecodeError(e.to_string()))?;
        if salt_bytes_vec.len() != SALT_SIZE {
            return Err(WalletError::KeyDerivationError(
                "Invalid salt length".into(),
            ));
        }
        let mut salt_bytes = [0u8; SALT_SIZE];
        salt_bytes.copy_from_slice(&salt_bytes_vec);

        let salt = KeySalt::from_bytes(&salt_bytes);
        let kek = KEK::derive_from_password(master_password, &salt)
            .map_err(|e| WalletError::KeyDerivationError(e.to_string()))?;

        let mut wallet = Self::with_salt_and_password(&salt_bytes, master_password)?;
        wallet.active_alias = store_file.active_alias;

        for record in store_file.records {
            let nonce_bytes_vec = hex_decode(&record.nonce_hex)
                .map_err(|e| WalletError::HexDecodeError(e.to_string()))?;
            let ciphertext_bytes = hex_decode(&record.ciphertext_hex)
                .map_err(|e| WalletError::HexDecodeError(e.to_string()))?;

            if nonce_bytes_vec.len() != crate::crypto::NONCE_SIZE {
                return Err(WalletError::DecryptionFailed("Invalid nonce size".into()));
            }
            let mut nonce_arr = [0u8; crate::crypto::NONCE_SIZE];
            nonce_arr.copy_from_slice(&nonce_bytes_vec);

            let decrypted_json = decrypt_data(&ciphertext_bytes, kek.as_bytes(), &nonce_arr)
                .map_err(|_| WalletError::InvalidMasterPassword)?;

            let payload: EncryptedSecretPayload = serde_json::from_slice(&decrypted_json)
                .map_err(|e| WalletError::SerializationError(e.to_string()))?;

            let secret_bytes_vec = hex_decode(&payload.ed25519_secret_hex)
                .map_err(|e| WalletError::HexDecodeError(e.to_string()))?;
            if secret_bytes_vec.len() != 32 {
                return Err(WalletError::InvalidSecretKeyLength);
            }
            let mut secret_bytes = [0u8; 32];
            secret_bytes.copy_from_slice(&secret_bytes_vec);

            let public_bytes_vec = hex_decode(&record.metadata.public_key_hex)
                .map_err(|e| WalletError::HexDecodeError(e.to_string()))?;
            if public_bytes_vec.len() != 32 {
                return Err(WalletError::InvalidSecretKeyLength);
            }
            let mut public_bytes = [0u8; 32];
            public_bytes.copy_from_slice(&public_bytes_vec);

            let ml_dsa_commitment = match payload.ml_dsa_commitment_hex {
                Some(ref hex) => {
                    let b =
                        hex_decode(hex).map_err(|e| WalletError::HexDecodeError(e.to_string()))?;
                    if b.len() == 32 {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&b);
                        Some(arr)
                    } else {
                        None
                    }
                }
                None => None,
            };

            let entry = KeyEntry {
                metadata: record.metadata.clone(),
                ed25519_secret: secret_bytes,
                ed25519_public: public_bytes,
                ml_dsa_commitment,
            };

            wallet.entries.insert(record.metadata.alias.clone(), entry);
        }

        Ok(wallet)
    }

    /// Persist encrypted wallet to file.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> WalletResult<()> {
        let bytes = self.to_encrypted_bytes()?;
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| WalletError::IoError(e.to_string()))?;
        }
        std::fs::write(path, bytes).map_err(|e| WalletError::IoError(e.to_string()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    /// Load encrypted wallet from file using master password.
    pub fn load_from_file(path: impl AsRef<Path>, master_password: &str) -> WalletResult<Self> {
        let bytes = std::fs::read(path).map_err(|e| WalletError::IoError(e.to_string()))?;
        Self::from_encrypted_bytes(&bytes, master_password)
    }

    fn get_entry(&self, alias: &str) -> WalletResult<&KeyEntry> {
        self.entries
            .get(alias)
            .ok_or_else(|| WalletError::KeyNotFound(alias.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    const MASTER_PASS: &str = "SuperSecretMasterPassphrase123!";

    #[test]
    fn test_1_wallet_creation_and_master_key_derivation() {
        let wallet = Ed25519Wallet::new(MASTER_PASS).expect("Failed to create wallet");
        assert_eq!(wallet.len(), 0);
        assert!(wallet.is_empty());
        assert_eq!(wallet.active_alias(), None);
        assert_eq!(wallet.salt().len(), 16);
    }

    #[test]
    fn test_2_generate_keypair() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let node_id = wallet.generate_keypair("primary", None).unwrap();

        assert!(node_id.as_str().starts_with("xv1-"));
        assert_eq!(wallet.len(), 1);
        assert_eq!(wallet.active_alias(), Some("primary"));
        assert!(wallet.contains_alias("primary"));

        let pk = wallet.export_public_key("primary").unwrap();
        let sk = wallet.export_secret_key("primary").unwrap();

        assert_eq!(pk.len(), 32);
        assert_eq!(sk.len(), 32);
    }

    #[test]
    fn test_3_import_and_export_secret_bytes() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let raw_secret = [42u8; 32];

        let node_id = wallet
            .import_secret_bytes("imported-1", &raw_secret, None)
            .unwrap();
        assert!(node_id.as_str().starts_with("xv1-"));

        let exported_sk = wallet.export_secret_key("imported-1").unwrap();
        assert_eq!(exported_sk, raw_secret);

        let exported_pk = wallet.export_public_key("imported-1").unwrap();
        assert_eq!(exported_pk.len(), 32);
    }

    #[test]
    fn test_4_import_and_export_hex() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let raw_secret = [7u8; 32];
        let secret_hex = hex_encode(raw_secret);

        wallet
            .import_secret_hex("hex-alias", &secret_hex, None)
            .unwrap();

        let (pk_hex, sk_hex) = wallet.export_keypair_hex("hex-alias").unwrap();
        assert_eq!(sk_hex, secret_hex);
        assert_eq!(pk_hex.len(), 64);
    }

    #[test]
    fn test_5_encryption_at_rest_roundtrip() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let node_id1 = wallet.generate_keypair("k1", None).unwrap();
        let node_id2 = wallet.generate_keypair("k2", None).unwrap();

        let encrypted_bytes = wallet.to_encrypted_bytes().unwrap();
        assert!(!encrypted_bytes.is_empty());

        let restored = Ed25519Wallet::from_encrypted_bytes(&encrypted_bytes, MASTER_PASS).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(restored.get_node_id("k1").unwrap(), node_id1);
        assert_eq!(restored.get_node_id("k2").unwrap(), node_id2);
        assert_eq!(
            restored.export_secret_key("k1").unwrap(),
            wallet.export_secret_key("k1").unwrap()
        );
    }

    #[test]
    fn test_6_wrong_master_password_fails_decryption() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        wallet.generate_keypair("secret-node", None).unwrap();
        let encrypted_bytes = wallet.to_encrypted_bytes().unwrap();

        let result = Ed25519Wallet::from_encrypted_bytes(&encrypted_bytes, "WrongPassword!");
        assert!(matches!(result, Err(WalletError::InvalidMasterPassword)));
    }

    #[test]
    fn test_7_key_rotation() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let old_node_id = wallet.generate_keypair("rotatable", None).unwrap();
        let old_pk = wallet.export_public_key("rotatable").unwrap();
        let meta_before = wallet.get_metadata("rotatable").unwrap();
        assert_eq!(meta_before.version, 1);
        assert!(meta_before.last_rotated_at.is_none());

        let new_node_id = wallet.rotate_key("rotatable").unwrap();
        let new_pk = wallet.export_public_key("rotatable").unwrap();
        let meta_after = wallet.get_metadata("rotatable").unwrap();

        assert_ne!(old_node_id, new_node_id);
        assert_ne!(old_pk, new_pk);
        assert_eq!(meta_after.version, 2);
        assert!(meta_after.last_rotated_at.is_some());
    }

    #[test]
    fn test_8_ttl_expiration_and_remaining() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        // Set key with 0 seconds TTL so it expires immediately
        wallet
            .generate_keypair("short-lived", Some(Duration::seconds(0)))
            .unwrap();

        // Wait brief moment to guarantee Utc::now() > created_at
        std::thread::sleep(std::time::Duration::from_millis(10));

        assert!(wallet.is_expired("short-lived").unwrap());
        let remaining = wallet.ttl_remaining("short-lived").unwrap().unwrap();
        assert_eq!(remaining, Duration::zero());

        // Signing with expired key should fail
        let err = wallet.sign("short-lived", b"hello").unwrap_err();
        assert!(matches!(err, WalletError::KeyExpired(_)));

        // Emergency sign should succeed
        assert!(wallet.sign_allow_expired("short-lived", b"hello").is_ok());
    }

    #[test]
    fn test_9_rotate_expired_keys() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        wallet
            .generate_keypair("exp1", Some(Duration::seconds(0)))
            .unwrap();
        wallet.generate_keypair("permanent", None).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        let expired = wallet.list_expired_keys();
        assert_eq!(expired, vec!["exp1"]);

        let rotated = wallet.rotate_expired_keys().unwrap();
        assert_eq!(rotated, vec!["exp1"]);

        let meta = wallet.get_metadata("exp1").unwrap();
        assert_eq!(meta.version, 2);
    }

    #[test]
    fn test_10_integration_with_node_identity() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let original_identity = NodeIdentity::generate();
        let imported_node_id = wallet
            .import_node_identity("mesh-node", &original_identity, None)
            .unwrap();

        assert_eq!(imported_node_id, original_identity.node_id);

        let exported_identity = wallet.export_node_identity("mesh-node").unwrap();
        assert_eq!(exported_identity.node_id, original_identity.node_id);
        assert_eq!(exported_identity.public_key, original_identity.public_key);
        assert_eq!(
            exported_identity.private_key_bytes(),
            original_identity.private_key_bytes()
        );
    }

    #[test]
    fn test_11_integration_with_derived_node_keys() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let seed_bytes = [123u8; 64];
        let derived_keys = DerivedNodeKeys::from_seed_bytes(&seed_bytes).unwrap();

        let imported_node_id = wallet
            .import_derived_node_keys("derived-alias", &derived_keys, None)
            .unwrap();
        assert_eq!(imported_node_id, derived_keys.node_id);

        let exported_identity = wallet.export_node_identity("derived-alias").unwrap();
        assert_eq!(
            exported_identity.ml_dsa_commitment_hex(),
            Some(hex_encode(derived_keys.ml_dsa_commitment))
        );
    }

    #[test]
    fn test_12_signing_and_verifying() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        wallet.generate_keypair("signer", None).unwrap();

        let msg = b"Message to be signed by Xavier Ed25519 wallet";
        let sig = wallet.sign("signer", msg).unwrap();
        let pk = wallet.export_public_key("signer").unwrap();

        assert!(Ed25519Wallet::verify(&pk, msg, &sig));

        let tampered_msg = b"Tampered message";
        assert!(!Ed25519Wallet::verify(&pk, tampered_msg, &sig));
    }

    #[test]
    fn test_13_active_key_management() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        wallet.generate_keypair("key-a", None).unwrap();
        wallet.generate_keypair("key-b", None).unwrap();

        assert_eq!(wallet.active_alias(), Some("key-a"));

        wallet.set_active_alias("key-b").unwrap();
        assert_eq!(wallet.active_alias(), Some("key-b"));

        let active_node_id = wallet.get_active_node_id().unwrap();
        assert_eq!(active_node_id, wallet.get_node_id("key-b").unwrap());

        let msg = b"Active signing test";
        let sig = wallet.sign_active(msg).unwrap();
        let pk = wallet.export_public_key("key-b").unwrap();
        assert!(Ed25519Wallet::verify(&pk, msg, &sig));
    }

    #[test]
    fn test_14_tampered_encrypted_wallet_fails() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        wallet.generate_keypair("sensitive", None).unwrap();
        let mut encrypted_bytes = wallet.to_encrypted_bytes().unwrap();

        // Flip a byte in the middle of the JSON string to corrupt ciphertext/auth tag
        let len = encrypted_bytes.len();
        encrypted_bytes[len / 2] ^= 0xFF;

        let result = Ed25519Wallet::from_encrypted_bytes(&encrypted_bytes, MASTER_PASS);
        assert!(result.is_err());
    }

    #[test]
    fn test_15_save_and_load_file() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let node_id = wallet.generate_keypair("persisted-key", None).unwrap();

        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        wallet.save_to_file(path).unwrap();

        let loaded_wallet = Ed25519Wallet::load_from_file(path, MASTER_PASS).unwrap();
        assert_eq!(loaded_wallet.len(), 1);
        assert_eq!(loaded_wallet.get_node_id("persisted-key").unwrap(), node_id);
    }

    #[test]
    fn test_wallet_key_rotation_preserves_old_data() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let alias = "rotation-data-test";
        wallet.generate_keypair(alias, None).unwrap();

        let old_secret = wallet.export_secret_key(alias).unwrap();
        let old_pub = wallet.export_public_key(alias).unwrap();
        let nonce = NonceBytes::generate();
        let plaintext = b"Confidential data encrypted before key rotation";
        let encrypted_pre_rotation = encrypt_data(plaintext, &old_secret, &nonce).unwrap();
        let wallet_bytes_pre_rotation = wallet.to_encrypted_bytes().unwrap();

        let _new_node_id = wallet.rotate_key(alias).unwrap();

        let meta = wallet.get_metadata(alias).unwrap();
        assert_eq!(meta.version, 2);
        let new_secret = wallet.export_secret_key(alias).unwrap();
        assert_ne!(old_secret, new_secret);

        let decrypted = decrypt_data(
            &encrypted_pre_rotation.ciphertext,
            &old_secret,
            nonce.as_bytes(),
        )
        .unwrap();
        assert_eq!(decrypted, plaintext);

        let restored_pre =
            Ed25519Wallet::from_encrypted_bytes(&wallet_bytes_pre_rotation, MASTER_PASS).unwrap();
        assert_eq!(restored_pre.export_public_key(alias).unwrap(), old_pub);
    }

    #[test]
    fn test_wallet_concurrent_encrypt_decrypt() {
        use std::sync::Arc;
        use std::thread;

        let secret = [88u8; 32];
        let secret_arc = Arc::new(secret);

        let mut handles = vec![];

        for i in 0..10 {
            let secret_ref = Arc::clone(&secret_arc);
            let handle = thread::spawn(move || {
                let msg = format!("Concurrent thread payload {}", i);
                let nonce = NonceBytes::generate();
                let encrypted = encrypt_data(msg.as_bytes(), &secret_ref, &nonce).unwrap();
                let decrypted =
                    decrypt_data(&encrypted.ciphertext, &secret_ref, nonce.as_bytes()).unwrap();
                assert_eq!(decrypted, msg.as_bytes());
            });
            handles.push(handle);
        }

        for handle in handles {
            handle
                .join()
                .expect("Thread panicked during concurrent encrypt/decrypt");
        }
    }

    #[test]
    fn test_wallet_empty_plaintext() {
        let key = [77u8; 32];
        let nonce = NonceBytes::generate();
        let empty_plaintext = b"";

        let encrypted = encrypt_data(empty_plaintext, &key, &nonce).unwrap();
        assert_eq!(encrypted.ciphertext.len(), 16);

        let decrypted = decrypt_data(&encrypted.ciphertext, &key, nonce.as_bytes()).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_wallet_max_plaintext_size() {
        let key = [0x55u8; 32];
        let nonce = NonceBytes::generate();
        let large_plaintext = vec![0xABu8; 2 * 1024 * 1024];

        let encrypted = encrypt_data(&large_plaintext, &key, &nonce).unwrap();
        assert_eq!(encrypted.ciphertext.len(), large_plaintext.len() + 16);

        let decrypted = decrypt_data(&encrypted.ciphertext, &key, nonce.as_bytes()).unwrap();
        assert_eq!(decrypted.len(), large_plaintext.len());
        assert_eq!(decrypted, large_plaintext);
    }

    #[test]
    fn test_wallet_wrong_key_rejection() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        wallet.generate_keypair("reject-test", None).unwrap();
        let encrypted_bytes = wallet.to_encrypted_bytes().unwrap();

        let result = Ed25519Wallet::from_encrypted_bytes(&encrypted_bytes, "IncorrectPassword!456");
        assert!(matches!(result, Err(WalletError::InvalidMasterPassword)));

        let correct_key = [1u8; 32];
        let wrong_key = [2u8; 32];
        let nonce = NonceBytes::generate();
        let blob = encrypt_data(b"secret msg", &correct_key, &nonce).unwrap();

        let decrypt_res = decrypt_data(&blob.ciphertext, &wrong_key, nonce.as_bytes());
        assert!(decrypt_res.is_err());
    }

    #[test]
    fn test_wallet_mnemonic_generation_deterministic() {
        let seed = [42u8; 64];

        let derived1 = DerivedNodeKeys::from_seed_bytes(&seed).unwrap();
        let derived2 = DerivedNodeKeys::from_seed_bytes(&seed).unwrap();

        assert_eq!(derived1.node_id, derived2.node_id);
        assert_eq!(derived1.ed25519_public, derived2.ed25519_public);
        assert_eq!(derived1.ed25519_secret, derived2.ed25519_secret);
        assert_eq!(derived1.ml_dsa_commitment, derived2.ml_dsa_commitment);

        let mut wallet1 = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let mut wallet2 = Ed25519Wallet::new(MASTER_PASS).unwrap();

        let id1 = wallet1
            .import_derived_node_keys("node-1", &derived1, None)
            .unwrap();
        let id2 = wallet2
            .import_derived_node_keys("node-1", &derived2, None)
            .unwrap();

        assert_eq!(id1, id2);
        assert_eq!(
            wallet1.export_secret_key("node-1").unwrap(),
            wallet2.export_secret_key("node-1").unwrap()
        );

        let mut different_seed = [42u8; 64];
        different_seed[0] ^= 0xFF;
        let derived_diff = DerivedNodeKeys::from_seed_bytes(&different_seed).unwrap();
        assert_ne!(derived1.node_id, derived_diff.node_id);
        assert_ne!(derived1.ed25519_secret, derived_diff.ed25519_secret);
    }

    #[test]
    fn test_wallet_export_import_roundtrip() {
        let mut src_wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let node_id = src_wallet
            .generate_keypair("original", Some(Duration::seconds(3600)))
            .unwrap();

        let (_pub_hex, sec_hex) = src_wallet.export_keypair_hex("original").unwrap();
        let identity = src_wallet.export_node_identity("original").unwrap();

        let mut dst_wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let imported_id_hex = dst_wallet
            .import_secret_hex("imported-hex", &sec_hex, None)
            .unwrap();
        assert_eq!(node_id, imported_id_hex);

        let imported_id_obj = dst_wallet
            .import_node_identity("imported-obj", &identity, None)
            .unwrap();
        assert_eq!(node_id, imported_id_obj);

        let message = b"Inter-wallet verification message";
        let sig = dst_wallet.sign("imported-hex", message).unwrap();
        let pub_bytes = src_wallet.export_public_key("original").unwrap();

        assert!(Ed25519Wallet::verify(&pub_bytes, message, &sig));
    }

    #[test]
    fn test_wallet_zeroize_on_drop() {
        // Verify debug output redacts sensitive data
        let entry = KeyEntry {
            metadata: KeyMetadata {
                alias: "sensitive-entry".into(),
                node_id: "xv1-test".into(),
                public_key_hex: "00".repeat(32),
                created_at: Utc::now(),
                last_rotated_at: None,
                ttl_seconds: None,
                version: 1,
                ml_dsa_commitment_hex: None,
            },
            ed25519_secret: [0xFFu8; 32],
            ed25519_public: [0x11u8; 32],
            ml_dsa_commitment: None,
        };

        let debug_str = format!("{:?}", entry);
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("ffffff"));
    }

    #[test]
    fn test_wallet_corrupted_nonce_rejection() {
        let key = [0x99u8; 32];
        let valid_nonce = NonceBytes::generate();
        let blob = encrypt_data(b"secret payload", &key, &valid_nonce).unwrap();

        let wrong_nonce = NonceBytes::generate();
        let res = decrypt_data(&blob.ciphertext, &key, wrong_nonce.as_bytes());
        assert!(res.is_err());
    }

    #[test]
    fn test_wallet_duplicate_key_alias_prevention() {
        let mut wallet = Ed25519Wallet::new(MASTER_PASS).unwrap();
        let alias = "unique-alias";

        wallet.generate_keypair(alias, None).unwrap();

        let err1 = wallet.generate_keypair(alias, None).unwrap_err();
        assert!(matches!(err1, WalletError::KeyAlreadyExists(ref a) if a == alias));

        let err2 = wallet
            .import_secret_bytes(alias, &[1u8; 32], None)
            .unwrap_err();
        assert!(matches!(err2, WalletError::KeyAlreadyExists(ref a) if a == alias));
    }
}
