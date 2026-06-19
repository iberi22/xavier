//! HMAC implementation for Xavier.
//!
//! Provides HMAC-SHA256 for message authentication and integrity verification.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Compute HMAC-SHA256 of the given data with the provided key.
pub fn compute_hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// Verify HMAC-SHA256 of the given data with the provided key and signature.
pub fn verify_hmac_sha256(key: &[u8], data: &[u8], signature: &[u8]) -> bool {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.verify_slice(signature).is_ok()
}
