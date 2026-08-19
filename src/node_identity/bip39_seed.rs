//! BIP39-24 seed generation and parsing (AUTH_RECOVERY_SPIKE Fase 0).

use anyhow::{anyhow, Result};
use bip39::{Language, Mnemonic};
use rand::RngCore;

/// Generated BIP39 material (entropy + mnemonic + BIP39 seed bytes).
#[derive(Clone)]
pub struct GeneratedSeed {
    /// 32-byte entropy (256-bit).
    pub entropy: [u8; 32],
    /// Space-separated 24 words (English wordlist — wallet-compatible).
    pub mnemonic_words: String,
    /// BIP39 `to_seed(passphrase)` — 64 bytes.
    pub seed_bytes: [u8; 64],
}

/// Helpers around BIP39-24.
pub struct SeedPhrase;

impl SeedPhrase {
    /// Generate fresh 256-bit entropy → BIP39-24 (+ optional BIP39 passphrase).
    pub fn generate_24(passphrase: Option<&str>) -> Result<GeneratedSeed> {
        let mut entropy = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut entropy);
        Self::from_entropy(&entropy, passphrase)
    }

    /// Rebuild from entropy (e.g. after Shamir combine).
    pub fn from_entropy(entropy: &[u8; 32], passphrase: Option<&str>) -> Result<GeneratedSeed> {
        let mnemonic = Mnemonic::from_entropy_in(Language::English, entropy)
            .map_err(|e| anyhow!("BIP39 from_entropy failed: {e}"))?;
        let words = mnemonic.to_string();
        let seed = mnemonic.to_seed(passphrase.unwrap_or(""));
        let mut seed_bytes = [0u8; 64];
        seed_bytes.copy_from_slice(&seed[..64]);
        Ok(GeneratedSeed {
            entropy: *entropy,
            mnemonic_words: words,
            seed_bytes,
        })
    }

    /// Parse and validate a BIP39-24 English mnemonic.
    pub fn parse_24(phrase: &str, passphrase: Option<&str>) -> Result<GeneratedSeed> {
        let mnemonic = Mnemonic::parse_in_normalized(Language::English, phrase)
            .map_err(|e| anyhow!("invalid BIP39 mnemonic: {e}"))?;
        let word_count = mnemonic.word_count();
        if word_count != 24 {
            anyhow::bail!("Fase 0 requires BIP39-24; got {word_count} words");
        }
        let entropy_vec = mnemonic.to_entropy();
        if entropy_vec.len() != 32 {
            anyhow::bail!("expected 32-byte entropy, got {}", entropy_vec.len());
        }
        let mut entropy = [0u8; 32];
        entropy.copy_from_slice(&entropy_vec);
        let seed = mnemonic.to_seed(passphrase.unwrap_or(""));
        let mut seed_bytes = [0u8; 64];
        seed_bytes.copy_from_slice(&seed[..64]);
        Ok(GeneratedSeed {
            entropy,
            mnemonic_words: mnemonic.to_string(),
            seed_bytes,
        })
    }

    pub fn validate_24(phrase: &str) -> bool {
        Self::parse_24(phrase, None).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_parse_roundtrip() {
        let g = SeedPhrase::generate_24(Some("pass")).unwrap();
        assert_eq!(g.mnemonic_words.split_whitespace().count(), 24);
        assert!(SeedPhrase::validate_24(&g.mnemonic_words));
        let again = SeedPhrase::parse_24(&g.mnemonic_words, Some("pass")).unwrap();
        assert_eq!(again.entropy, g.entropy);
        assert_eq!(again.seed_bytes, g.seed_bytes);
    }

    #[test]
    fn rejects_12_word_as_fase0() {
        let m = Mnemonic::generate_in(Language::English, 12).unwrap();
        assert!(SeedPhrase::parse_24(&m.to_string(), None).is_err());
    }
}
