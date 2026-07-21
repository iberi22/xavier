// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Pairing Secret Registry — Persistent storage for pending pairing secrets
//!
//! Stores secrets generated for node pairing to verify them during handshake.
//! Secrets are stored as a JSON map of secret -> metadata.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.secrets)?;
        std::fs::write(&self.storage_path, json)
            .context("Failed to write pairing secret registry file")?;
        Ok(())
    }

    pub fn register_secret(&mut self, secret: String, expires_at: u64) -> Result<()> {
        self.secrets.insert(
            secret,
            PairingSecretMetadata {
                expires_at,
                created_at: chrono::Utc::now().timestamp(),
            },
        );
        self.save()
    }

    pub fn verify_and_remove(&mut self, secret: &str) -> Result<bool> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if let Some(metadata) = self.secrets.get(secret) {
            if metadata.expires_at >= now {
                self.secrets.remove(secret);
                self.save()?;
                return Ok(true);
            } else {
                self.secrets.remove(secret);
                self.save()?;
                return Ok(false);
            }
        }
        Ok(false)
    }

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
