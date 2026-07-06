use anyhow::Result;
use rand::{thread_rng, RngCore};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Default cost for hashing (number of iterations).
pub const DEFAULT_COST: u32 = 12;
/// Maximum allowed cost to prevent DoS.
pub const MAX_COST: u32 = 16;

/// Hashes a password using SHA-256 and a random 16-byte salt.
/// The output format is "cost:salt_hex:hash_hex".
pub fn hash(password: &str, cost: u32) -> Result<String> {
    let mut salt = [0u8; 16];
    thread_rng().fill_bytes(&mut salt);

    let effective_cost = cost.min(MAX_COST);
    let hash = compute_hash(password, &salt, effective_cost);

    Ok(format!(
        "{}:{}:{}",
        effective_cost,
        crate::crypto::hex_encode(salt),
        crate::crypto::hex_encode(hash)
    ))
}

/// Verifies a password against a previously generated hash string.
pub fn verify(password: &str, hashed: &str) -> Result<bool> {
    let parts: Vec<&str> = hashed.split(':').collect();
    if parts.len() != 3 {
        return Ok(false);
    }

    let cost: u32 = parts[0].parse()?;
    if cost > MAX_COST {
        return Ok(false);
    }

    let salt = crate::crypto::hex_decode(parts[1])?;
    let original_hash = crate::crypto::hex_decode(parts[2])?;

    let hash = compute_hash(password, &salt, cost);

    Ok(hash.ct_eq(&original_hash).into())
}

fn compute_hash(password: &str, salt: &[u8], cost: u32) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(password.as_bytes());
    let mut hash = hasher.finalize().to_vec();

    // Iteration loop to make it more expensive
    for _ in 0..(1 << cost) {
        let mut hasher = Sha256::new();
        hasher.update(&hash);
        hash = hasher.finalize().to_vec();
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_verify() {
        let password = "secure_password_123";
        let hashed = hash(password, 8).unwrap();

        assert!(verify(password, &hashed).unwrap());
        assert!(!verify("wrong_password", &hashed).unwrap());
    }
}
