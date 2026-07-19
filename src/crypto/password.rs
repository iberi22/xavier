use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

/// Default Argon2 cost parameter
pub const DEFAULT_COST: u32 = 3;

/// Hashes a password using Argon2id.
pub fn hash(password: &str, _cost: u32) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);

    // Argon2 with default params (Argon2id)
    let argon2 = Argon2::default();

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("password hashing failed: {}", e))?
        .to_string();

    Ok(password_hash)
}

/// Verifies a password against a previously generated Argon2id hash string.
pub fn verify(password: &str, hashed: &str) -> Result<bool> {
    let parsed_hash =
        PasswordHash::new(hashed).map_err(|e| anyhow!("invalid password hash format: {}", e))?;

    let argon2 = Argon2::default();

    Ok(argon2
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_verify() {
        let password = "secure_password_123";
        let hashed = hash(password, 0).unwrap();

        assert!(verify(password, &hashed).unwrap());
        assert!(!verify("wrong_password", &hashed).unwrap());
    }
}
