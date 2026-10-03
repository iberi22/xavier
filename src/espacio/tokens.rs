//! Space-scoped bearer tokens (WP-13l).
//!
//! Format: `xsp_{space_id}_{43-char base64url of 32 random bytes}`.
//!
//! * Only the SHA-256 of the whole token string is stored, in the space's own
//!   `espacio.sqlite` (`tokens` table). The secret is 256 bits from `OsRng`,
//!   so an unsalted hash is sufficient. The raw token is returned once at
//!   issue time and cannot be recovered.
//! * The space id in the prefix selects which database to open; it is parsed
//!   with [`validate_space_id`] and opened with `get_existing`, so a forged
//!   prefix never creates a directory or a database.
//! * The hash is bound to the full string, so a secret replayed under another
//!   space id hashes differently and is simply unknown there.
//! * Verification fails closed: unknown, malformed, revoked, expired, or
//!   belonging to a removed member all yield the same [`TokenError`].

use anyhow::{anyhow, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use rand::RngCore;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::invite::SpaceRole;
use super::manager::{SpaceInfo, SpaceManager};
use super::store::{validate_space_id, RevokeOutcome, SpaceStore, SpaceStores};

/// Token prefix.
pub const TOKEN_PREFIX: &str = "xsp_";
/// Length of the base64url secret (32 bytes, no padding).
const SECRET_LEN: usize = 43;

/// Identity proven by a valid space token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceAuth {
    pub space_id: String,
    pub member_id: String,
    pub role: SpaceRole,
}

/// Request extension inserted by the auth middleware.
pub type SpaceContext = SpaceAuth;

/// Why a token was refused. Deliberately coarse: callers map every variant to
/// the same 401 so the response does not tell an attacker which check failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    #[error("malformed space token")]
    Malformed,
    #[error("invalid space token")]
    Invalid,
}

/// Token metadata for listings. Never contains the secret or its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenInfo {
    pub token_id: String,
    pub member_id: String,
    pub role: SpaceRole,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked: bool,
}

fn ts(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn role_from(s: &str) -> Result<SpaceRole> {
    match s {
        "admin" => Ok(SpaceRole::Admin),
        "moderator" => Ok(SpaceRole::Moderator),
        "member" => Ok(SpaceRole::Member),
        "reader" => Ok(SpaceRole::Reader),
        other => Err(anyhow!("unknown role {other:?}")),
    }
}

fn rank(role: SpaceRole) -> u8 {
    match role {
        SpaceRole::Admin => 3,
        SpaceRole::Moderator => 2,
        SpaceRole::Member => 1,
        SpaceRole::Reader => 0,
    }
}

/// Hex SHA-256 of the full token string.
fn hash_token(raw: &str) -> String {
    crate::crypto::hex_encode(Sha256::digest(raw.as_bytes()))
}

/// Split a raw token into its space id, validating the shape. Does not touch
/// storage.
pub fn parse_token(raw: &str) -> Result<&str, TokenError> {
    let rest = raw
        .strip_prefix(TOKEN_PREFIX)
        .ok_or(TokenError::Malformed)?;
    // ASCII only: keeps the byte slicing below on char boundaries.
    if !rest.is_ascii() || rest.len() < SECRET_LEN + 2 {
        return Err(TokenError::Malformed);
    }
    let split = rest.len() - SECRET_LEN;
    let (head, secret) = rest.split_at(split);
    let space_id = head.strip_suffix('_').ok_or(TokenError::Malformed)?;
    if !secret
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(TokenError::Malformed);
    }
    validate_space_id(space_id).map_err(|_| TokenError::Malformed)?;
    Ok(space_id)
}

/// A freshly issued token. `raw` is shown once and never stored.
#[derive(Debug, Clone)]
pub struct IssuedToken {
    pub token_id: String,
    pub raw: String,
}

impl SpaceStore {
    /// Issue a token for `member_id` with `role`. Returns the raw token once.
    pub fn issue_token(
        &self,
        space_id: &str,
        member_id: &str,
        role: SpaceRole,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<IssuedToken> {
        validate_space_id(space_id)?;
        let mut secret = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut secret);
        let raw = format!(
            "{TOKEN_PREFIX}{space_id}_{}",
            crate::auth2::oauth::b64url_encode(&secret)
        );
        debug_assert_eq!(
            raw.len(),
            TOKEN_PREFIX.len() + space_id.len() + 1 + SECRET_LEN
        );
        let token_id = ulid::Ulid::new().to_string();
        self.lock().execute(
            "INSERT INTO tokens (token_id, token_hash, member_id, role, created_at, expires_at, revoked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![
                token_id,
                hash_token(&raw),
                member_id,
                role.as_str(),
                ts(&Utc::now()),
                expires_at.as_ref().map(ts)
            ],
        )?;
        Ok(IssuedToken { token_id, raw })
    }

    /// Revoke a token by id. Idempotent outcome reporting.
    pub fn revoke_token(&self, token_id: &str) -> Result<RevokeOutcome> {
        let conn = self.lock();
        let state: Option<i64> = conn
            .query_row(
                "SELECT revoked FROM tokens WHERE token_id = ?1",
                params![token_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(match state {
            None => RevokeOutcome::NotFound,
            Some(0) => {
                conn.execute(
                    "UPDATE tokens SET revoked = 1 WHERE token_id = ?1",
                    params![token_id],
                )?;
                RevokeOutcome::Revoked
            }
            Some(_) => RevokeOutcome::AlreadyRevoked,
        })
    }

    /// Token metadata, oldest first. No secrets, no hashes.
    pub fn list_tokens(&self) -> Result<Vec<TokenInfo>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT token_id, member_id, role, created_at, expires_at, revoked
             FROM tokens ORDER BY created_at, token_id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(token_id, member_id, role, created, expires, revoked)| {
                Ok(TokenInfo {
                    token_id,
                    member_id,
                    role: role_from(&role)?,
                    created_at: DateTime::parse_from_rfc3339(&created)?.with_timezone(&Utc),
                    expires_at: expires
                        .map(|e| DateTime::parse_from_rfc3339(&e).map(|d| d.with_timezone(&Utc)))
                        .transpose()?,
                    revoked: revoked != 0,
                })
            })
            .collect()
    }

    /// Verify a raw token against this store. `space_id` is the id parsed
    /// from the token prefix.
    pub fn verify_token(&self, space_id: &str, raw: &str) -> Result<SpaceAuth, TokenError> {
        let want = hash_token(raw);
        let row = self
            .lock()
            .query_row(
                "SELECT token_hash, member_id, role, expires_at, revoked
                 FROM tokens WHERE token_hash = ?1",
                params![want],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| TokenError::Invalid)?;
        let (stored, member_id, role, expires, revoked) = row.ok_or(TokenError::Invalid)?;
        // Constant-time comparison of the digests, independent of the index.
        if !bool::from(stored.as_bytes().ct_eq(want.as_bytes())) {
            return Err(TokenError::Invalid);
        }
        if revoked != 0 {
            return Err(TokenError::Invalid);
        }
        if let Some(e) = expires {
            // An unparseable expiry fails closed.
            let exp = DateTime::parse_from_rfc3339(&e).map_err(|_| TokenError::Invalid)?;
            if Utc::now() >= exp.with_timezone(&Utc) {
                return Err(TokenError::Invalid);
            }
        }
        let token_role = role_from(&role).map_err(|_| TokenError::Invalid)?;
        // The member must still exist; its current role caps the token role,
        // so removing or demoting a member takes effect immediately.
        let member = self
            .member(&member_id)
            .map_err(|_| TokenError::Invalid)?
            .ok_or(TokenError::Invalid)?;
        let effective = if rank(member.role) < rank(token_role) {
            member.role
        } else {
            token_role
        };
        Ok(SpaceAuth {
            space_id: space_id.to_string(),
            member_id,
            role: effective,
        })
    }

    /// Insert a member only if absent (never upgrades or overwrites an
    /// existing member). Returns whether a row was inserted.
    pub fn add_member_if_absent(&self, node_id: &str, role: SpaceRole) -> Result<bool> {
        let n = self.lock().execute(
            "INSERT OR IGNORE INTO members (node_id, role, joined_at) VALUES (?1, ?2, ?3)",
            params![node_id, role.as_str(), ts(&Utc::now())],
        )?;
        Ok(n > 0)
    }
}

/// Verify a raw token against the stores only (no registry check). Never
/// creates anything for an unknown space.
pub fn verify_in_stores(stores: &SpaceStores, raw: &str) -> Result<SpaceAuth, TokenError> {
    let space_id = parse_token(raw)?;
    let store = stores
        .get_existing(space_id)
        .map_err(|_| TokenError::Invalid)?
        .ok_or(TokenError::Invalid)?;
    store.verify_token(space_id, raw)
}

/// Verify a raw token for a registered, available space.
pub async fn verify_with_manager(
    manager: &SpaceManager,
    raw: &str,
) -> Result<SpaceAuth, TokenError> {
    let space_id = parse_token(raw)?;
    manager
        .get(space_id)
        .await
        .map_err(|_| TokenError::Invalid)?;
    verify_in_stores(&manager.stores(), raw)
}

/// Create a space and issue its owner an admin token (returned once, never
/// stored in clear). If issuing fails the space is removed again.
pub async fn create_space_with_owner_token(
    manager: &SpaceManager,
    id: String,
    name: String,
    description: String,
    owner_node: String,
    is_public: bool,
) -> Result<(SpaceInfo, String)> {
    let info = manager
        .create(id.clone(), name, description, owner_node.clone(), is_public)
        .await?;
    let issued = manager
        .stores()
        .get(&id)
        .and_then(|s| s.issue_token(&id, &owner_node, SpaceRole::Admin, None));
    match issued {
        Ok(t) => Ok((info, t.raw)),
        Err(e) => {
            let _ = manager.delete(&id).await;
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn stores_with(space: &str) -> (std::sync::Arc<SpaceStores>, std::sync::Arc<SpaceStore>) {
        let stores = SpaceStores::in_memory();
        let s = stores.get(space).unwrap();
        s.add_member("alice", SpaceRole::Admin).unwrap();
        s.add_member("bob", SpaceRole::Reader).unwrap();
        (stores, s)
    }

    #[test]
    fn format_is_xsp_space_43() {
        let (_, s) = stores_with("esp_a");
        let t = s
            .issue_token("esp_a", "alice", SpaceRole::Admin, None)
            .unwrap();
        let rest = t.raw.strip_prefix("xsp_esp_a_").unwrap();
        assert_eq!(rest.len(), 43);
        assert!(rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_eq!(parse_token(&t.raw).unwrap(), "esp_a");
        let t2 = s
            .issue_token("esp_a", "alice", SpaceRole::Admin, None)
            .unwrap();
        assert_ne!(t.raw, t2.raw);
    }

    #[test]
    fn valid_token_verifies_with_role() {
        let (stores, s) = stores_with("esp_a");
        let t = s
            .issue_token("esp_a", "bob", SpaceRole::Reader, None)
            .unwrap();
        let auth = verify_in_stores(&stores, &t.raw).unwrap();
        assert_eq!(
            auth,
            SpaceAuth {
                space_id: "esp_a".into(),
                member_id: "bob".into(),
                role: SpaceRole::Reader
            }
        );
    }

    #[test]
    fn malformed_tokens_rejected() {
        let good = "A".repeat(43);
        for bad in [
            String::new(),
            "xsp_".into(),
            "xsp__".into(),
            format!("xsp_{good}"),
            format!("xsp__{good}"),
            format!("xsp_../x_{good}"),
            format!("xsp_a/b_{good}"),
            format!("xsp_a.b_{good}"),
            format!("xsp_é_{good}"),
            format!("xsp_a_{}", "A".repeat(42)),
            format!("xsp_a_{}", "A".repeat(44)),
            format!("xsp_a_{}+", "A".repeat(42)),
            format!("xsp_a_{}\0", "A".repeat(42)),
            format!("xsp_{}_{good}", "a".repeat(65)),
            format!("xav_a_{good}"),
            format!("XSP_a_{good}"),
        ] {
            assert!(parse_token(&bad).is_err(), "{bad:?}");
        }
        assert_eq!(parse_token(&format!("xsp_a_{good}")), Ok("a"));
    }

    #[test]
    fn forged_unknown_and_cross_space_tokens_rejected() {
        let (stores, s) = stores_with("esp_a");
        let _ = stores.get("esp_b").unwrap();
        let t = s
            .issue_token("esp_a", "alice", SpaceRole::Admin, None)
            .unwrap();
        // Forged secret.
        let forged = format!("xsp_esp_a_{}", "B".repeat(43));
        assert_eq!(verify_in_stores(&stores, &forged), Err(TokenError::Invalid));
        // A's secret replayed under B's prefix.
        let secret = t.raw.strip_prefix("xsp_esp_a_").unwrap();
        let replay = format!("xsp_esp_b_{secret}");
        assert_eq!(verify_in_stores(&stores, &replay), Err(TokenError::Invalid));
        // Unknown space: must not create anything.
        let ghost = format!("xsp_ghost_{secret}");
        assert_eq!(verify_in_stores(&stores, &ghost), Err(TokenError::Invalid));
        assert!(stores.get_existing("ghost").unwrap().is_none());
    }

    #[test]
    fn revoked_expired_and_removed_member_rejected() {
        let (stores, s) = stores_with("esp_a");
        let t = s
            .issue_token("esp_a", "alice", SpaceRole::Admin, None)
            .unwrap();
        assert!(verify_in_stores(&stores, &t.raw).is_ok());
        assert_eq!(s.revoke_token(&t.token_id).unwrap(), RevokeOutcome::Revoked);
        assert_eq!(
            s.revoke_token(&t.token_id).unwrap(),
            RevokeOutcome::AlreadyRevoked
        );
        assert_eq!(s.revoke_token("nope").unwrap(), RevokeOutcome::NotFound);
        assert_eq!(verify_in_stores(&stores, &t.raw), Err(TokenError::Invalid));

        let past = Utc::now() - Duration::seconds(1);
        let e = s
            .issue_token("esp_a", "alice", SpaceRole::Admin, Some(past))
            .unwrap();
        assert_eq!(verify_in_stores(&stores, &e.raw), Err(TokenError::Invalid));
        let future = Utc::now() + Duration::hours(1);
        let f = s
            .issue_token("esp_a", "alice", SpaceRole::Admin, Some(future))
            .unwrap();
        assert!(verify_in_stores(&stores, &f.raw).is_ok());

        s.remove_member("alice").unwrap();
        assert_eq!(verify_in_stores(&stores, &f.raw), Err(TokenError::Invalid));
    }

    #[test]
    fn member_demotion_caps_token_role() {
        let (stores, s) = stores_with("esp_a");
        let t = s
            .issue_token("esp_a", "alice", SpaceRole::Admin, None)
            .unwrap();
        s.add_member("alice", SpaceRole::Reader).unwrap();
        assert_eq!(
            verify_in_stores(&stores, &t.raw).unwrap().role,
            SpaceRole::Reader
        );
        // A token can never exceed the member's role the other way either.
        let r = s
            .issue_token("esp_a", "bob", SpaceRole::Admin, None)
            .unwrap();
        assert_eq!(
            verify_in_stores(&stores, &r.raw).unwrap().role,
            SpaceRole::Reader
        );
    }

    #[test]
    fn listing_has_no_secret_or_hash() {
        let (_, s) = stores_with("esp_a");
        let t = s
            .issue_token("esp_a", "alice", SpaceRole::Admin, None)
            .unwrap();
        let list = s.list_tokens().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].token_id, t.token_id);
        let dump = format!("{list:?}");
        let secret = t.raw.strip_prefix("xsp_esp_a_").unwrap();
        assert!(!dump.contains(secret));
        assert!(!dump.contains(&hash_token(&t.raw)));
    }

    #[test]
    fn only_the_hash_reaches_the_database_file() {
        let tmp = tempfile::tempdir().unwrap();
        let stores = SpaceStores::open(tmp.path());
        let s = stores.get("esp_a").unwrap();
        s.add_member("alice", SpaceRole::Admin).unwrap();
        let t = s
            .issue_token("esp_a", "alice", SpaceRole::Admin, None)
            .unwrap();
        let secret = t.raw.strip_prefix("xsp_esp_a_").unwrap().to_string();
        drop(s);
        let conn = rusqlite::Connection::open(
            tmp.path()
                .join("spaces/esp_a")
                .join(super::super::store::DB_FILE),
        )
        .unwrap();
        let stored: String = conn
            .query_row("SELECT token_hash FROM tokens", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, hash_token(&t.raw));
        assert_ne!(stored, t.raw);
        drop(conn);
        // Raw bytes of every file under the space directory.
        for e in std::fs::read_dir(tmp.path().join("spaces/esp_a")).unwrap() {
            let bytes = std::fs::read(e.unwrap().path()).unwrap();
            assert!(
                !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                "raw secret found on disk"
            );
        }
    }

    #[test]
    fn tokens_survive_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let raw = {
            let stores = SpaceStores::open(tmp.path());
            let s = stores.get("esp_a").unwrap();
            s.add_member("alice", SpaceRole::Admin).unwrap();
            s.issue_token("esp_a", "alice", SpaceRole::Admin, None)
                .unwrap()
                .raw
        };
        let stores = SpaceStores::open(tmp.path());
        assert!(verify_in_stores(&stores, &raw).is_ok());
    }

    #[test]
    fn add_member_if_absent_never_overwrites() {
        let (_, s) = stores_with("esp_a");
        assert!(!s.add_member_if_absent("alice", SpaceRole::Reader).unwrap());
        assert_eq!(s.member("alice").unwrap().unwrap().role, SpaceRole::Admin);
        assert!(s.add_member_if_absent("carol", SpaceRole::Member).unwrap());
    }
}
