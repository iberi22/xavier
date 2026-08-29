//! MeshKeyringStore — OS Keyring & Ed25519 Keystore Storage
//!
//! Provides secure key management for Xavier Mesh node identities using system
//! keyring (via `keyring` crate) with an AES-256-GCM local encrypted file
//! fallback when system keyring is unaccessible or operating in headless
//! environments.

use anyhow::{Context, Result};
use keyring::Entry;
use sha2::Digest;
use std::path::PathBuf;

use xavier::crypto::encryption::{decrypt_data, encrypt_data, NonceBytes};
use xavier::crypto::NONCE_SIZE;
use xavier::mesh::node::NodeIdentity;

/// Service name used for OS keyring entries.
pub const SERVICE_NAME: &str = "xavier-mesh";

/// Secure key storage for Xavier Mesh node identities and Ed25519 private keys.
#[derive(Debug, Clone)]
pub struct MeshKeyringStore {
    service: String,
    fallback_dir: PathBuf,
}

impl Default for MeshKeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MeshKeyringStore {
    /// Create a new `MeshKeyringStore` with standard default paths.
    pub fn new() -> Self {
        let fallback_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".xavier"))
            .join("xavier")
            .join("keystore");
        Self {
            service: SERVICE_NAME.to_string(),
            fallback_dir,
        }
    }

    /// Create a `MeshKeyringStore` with a custom fallback storage path.
    pub fn with_path(fallback_path: PathBuf) -> Self {
        Self {
            service: SERVICE_NAME.to_string(),
            fallback_dir: fallback_path,
        }
    }

    /// Save a complete `NodeIdentity` into the keyring or encrypted fallback store.
    pub fn save_identity(&self, identity: &NodeIdentity) -> Result<()> {
        self.save_key(identity.node_id.as_str(), identity.private_key_bytes())
    }

    /// Save Ed25519 private key seed bytes for `node_id`.
    pub fn save_key(&self, node_id: &str, private_key_bytes: &[u8]) -> Result<()> {
        if private_key_bytes.len() != 32 {
            anyhow::bail!("Ed25519 private key must be exactly 32 bytes");
        }

        let hex_key = xavier::crypto::hex_encode(private_key_bytes);

        // Attempt OS keyring first, verifying persistence across entry handles
        if let Ok(entry) = Entry::new(&self.service, node_id) {
            if entry.set_password(&hex_key).is_ok() {
                if let Ok(verify_entry) = Entry::new(&self.service, node_id) {
                    if let Ok(retrieved) = verify_entry.get_password() {
                        if retrieved == hex_key {
                            return Ok(());
                        }
                    }
                }
            }
        }

        // Fallback to AES-256-GCM encrypted local store
        self.save_key_fallback(node_id, private_key_bytes)
    }

    /// Load a `NodeIdentity` from keyring or encrypted fallback store.
    pub fn load_identity(&self, node_id: &str) -> Result<NodeIdentity> {
        let private_key = self.load_key(node_id)?;
        NodeIdentity::from_private_key_bytes(&private_key)
    }

    /// Load Ed25519 private key seed bytes for `node_id`.
    pub fn load_key(&self, node_id: &str) -> Result<Vec<u8>> {
        // Attempt OS keyring first
        if let Ok(entry) = Entry::new(&self.service, node_id) {
            if let Ok(password) = entry.get_password() {
                if let Ok(bytes) = xavier::crypto::hex_decode(&password) {
                    if bytes.len() == 32 {
                        return Ok(bytes);
                    }
                }
            }
        }

        // Attempt encrypted local fallback store
        self.load_key_fallback(node_id)
    }

    /// Delete stored key for `node_id` from both keyring and local store.
    pub fn delete_key(&self, node_id: &str) -> Result<()> {
        let mut deleted = false;

        // Try deleting from OS keyring
        if let Ok(entry) = Entry::new(&self.service, node_id) {
            if entry.delete_credential().is_ok() {
                deleted = true;
            }
        }

        // Try deleting from fallback file store
        let file_path = self.fallback_dir.join(format!("{}.enc", node_id));
        if file_path.exists() {
            std::fs::remove_file(&file_path)?;
            deleted = true;
        }

        if deleted {
            Ok(())
        } else {
            anyhow::bail!("No stored key found for node_id: {}", node_id)
        }
    }

    /// Check if OS keyring service is accessible and functioning across entries.
    pub fn is_keyring_available(&self) -> bool {
        let test_key = format!("__test_probe_{}", uuid::Uuid::new_v4());
        if let Ok(entry) = Entry::new(&self.service, &test_key) {
            if entry.set_password("probe_val").is_ok() {
                if let Ok(verify_entry) = Entry::new(&self.service, &test_key) {
                    if let Ok(val) = verify_entry.get_password() {
                        let _ = verify_entry.delete_credential();
                        return val == "probe_val";
                    }
                }
            }
        }
        false
    }

    // ---------------------------------------------------------------------------
    // Encrypted Fallback Storage Helpers
    // ---------------------------------------------------------------------------

    fn derive_master_key(&self) -> Result<[u8; 32]> {
        std::fs::create_dir_all(&self.fallback_dir)?;
        let salt_path = self.fallback_dir.join(".master.salt");

        let salt = if salt_path.exists() {
            std::fs::read(&salt_path)?
        } else {
            let mut s = [0u8; 16];
            rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut s);
            std::fs::write(&salt_path, s)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&salt_path, std::fs::Permissions::from_mode(0o600));
            }
            s.to_vec()
        };

        let mut hasher = sha2::Sha256::new();
        hasher.update(b"xavier-mesh-fallback-key-v1");
        hasher.update(&salt);
        let res = hasher.finalize();

        let mut key = [0u8; 32];
        key.copy_from_slice(&res);
        Ok(key)
    }

    fn save_key_fallback(&self, node_id: &str, private_key_bytes: &[u8]) -> Result<()> {
        let key = self.derive_master_key()?;
        let nonce = NonceBytes::generate();

        let blob = encrypt_data(private_key_bytes, &key, &nonce)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {e:?}"))?;

        let file_bytes = blob.to_bytes();
        let file_path = self.fallback_dir.join(format!("{}.enc", node_id));

        std::fs::write(&file_path, &file_bytes)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    fn load_key_fallback(&self, node_id: &str) -> Result<Vec<u8>> {
        let file_path = self.fallback_dir.join(format!("{}.enc", node_id));
        if !file_path.exists() {
            anyhow::bail!("Key file not found for node_id: {}", node_id);
        }

        let key = self.derive_master_key()?;
        let file_bytes = std::fs::read(&file_path)?;

        if file_bytes.len() < NONCE_SIZE {
            anyhow::bail!("Encrypted key payload corrupted");
        }

        let (nonce_bytes, ciphertext) = file_bytes.split_at(NONCE_SIZE);
        let nonce_arr: [u8; NONCE_SIZE] = nonce_bytes
            .try_into()
            .context("Invalid nonce length in encrypted fallback")?;

        let plaintext = decrypt_data(ciphertext, &key, &nonce_arr)
            .map_err(|e| anyhow::anyhow!("Decryption failed: {e:?}"))?;

        Ok(plaintext)
    }
}
