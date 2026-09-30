//! Master Key Management and Key Hierarchy for Xavier
//!
//! Handles generation, persistence (keyring + fallback), and derivation of
//! encryption keys using HKDF-SHA256.

pub(crate) use crate::keystore::{ensure_private_dir, write_private_file};
pub use crate::keystore::{MasterKeyManager, MASTER_KEY_LEN};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_keys_reexports() {
        assert_eq!(MASTER_KEY_LEN, 32);
    }
}
