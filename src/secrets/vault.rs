//! Hardware-backed secret vault using the system keyring.
//!
//! Provides secure storage for sensitive tokens (XAVIER_TOKEN, API keys)
//! using DPAPI/Credential Manager on Windows and Keychain on macOS.
//!
//! Falls back to local encrypted file vault when the system keyring
//! is unavailable (e.g., non-interactive Windows sessions, CI, etc.).

use std::sync::OnceLock;

use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};
use crate::secrets::{SecretError, SecretResult};
use crate::security::encryption_keys::MasterKeyManager;
use keyring::Entry;

/// Global fallback vault storage, lazily initialized
struct VaultBackend {
    #[allow(dead_code)]
    service_name: String,
    storage_dir: std::path::PathBuf,
    vault_key: [u8; 32],
}

static BACKEND: OnceLock<Option<VaultBackend>> = OnceLock::new();

fn init_backend(service_name: &str) -> &'static Option<VaultBackend> {
    BACKEND.get_or_init(|| {
        // Initialize master key (handles keyring + file fallback internally)
        match MasterKeyManager::load_or_init() {
            Ok(mkm) => {
                let home = match dirs::home_dir() {
                    Some(h) => h,
                    None => {
                        tracing::warn!("HardwareVault: no home dir, fallback vault unavailable");
                        return None;
                    }
                };
                let storage_dir = home.join(".xavier").join("secrets");
                if let Err(e) = std::fs::create_dir_all(&storage_dir) {
                    tracing::warn!("HardwareVault: cannot create secrets dir: {e}");
                    return None;
                }
                // Derive a vault-specific key from the master key
                use sha2::{Digest, Sha256};
                let vault_key: [u8; 32] = {
                    let mut hasher = Sha256::new();
                    hasher.update(b"xavier-hardware-vault-fallback-v1");
                    hasher.update(mkm.vault_key().unwrap_or([0u8; 32]));
                    hasher.finalize().into()
                };
                Some(VaultBackend {
                    service_name: service_name.to_string(),
                    storage_dir,
                    vault_key,
                })
            }
            Err(e) => {
                tracing::warn!("HardwareVault: master key init failed: {e}");
                None
            }
        }
    })
}

pub struct HardwareVault {
    service_name: String,
}

impl HardwareVault {
    pub fn new(service_name: &str) -> Self {
        Self {
            service_name: service_name.to_string(),
        }
    }

    pub fn store_secret(&self, key: &str, value: &str) -> SecretResult<()> {
        // Try keyring first
        if let Err(e) = self.try_keyring_store(key, value) {
            tracing::debug!("Keyring store failed, using fallback vault: {e}");
            self.try_fallback_store(key, value)?;
        }
        // Always also store in fallback (belt and suspenders)
        let _ = self.try_fallback_store(key, value);
        Ok(())
    }

    pub fn get_secret(&self, key: &str) -> SecretResult<String> {
        // Try keyring first
        match self.try_keyring_get(key) {
            Ok(val) => return Ok(val),
            Err(e) => {
                tracing::debug!("Keyring get failed, trying fallback vault: {e}");
            }
        }
        // Fallback to local encrypted vault
        self.try_fallback_get(key)
    }

    pub fn delete_secret(&self, key: &str) -> SecretResult<()> {
        let keyring_ok = self.try_keyring_delete(key).is_ok();
        let fallback_ok = self.try_fallback_delete(key).is_ok();
        if keyring_ok || fallback_ok {
            Ok(())
        } else {
            Err(SecretError::NotFound(key.to_string()))
        }
    }

    // --- keyring helpers ---

    fn try_keyring_store(&self, key: &str, value: &str) -> SecretResult<()> {
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| SecretError::ProviderError(format!("Keyring error: {}", e)))?;
        entry.set_password(value)
            .map_err(|e| SecretError::ProviderError(format!("Failed to store secret: {}", e)))?;
        Ok(())
    }

    fn try_keyring_get(&self, key: &str) -> SecretResult<String> {
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| SecretError::ProviderError(format!("Keyring error: {}", e)))?;
        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => SecretError::NotFound(key.to_string()),
            _ => SecretError::ProviderError(format!("Failed to retrieve secret: {}", e)),
        })
    }

    fn try_keyring_delete(&self, key: &str) -> SecretResult<()> {
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| SecretError::ProviderError(format!("Keyring error: {}", e)))?;
        entry.delete_credential().map_err(|e| match e {
            keyring::Error::NoEntry => SecretError::NotFound(key.to_string()),
            _ => SecretError::ProviderError(format!("Failed to delete secret: {}", e)),
        })?;
        Ok(())
    }

    // --- fallback vault helpers ---

    fn backend(&self) -> SecretResult<&'static VaultBackend> {
        init_backend(&self.service_name);
        BACKEND.get().ok_or_else(|| {
            SecretError::ProviderError("Vault backend not initialized".to_string())
        })?.as_ref().ok_or_else(|| {
            SecretError::ProviderError("Fallback vault unavailable (no master key)".to_string())
        })
    }

    fn try_fallback_store(&self, key: &str, value: &str) -> SecretResult<()> {
        let backend = self.backend()?;
        let path = backend.storage_dir.join(format!("{}.enc", key));
        let nonce = NonceBytes::generate();
        let encrypted = aes_encrypt(value.as_bytes(), &backend.vault_key, &nonce)
            .map_err(|e| SecretError::ProviderError(format!("Encryption failed: {e}")))?;
        std::fs::write(path, encrypted)
            .map_err(|e| SecretError::ProviderError(format!("Fallback write failed: {e}")))?;
        Ok(())
    }

    fn try_fallback_get(&self, key: &str) -> SecretResult<String> {
        let backend = self.backend()?;
        let path = backend.storage_dir.join(format!("{}.enc", key));
        if !path.exists() {
            return Err(SecretError::NotFound(key.to_string()));
        }
        let encrypted_data = std::fs::read(&path)
            .map_err(|_| SecretError::NotFound(key.to_string()))?;
        let decrypted = aes_decrypt(&encrypted_data, &backend.vault_key)
            .map_err(|e| SecretError::ProviderError(format!("Decryption failed: {e}")))?;
        String::from_utf8(decrypted)
            .map_err(|_| SecretError::ProviderError("Secret contains invalid UTF-8".to_string()))
    }

    fn try_fallback_delete(&self, key: &str) -> SecretResult<()> {
        let backend = self.backend()?;
        let path = backend.storage_dir.join(format!("{}.enc", key));
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| SecretError::ProviderError(format!("Fallback delete failed: {e}")))?;
            Ok(())
        } else {
            Err(SecretError::NotFound(key.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires interactive keyring access"]
    fn test_hardware_vault_ops() {
        let vault = HardwareVault::new("xavier-test-vault");
        let key = "test-token";
        let value = "super-secret-value";

        vault.store_secret(key, value).unwrap();
        assert_eq!(vault.get_secret(key).unwrap(), value);
        vault.delete_secret(key).unwrap();
        assert!(vault.get_secret(key).is_err());
    }
}
