//! Local vault: seal 32-byte entropy with Argon2id(PIN) ⊕ device_key + AES-256-GCM.
//!
//! WebAuthn is modeled as an optional 32-byte `device_key` produced outside this
//! crate (passkey / OS keystore). PIN alone works for headless/CLI Fase 0.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const VAULT_VERSION: u8 = 1;
const ARGON2_M_KIB: u32 = 64 * 1024;
const ARGON2_T: u32 = 3;
const ARGON2_P: u32 = 1;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("invalid pin or device key")]
    AuthFailed,
    #[error("vault corrupt: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Sealed vault stored on disk (no plaintext seed).
#[derive(Clone, Serialize, Deserialize)]
pub struct SealedVault {
    pub version: u8,
    pub salt: [u8; 16],
    pub nonce: [u8; 12],
    /// Ciphertext of: entropy(32) || passphrase_len(u16 BE) || passphrase_utf8
    pub ciphertext: Vec<u8>,
    /// Whether a device_key was mixed in at seal time.
    pub uses_device_key: bool,
}

impl std::fmt::Debug for SealedVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealedVault")
            .field("version", &self.version)
            .field("ciphertext_len", &self.ciphertext.len())
            .field("uses_device_key", &self.uses_device_key)
            .finish()
    }
}

/// Opened vault payload.
#[derive(Clone)]
pub struct OpenedVault {
    pub entropy: [u8; 32],
    pub passphrase: String,
}

impl SealedVault {
    pub fn seal(
        entropy: &[u8; 32],
        passphrase: &str,
        pin: &str,
        device_key: Option<&[u8; 32]>,
    ) -> Result<Self> {
        if pin.is_empty() {
            anyhow::bail!("PIN must not be empty");
        }
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        rand::rngs::OsRng.fill_bytes(&mut nonce);

        let key = derive_vault_key(pin, &salt, device_key)?;
        let mut plaintext = Vec::with_capacity(32 + 2 + passphrase.len());
        plaintext.extend_from_slice(entropy);
        let plen = u16::try_from(passphrase.len()).map_err(|_| anyhow!("passphrase too long"))?;
        plaintext.extend_from_slice(&plen.to_be_bytes());
        plaintext.extend_from_slice(passphrase.as_bytes());

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| anyhow!("AES key init: {e}"))?;
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
            .map_err(|e| anyhow!("encrypt failed: {e}"))?;

        Ok(Self {
            version: VAULT_VERSION,
            salt,
            nonce,
            ciphertext: ct,
            uses_device_key: device_key.is_some(),
        })
    }

    pub fn unlock(
        &self,
        pin: &str,
        device_key: Option<&[u8; 32]>,
    ) -> Result<OpenedVault, VaultError> {
        if self.version != VAULT_VERSION {
            return Err(VaultError::Corrupt(format!(
                "unsupported vault version {}",
                self.version
            )));
        }
        if self.uses_device_key && device_key.is_none() {
            return Err(VaultError::AuthFailed);
        }
        let key = derive_vault_key(pin, &self.salt, device_key).map_err(VaultError::Other)?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| VaultError::Other(anyhow!("{e}")))?;
        let pt = cipher
            .decrypt(Nonce::from_slice(&self.nonce), self.ciphertext.as_ref())
            .map_err(|_| VaultError::AuthFailed)?;
        if pt.len() < 34 {
            return Err(VaultError::Corrupt("plaintext too short".into()));
        }
        let mut entropy = [0u8; 32];
        entropy.copy_from_slice(&pt[..32]);
        let plen = u16::from_be_bytes([pt[32], pt[33]]) as usize;
        if pt.len() != 34 + plen {
            return Err(VaultError::Corrupt("passphrase length mismatch".into()));
        }
        let passphrase = String::from_utf8(pt[34..].to_vec())
            .map_err(|e| VaultError::Corrupt(e.to_string()))?;
        Ok(OpenedVault { entropy, passphrase })
    }
}

fn derive_vault_key(
    pin: &str,
    salt: &[u8; 16],
    device_key: Option<&[u8; 32]>,
) -> Result<[u8; 32]> {
    let params = Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(32))
        .map_err(|e| anyhow!("argon2 params: {e}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut pin_key = [0u8; 32];
    argon
        .hash_password_into(pin.as_bytes(), salt, &mut pin_key)
        .map_err(|e| anyhow!("argon2 hash: {e}"))?;

    if let Some(dk) = device_key {
        for i in 0..32 {
            pin_key[i] ^= dk[i];
        }
    }
    Ok(pin_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_unlock_roundtrip() {
        let entropy = [0x11u8; 32];
        let vault = SealedVault::seal(&entropy, "phrase", "424242", None).unwrap();
        let open = vault.unlock("424242", None).unwrap();
        assert_eq!(open.entropy, entropy);
        assert_eq!(open.passphrase, "phrase");
        assert!(matches!(
            vault.unlock("wrong", None),
            Err(VaultError::AuthFailed)
        ));
    }

    #[test]
    fn device_key_required_when_set() {
        let dk = [0x55u8; 32];
        let entropy = [0x22u8; 32];
        let vault = SealedVault::seal(&entropy, "", "111111", Some(&dk)).unwrap();
        assert!(vault.unlock("111111", None).is_err());
        let open = vault.unlock("111111", Some(&dk)).unwrap();
        assert_eq!(open.entropy, entropy);
    }
}
