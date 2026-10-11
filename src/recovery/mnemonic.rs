//! BIP39 path: 24 words that *encode* a random recovery key.
//!
//! # Why encode and not derive
//!
//! The intuitive design — `record_key = KDF(mnemonic)` — is the wrong one. It
//! makes the words themselves the vault's only secret: anyone who reads the
//! paper derives the key, the key can never be rotated without invalidating the
//! paper, and the entropy of the vault becomes exactly the entropy of the
//! sentence rather than 256 independent bits.
//!
//! Instead [`MnemonicPath::seal`] draws a **fresh random 32-byte recovery key**
//! and encodes it as 24 words (256 bits of entropy + BIP39 checksum). That key
//! wraps the record key. Consequences:
//!
//! - Rotating the record key re-wraps onto the *same* paper, no new words.
//! - The words are high entropy and not brute-forceable; the weak link is
//!   *keeping* the paper, which is a physical-security problem, not a
//!   cryptographic one.
//! - A leaked paper is a real compromise, so it is treated as a secret: the
//!   sealed blob never stores the mnemonic, only its checksum-independent
//!   payload, and [`MnemonicPath::open`] takes the words from the caller.
//!
//! Spanish wordlist, matching `src/security/recovery.rs` and the node identity
//! convention (`src/node_identity/`).

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{bail, Context, Result};
use argon2::Argon2;
use bip39::{Language, Mnemonic};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::crypto::hex_decode;
use crate::crypto::hex_encode;

/// Words per mnemonic. 24 words = 256 bits of entropy.
pub const MNEMONIC_WORD_COUNT: usize = 24;

/// Entropy bytes needed for [`MNEMONIC_WORD_COUNT`] words.
pub const MNEMONIC_ENTROPY_BYTES: usize = 32;

/// Domain separator for the wrapping key derived from the recovery key.
const WRAP_DOMAIN: &[u8] = b"xavier-recovery-mnemonic-wrap-v1";

/// A sealed blob: the record key (and, optionally, the master key) encrypted
/// under a key derived from a mnemonic-encoded recovery key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MnemonicSeal {
    /// Argon2id salt, hex.
    pub salt: String,
    /// AES-256-GCM nonce, hex (12 bytes).
    pub nonce: String,
    /// Ciphertext, hex.
    pub ciphertext: String,
    /// Argon2id parameters as embedded in the derivation, for auditability.
    pub kdf: String,
}

pub struct MnemonicPath;

impl MnemonicPath {
    /// Draw a random 256-bit recovery key and render it as 24 Spanish words.
    ///
    /// The key is *encoded* in the sentence, not derived from it: recovering
    /// requires the sentence, but the sentence is not a passphrase to be
    /// guessed.
    pub fn generate() -> Result<(String, [u8; MNEMONIC_ENTROPY_BYTES])> {
        let mut entropy = [0u8; MNEMONIC_ENTROPY_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut entropy);
        let mnemonic = Mnemonic::from_entropy_in(Language::Spanish, &entropy)
            .context("cannot build mnemonic from fresh entropy")?;
        Ok((mnemonic.to_string(), entropy))
    }

    /// Validate that a word string is a well-formed 24-word Spanish mnemonic.
    pub fn validate(words: &str) -> bool {
        Mnemonic::parse_in_normalized(Language::Spanish, words).is_ok()
    }

    /// Recover the 256-bit recovery key encoded in a mnemonic.
    pub fn recovery_key_from_words(words: &str) -> Result<[u8; MNEMONIC_ENTROPY_BYTES]> {
        let mnemonic = Mnemonic::parse_in_normalized(Language::Spanish, words.trim())
            .context("invalid recovery mnemonic")?;
        let entropy = mnemonic.to_entropy();
        if entropy.len() != MNEMONIC_ENTROPY_BYTES {
            bail!(
                "expected {} bytes of entropy, found {}",
                MNEMONIC_ENTROPY_BYTES,
                entropy.len()
            );
        }
        let mut out = [0u8; MNEMONIC_ENTROPY_BYTES];
        out.copy_from_slice(&entropy);
        Ok(out)
    }

    /// Seal a key under a mnemonic.
    ///
    /// The wrapping key comes from Argon2id over the *recovery key bytes* with a
    /// domain label, so the resulting blob is never the master key under a raw
    /// mnemonic-derived KEK that another subsystem might reuse.
    pub fn seal(key: &[u8; 32], words: &str) -> Result<MnemonicSeal> {
        let recovery = Self::recovery_key_from_words(words)?;

        let mut salt = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        let argon2 = Argon2::default();
        let wrapping = Self::derive_wrapping_key(&recovery, &salt, &argon2)?;

        let nonce_bytes = crate::crypto::encryption::NonceBytes::generate();
        let cipher = Aes256Gcm::new_from_slice(&wrapping).context("cipher creation failed")?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(nonce_bytes.as_bytes()), key.as_slice())
            .map_err(|e| anyhow::anyhow!("seal failed: {e}"))?;

        Ok(MnemonicSeal {
            salt: hex_encode(salt),
            nonce: hex_encode(nonce_bytes.as_bytes()),
            ciphertext: hex_encode(&ciphertext),
            kdf: Self::kdf_label_for(&argon2),
        })
    }

    pub(crate) fn kdf_label() -> String {
        Self::kdf_label_for(&Argon2::default())
    }

    fn kdf_label_for(argon2: &Argon2<'_>) -> String {
        let p = argon2.params();
        format!(
            "argon2id-m{}-t{}-p{}-{WRAP_DOMAIN:?}",
            p.m_cost() / 1024,
            p.t_cost(),
            p.p_cost()
        )
    }

    /// Unseal a key with the mnemonic. Wrong words fail the AEAD tag.
    pub fn open(seal: &MnemonicSeal, words: &str) -> Result<[u8; 32]> {
        let argon2 = Self::stored_argon2(&seal.kdf)?;
        let recovery = Self::recovery_key_from_words(words)?;
        let salt = hex_decode(&seal.salt).context("seal salt is not hex")?;
        let nonce = hex_decode(&seal.nonce).context("seal nonce is not hex")?;
        let ciphertext = hex_decode(&seal.ciphertext).context("seal ciphertext is not hex")?;
        if salt.len() != 16 || nonce.len() != 12 {
            bail!("seal has wrong salt/nonce dimensions");
        }

        let wrapping = Self::derive_wrapping_key(&recovery, &salt, &argon2)?;
        let cipher = Aes256Gcm::new_from_slice(&wrapping).context("cipher creation failed")?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext.as_slice())
            .map_err(|_| anyhow::anyhow!("recovery failed: wrong mnemonic or tampered seal"))?;
        if plaintext.len() != 32 {
            bail!("unsealed key has wrong length {}", plaintext.len());
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&plaintext);
        Ok(out)
    }

    fn stored_argon2(label: &str) -> Result<Argon2<'static>> {
        let suffix = format!("-{WRAP_DOMAIN:?}");
        let costs = label
            .strip_prefix("argon2id-m")
            .and_then(|s| s.strip_suffix(&suffix))
            .context("unsupported recovery KDF label")?;
        let (m, rest) = costs
            .split_once("-t")
            .context("invalid recovery KDF costs")?;
        let (t, p) = rest
            .split_once("-p")
            .context("invalid recovery KDF costs")?;
        let m: u32 = m.parse().context("invalid Argon2id memory cost")?;
        let m = m
            .checked_mul(1024)
            .context("invalid Argon2id memory cost")?;
        super::passphrase::validated_argon2(
            m,
            t.parse().context("invalid Argon2id time cost")?,
            p.parse().context("invalid Argon2id parallelism")?,
        )
    }

    fn derive_wrapping_key(
        recovery: &[u8; MNEMONIC_ENTROPY_BYTES],
        salt: &[u8],
        argon2: &Argon2<'_>,
    ) -> Result<[u8; 32]> {
        let mut material = Vec::with_capacity(WRAP_DOMAIN.len() + recovery.len());
        material.extend_from_slice(WRAP_DOMAIN);
        material.extend_from_slice(recovery);
        let mut key = [0u8; 32];
        argon2
            .hash_password_into(&material, salt, &mut key)
            .map_err(|e| anyhow::anyhow!("Argon2id derivation failed: {e}"))?;
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Produced once by the base commit's seal code.
    const BASE_MN_WORDS: &str = "tejado tomar alteza tope polvo agudo zafiro avión fresa órbita palco nieto instante agudo derecho guardia litio molino ficción rudo teatro alacrán célebre gafas";
    const BASE_MN_SALT: &str = "a170cbe6f0f38b709a5704d03eb44476";
    const BASE_MN_NONCE: &str = "73af7366336733b50e9fa52a";
    const BASE_MN_CT: &str = "e1d6cb805ddd4b3f01c0e4077fb8d76c431c3a98c7606a15c236d2b6cd4cc7674291a9cea5985497b76830bbc9eeba09";
    const BASE_MN_KDF: &str = "argon2id-m19-t2-p1-[120, 97, 118, 105, 101, 114, 45, 114, 101, 99, 111, 118, 101, 114, 121, 45, 109, 110, 101, 109, 111, 110, 105, 99, 45, 119, 114, 97, 112, 45, 118, 49]";

    #[test]
    fn generated_mnemonic_roundtrips_to_the_same_key() {
        let (words, original) = MnemonicPath::generate().unwrap();
        assert_eq!(words.split_whitespace().count(), MNEMONIC_WORD_COUNT);
        assert!(MnemonicPath::validate(&words));
        assert_eq!(
            MnemonicPath::recovery_key_from_words(&words).unwrap(),
            original
        );
    }

    /// AC: the words carry 256 bits, so they are not guessable.
    #[test]
    fn mnemonic_entropy_is_256_bits() {
        let (words, key) = MnemonicPath::generate().unwrap();
        assert_eq!(
            MNEMONIC_WORD_COUNT * 11,
            264,
            "24 words -> 256 bits + 8 checksum"
        );
        assert_eq!(key.len(), 32);
        assert!(MnemonicPath::validate(&words));
    }

    /// AC: two generations differ (fresh randomness, not a fixed seed).
    #[test]
    fn each_generation_is_distinct() {
        let (_, a) = MnemonicPath::generate().unwrap();
        let (_, b) = MnemonicPath::generate().unwrap();
        assert_ne!(a, b);
    }

    /// AC: seal/open roundtrip returns the original key.
    #[test]
    fn seal_open_roundtrip() {
        let (words, _) = MnemonicPath::generate().unwrap();
        let key = [0x5Cu8; 32];
        let seal = MnemonicPath::seal(&key, &words).unwrap();
        assert_eq!(MnemonicPath::open(&seal, &words).unwrap(), key);
    }

    /// AC: a wrong mnemonic fails authentication instead of returning garbage.
    #[test]
    fn wrong_mnemonic_fails() {
        let (words, _) = MnemonicPath::generate().unwrap();
        let (other, _) = MnemonicPath::generate().unwrap();
        let seal = MnemonicPath::seal(&[7u8; 32], &words).unwrap();
        assert!(MnemonicPath::open(&seal, &other).is_err());
    }

    /// AC: tampering with the ciphertext is detected.
    #[test]
    fn tampered_seal_fails() {
        let (words, _) = MnemonicPath::generate().unwrap();
        let mut seal = MnemonicPath::seal(&[9u8; 32], &words).unwrap();
        let mut bytes = hex_decode(&seal.ciphertext).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        seal.ciphertext = hex_encode(&bytes);
        assert!(MnemonicPath::open(&seal, &words).is_err());
    }

    /// Invariant: the seal must not contain the key or the words in the clear.
    #[test]
    fn seal_leaks_neither_key_nor_words() {
        let (words, _) = MnemonicPath::generate().unwrap();
        let key = [0x3Eu8; 32];
        let seal = MnemonicPath::seal(&key, &words).unwrap();
        let blob = format!("{seal:?}");
        let first_word = words.split_whitespace().next().unwrap();
        assert!(!blob.contains(first_word), "first word leaked");
        assert!(
            !blob.contains(&hex_encode(key)),
            "plaintext key leaked into the seal"
        );
    }

    /// Invalid word lists must be rejected before any crypto runs.
    #[test]
    fn invalid_mnemonic_is_rejected() {
        assert!(!MnemonicPath::validate(
            "no es un mnemonic valido en espanol"
        ));
        assert!(MnemonicPath::recovery_key_from_words("abc def").is_err());
    }
    #[test]
    fn default_label_is_unchanged() {
        assert_eq!(
            MnemonicPath::kdf_label(),
            format!("argon2id-m19-t2-p1-{WRAP_DOMAIN:?}")
        );
        let (words, _) = MnemonicPath::generate().unwrap();
        let seal = MnemonicPath::seal(&[1u8; 32], &words).unwrap();
        assert_eq!(seal.kdf, MnemonicPath::kdf_label());
    }

    #[test]
    fn out_of_range_labels_are_rejected() {
        let d = format!("{WRAP_DOMAIN:?}");
        let ok = format!("argon2id-m19-t2-p1-{d}");
        assert!(MnemonicPath::stored_argon2(&ok).is_ok());
        for label in [
            format!("argon2id-m18-t2-p1-{d}"),
            format!("argon2id-m4097-t2-p1-{d}"),
            format!("argon2id-m19-t1-p1-{d}"),
            format!("argon2id-m19-t17-p1-{d}"),
            format!("argon2id-m19-t2-p0-{d}"),
            format!("argon2id-m19-t2-p17-{d}"),
            format!("argon2id-m4294967295-t2-p1-{d}"),
            format!("argon2id-mx-t2-p1-{d}"),
            format!("argon2id-m19-t2-{d}"),
            "argon2id-m19-t2-p1-other".to_string(),
            "scrypt".to_string(),
        ] {
            assert!(MnemonicPath::stored_argon2(&label).is_err(), "{label}");
        }
    }

    #[test]
    fn base_produced_mnemonic_seal_opens() {
        let seal = MnemonicSeal {
            salt: BASE_MN_SALT.into(),
            nonce: BASE_MN_NONCE.into(),
            ciphertext: BASE_MN_CT.into(),
            kdf: BASE_MN_KDF.into(),
        };
        assert!(seal.kdf.starts_with("argon2id-m19-t2-p1-"));
        assert_eq!(
            MnemonicPath::open(&seal, BASE_MN_WORDS).unwrap(),
            [0xA5u8; 32]
        );
    }
}
