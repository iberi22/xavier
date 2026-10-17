//! MKT-09 AccessGrant delivery with a buyer-wrapped key.
//!
//! When a buyer purchases or requests access to a dataset package, the package key
//! must be wrapped specifically for that buyer using asymmetric cryptography.
//! The access grant carries a time limit (expiration timestamp) and can be revoked.
//!
//! Security guarantees:
//! - Nothing is delivered in plaintext: the package key is wrapped (encrypted) for the buyer's public key.
//! - The buyer decrypts using their private key.
//! - After revocation, new grants fail and existing grants cannot be used.
//! - Expired grants cannot be decrypted or used.

use ed25519_dalek::{Signature, VerifyingKey};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::auth::Actor;
use crate::error::MarketError;
use crate::store::{non_empty, now_ms, MarketStore};

/// Domain separator for AccessGrant signatures.
pub const ACCESS_GRANT_DOMAIN: &str = "xavier-market:access-grant:v1";

/// Errors that can occur during key wrapping, grant creation, revocation, or decryption.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum GrantError {
    #[error("grant {0} not found")]
    NotFound(String),
    #[error("grant {0} has expired")]
    Expired(String),
    #[error("grant {0} has been revoked")]
    Revoked(String),
    #[error("listing {0} has been revoked")]
    ListingRevoked(String),
    #[error("invalid key format: {0}")]
    InvalidKey(String),
    #[error("invalid signature format: {0}")]
    InvalidSignature(String),
    #[error("decryption failed or key authentication mismatch")]
    DecryptionFailed,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
}

/// An access grant record delivering a wrapped package key to a buyer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessGrant {
    /// Unique identifier of the grant.
    pub id: String,
    /// The listing id this grant provides access to.
    pub listing_id: String,
    /// The buyer's identity (hex-encoded public key).
    pub buyer_id: String,
    /// The seller/issuer identity (hex-encoded public key).
    pub seller_id: String,
    /// The ephemeral public key (hex-encoded 32 bytes) used to wrap the package key.
    pub ephemeral_pubkey_hex: String,
    /// The wrapped (encrypted) package key with authentication tag (hex-encoded bytes).
    pub wrapped_key_hex: String,
    /// The 12-byte nonce used in encryption (hex-encoded).
    pub nonce_hex: String,
    /// Unix timestamp (ms) when this grant was created.
    pub created_at_ms: i64,
    /// Unix timestamp (ms) when this grant expires.
    pub expires_at_ms: i64,
    /// Unix timestamp (ms) when this grant was revoked, if any.
    pub revoked_at_ms: Option<i64>,
    /// Hex-encoded Ed25519 signature from the seller/platform over the grant metadata.
    pub signature_hex: String,
}

impl AccessGrant {
    /// Checks whether the grant is currently active at `now_ms`.
    /// Returns false if revoked or expired.
    pub fn is_active_at(&self, now_ms: i64) -> bool {
        self.revoked_at_ms.is_none() && now_ms < self.expires_at_ms
    }

    /// Returns a sanitized copy of this grant where key material
    /// (`wrapped_key_hex`, `ephemeral_pubkey_hex`, `nonce_hex`) is stripped (empty)
    /// if the grant is revoked or expired at `now_ms`.
    pub fn sanitized_if_inactive(&self, now_ms: i64) -> Self {
        if self.is_active_at(now_ms) {
            self.clone()
        } else {
            let mut sanitized = self.clone();
            sanitized.wrapped_key_hex.clear();
            sanitized.ephemeral_pubkey_hex.clear();
            sanitized.nonce_hex.clear();
            sanitized
        }
    }

    /// Verifies the seller's signature on this grant.
    pub fn verify_signature(&self) -> Result<(), GrantError> {
        let seller_pk_bytes = hex::decode(&self.seller_id)
            .map_err(|e| GrantError::InvalidKey(format!("invalid seller_id hex: {e}")))?;
        if seller_pk_bytes.len() != 32 {
            return Err(GrantError::InvalidKey(
                "seller_id must be 32 bytes".to_string(),
            ));
        }
        let seller_pk: [u8; 32] = seller_pk_bytes
            .try_into()
            .map_err(|_| GrantError::InvalidKey("slice convert failed".to_string()))?;
        let verifying_key = VerifyingKey::from_bytes(&seller_pk)
            .map_err(|e| GrantError::InvalidKey(format!("invalid ed25519 verifying key: {e}")))?;

        let sig_bytes = hex::decode(&self.signature_hex)
            .map_err(|e| GrantError::InvalidSignature(format!("invalid signature hex: {e}")))?;
        if sig_bytes.len() != 64 {
            return Err(GrantError::InvalidSignature(
                "signature must be 64 bytes".to_string(),
            ));
        }
        let sig_arr: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| GrantError::InvalidSignature("slice convert failed".to_string()))?;
        let signature = Signature::from_bytes(&sig_arr);

        let msg = grant_signing_bytes(
            &self.listing_id,
            &self.buyer_id,
            &self.ephemeral_pubkey_hex,
            &self.wrapped_key_hex,
            &self.nonce_hex,
            self.expires_at_ms,
        );

        verifying_key
            .verify_strict(&msg, &signature)
            .map_err(|_| GrantError::InvalidSignature("signature verification failed".to_string()))
    }
}

/// Computes canonical signing bytes for an AccessGrant over fields controlled by the seller:
/// listing_id, buyer_x25519_pubkey, ephemeral_pubkey, wrapped_key, nonce, expires_at_ms.
pub fn grant_signing_bytes(
    listing_id: &str,
    buyer_x25519_pubkey_hex: &str,
    ephemeral_pubkey_hex: &str,
    wrapped_key_hex: &str,
    nonce_hex: &str,
    expires_at_ms: i64,
) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(ACCESS_GRANT_DOMAIN.as_bytes());
    h.update(b"\n");
    h.update(listing_id.as_bytes());
    h.update(b"\n");
    h.update(buyer_x25519_pubkey_hex.as_bytes());
    h.update(b"\n");
    h.update(ephemeral_pubkey_hex.as_bytes());
    h.update(b"\n");
    h.update(wrapped_key_hex.as_bytes());
    h.update(b"\n");
    h.update(nonce_hex.as_bytes());
    h.update(b"\n");
    h.update(expires_at_ms.to_string().as_bytes());
    h.finalize().to_vec()
}

/// In-memory helper to wrap a package key for a buyer using X25519-ECDH + ChaCha20Poly1305.
///
/// Steps:
/// 1. Takes buyer's 32-byte public key (X25519 or Ed25519 public key as identity).
/// 2. Generates an ephemeral X25519 keypair.
/// 3. Computes Diffie-Hellman shared secret with buyer's public key.
/// 4. Derives encryption key via HKDF / SHA-256 with domain separation.
/// 5. Encrypts `plaintext_package_key` with ChaCha20Poly1305 (using 12-byte random nonce).
/// 6. Returns `(wrapped_key_hex, ephemeral_pubkey_hex, nonce_hex)`.
pub fn wrap_key_for_buyer(
    plaintext_package_key: &[u8],
    buyer_pubkey_bytes: &[u8; 32],
    ephemeral_seed: &[u8; 32],
    nonce: &[u8; 12],
) -> Result<(String, String, String), GrantError> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Nonce};
    use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

    // Ephemeral X25519 keypair from seed
    let ephemeral_secret = StaticSecret::from(*ephemeral_seed);
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);

    // Buyer public key as X25519
    let buyer_public = X25519PublicKey::from(*buyer_pubkey_bytes);

    // ECDH shared secret
    let shared_secret = ephemeral_secret.diffie_hellman(&buyer_public);

    // Key derivation using SHA-256 domain
    let mut hasher = Sha256::new();
    hasher.update(b"XAVIER_MARKET_ACCESS_GRANT_KEY_WRAP_V1:");
    hasher.update(shared_secret.as_bytes());
    hasher.update(ephemeral_public.as_bytes());
    hasher.update(buyer_public.as_bytes());
    let derived_key = hasher.finalize();

    // Authenticated encryption
    let cipher = ChaCha20Poly1305::new_from_slice(&derived_key)
        .map_err(|_| GrantError::InvalidKey("invalid derived key length".to_string()))?;
    let chacha_nonce = Nonce::from_slice(nonce);

    let ciphertext = cipher
        .encrypt(chacha_nonce, plaintext_package_key)
        .map_err(|_| GrantError::DecryptionFailed)?;

    Ok((
        hex::encode(ciphertext),
        hex::encode(ephemeral_public.as_bytes()),
        hex::encode(nonce),
    ))
}

/// Unwraps (decrypts) a wrapped package key as the buyer using the buyer's private key.
///
/// Verifies:
/// 1. Grant expiration: fails if expired.
/// 2. Grant revocation: fails if revoked.
/// 3. Authenticated decryption: fails if ciphertext, nonce or key was tampered with.
pub fn unwrap_key_as_buyer(
    grant: &AccessGrant,
    buyer_secret_bytes: &[u8; 32],
    now_ms: i64,
) -> Result<Vec<u8>, GrantError> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Nonce};
    use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

    if grant.revoked_at_ms.is_some() {
        return Err(GrantError::Revoked(grant.id.clone()));
    }
    if now_ms >= grant.expires_at_ms {
        return Err(GrantError::Expired(grant.id.clone()));
    }

    let ephemeral_pub_bytes = hex::decode(&grant.ephemeral_pubkey_hex)
        .map_err(|e| GrantError::InvalidKey(format!("invalid ephemeral_pubkey_hex: {e}")))?;
    if ephemeral_pub_bytes.len() != 32 {
        return Err(GrantError::InvalidKey(
            "ephemeral pubkey must be 32 bytes".to_string(),
        ));
    }
    let eph_arr: [u8; 32] = ephemeral_pub_bytes
        .try_into()
        .map_err(|_| GrantError::InvalidKey("slice error".to_string()))?;
    let ephemeral_public = X25519PublicKey::from(eph_arr);

    let buyer_secret = StaticSecret::from(*buyer_secret_bytes);
    let buyer_public = X25519PublicKey::from(&buyer_secret);

    let shared_secret = buyer_secret.diffie_hellman(&ephemeral_public);

    let mut hasher = Sha256::new();
    hasher.update(b"XAVIER_MARKET_ACCESS_GRANT_KEY_WRAP_V1:");
    hasher.update(shared_secret.as_bytes());
    hasher.update(ephemeral_public.as_bytes());
    hasher.update(buyer_public.as_bytes());
    let derived_key = hasher.finalize();

    let ciphertext = hex::decode(&grant.wrapped_key_hex)
        .map_err(|e| GrantError::InvalidInput(format!("invalid wrapped_key_hex: {e}")))?;
    let nonce_bytes = hex::decode(&grant.nonce_hex)
        .map_err(|e| GrantError::InvalidInput(format!("invalid nonce_hex: {e}")))?;
    if nonce_bytes.len() != 12 {
        return Err(GrantError::InvalidInput(
            "nonce must be 12 bytes".to_string(),
        ));
    }

    let cipher = ChaCha20Poly1305::new_from_slice(&derived_key)
        .map_err(|_| GrantError::InvalidKey("invalid derived key length".to_string()))?;
    let chacha_nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = cipher
        .decrypt(chacha_nonce, ciphertext.as_ref())
        .map_err(|_| GrantError::DecryptionFailed)?;

    Ok(plaintext)
}

/// Grant methods on `MarketStore`.
impl MarketStore {
    /// Creates and stores a new `AccessGrant` for a buyer on a listing.
    ///
    /// Requires that:
    /// - `actor` is the publisher/seller of the listing.
    /// - Listing exists and is not revoked.
    /// - All hex fields are strictly valid and meet required cryptographic lengths.
    /// - `ttl_seconds` is non-zero and does not exceed `grant_max_ttl_seconds`.
    /// - The seller's signature over the grant signing bytes verifies against `actor.id()`.
    ///
    /// If the listing was revoked, new grants fail immediately with `ListingRevoked`.
    pub fn create_access_grant(
        &self,
        actor: &Actor,
        listing_id: &str,
        buyer_x25519_pubkey_hex: &str,
        ephemeral_pubkey_hex: &str,
        wrapped_key_hex: &str,
        nonce_hex: &str,
        ttl_seconds: Option<u64>,
        signature_hex: &str,
    ) -> Result<AccessGrant, MarketError> {
        let listing_id = non_empty("listing_id", listing_id)?;
        let buyer_pub_hex = non_empty("buyer_x25519_pubkey_hex", buyer_x25519_pubkey_hex)?;
        let ephemeral_pubkey_hex = non_empty("ephemeral_pubkey_hex", ephemeral_pubkey_hex)?;
        let wrapped_key_hex = non_empty("wrapped_key_hex", wrapped_key_hex)?;
        let nonce_hex = non_empty("nonce_hex", nonce_hex)?;
        let signature_hex = non_empty("signature_hex", signature_hex)?;

        // Validate hex encoding and lengths
        let buyer_bytes = hex::decode(&buyer_pub_hex).map_err(|e| {
            MarketError::InvalidInput(format!("buyer_x25519_pubkey_hex is not valid hex: {e}"))
        })?;
        if buyer_bytes.len() != 32 {
            return Err(MarketError::InvalidInput(
                "buyer_x25519_pubkey_hex must be exactly 32 bytes".to_string(),
            ));
        }

        let eph_bytes = hex::decode(&ephemeral_pubkey_hex).map_err(|e| {
            MarketError::InvalidInput(format!("ephemeral_pubkey_hex is not valid hex: {e}"))
        })?;
        if eph_bytes.len() != 32 {
            return Err(MarketError::InvalidInput(
                "ephemeral_pubkey_hex must be exactly 32 bytes".to_string(),
            ));
        }

        let nonce_bytes = hex::decode(&nonce_hex)
            .map_err(|e| MarketError::InvalidInput(format!("nonce_hex is not valid hex: {e}")))?;
        if nonce_bytes.len() != 12 {
            return Err(MarketError::InvalidInput(
                "nonce_hex must be exactly 12 bytes".to_string(),
            ));
        }

        let wrapped_bytes = hex::decode(&wrapped_key_hex).map_err(|e| {
            MarketError::InvalidInput(format!("wrapped_key_hex is not valid hex: {e}"))
        })?;
        if wrapped_bytes.len() < 16 {
            return Err(MarketError::InvalidInput(
                "wrapped_key_hex must contain at least 16 bytes of ciphertext".to_string(),
            ));
        }

        let sig_bytes = hex::decode(&signature_hex).map_err(|e| {
            MarketError::InvalidInput(format!("signature_hex is not valid hex: {e}"))
        })?;
        if sig_bytes.len() != 64 {
            return Err(MarketError::InvalidInput(
                "signature_hex must be exactly 64 bytes".to_string(),
            ));
        }
        let sig_arr: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| MarketError::InvalidInput("signature length mismatch".to_string()))?;
        let signature = Signature::from_bytes(&sig_arr);

        // Validate TTL
        let max_ttl = self.config().grant_max_ttl_seconds;
        let default_ttl = self.config().grant_default_ttl_seconds;
        let ttl_secs = match ttl_seconds {
            Some(0) => {
                return Err(MarketError::InvalidInput(
                    "ttl_seconds must be greater than zero".to_string(),
                ))
            }
            Some(t) if t > max_ttl => {
                return Err(MarketError::InvalidInput(format!(
                    "ttl_seconds ({t}) exceeds maximum allowed ({max_ttl})"
                )))
            }
            Some(t) => t,
            None => default_ttl,
        };

        let created_at = now_ms();
        let ttl_ms = (ttl_secs as i64)
            .checked_mul(1000)
            .ok_or_else(|| MarketError::InvalidInput("ttl overflow".to_string()))?;
        let expires_at = created_at
            .checked_add(ttl_ms)
            .ok_or_else(|| MarketError::InvalidInput("expires_at overflow".to_string()))?;

        let mut conn = self.conn();
        let tx = conn.transaction()?;

        // Verify listing exists and is not revoked
        let (seller_id, revoked_at): (String, Option<i64>) = tx
            .query_row(
                "SELECT seller_id, revoked_at_ms FROM market_listings WHERE id = ?1",
                params![listing_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| MarketError::ListingNotFound(listing_id.clone()))?;

        if revoked_at.is_some() {
            return Err(MarketError::ListingRevoked(listing_id));
        }

        if !seller_id.eq_ignore_ascii_case(actor.id()) {
            return Err(MarketError::Forbidden(
                "only the listing seller can issue access grants".to_string(),
            ));
        }

        // Verify seller public key from actor.id()
        let seller_pk_bytes = hex::decode(actor.id()).map_err(|e| {
            MarketError::InvalidInput(format!("seller actor id is not valid hex: {e}"))
        })?;
        if seller_pk_bytes.len() != 32 {
            return Err(MarketError::InvalidInput(
                "seller actor id must be 32 bytes".to_string(),
            ));
        }
        let seller_pk: [u8; 32] = seller_pk_bytes.try_into().map_err(|_| {
            MarketError::InvalidInput("seller public key slice mismatch".to_string())
        })?;
        let verifying_key = VerifyingKey::from_bytes(&seller_pk).map_err(|e| {
            MarketError::InvalidInput(format!("invalid ed25519 verifying key: {e}"))
        })?;

        // Verify signature over canonical grant fields
        let grant_id = format!("grant_{}", Uuid::new_v4());
        let signing_bytes = grant_signing_bytes(
            &listing_id,
            &buyer_pub_hex,
            &ephemeral_pubkey_hex,
            &wrapped_key_hex,
            &nonce_hex,
            expires_at,
        );

        verifying_key
            .verify_strict(&signing_bytes, &signature)
            .map_err(|_| {
                MarketError::InvalidInput("grant signature verification failed".to_string())
            })?;

        let grant = AccessGrant {
            id: grant_id,
            listing_id,
            buyer_id: buyer_pub_hex,
            seller_id: actor.id().to_string(),
            ephemeral_pubkey_hex,
            wrapped_key_hex,
            nonce_hex,
            created_at_ms: created_at,
            expires_at_ms: expires_at,
            revoked_at_ms: None,
            signature_hex,
        };

        tx.execute(
            "INSERT INTO market_access_grants (
                id, listing_id, buyer_id, seller_id,
                ephemeral_pubkey, wrapped_key, nonce,
                created_at_ms, expires_at_ms, revoked_at_ms, signature
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                grant.id,
                grant.listing_id,
                grant.buyer_id,
                grant.seller_id,
                grant.ephemeral_pubkey_hex,
                grant.wrapped_key_hex,
                grant.nonce_hex,
                grant.created_at_ms,
                grant.expires_at_ms,
                grant.revoked_at_ms,
                grant.signature_hex,
            ],
        )?;

        tx.commit()?;
        Ok(grant)
    }

    /// Fetches an access grant by id.
    pub fn get_access_grant(&self, grant_id: &str) -> Result<Option<AccessGrant>, MarketError> {
        let conn = self.conn();
        query_grant(&conn, grant_id)
    }

    /// Lists active or all grants for a given buyer.
    pub fn list_grants_for_buyer(&self, buyer_id: &str) -> Result<Vec<AccessGrant>, MarketError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, listing_id, buyer_id, seller_id,
                    ephemeral_pubkey, wrapped_key, nonce,
                    created_at_ms, expires_at_ms, revoked_at_ms, signature
             FROM market_access_grants
             WHERE buyer_id = ?1
             ORDER BY created_at_ms DESC",
        )?;
        let rows = stmt
            .query_map(params![buyer_id], raw_grant)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Lists grants issued for a given listing.
    pub fn list_grants_for_listing(
        &self,
        listing_id: &str,
    ) -> Result<Vec<AccessGrant>, MarketError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, listing_id, buyer_id, seller_id,
                    ephemeral_pubkey, wrapped_key, nonce,
                    created_at_ms, expires_at_ms, revoked_at_ms, signature
             FROM market_access_grants
             WHERE listing_id = ?1
             ORDER BY created_at_ms DESC",
        )?;
        let rows = stmt
            .query_map(params![listing_id], raw_grant)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Revokes an individual access grant.
    ///
    /// Can only be performed by the listing's seller (or platform actor).
    pub fn revoke_access_grant(
        &self,
        actor: &Actor,
        grant_id: &str,
    ) -> Result<AccessGrant, MarketError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;

        let mut grant = query_grant(&tx, grant_id)?
            .ok_or_else(|| MarketError::AccessGrantNotFound(grant_id.to_string()))?;

        if !grant.seller_id.eq_ignore_ascii_case(actor.id()) {
            return Err(MarketError::Forbidden(
                "only the seller can revoke this access grant".to_string(),
            ));
        }

        if grant.revoked_at_ms.is_none() {
            let revoked_at = now_ms();
            tx.execute(
                "UPDATE market_access_grants SET revoked_at_ms = ?2 WHERE id = ?1",
                params![grant.id, revoked_at],
            )?;
            grant.revoked_at_ms = Some(revoked_at);
        }

        tx.commit()?;
        Ok(grant)
    }
}

fn query_grant(conn: &Connection, id: &str) -> Result<Option<AccessGrant>, MarketError> {
    let row = conn
        .query_row(
            "SELECT id, listing_id, buyer_id, seller_id,
                    ephemeral_pubkey, wrapped_key, nonce,
                    created_at_ms, expires_at_ms, revoked_at_ms, signature
             FROM market_access_grants WHERE id = ?1",
            params![id],
            raw_grant,
        )
        .optional()?;
    Ok(row)
}

fn raw_grant(row: &Row<'_>) -> rusqlite::Result<AccessGrant> {
    Ok(AccessGrant {
        id: row.get(0)?,
        listing_id: row.get(1)?,
        buyer_id: row.get(2)?,
        seller_id: row.get(3)?,
        ephemeral_pubkey_hex: row.get(4)?,
        wrapped_key_hex: row.get(5)?,
        nonce_hex: row.get(6)?,
        created_at_ms: row.get(7)?,
        expires_at_ms: row.get(8)?,
        revoked_at_ms: row.get(9)?,
        signature_hex: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{License, MarketConfig};
    use crate::model::NewListing;
    use ed25519_dalek::{Signer as _, SigningKey};

    fn test_seller() -> (SigningKey, Actor) {
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let actor = Actor::for_test(&hex::encode(key.verifying_key().as_bytes()));
        (key, actor)
    }

    fn test_buyer() -> (x25519_dalek::StaticSecret, [u8; 32], String) {
        let secret = x25519_dalek::StaticSecret::from([2u8; 32]);
        let public = x25519_dalek::PublicKey::from(&secret);
        let pub_bytes = *public.as_bytes();
        let pub_hex = hex::encode(pub_bytes);
        (secret, pub_bytes, pub_hex)
    }

    #[test]
    fn test_wrap_and_unwrap_key_success() {
        let (_buyer_sec, buyer_pub_bytes, _buyer_pub_hex) = test_buyer();
        let package_key = b"super-secret-package-aes-key-32b";
        let ephemeral_seed = [3u8; 32];
        let nonce = [4u8; 12];

        let (wrapped, eph_pub, nonce_hex) =
            wrap_key_for_buyer(package_key, &buyer_pub_bytes, &ephemeral_seed, &nonce).unwrap();

        let grant = AccessGrant {
            id: "grant-1".to_string(),
            listing_id: "list-1".to_string(),
            buyer_id: hex::encode(buyer_pub_bytes),
            seller_id: "seller-1".to_string(),
            ephemeral_pubkey_hex: eph_pub,
            wrapped_key_hex: wrapped,
            nonce_hex,
            created_at_ms: 1000,
            expires_at_ms: 5000,
            revoked_at_ms: None,
            signature_hex: "00".repeat(64),
        };

        let buyer_secret_bytes = [2u8; 32];
        let decrypted = unwrap_key_as_buyer(&grant, &buyer_secret_bytes, 2000).unwrap();
        assert_eq!(decrypted, package_key);
    }

    #[test]
    fn test_unwrap_expired_grant_fails() {
        let (_buyer_sec, buyer_pub_bytes, _buyer_pub_hex) = test_buyer();
        let package_key = b"pkg-key-123";
        let ephemeral_seed = [3u8; 32];
        let nonce = [4u8; 12];

        let (wrapped, eph_pub, nonce_hex) =
            wrap_key_for_buyer(package_key, &buyer_pub_bytes, &ephemeral_seed, &nonce).unwrap();

        let grant = AccessGrant {
            id: "grant-expired".to_string(),
            listing_id: "list-1".to_string(),
            buyer_id: hex::encode(buyer_pub_bytes),
            seller_id: "seller-1".to_string(),
            ephemeral_pubkey_hex: eph_pub,
            wrapped_key_hex: wrapped,
            nonce_hex,
            created_at_ms: 1000,
            expires_at_ms: 5000,
            revoked_at_ms: None,
            signature_hex: "00".repeat(64),
        };

        let buyer_secret_bytes = [2u8; 32];
        // At or after expires_at_ms (5000)
        let res = unwrap_key_as_buyer(&grant, &buyer_secret_bytes, 5000);
        assert!(matches!(res, Err(GrantError::Expired(_))));
    }

    #[test]
    fn test_unwrap_revoked_grant_fails() {
        let (_buyer_sec, buyer_pub_bytes, _buyer_pub_hex) = test_buyer();
        let package_key = b"pkg-key-123";
        let ephemeral_seed = [3u8; 32];
        let nonce = [4u8; 12];

        let (wrapped, eph_pub, nonce_hex) =
            wrap_key_for_buyer(package_key, &buyer_pub_bytes, &ephemeral_seed, &nonce).unwrap();

        let grant = AccessGrant {
            id: "grant-revoked".to_string(),
            listing_id: "list-1".to_string(),
            buyer_id: hex::encode(buyer_pub_bytes),
            seller_id: "seller-1".to_string(),
            ephemeral_pubkey_hex: eph_pub,
            wrapped_key_hex: wrapped,
            nonce_hex,
            created_at_ms: 1000,
            expires_at_ms: 5000,
            revoked_at_ms: Some(2500),
            signature_hex: "00".repeat(64),
        };

        let buyer_secret_bytes = [2u8; 32];
        let res = unwrap_key_as_buyer(&grant, &buyer_secret_bytes, 2000);
        assert!(matches!(res, Err(GrantError::Revoked(_))));
    }

    #[test]
    fn test_tampered_wrapped_key_fails_decryption() {
        let (_buyer_sec, buyer_pub_bytes, _buyer_pub_hex) = test_buyer();
        let package_key = b"pkg-key-123";
        let ephemeral_seed = [3u8; 32];
        let nonce = [4u8; 12];

        let (wrapped, eph_pub, nonce_hex) =
            wrap_key_for_buyer(package_key, &buyer_pub_bytes, &ephemeral_seed, &nonce).unwrap();

        let mut tampered_bytes = hex::decode(&wrapped).unwrap();
        tampered_bytes[0] ^= 0x01; // flip bit
        let tampered_wrapped = hex::encode(tampered_bytes);

        let grant = AccessGrant {
            id: "grant-tampered".to_string(),
            listing_id: "list-1".to_string(),
            buyer_id: hex::encode(buyer_pub_bytes),
            seller_id: "seller-1".to_string(),
            ephemeral_pubkey_hex: eph_pub,
            wrapped_key_hex: tampered_wrapped,
            nonce_hex,
            created_at_ms: 1000,
            expires_at_ms: 5000,
            revoked_at_ms: None,
            signature_hex: "00".repeat(64),
        };

        let buyer_secret_bytes = [2u8; 32];
        let res = unwrap_key_as_buyer(&grant, &buyer_secret_bytes, 2000);
        assert!(matches!(res, Err(GrantError::DecryptionFailed)));
    }

    #[test]
    fn test_store_grant_lifecycle_and_revocation() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let (seller_key, seller_actor) = test_seller();
        let (_buyer_sec, buyer_pub_bytes, buyer_pub_hex) = test_buyer();

        // Create listing
        let listing = store
            .create_listing(
                &seller_actor,
                NewListing {
                    seller_id: seller_actor.id().to_string(),
                    title: "Encrypted dataset".to_string(),
                    license: License::CcBy40,
                    price_credits: 10,
                },
            )
            .unwrap();

        // Wrap key
        let package_key = b"secret-dataset-key";
        let ephemeral_seed = [5u8; 32];
        let nonce = [6u8; 12];
        let (wrapped, eph_pub, nonce_hex) =
            wrap_key_for_buyer(package_key, &buyer_pub_bytes, &ephemeral_seed, &nonce).unwrap();

        let ttl_seconds = 3600u64;
        let created_at = now_ms();
        let expires_at = created_at + (ttl_seconds as i64 * 1000);
        let signing_bytes = grant_signing_bytes(
            &listing.id,
            &buyer_pub_hex,
            &eph_pub,
            &wrapped,
            &nonce_hex,
            expires_at,
        );
        let sig = seller_key.sign(&signing_bytes);
        let signature_hex = hex::encode(sig.to_bytes());

        // Create grant in store
        let grant = store
            .create_access_grant(
                &seller_actor,
                &listing.id,
                &buyer_pub_hex,
                &eph_pub,
                &wrapped,
                &nonce_hex,
                Some(ttl_seconds),
                &signature_hex,
            )
            .unwrap();

        assert_eq!(grant.listing_id, listing.id);
        assert_eq!(grant.buyer_id, buyer_pub_hex);
        assert_eq!(grant.seller_id, seller_actor.id());
        assert!(grant.revoked_at_ms.is_none());
        assert!(grant.verify_signature().is_ok());

        // Active grant sanitized is identical
        let sanitized_active = grant.sanitized_if_inactive(created_at + 10);
        assert_eq!(sanitized_active.wrapped_key_hex, wrapped);
        assert_eq!(sanitized_active.ephemeral_pubkey_hex, eph_pub);
        assert_eq!(sanitized_active.nonce_hex, nonce_hex);

        // Expired grant sanitized has stripped keys
        let sanitized_expired = grant.sanitized_if_inactive(expires_at + 100);
        assert!(sanitized_expired.wrapped_key_hex.is_empty());
        assert!(sanitized_expired.ephemeral_pubkey_hex.is_empty());
        assert!(sanitized_expired.nonce_hex.is_empty());

        // Buyer list grants
        let buyer_grants = store.list_grants_for_buyer(&buyer_pub_hex).unwrap();
        assert_eq!(buyer_grants.len(), 1);
        assert_eq!(buyer_grants[0].id, grant.id);

        // Listing list grants
        let listing_grants = store.list_grants_for_listing(&listing.id).unwrap();
        assert_eq!(listing_grants.len(), 1);
        assert_eq!(listing_grants[0].id, grant.id);

        // Buyer decrypts successfully
        let buyer_secret_bytes = [2u8; 32];
        let pt =
            unwrap_key_as_buyer(&grant, &buyer_secret_bytes, grant.created_at_ms + 100).unwrap();
        assert_eq!(pt, package_key);

        // Revoke grant
        let revoked_grant = store.revoke_access_grant(&seller_actor, &grant.id).unwrap();
        assert!(revoked_grant.revoked_at_ms.is_some());

        // Revoked grant sanitized has stripped keys
        let sanitized_revoked = revoked_grant.sanitized_if_inactive(created_at + 20);
        assert!(sanitized_revoked.wrapped_key_hex.is_empty());
        assert!(sanitized_revoked.ephemeral_pubkey_hex.is_empty());
        assert!(sanitized_revoked.nonce_hex.is_empty());

        // After revocation, unwrap fails
        let unwrap_res = unwrap_key_as_buyer(
            &revoked_grant,
            &buyer_secret_bytes,
            grant.created_at_ms + 200,
        );
        assert!(matches!(unwrap_res, Err(GrantError::Revoked(_))));

        // Revoking non-existent grant returns AccessGrantNotFound
        let not_found_err = store.revoke_access_grant(&seller_actor, "grant_nonexistent");
        assert!(matches!(
            not_found_err,
            Err(MarketError::AccessGrantNotFound(_))
        ));
    }

    #[test]
    fn test_revoked_listing_rejects_new_grants() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let (seller_key, seller_actor) = test_seller();
        let (_buyer_sec, buyer_pub_bytes, buyer_pub_hex) = test_buyer();

        let listing = store
            .create_listing(
                &seller_actor,
                NewListing {
                    seller_id: seller_actor.id().to_string(),
                    title: "Encrypted dataset 2".to_string(),
                    license: License::CcBy40,
                    price_credits: 10,
                },
            )
            .unwrap();

        // Revoke the listing
        store.revoke_listing(&seller_actor, &listing.id).unwrap();

        // Attempting to create grant for revoked listing must fail
        let ephemeral_seed = [5u8; 32];
        let nonce = [6u8; 12];
        let (wrapped, eph_pub, nonce_hex) =
            wrap_key_for_buyer(b"key", &buyer_pub_bytes, &ephemeral_seed, &nonce).unwrap();

        let expires_at = now_ms() + 3600 * 1000;
        let signing_bytes = grant_signing_bytes(
            &listing.id,
            &buyer_pub_hex,
            &eph_pub,
            &wrapped,
            &nonce_hex,
            expires_at,
        );
        let sig = seller_key.sign(&signing_bytes);
        let signature_hex = hex::encode(sig.to_bytes());

        let res = store.create_access_grant(
            &seller_actor,
            &listing.id,
            &buyer_pub_hex,
            &eph_pub,
            &wrapped,
            &nonce_hex,
            Some(3600),
            &signature_hex,
        );

        assert!(matches!(res, Err(MarketError::ListingRevoked(_))));
    }

    #[test]
    fn test_create_grant_input_validation() {
        let store = MarketStore::open_in_memory(MarketConfig::default()).unwrap();
        let (seller_key, seller_actor) = test_seller();
        let (_buyer_sec, _buyer_pub_bytes, buyer_pub_hex) = test_buyer();

        let listing = store
            .create_listing(
                &seller_actor,
                NewListing {
                    seller_id: seller_actor.id().to_string(),
                    title: "Encrypted dataset 3".to_string(),
                    license: License::CcBy40,
                    price_credits: 10,
                },
            )
            .unwrap();

        let valid_eph = "00".repeat(32);
        let valid_wrapped = "00".repeat(16);
        let valid_nonce = "00".repeat(12);
        let valid_sig = "00".repeat(64);

        // ttl == 0 rejected
        let res_ttl_zero = store.create_access_grant(
            &seller_actor,
            &listing.id,
            &buyer_pub_hex,
            &valid_eph,
            &valid_wrapped,
            &valid_nonce,
            Some(0),
            &valid_sig,
        );
        assert!(matches!(res_ttl_zero, Err(MarketError::InvalidInput(_))));

        // ttl > max_ttl rejected
        let res_ttl_huge = store.create_access_grant(
            &seller_actor,
            &listing.id,
            &buyer_pub_hex,
            &valid_eph,
            &valid_wrapped,
            &valid_nonce,
            Some(999_999_999),
            &valid_sig,
        );
        assert!(matches!(res_ttl_huge, Err(MarketError::InvalidInput(_))));

        // short buyer pubkey rejected
        let res_short_buyer = store.create_access_grant(
            &seller_actor,
            &listing.id,
            "1234",
            &valid_eph,
            &valid_wrapped,
            &valid_nonce,
            Some(3600),
            &valid_sig,
        );
        assert!(matches!(res_short_buyer, Err(MarketError::InvalidInput(_))));

        // short nonce rejected
        let res_short_nonce = store.create_access_grant(
            &seller_actor,
            &listing.id,
            &buyer_pub_hex,
            &valid_eph,
            &valid_wrapped,
            "00".repeat(11).as_str(),
            Some(3600),
            &valid_sig,
        );
        assert!(matches!(res_short_nonce, Err(MarketError::InvalidInput(_))));

        // short wrapped key rejected (<16 bytes)
        let res_short_wrapped = store.create_access_grant(
            &seller_actor,
            &listing.id,
            &buyer_pub_hex,
            &valid_eph,
            "00".repeat(15).as_str(),
            &valid_nonce,
            Some(3600),
            &valid_sig,
        );
        assert!(matches!(
            res_short_wrapped,
            Err(MarketError::InvalidInput(_))
        ));

        // non-seller actor forbidden
        let (_stranger_key, stranger_actor) = (
            SigningKey::from_bytes(&[9u8; 32]),
            Actor::for_test(&hex::encode([9u8; 32])),
        );
        let res_non_seller = store.create_access_grant(
            &stranger_actor,
            &listing.id,
            &buyer_pub_hex,
            &valid_eph,
            &valid_wrapped,
            &valid_nonce,
            Some(3600),
            &valid_sig,
        );
        assert!(matches!(res_non_seller, Err(MarketError::Forbidden(_))));
    }
}
