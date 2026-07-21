// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Password Recovery System for Xavier
//! Uses 12-word BIP39 seed phrases in Spanish

use crate::utils::crypto::sha256_hex;
use anyhow::{anyhow, Result};
use bip39::{Language, Mnemonic};
use rand::Rng;

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

    /// Hashes the seed phrase
    pub fn hash_seed_phrase(phrase: &str) -> String {
        sha256_hex(phrase.as_bytes())
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

    /// Hashes a backup code for storage.
    pub fn hash_backup_code(code: &str) -> String {
        sha256_hex(code.as_bytes())
    }
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
}
