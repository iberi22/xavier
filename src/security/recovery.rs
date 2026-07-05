use crate::utils::crypto::hex_encode;
use anyhow::Result;
use bip39::{Language, Mnemonic};
use rand::Rng;
use sha2::{Digest, Sha256};

/// Recovery manager for handling seed phrases and backup codes.
pub struct RecoveryManager;

impl RecoveryManager {
    /// Generates a 12-word seed phrase in Spanish.
    pub fn generate_seed_phrase() -> Result<String> {
        let mnemonic = Mnemonic::generate_in(Language::Spanish, 12)?;
        Ok(mnemonic.to_string())
    }

    /// Verifies if a seed phrase is valid BIP39 in Spanish.
    pub fn verify_seed_phrase(phrase: &str) -> bool {
        Mnemonic::parse_in(Language::Spanish, phrase).is_ok()
    }

    /// Computes the SHA-256 hash of a seed phrase for storage.
    pub fn hash_seed_phrase(phrase: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(phrase.as_bytes());
        hex_encode(&hasher.finalize())
    }

    /// Generates 10 single-use backup codes.
    /// Each code is 8 alphanumeric characters in format XXXX-XXXX.
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
        let mut hasher = Sha256::new();
        hasher.update(code.as_bytes());
        hex_encode(&hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_seed_phrase() {
        let phrase = RecoveryManager::generate_seed_phrase().unwrap();
        let words: Vec<&str> = phrase.split_whitespace().collect();
        assert_eq!(words.len(), 12);
        assert!(RecoveryManager::verify_seed_phrase(&phrase));
    }

    #[test]
    fn test_verify_invalid_seed_phrase() {
        assert!(!RecoveryManager::verify_seed_phrase(
            "this is not a valid seed phrase at all"
        ));
    }

    #[test]
    fn test_hash_seed_phrase() {
        let phrase =
            "abaco abeja abismo abrir absorber abuelo acento aceptar acero acierto acosar activo";
        let hash = RecoveryManager::hash_seed_phrase(phrase);
        assert!(!hash.is_empty());
        // Verify idempotency
        assert_eq!(hash, RecoveryManager::hash_seed_phrase(phrase));
    }

    #[test]
    fn test_generate_backup_codes() {
        let codes = RecoveryManager::generate_backup_codes();
        assert_eq!(codes.len(), 10);
        for code in codes {
            assert_eq!(code.len(), 9); // XXXX-XXXX
            assert!(code.contains('-'));
        }
    }

    #[test]
    fn test_hash_backup_code() {
        let code = "A3B7-K9X2";
        let hash = RecoveryManager::hash_backup_code(code);
        assert!(!hash.is_empty());
        assert_eq!(hash, RecoveryManager::hash_backup_code(code));
    }
}
