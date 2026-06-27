//! Master Key Management and Key Hierarchy for Xavier
//!
//! Handles generation, persistence (keyring + fallback), and derivation of
//! encryption keys using HKDF-SHA256.

use anyhow::{anyhow, Result};
use hkdf::Hkdf;
use keyring::Entry;
use rand::RngCore;
use sha2::Sha256;
use std::fs;
use std::path::PathBuf;

use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};
use crate::utils::crypto::{hex_decode, hex_encode};

const SERVICE_NAME: &str = "xavier-memory-runtime";
const MASTER_KEY_ENTRY: &str = "master-key";
const MASTER_KEY_LEN: usize = 32; // 256 bits

/// Master Key Manager handles the core encryption key for the system.
pub struct MasterKeyManager {
    master_key: [u8; MASTER_KEY_LEN],
}

impl MasterKeyManager {
    /// Load or initialize the master key
    pub fn load_or_init() -> Result<Self> {
        if let Ok(key) = Self::load_from_keyring() {
            return Ok(Self { master_key: key });
        }

        if let Ok(key) = Self::load_from_fallback() {
            // Found in fallback, try to restore to keyring
            let _ = Self::save_to_keyring(&key);
            return Ok(Self { master_key: key });
        }

        // Initialize new master key
        let mut key = [0u8; MASTER_KEY_LEN];
        rand::thread_rng().fill_bytes(&mut key);

        // Persist
        let keyring_res = Self::save_to_keyring(&key);
        let fallback_res = Self::save_to_fallback(&key);

        if keyring_res.is_err() && fallback_res.is_err() {
            return Err(anyhow!("Failed to persist master key to both keyring and fallback storage"));
        }

        Ok(Self { master_key: key })
    }

    fn load_from_keyring() -> Result<[u8; MASTER_KEY_LEN]> {
        let entry = Entry::new(SERVICE_NAME, MASTER_KEY_ENTRY)?;
        let hex_key = entry.get_password()?;
        let key_vec = hex_decode(&hex_key)?;
        let mut key = [0u8; MASTER_KEY_LEN];
        if key_vec.len() != MASTER_KEY_LEN {
            return Err(anyhow!("Invalid master key length in keyring"));
        }
        key.copy_from_slice(&key_vec);
        Ok(key)
    }

    fn save_to_keyring(key: &[u8; MASTER_KEY_LEN]) -> Result<()> {
        let entry = Entry::new(SERVICE_NAME, MASTER_KEY_ENTRY)?;
        entry.set_password(&hex_encode(key))?;
        Ok(())
    }

    fn get_fallback_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".xavier").join("master.key")
    }

    fn get_fallback_encryption_key() -> [u8; 32] {
        // Derive a key from machine-specific info
        use sysinfo::System;
        let mut s = System::new_all();
        s.refresh_all();

        let host_name = System::host_name().unwrap_or_else(|| "unknown-host".to_string());

        let mut hasher = Sha256::default();
        use sha2::Digest;
        hasher.update(host_name.as_bytes());
        hasher.update(b"xavier-fallback-salt-v1");

        let mut key = [0u8; 32];
        key.copy_from_slice(&hasher.finalize());
        key
    }

    fn load_from_fallback() -> Result<[u8; MASTER_KEY_LEN]> {
        let path = Self::get_fallback_path();
        if !path.exists() {
            return Err(anyhow!("Fallback master key file not found"));
        }

        let encrypted_data = fs::read(path)?;
        let enc_key = Self::get_fallback_encryption_key();

        let decrypted = aes_decrypt(&encrypted_data, &enc_key)
            .map_err(|e| anyhow!("Failed to decrypt fallback master key: {}", e))?;

        if decrypted.len() != MASTER_KEY_LEN {
            return Err(anyhow!("Invalid master key length in fallback"));
        }

        let mut key = [0u8; MASTER_KEY_LEN];
        key.copy_from_slice(&decrypted);
        Ok(key)
    }

    fn save_to_fallback(key: &[u8; MASTER_KEY_LEN]) -> Result<()> {
        let path = Self::get_fallback_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let enc_key = Self::get_fallback_encryption_key();
        let nonce = NonceBytes::generate();
        let encrypted = aes_encrypt(key, &enc_key, &nonce)
            .map_err(|e| anyhow!("Failed to encrypt fallback master key: {}", e))?;

        fs::write(path, encrypted)?;
        Ok(())
    }

    /// Derive a specific key using HKDF-SHA256
    pub fn derive_key(&self, info: &[u8], output: &mut [u8]) -> Result<()> {
        let hkdf = Hkdf::<Sha256>::new(None, &self.master_key);
        hkdf.expand(info, output)
            .map_err(|e| anyhow!("HKDF expansion failed: {}", e))?;
        Ok(())
    }

    /// Get the derived auth database key
    pub fn auth_db_key(&self) -> Result<[u8; 32]> {
        let mut key = [0u8; 32];
        self.derive_key(b"xavier-auth-db-v1", &mut key)?;
        Ok(key)
    }

    /// Get the derived RSA keypair encryption key
    pub fn rsa_key(&self) -> Result<[u8; 32]> {
        let mut key = [0u8; 32];
        self.derive_key(b"xavier-rsa-keypair-v1", &mut key)?;
        Ok(key)
    }

    /// Get the derived secrets vault key
    pub fn vault_key(&self) -> Result<[u8; 32]> {
        let mut key = [0u8; 32];
        self.derive_key(b"xavier-secrets-vault-v1", &mut key)?;
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_derivation_consistency() {
        let mut master_key = [0u8; 32];
        master_key[0] = 1;
        let manager = MasterKeyManager { master_key };

        let mut key1 = [0u8; 32];
        manager.derive_key(b"test-info", &mut key1).unwrap();

        let mut key2 = [0u8; 32];
        manager.derive_key(b"test-info", &mut key2).unwrap();

        assert_eq!(key1, key2);

        let mut key3 = [0u8; 32];
        manager.derive_key(b"other-info", &mut key3).unwrap();
        assert_ne!(key1, key3);
    }
}
