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

    /// [`PassphrasePath::seal`] with explicit Argon2id cost parameters.
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
        let argon2 = validated_argon2(
            argon2.params().m_cost(),
            argon2.params().t_cost(),
            argon2.params().p_cost(),
        )?;
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
                "m_cost": argon2.params().m_cost(),
                "t_cost": argon2.params().t_cost(),
                "p_cost": argon2.params().p_cost(),
            })
            .to_string(),
        })
    }

    /// Unseal a key with the passphrase.
    pub fn open(seal: &PassphraseSeal, passphrase: &str) -> Result<[u8; 32]> {
        Self::open_with(seal, passphrase, Self::stored_argon2(seal)?)
    }

    /// [`PassphrasePath::open`] with explicit Argon2 parameters.
    pub fn open_with(
        seal: &PassphraseSeal,
        passphrase: &str,
        argon2: Argon2<'_>,
    ) -> Result<[u8; 32]> {
        let stored = Self::stored_argon2(seal)?;
        let (a, b) = (stored.params(), argon2.params());
        if (a.m_cost(), a.t_cost(), a.p_cost()) != (b.m_cost(), b.t_cost(), b.p_cost()) {
            bail!("explicit Argon2id parameters do not match the seal");
        }
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

    fn stored_argon2(seal: &PassphraseSeal) -> Result<Argon2<'static>> {
        #[derive(Deserialize)]
        struct StoredParams {
            variant: String,
            domain: String,
            m_cost: u32,
            t_cost: u32,
            p_cost: u32,
        }
        let params: StoredParams =
            serde_json::from_str(&seal.params).context("invalid stored Argon2id parameters")?;
        if params.variant != "argon2id" || params.domain.as_bytes() != SEAL_DOMAIN {
            bail!("unsupported recovery KDF label");
        }
        validated_argon2(params.m_cost, params.t_cost, params.p_cost)
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

pub(crate) fn validated_argon2(m: u32, t: u32, p: u32) -> Result<Argon2<'static>> {
    if !(19456..=4 * 1024 * 1024).contains(&m) || !(2..=16).contains(&t) || !(1..=16).contains(&p) {
        bail!("stored Argon2id parameters are outside supported bounds");
    }
    let params = argon2::Params::new(m, t, p, Some(32))
        .map_err(|e| anyhow::anyhow!("invalid Argon2id parameters: {e}"))?;
    Ok(Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        params,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: fn() -> Argon2<'static> = Argon2::default;

    // Produced once by the base commit's seal code (key = [0xA5; 32]).
    const BASE_PASSPHRASE_SEAL: &str = "{\"salt\":\"acad629114431d0eaf75390d02924cf4\",\"nonce\":\"f8f7b14a9d064125cefa4a0c\",\"ciphertext\":\"2057f1d600a20b84762ad345fe9f2503838c7e9e14b889d54a976bcb2280e59198666e4d97971e76a89f385b489dd617\",\"params\":\"{\\\"domain\\\":\\\"xavier-recovery-passphrase-v1\\\",\\\"m_cost\\\":19456,\\\"p_cost\\\":1,\\\"t_cost\\\":2,\\\"variant\\\":\\\"argon2id\\\"}\"}";

    #[test]
    fn seal_open_roundtrip() {
        let key = [0x21u8; 32];
        let seal =
            PassphrasePath::seal_with(&key, "a sufficiently long passphrase", DEFAULT()).unwrap();
        assert_eq!(
            PassphrasePath::open_with(&seal, "a sufficiently long passphrase", DEFAULT()).unwrap(),
            key
        );
    }

    /// AC: a wrong passphrase fails authentication.
    #[test]
    fn wrong_passphrase_fails() {
        let seal = PassphrasePath::seal_with(&[1u8; 32], "correct horse battery staple", DEFAULT())
            .unwrap();
        assert!(PassphrasePath::open_with(&seal, "wrong horse battery stable", DEFAULT()).is_err());
    }

    /// AC: salts are random, so the same passphrase yields different blobs.
    #[test]
    fn salt_is_random_per_seal() {
        let a = PassphrasePath::seal_with(&[2u8; 32], "same passphrase here", DEFAULT()).unwrap();
        let b = PassphrasePath::seal_with(&[2u8; 32], "same passphrase here", DEFAULT()).unwrap();
        assert_ne!(a.salt, b.salt);
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    /// AC: weak passphrases are refused rather than silently accepted.
    #[test]
    fn short_passphrase_is_refused() {
        assert!(PassphrasePath::seal_with(&[3u8; 32], "corto", DEFAULT()).is_err());
    }

    /// Invariant: the seal never contains the plaintext key.
    #[test]
    fn seal_does_not_contain_the_key() {
        let key = [0x77u8; 32];
        let seal = PassphrasePath::seal_with(&key, "a long enough passphrase", DEFAULT()).unwrap();
        assert!(!seal.ciphertext.contains(&hex_encode(key)));
        assert!(!format!("{seal:?}").contains("a long enough passphrase"));
    }
    #[test]
    fn default_params_are_accepted_by_bounds() {
        assert!(validated_argon2(19456, 2, 1).is_ok());
        assert!(validated_argon2(4 * 1024 * 1024, 16, 16).is_ok());
    }

    #[test]
    fn out_of_range_params_are_rejected() {
        for (m, t, p) in [
            (19455, 2, 1),
            (4 * 1024 * 1024 + 1, 2, 1),
            (u32::MAX, 2, 1),
            (19456, 1, 1),
            (19456, 17, 1),
            (19456, 2, 0),
            (19456, 2, 17),
        ] {
            assert!(validated_argon2(m, t, p).is_err(), "{m}/{t}/{p}");
        }
    }

    #[test]
    fn out_of_range_stored_params_are_rejected_before_derivation() {
        let seal = PassphrasePath::seal(&[5u8; 32], "a sufficiently long passphrase").unwrap();
        for (field, value) in [
            ("m_cost", 19455u64),
            ("m_cost", 4_194_305),
            ("t_cost", 1),
            ("t_cost", 17),
            ("p_cost", 0),
            ("p_cost", 17),
        ] {
            let mut json: serde_json::Value = serde_json::from_str(&seal.params).unwrap();
            json[field] = serde_json::json!(value);
            let tampered = PassphraseSeal {
                params: json.to_string(),
                ..seal.clone()
            };
            let err = PassphrasePath::open(&tampered, "a sufficiently long passphrase")
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("outside supported bounds"),
                "{field}={value}: {err}"
            );
        }
    }

    #[test]
    fn open_with_mismatching_cost_is_rejected() {
        let seal = PassphrasePath::seal(&[6u8; 32], "a sufficiently long passphrase").unwrap();
        let other = argon2::Params::new(19456, 3, 1, None).unwrap();
        let other = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, other);
        assert!(PassphrasePath::open_with(&seal, "a sufficiently long passphrase", other).is_err());
    }

    #[test]
    fn base_produced_passphrase_seal_opens() {
        let seal: PassphraseSeal = serde_json::from_str(BASE_PASSPHRASE_SEAL).unwrap();
        assert_eq!(
            PassphrasePath::open(&seal, "base fixture passphrase").unwrap(),
            [0xA5u8; 32]
        );
        assert!(PassphrasePath::open(&seal, "base fixture passphrasf").is_err());
    }
}
