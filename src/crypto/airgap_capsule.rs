//! Air-Gap Offline Capsule Protocol & Binary Packaging
//!
//! Provides authenticated encryption (AES-256-GCM) with key derivation (Argon2id)
//! for packaging cold-storage files, folders, and cryptographic assets into `.swal_capsule`
//! containers for secure physical transport (USB drives, air-gapped computers).

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{bail, Context, Result};
use argon2::Argon2;
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Magic bytes identifying a SWAL Airgap Capsule file: `SWALCAPS`
pub const AIRGAP_MAGIC: &[u8; 8] = b"SWALCAPS";

/// Current binary specification format version
pub const AIRGAP_VERSION: u8 = 1;

/// Payload kind stored within the encrypted capsule
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapsulePayloadKind {
    SingleFile = 1,
    DirectoryArchive = 2,
    DatabaseBackup = 3,
    MemoryDump = 4,
    SecretsVault = 5,
}

impl CapsulePayloadKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::SingleFile),
            2 => Some(Self::DirectoryArchive),
            3 => Some(Self::DatabaseBackup),
            4 => Some(Self::MemoryDump),
            5 => Some(Self::SecretsVault),
            _ => None,
        }
    }
}

/// Metadata header describing the encrypted capsule
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AirgapCapsuleHeader {
    pub version: u8,
    pub payload_kind: CapsulePayloadKind,
    pub original_filename: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub salt: String,
    pub nonce: String,
    pub ciphertext_length: usize,
}

/// Main structure representing the capsule engine
pub struct AirgapCapsule;

impl AirgapCapsule {
    /// Encrypts plaintext data with an Argon2id-derived key and packages it into a binary capsule
    pub fn pack_with_passphrase(
        plaintext: &[u8],
        payload_kind: CapsulePayloadKind,
        original_filename: String,
        author: String,
        passphrase: &str,
    ) -> Result<Vec<u8>> {
        let mut rng = rand::thread_rng();
        let mut salt = [0u8; 16];
        rng.fill_bytes(&mut salt);

        let mut nonce_bytes = [0u8; 12];
        rng.fill_bytes(&mut nonce_bytes);

        // Derive 32-byte key using Argon2id
        let mut key = [0u8; 32];
        let argon2 = Argon2::default();
        argon2
            .hash_password_into(passphrase.as_bytes(), &salt, &mut key)
            .map_err(|e| anyhow::anyhow!("Argon2id key derivation error: {}", e))?;

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("Cipher creation error: {}", e))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("AES-GCM encryption error: {}", e))?;

        let header = AirgapCapsuleHeader {
            version: AIRGAP_VERSION,
            payload_kind,
            original_filename,
            author,
            created_at: Utc::now(),
            salt: crate::crypto::hex_encode(salt),
            nonce: crate::crypto::hex_encode(nonce_bytes),
            ciphertext_length: ciphertext.len(),
        };

        let header_json = serde_json::to_vec(&header).context("Failed to serialize header")?;
        let header_len = header_json.len() as u32;

        let mut output = Vec::with_capacity(8 + 4 + header_json.len() + ciphertext.len());
        output.extend_from_slice(AIRGAP_MAGIC);
        output.extend_from_slice(&header_len.to_le_bytes());
        output.extend_from_slice(&header_json);
        output.extend_from_slice(&ciphertext);

        Ok(output)
    }

    /// Inspects and parses the unencrypted metadata header without decrypting payload
    pub fn inspect_header(capsule_bytes: &[u8]) -> Result<AirgapCapsuleHeader> {
        if capsule_bytes.len() < 12 {
            bail!("Capsule is too short to contain valid header");
        }

        if &capsule_bytes[..8] != AIRGAP_MAGIC {
            bail!("Invalid capsule magic bytes. Expected SWALCAPS");
        }

        let header_len = u32::from_le_bytes(
            capsule_bytes[8..12]
                .try_into()
                .context("Failed to read header length")?,
        ) as usize;

        if capsule_bytes.len() < 12 + header_len {
            bail!("Capsule truncated: missing header data");
        }

        let header_slice = &capsule_bytes[12..12 + header_len];
        let header: AirgapCapsuleHeader =
            serde_json::from_slice(header_slice).context("Invalid capsule header JSON")?;

        Ok(header)
    }

    /// Decrypts and authenticates a capsule file using the provided passphrase
    pub fn unpack_with_passphrase(
        capsule_bytes: &[u8],
        passphrase: &str,
    ) -> Result<(AirgapCapsuleHeader, Vec<u8>)> {
        let header = Self::inspect_header(capsule_bytes)?;

        let header_len = u32::from_le_bytes(capsule_bytes[8..12].try_into()?) as usize;
        let ciphertext = &capsule_bytes[12 + header_len..];

        if ciphertext.len() != header.ciphertext_length {
            bail!("Ciphertext length mismatch: capsule header corrupt or truncated");
        }

        let salt = crate::crypto::hex_decode(&header.salt).context("Failed to decode salt hex")?;
        let nonce_bytes =
            crate::crypto::hex_decode(&header.nonce).context("Failed to decode nonce hex")?;

        if salt.len() != 16 || nonce_bytes.len() != 12 {
            bail!("Invalid salt or nonce dimensions in capsule header");
        }

        let mut key = [0u8; 32];
        let argon2 = Argon2::default();
        argon2
            .hash_password_into(passphrase.as_bytes(), &salt, &mut key)
            .map_err(|e| anyhow::anyhow!("Argon2id derivation error: {}", e))?;

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("Cipher creation error: {}", e))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow::anyhow!("Decryption failed: incorrect passphrase or tampered capsule data"))?;

        Ok((header, plaintext))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_unpack_roundtrip() {
        let original_data = b"SWAL Network Sovereign Node Genesis Key: 0xDEADBEEF123456";
        let passphrase = "correct-battery-horse-stapler";

        let packed = AirgapCapsule::pack_with_passphrase(
            original_data,
            CapsulePayloadKind::SecretsVault,
            "keys.json".to_string(),
            "operator-bela".to_string(),
            passphrase,
        )
        .expect("Packaging should succeed");

        assert_eq!(&packed[..8], AIRGAP_MAGIC);

        let header = AirgapCapsule::inspect_header(&packed).expect("Inspection should succeed");
        assert_eq!(header.payload_kind, CapsulePayloadKind::SecretsVault);
        assert_eq!(header.original_filename, "keys.json");
        assert_eq!(header.author, "operator-bela");

        let (unpacked_header, plaintext) =
            AirgapCapsule::unpack_with_passphrase(&packed, passphrase)
                .expect("Unpacking should succeed");

        assert_eq!(unpacked_header, header);
        assert_eq!(plaintext, original_data);
    }

    #[test]
    fn test_invalid_passphrase_fails() {
        let original_data = b"Classified RAG Corpus";
        let packed = AirgapCapsule::pack_with_passphrase(
            original_data,
            CapsulePayloadKind::MemoryDump,
            "rag.bin".to_string(),
            "hermes".to_string(),
            "correct-passphrase",
        )
        .unwrap();

        let result = AirgapCapsule::unpack_with_passphrase(&packed, "wrong-passphrase");
        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let original_data = b"Critical Database Backup";
        let mut packed = AirgapCapsule::pack_with_passphrase(
            original_data,
            CapsulePayloadKind::DatabaseBackup,
            "db.sqlite".to_string(),
            "admin".to_string(),
            "safe-password",
        )
        .unwrap();

        // Corrupt last byte of ciphertext (auth tag)
        let last_idx = packed.len() - 1;
        packed[last_idx] ^= 0xFF;

        let result = AirgapCapsule::unpack_with_passphrase(&packed, "safe-password");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_magic_bytes() {
        let mut corrupted = vec![0u8; 32];
        corrupted[..8].copy_from_slice(b"BADMAGIC");

        let result = AirgapCapsule::inspect_header(&corrupted);
        assert!(result.is_err());
    }
}