//! RSA Keypair protection for Xavier
//!
//! Generates RSA-4096 keypairs and stores the private key encrypted
//! using AES-256-GCM.

use anyhow::{anyhow, Result};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding};

use rsa::{RsaPrivateKey, RsaPublicKey};
use std::fs;
use std::path::{Path, PathBuf};

use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};
use crate::security::encryption_keys::MasterKeyManager;

/// Manages the system RSA keypair used for JWT signing and other identity tasks.
pub struct RsaKeypairManager {
    storage_dir: PathBuf,
    encryption_key: [u8; 32],
}

impl RsaKeypairManager {
    pub fn new(storage_dir: impl AsRef<Path>, encryption_key: [u8; 32]) -> Self {
        Self {
            storage_dir: storage_dir.as_ref().to_path_buf(),
            encryption_key,
        }
    }

    pub fn init_default(master_key_mgr: &MasterKeyManager) -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not find home directory"))?;
        let storage_dir = home.join(".xavier");
        let encryption_key = master_key_mgr.rsa_key()?;
        Ok(Self::new(storage_dir, encryption_key))
    }

    /// Generate and save a new keypair if it doesn't exist
    pub fn ensure_keypair(&self) -> Result<()> {
        let priv_path = self.storage_dir.join("rsa_keypair.enc");
        let pub_path = self.storage_dir.join("rsa_keypair.pub");


        if priv_path.exists() && pub_path.exists() {
            return Ok(());
        }

        let mut rng = rand::thread_rng();
        let bits = 4096;
        let priv_key = RsaPrivateKey::new(&mut rng, bits)
            .map_err(|e| anyhow!("Failed to generate RSA key: {}", e))?;
        let pub_key = RsaPublicKey::from(&priv_key);

        // Encode and save public key (plain text PEM)
        let pub_pem = pub_key.to_public_key_pem(LineEnding::LF)
            .map_err(|e| anyhow!("Failed to encode public key: {}", e))?;
        fs::write(pub_path, pub_pem)?;

        // Encode, encrypt and save private key
        let priv_pem = priv_key.to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| anyhow!("Failed to encode private key: {}", e))?;

        let nonce = NonceBytes::generate();
        let encrypted_priv = aes_encrypt(priv_pem.as_bytes(), &self.encryption_key, &nonce)
            .map_err(|e| anyhow!("Failed to encrypt private key: {}", e))?;

        fs::write(priv_path, encrypted_priv)?;

        Ok(())
    }

    /// Load the private key (decrypted)
    pub fn load_private_key(&self) -> Result<RsaPrivateKey> {
        let priv_path = self.storage_dir.join("rsa_keypair.enc");
        if !priv_path.exists() {
            return Err(anyhow!("Private key not found at {:?}", priv_path));
        }

        let encrypted_data = fs::read(priv_path)?;
        let decrypted_bytes = aes_decrypt(&encrypted_data, &self.encryption_key)
            .map_err(|e| anyhow!("Failed to decrypt private key: {}", e))?;

        let pem = String::from_utf8(decrypted_bytes)
            .map_err(|e| anyhow!("Decrypted private key is not valid UTF-8: {}", e))?;

        let priv_key = RsaPrivateKey::from_pkcs8_pem(&pem)
            .map_err(|e| anyhow!("Failed to parse private key: {}", e))?;

        Ok(priv_key)
    }

    /// Load the public key
    pub fn load_public_key(&self) -> Result<RsaPublicKey> {
        let pub_path = self.storage_dir.join("rsa_keypair.pub");
        if !pub_path.exists() {
            return Err(anyhow!("Public key not found at {:?}", pub_path));
        }

        let pem = fs::read_to_string(pub_path)?;
        let pub_key = RsaPublicKey::from_public_key_pem(&pem)
            .map_err(|e| anyhow!("Failed to parse public key: {}", e))?;

        Ok(pub_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::traits::PublicKeyParts;
    use tempfile::tempdir;

    #[test]
    fn test_rsa_keypair_roundtrip() {
        let tmp = tempdir().unwrap();
        let enc_key = [1u8; 32];
        let manager = RsaKeypairManager::new(tmp.path(), enc_key);

        manager.ensure_keypair().unwrap();

        assert!(tmp.path().join("rsa_keypair.enc").exists());
        assert!(tmp.path().join("rsa_keypair.pub").exists());

        let priv_key = manager.load_private_key().unwrap();
        let pub_key = manager.load_public_key().unwrap();

        assert_eq!(priv_key.size(), 4096 / 8);
        assert_eq!(RsaPublicKey::from(&priv_key), pub_key);
    }
}
