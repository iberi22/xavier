// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Local encrypted secrets vault for Xavier
//!
//! Stores system-level secrets (API keys, tokens) in encrypted files
//! on the local filesystem.

use anyhow::{anyhow, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};
use crate::security::encryption_keys::MasterKeyManager;

/// Local secrets vault using filesystem storage and AES-256-GCM encryption.
pub struct LocalSecretsVault {
    storage_dir: PathBuf,
    vault_key: [u8; 32],
}

impl LocalSecretsVault {
    /// Create a new vault instance
    pub fn new(storage_dir: impl AsRef<Path>, vault_key: [u8; 32]) -> Self {
        Self {
            storage_dir: storage_dir.as_ref().to_path_buf(),
            vault_key,
        }
    }

    /// Initialize the vault at default location ~/.xavier/secrets/
    pub fn init_default(master_key_mgr: &MasterKeyManager) -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not find home directory"))?;
        let storage_dir = home.join(".xavier").join("secrets");

        if !storage_dir.exists() {
            fs::create_dir_all(&storage_dir)?;
        }

        let vault_key = master_key_mgr.vault_key()?;
        Ok(Self::new(storage_dir, vault_key))
    }

    /// Store a secret by name
    pub fn set(&self, name: &str, value: &str) -> Result<()> {
        let path = self.storage_dir.join(format!("{}.enc", name));
        let nonce = NonceBytes::generate();
        let encrypted = aes_encrypt(value.as_bytes(), &self.vault_key, &nonce)
            .map_err(|e| anyhow!("Encryption failed: {}", e))?;

        fs::write(path, encrypted)?;
        Ok(())
    }

    /// Retrieve a secret by name
    pub fn get(&self, name: &str) -> Result<String> {
        let path = self.storage_dir.join(format!("{}.enc", name));
        if !path.exists() {
            return Err(anyhow!("Secret '{}' not found", name));
        }

        let encrypted_data = fs::read(path)?;
        let decrypted_bytes = aes_decrypt(&encrypted_data, &self.vault_key)
            .map_err(|e| anyhow!("Decryption failed for secret '{}': {}", name, e))?;

        let value = String::from_utf8(decrypted_bytes)
            .map_err(|e| anyhow!("Secret contains invalid UTF-8: {}", e))?;

        Ok(value)
    }

    /// Delete a secret by name
    pub fn delete(&self, name: &str) -> Result<()> {
        let path = self.storage_dir.join(format!("{}.enc", name));
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// List all stored secret names
    pub fn list(&self) -> Result<Vec<String>> {
        let mut secrets = Vec::new();
        for entry in fs::read_dir(&self.storage_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("enc") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    secrets.push(name.to_string());
                }
            }
        }
        Ok(secrets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_vault_roundtrip() {
        let tmp = tempdir().unwrap();
        let vault_key = [0u8; 32];
        let vault = LocalSecretsVault::new(tmp.path(), vault_key);

        let name = "openai-api-key";
        let value = "sk-1234567890abcdef";

        vault.set(name, value).unwrap();
        let retrieved = vault.get(name).unwrap();
        assert_eq!(value, retrieved);

        let secrets = vault.list().unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0], name);

        vault.delete(name).unwrap();
        assert!(vault.get(name).is_err());
    }
}
