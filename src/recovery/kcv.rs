//! Key-check value: prove a candidate key is *the* right one, non-destructively.
//!
//! `HMAC-SHA256(record_key, "xavier-kcv-v1")` lets a restore assert "this is the
//! key these rows were written with" **before** writing anything. Without it the
//! only available signal is "the database looks empty", which is exactly the
//! state a *wrong* key also produces.
//!
//! Properties that matter for recovery:
//!
//! - **Non-destructive.** Computes over the key alone; never reads or writes a
//!   row. A failed match changes nothing on disk.
//! - **Wrong key is a distinguishable failure.** [`KcvError::Mismatch`] means
//!   "wrong key", [`KcvError::Absent`] means "nothing stored to check against" —
//!   a fresh install must not be reported as a mismatch.
//! - **Not an oracle for guessing.** HMAC with a 256-bit random key is not
//!   brute-forceable; storing it does not weaken the key it checks.
//! - **Domain separated.** An attacker who can flip one KCV file cannot reuse the
//!   value for another purpose, because the label is bound into the MAC.

use crate::crypto::hex_encode;
use crate::crypto::hmac::hmac_sha256;
use subtle::ConstantTimeEq;

/// Domain-separation label. Changing it invalidates every stored KCV, so it is
/// versioned rather than edited in place.
pub const KCV_LABEL: &[u8] = b"xavier-kcv-v1";

/// Number of hex characters in a stored KCV.
const KCV_HEX_LEN: usize = 64;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KcvError {
    /// The candidate key does not match the stored KCV.
    #[error("key-check value mismatch: this is not the key these records were written with")]
    Mismatch,
    /// No KCV is stored, so correctness cannot be established.
    #[error("no key-check value stored; correctness cannot be proven")]
    Absent,
    /// The stored KCV is not 64 hex characters.
    #[error("malformed key-check value: expected {KCV_HEX_LEN} hex characters")]
    Malformed,
}

/// Compute the KCV for a key. The raw MAC is returned; callers store it hex.
///
/// This never logs, prints, or formats the key itself.
pub fn compute_kcv(key: &[u8; 32]) -> [u8; 32] {
    hmac_sha256(key, KCV_LABEL)
}

/// Encode a KCV for storage.
pub fn encode_kcv(key: &[u8; 32]) -> String {
    hex_encode(compute_kcv(key))
}

/// Verify a candidate key against a stored hex KCV.
///
/// Comparison is constant time, and a length mismatch short-circuits to
/// [`KcvError::Malformed`] rather than being treated as a mismatch.
pub fn verify_kcv(key: &[u8; 32], stored_hex: &str) -> Result<(), KcvError> {
    let stored = stored_hex.trim();
    if stored.is_empty() {
        return Err(KcvError::Absent);
    }
    if stored.len() != KCV_HEX_LEN || !stored.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(KcvError::Malformed);
    }
    let expected = compute_kcv(key);
    let expected_hex = hex_encode(expected).into_bytes();
    if expected_hex.ct_eq(stored.as_bytes()).into() {
        Ok(())
    } else {
        Err(KcvError::Mismatch)
    }
}

/// Constant-time equality of two 32-byte KCV values.
pub fn kcv_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kcv_is_deterministic_and_key_bound() {
        let k1 = [0x11u8; 32];
        let k2 = [0x22u8; 32];
        assert_eq!(encode_kcv(&k1), encode_kcv(&k1));
        assert_ne!(encode_kcv(&k1), encode_kcv(&k2));
        assert_eq!(encode_kcv(&k1).len(), KCV_HEX_LEN);
    }

    /// AC: the KCV identifies the correct key without any database access.
    #[test]
    fn kcv_verifies_the_right_key() {
        let key = [0xABu8; 32];
        let stored = encode_kcv(&key);
        assert_eq!(verify_kcv(&key, &stored), Ok(()));
    }

    /// AC: a wrong key is a *distinguishable* error, not "empty database".
    #[test]
    fn wrong_key_is_mismatch_not_absent() {
        let stored = encode_kcv(&[0x01u8; 32]);
        assert_eq!(verify_kcv(&[0x02u8; 32], &stored), Err(KcvError::Mismatch));
        // Crucially distinct from the fresh-install case below.
        assert_ne!(
            verify_kcv(&[0x02u8; 32], &stored),
            verify_kcv(&[0x02u8; 32], "")
        );
    }

    /// AC: fresh install must not be reported as a mismatch.
    #[test]
    fn absent_kcv_is_its_own_error() {
        assert_eq!(verify_kcv(&[0x02u8; 32], ""), Err(KcvError::Absent));
        assert_eq!(verify_kcv(&[0x02u8; 32], "   "), Err(KcvError::Absent));
    }

    /// AC: a truncated/garbled KCV is malformed, never silently accepted.
    #[test]
    fn malformed_kcv_is_rejected() {
        let key = [0x07u8; 32];
        let stored = encode_kcv(&key);
        assert_eq!(
            verify_kcv(&key, &stored[..10]),
            Err(KcvError::Malformed),
            "truncated KCV must not pass"
        );
        let zzz = "z".repeat(KCV_HEX_LEN);
        assert_eq!(verify_kcv(&key, &zzz), Err(KcvError::Malformed));
    }

    /// The stored value must not be usable outside its labelled purpose.
    #[test]
    fn kcv_is_domain_separated() {
        let key = [0x33u8; 32];
        let kcv = compute_kcv(&key);
        let raw_hmac = hmac_sha256(&key, b"");
        assert_ne!(kcv, raw_hmac, "KCV must not equal a bare HMAC of the key");
    }

    /// The KCV must not reveal the key (it is an HMAC, not the key bytes).
    #[test]
    fn kcv_does_not_leak_key_material() {
        let key = [0x5Au8; 32];
        let kcv = compute_kcv(&key);
        assert_ne!(&kcv[..4], &key[..4]);
        assert_ne!(kcv, key);
    }
}
