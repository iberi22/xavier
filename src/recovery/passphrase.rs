//! Passphrase path: a local bootstrap sealed under a user passphrase.
//!
//! This is the path that survives a **lost or dead disk**, provided the file is
//! copied to media outside the host (USB, printed QR, an offline machine). It is
//! deliberately a separate derivation from [`crate::recovery::MnemonicPath`] so
//! that the two paths fail independently: losing the paper does not lock you out
//! if the file survives, and vice versa.
//!
//! Argon2id parameters are Argon2::default() (m=19456 KiB, t=2, p=1), matching
//! `src/crypto/airgap_capsule.rs:79`. A passphrase is low-entropy, so the cost
//! factor is what makes an offline guessing attack expensive; the salt is
//! random per seal, so two seals of the same passphrase never collide.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{bail, Context, Result};
use argon2::Argon2;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::crypto::hex_decode;
use crate::crypto::hex_encode;

/// Domain separator, so a passphrase-sealed blob can never be mistaken for an
/// airgap capsule or a vault entry by another subsystem.
const SEAL_DOMAIN: &[u8] = b"xavier-recovery-passphrase-v1";

/// Refuse absurdly short passphrases rather than sealing data that is weakly protected.
pub const MIN_PASSPHRASE_LEN: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PassphraseSeal {
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
    /// JSON-encoded Argon2id parameters, so a future cost increase stays readable.
    pub params: String,
}

pub struct PassphrasePath;

impl PassphrasePath {
    /// Seal a key under a passphrase.
    pub fn seal(key: &[u8; 32], passphrase: &str) -> Result<PassphraseSeal> {
        Self::seal_with(key, passphrase, Argon2::default())
    }

    /// [`PassphrasePath::seal`] with explicit Argon2 parameters (tests use a cheap
    /// config so the suite stays fast).
    pub fn seal_with(
        key: &[u8; 32],
        passphrase: &str,
        argon2: Argon2<'_>,
    ) -> Result<PassphraseSeal> {
        if passphrase.chars().count() < MIN_PASSPHRASE_LEN {
            bail!(
                "passphrase too short: {} characters minimum",
                MIN_PASSPHRASE_LEN
            );
        }
        let mut salt = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        let wrapping = Self::derive(passphrase, &salt, &argon2)?;

        let nonce = crate::crypto::encryption::NonceBytes::generate();
        let cipher = Aes256Gcm::new_from_slice(&wrapping).context("cipher creation failed")?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(nonce.as_bytes()), key.as_slice())
            .map_err(|e| anyhow::anyhow!("seal failed: {e}"))?;

        Ok(PassphraseSeal {
            salt: hex_encode(salt),
            nonce: hex_encode(nonce.as_bytes()),
            ciphertext: hex_encode(&ciphertext),
            params: serde_json::json!({
                "variant": "argon2id",
                "domain": String::from_utf8_lossy(SEAL_DOMAIN),
                "m_cost": 19456, "t_cost": 2, "p_cost": 1,
            })
            .to_string(),
        })
    }

    /// Unseal a key with the passphrase.
    pub fn open(seal: &PassphraseSeal, passphrase: &str) -> Result<[u8; 32]> {
        Self::open_with(seal, passphrase, Argon2::default())
    }

    /// [`PassphrasePath::open`] with explicit Argon2 parameters.
    pub fn open_with(
        seal: &PassphraseSeal,
        passphrase: &str,
        argon2: Argon2<'_>,
    ) -> Result<[u8; 32]> {
        let salt = hex_decode(&seal.salt).context("seal salt is not hex")?;
        let nonce = hex_decode(&seal.nonce).context("seal nonce is not hex")?;
        let ciphertext = hex_decode(&seal.ciphertext).context("seal ciphertext is not hex")?;
        if salt.len() != 16 || nonce.len() != 12 {
            bail!("seal has wrong salt/nonce dimensions");
        }

        let wrapping = Self::derive(passphrase, &salt, &argon2)?;
        let cipher = Aes256Gcm::new_from_slice(&wrapping).context("cipher creation failed")?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext.as_slice())
            .map_err(|_| anyhow::anyhow!("recovery failed: wrong passphrase or tampered seal"))?;
        if plaintext.len() != 32 {
            bail!("unsealed key has wrong length {}", plaintext.len());
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&plaintext);
        Ok(out)
    }

    /// Domain-separated: the passphrase is never used as a bare KEK, so a blob
    /// from this module cannot be opened by, or confused with, another one.
    fn derive(passphrase: &str, salt: &[u8], argon2: &Argon2<'_>) -> Result<[u8; 32]> {
        let mut material = Vec::with_capacity(SEAL_DOMAIN.len() + passphrase.len());
        material.extend_from_slice(SEAL_DOMAIN);
        material.extend_from_slice(passphrase.as_bytes());
        let mut key = [0u8; 32];
        argon2
            .hash_password_into(&material, salt, &mut key)
            .map_err(|e| anyhow::anyhow!("Argon2id derivation failed: {e}"))?;
        Ok(key)
    }
}

/// Cheap Argon2 config for tests: this suite runs on every CI cycle and the
/// KDF's cost is already covered by dedicated crypto tests elsewhere.
pub(crate) fn test_argon2() -> Argon2<'static> {
    Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(64, 1, 1, None).expect("valid params"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHEAP: fn() -> Argon2<'static> = test_argon2;

    #[test]
    fn seal_open_roundtrip() {
        let key = [0x21u8; 32];
        let seal =
            PassphrasePath::seal_with(&key, "a sufficiently long passphrase", CHEAP()).unwrap();
        assert_eq!(
            PassphrasePath::open_with(&seal, "a sufficiently long passphrase", CHEAP()).unwrap(),
            key
        );
    }

    /// AC: a wrong passphrase fails authentication.
    #[test]
    fn wrong_passphrase_fails() {
        let seal =
            PassphrasePath::seal_with(&[1u8; 32], "correct horse battery staple", CHEAP()).unwrap();
        assert!(PassphrasePath::open_with(&seal, "wrong horse battery stable", CHEAP()).is_err());
    }

    /// AC: salts are random, so the same passphrase yields different blobs.
    #[test]
    fn salt_is_random_per_seal() {
        let a = PassphrasePath::seal_with(&[2u8; 32], "same passphrase here", CHEAP()).unwrap();
        let b = PassphrasePath::seal_with(&[2u8; 32], "same passphrase here", CHEAP()).unwrap();
        assert_ne!(a.salt, b.salt);
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    /// AC: weak passphrases are refused rather than silently accepted.
    #[test]
    fn short_passphrase_is_refused() {
        assert!(PassphrasePath::seal_with(&[3u8; 32], "corto", CHEAP()).is_err());
    }

    /// Invariant: the seal never contains the plaintext key.
    #[test]
    fn seal_does_not_contain_the_key() {
        let key = [0x77u8; 32];
        let seal = PassphrasePath::seal_with(&key, "a long enough passphrase", CHEAP()).unwrap();
        assert!(!seal.ciphertext.contains(&hex_encode(key)));
        assert!(!format!("{seal:?}").contains("a long enough passphrase"));
    }
}
