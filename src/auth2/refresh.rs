use crate::auth2::db::{AuthDb, RefreshToken};
use anyhow::{anyhow, Result};
use rand::{thread_rng, RngCore};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct RefreshTokenManager<'a> {
    db: &'a AuthDb,
}

impl<'a> RefreshTokenManager<'a> {
    /// New.
    pub fn new(db: &'a AuthDb) -> Self {
        Self { db }
    }

    /// Generate token.
    pub fn generate_token(&self, user_id: &str, device_info: Option<String>) -> Result<String> {
        let mut token_bytes = [0u8; 32];
        thread_rng().fill_bytes(&mut token_bytes);
        let token = crate::crypto::base64_encode(token_bytes); // base64url would be better but I'll use standard for now or implement it

        let hash = self.hash_token(&token);

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let expires_at = now + 30 * 24 * 60 * 60; // 30 days

        let refresh_token = RefreshToken {
            id: ulid::Ulid::new().to_string(),
            user_id: user_id.to_string(),
            token_hash: hash,
            device_info,
            expires_at,
            created_at: now,
            revoked: false,
        };

        self.db.store_refresh_token(&refresh_token)?;

        Ok(token)
    }

    /// Rotate token.
    pub fn rotate_token(
        &self,
        token: &str,
        device_info: Option<String>,
    ) -> Result<(String, String)> {
        let hash = self.hash_token(token);

        let stored_token = self
            .db
            .get_refresh_token_by_hash(&hash)?
            .ok_or_else(|| anyhow!("Invalid refresh token"))?;

        if stored_token.revoked {
            // Theft detection: revoke ALL tokens for this user
            self.db.revoke_all_user_tokens(&stored_token.user_id)?;
            return Err(anyhow!(
                "Token already revoked. Potential theft detected. All sessions invalidated."
            ));
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        if stored_token.expires_at < now {
            return Err(anyhow!("Refresh token expired"));
        }

        // Revoke old token
        self.db.revoke_refresh_token(&stored_token.id)?;

        // Generate new token
        let new_token = self.generate_token(&stored_token.user_id, device_info)?;

        Ok((new_token, stored_token.user_id))
    }

    fn hash_token(&self, token: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        crate::crypto::hex_encode(hasher.finalize())
    }
}
