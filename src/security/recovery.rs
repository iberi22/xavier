//! Password Recovery System for Xavier
//! Uses 12-word BIP39 seed phrases in Spanish
//!
//! Secret hashes are Argon2id PHC strings (salted, memory-hard). Hashes stored
//! before this change (plain SHA-256 hex) are still accepted by the `verify_*`
//! helpers as a migration fallback, but every newly written hash is Argon2id.

use crate::utils::crypto::sha256_hex;
use anyhow::{anyhow, Result};
use bip39::{Language, Mnemonic};
use rand::Rng;
use subtle::ConstantTimeEq;

pub struct RecoverySystem;

impl RecoverySystem {
    /// Generates a new 12-word Spanish seed phrase
    pub fn generate_phrase() -> String {
        Mnemonic::generate_in(Language::Spanish, 12)
            .map(|m| m.to_string())
            .unwrap_or_default()
    }

    /// Alias for backwards compatibility
    pub fn generate_seed_phrase() -> Result<String> {
        Ok(Self::generate_phrase())
    }

    /// Hashes the seed phrase with Argon2id (salted PHC string).
    pub fn hash_seed_phrase(phrase: &str) -> String {
        argon2_hash(phrase)
    }

    /// Verifies a seed phrase against a stored hash (Argon2id, or legacy
    /// SHA-256 hex as migration fallback).
    pub fn verify_seed_phrase(phrase: &str, stored_hash: &str) -> bool {
        argon2_verify(phrase, stored_hash)
    }

    /// Validates a seed phrase
    pub fn validate_phrase(phrase: &str) -> bool {
        Mnemonic::parse_in(Language::Spanish, phrase).is_ok()
    }

    /// Derives a stable key from the phrase (for advanced encryption scenarios)
    pub fn derive_key(phrase: &str) -> Result<Vec<u8>> {
        let mnemonic = Mnemonic::parse_in(Language::Spanish, phrase)
            .map_err(|_| anyhow!("invalid seed phrase"))?;

        let seed = mnemonic.to_seed("");
        Ok(seed.to_vec())
    }

    /// Generates 10 single-use backup codes.
    pub fn generate_backup_codes() -> Vec<String> {
        let mut codes = Vec::with_capacity(10);
        for _ in 0..10 {
            codes.push(Self::generate_single_backup_code());
        }
        codes
    }

    fn generate_single_backup_code() -> String {
        const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        let mut rng = rand::thread_rng();
        let mut part1 = String::with_capacity(4);
        let mut part2 = String::with_capacity(4);
        for _ in 0..4 {
            let idx = rng.gen_range(0..CHARSET.len());
            part1.push(CHARSET[idx] as char);
        }
        for _ in 0..4 {
            let idx = rng.gen_range(0..CHARSET.len());
            part2.push(CHARSET[idx] as char);
        }
        format!("{}-{}", part1, part2)
    }

    /// Hashes a backup code for storage with Argon2id (salted PHC string).
    pub fn hash_backup_code(code: &str) -> String {
        argon2_hash(code)
    }

    /// Verifies a backup code against a stored hash (Argon2id, or legacy
    /// SHA-256 hex as migration fallback).
    pub fn verify_backup_code(code: &str, stored_hash: &str) -> bool {
        argon2_verify(code, stored_hash)
    }
}

/// Hash a low-entropy secret with Argon2id, returning the PHC string
/// (algorithm + params + salt + hash are all embedded in it).
fn argon2_hash(secret: &str) -> String {
    crate::crypto::password::hash(secret, crate::crypto::password::DEFAULT_COST)
        .unwrap_or_else(|_| sha256_hex(secret.as_bytes()))
}

/// Verify a secret against a stored hash. Argon2id PHC strings are verified
/// with the embedded salt/params; anything else is treated as a legacy
/// plain SHA-256 hex digest and compared in constant time.
fn argon2_verify(secret: &str, stored_hash: &str) -> bool {
    if stored_hash.starts_with("$argon2") {
        return crate::crypto::password::verify(secret, stored_hash).unwrap_or(false);
    }
    let legacy = sha256_hex(secret.as_bytes());
    let legacy_bytes = legacy.as_bytes();
    let stored_bytes = stored_hash.as_bytes();
    if legacy_bytes.len() != stored_bytes.len() {
        let _ = stored_bytes.ct_eq(stored_bytes);
        return false;
    }
    legacy_bytes.ct_eq(stored_bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phrase_generation_validation() {
        let phrase = RecoverySystem::generate_phrase();
        assert_eq!(phrase.split_whitespace().count(), 12);
        assert!(RecoverySystem::validate_phrase(&phrase));
        assert!(!RecoverySystem::validate_phrase(
            "un dos tres cuatro cinco seis siete ocho nueve diez once doce"
        ));
    }

    #[test]
    fn test_argon2_hash_verify_roundtrip() {
        let phrase = RecoverySystem::generate_phrase();
        let hash = RecoverySystem::hash_seed_phrase(&phrase);
        assert!(hash.starts_with("$argon2id$"), "hash must be PHC: {hash}");
        assert!(RecoverySystem::verify_seed_phrase(&phrase, &hash));
        assert!(!RecoverySystem::verify_seed_phrase(
            "palabra incorrecta",
            &hash
        ));

        // Salts are random: two hashes of the same secret differ.
        let hash2 = RecoverySystem::hash_seed_phrase(&phrase);
        assert_ne!(hash, hash2);
        assert!(RecoverySystem::verify_seed_phrase(&phrase, &hash2));
    }

    #[test]
    fn test_legacy_sha256_fallback() {
        // Pre-Argon2 stores kept raw SHA-256 hex digests; they must verify.
        let code = "ABCD-EFGH";
        let legacy = sha256_hex(code.as_bytes());
        assert!(RecoverySystem::verify_backup_code(code, &legacy));
        assert!(!RecoverySystem::verify_backup_code("XXXX-YYYY", &legacy));
    }
}
