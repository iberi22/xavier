//! Pairing Secret Registry — Persistent storage for pending pairing secrets
//!
//! Stores secrets generated for node pairing to verify them during handshake.
//! Secrets are stored as a JSON map of secret -> metadata.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn hash_pairing_secret(secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    crate::utils::crypto::hex_encode(&hasher.finalize())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingSecretMetadata {
    pub expires_at: u64,
    pub created_at: i64,
}

pub struct PairingSecretRegistry {
    secrets: HashMap<String, PairingSecretMetadata>,
    storage_path: PathBuf,
}

impl PairingSecretRegistry {
    /// Load.
    pub fn load() -> Result<Self> {
        let config_dir = if let Ok(val) = std::env::var("XAVIER_CONFIG_DIR") {
            PathBuf::from(val)
        } else {
            dirs::config_dir()
                .context("Could not determine config directory")?
                .join("xavier")
        };
        Self::load_from(config_dir.join("mesh_pairing_secrets.json"))
    }

    /// Load from.
    pub fn load_from(storage_path: PathBuf) -> Result<Self> {
        if let Some(parent) = storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !storage_path.exists() {
            return Ok(Self {
                secrets: HashMap::new(),
                storage_path,
            });
        }

        let raw = std::fs::read_to_string(&storage_path)
            .context("Failed to read pairing secret registry file")?;
        let secrets: HashMap<String, PairingSecretMetadata> =
            serde_json::from_str(&raw).context("Failed to parse pairing secret registry JSON")?;

        Ok(Self {
            secrets,
            storage_path,
        })
    }

    /// Save using a private 0600 file and atomic rename.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.secrets)?;
        let temp_path = self
            .storage_path
            .with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
        crate::keystore::create_private_file_new(&temp_path, json.as_bytes())
            .context("Failed to write temporary pairing secret registry file")?;
        std::fs::rename(&temp_path, &self.storage_path)
            .context("Failed to replace pairing secret registry file")?;
        Ok(())
    }

    /// Register secret.
    pub fn register_secret(&mut self, secret: String, expires_at: u64) -> Result<()> {
        let key = hash_pairing_secret(&secret);
        self.secrets.insert(
            key,
            PairingSecretMetadata {
                expires_at,
                created_at: chrono::Utc::now().timestamp(),
            },
        );
        self.save()
    }

    /// Verify and remove.
    pub fn verify_and_remove(&mut self, secret: &str) -> Result<bool> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let hashed = hash_pairing_secret(secret);
        let key = if self.secrets.contains_key(&hashed) {
            Some(hashed)
        } else if self.secrets.contains_key(secret) {
            Some(secret.to_string())
        } else {
            None
        };

        if let Some(k) = key {
            if let Some(metadata) = self.secrets.remove(&k) {
                self.save()?;
                return Ok(metadata.expires_at >= now);
            }
        }
        Ok(false)
    }

    /// Cleanup expired.
    pub fn cleanup_expired(&mut self) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let initial_len = self.secrets.len();
        self.secrets.retain(|_, meta| meta.expires_at >= now);

        if self.secrets.len() < initial_len {
            self.save()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pairing_registry_permissions_and_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairing.json");
        let mut reg = PairingSecretRegistry::load_from(path.clone()).unwrap();
        let secret = "secret-token-xyz-123".to_string();
        reg.register_secret(secret.clone(), 9_999_999_999).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&path).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(
                mode, 0o600,
                "pairing secret registry file must be mode 0600"
            );
        }

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains(&secret),
            "pairing secret must not be stored in plaintext"
        );

        assert!(reg.verify_and_remove(&secret).unwrap());
        assert!(!reg.verify_and_remove(&secret).unwrap());
    }
}
