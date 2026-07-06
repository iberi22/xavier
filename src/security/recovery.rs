//! Password Recovery System for Xavier
//! Uses 12-word BIP39 seed phrases in Spanish

use anyhow::{anyhow, Result};
use bip39::{Mnemonic, Language};

pub struct RecoverySystem;

impl RecoverySystem {
    /// Generates a new 12-word Spanish seed phrase
    pub fn generate_phrase() -> String {
        Mnemonic::generate_in(Language::Spanish, 12)
            .map(|m| m.to_string())
            .unwrap_or_default()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phrase_generation_validation() {
        let phrase = RecoverySystem::generate_phrase();
        assert_eq!(phrase.split_whitespace().count(), 12);
        assert!(RecoverySystem::validate_phrase(&phrase));
        assert!(!RecoverySystem::validate_phrase("un dos tres cuatro cinco seis siete ocho nueve diez once doce"));
    }
}
