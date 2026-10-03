//! Space invites — signed, expiring invitations for Spaces (T-02)
//!
//! An invite may carry an Ed25519 signature (hex) over its canonical payload.
//! Signatures are only checked when the caller supplies a trusted verifying
//! key (`SpaceInvite::verify`, `InviteManager::validate_trusted`); unsigned
//! invites remain accepted by plain `validate`. Invites expire after 24h.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::store::{RevokeOutcome, SpaceStore, SpaceStores};

/// Role granted by an invite
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceRole {
    Admin,
    Moderator,
    Member,
    Reader,
}

impl SpaceRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Moderator => "moderator",
            Self::Member => "member",
            Self::Reader => "reader",
        }
    }
}

/// A signed invite to join a Space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceInvite {
    /// Unique invite id (ULID)
    pub id: String,
    /// Target space id
    pub space_id: String,
    /// Inviter node id (must be admin/moderator of the space)
    pub inviter_node: String,
    /// Target node id (invited peer)
    pub target_node: String,
    /// Role to grant on acceptance
    pub role: SpaceRole,
    /// Creation time
    pub created_at: DateTime<Utc>,
    /// Expiry (default 24h after creation)
    pub expires_at: DateTime<Utc>,
    /// Optional Ed25519 signature (hex) over canonical payload. None = unsigned (dev).
    pub signature: Option<String>,
    /// Revoked flag
    pub revoked: bool,
}

impl SpaceInvite {
    /// Canonical payload that is signed (deterministic)
    pub fn canonical_payload(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.id,
            self.space_id,
            self.inviter_node,
            self.target_node,
            self.role.as_str()
        )
    }

    /// True if the invite carries a signature (not necessarily valid).
    pub fn is_signed(&self) -> bool {
        self.signature.is_some()
    }

    /// Verify the signature against a trusted key. Unsigned, malformed or
    /// forged signatures all return an error.
    pub fn verify(&self, trusted: &VerifyingKey) -> Result<()> {
        let sig_hex = self
            .signature
            .as_deref()
            .ok_or_else(|| anyhow!("Invite {} is unsigned", self.id))?;
        let bytes = crate::crypto::hex_decode(sig_hex)
            .map_err(|_| anyhow!("Invite {} signature malformed", self.id))?;
        let sig = Signature::from_slice(&bytes)
            .map_err(|_| anyhow!("Invite {} signature malformed", self.id))?;
        trusted
            .verify(self.canonical_payload().as_bytes(), &sig)
            .map_err(|_| anyhow!("Invite {} signature invalid", self.id))
    }

    /// Check if invite is expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if invite is still valid (not revoked and not expired)
    pub fn is_valid(&self) -> bool {
        !self.revoked && !self.is_expired()
    }
}

/// Result of redeeming an invite. `token` is shown once and never stored.
#[derive(Debug, Clone)]
pub struct AcceptedInvite {
    pub space_id: String,
    pub member_id: String,
    pub role: SpaceRole,
    pub token_id: String,
    pub token: String,
}

/// Invite registry backed by each space's `espacio.sqlite` (in-memory
/// databases for `new()`). Revoked state and signatures are persisted; an
/// id -> space index is rebuilt from the stores when opened.
#[derive(Debug, Default)]
pub struct InviteManager {
    stores: Arc<SpaceStores>,
    /// invite id -> space id
    index: Arc<RwLock<HashMap<String, String>>>,
}

impl InviteManager {
    /// Non-persistent manager (tests, ephemeral use).
    pub fn new() -> Self {
        Self::default()
    }

    /// Persistent manager over `{root}/spaces/{space_id}/espacio.sqlite`.
    pub fn open(root: impl AsRef<std::path::Path>) -> Self {
        Self::with_stores(SpaceStores::open(root))
    }

    /// Manager sharing a store registry (e.g. `SpaceManager::stores()`).
    /// Reloads the invite index from every space database on disk.
    pub fn with_stores(stores: Arc<SpaceStores>) -> Self {
        let mut index = HashMap::new();
        for space_id in stores.known_space_ids() {
            match stores.get(&space_id).and_then(|s| s.invite_ids()) {
                Ok(ids) => {
                    for id in ids {
                        index.insert(id, space_id.clone());
                    }
                }
                Err(e) => tracing::error!("espacio: cannot load invites of {space_id}: {e}"),
            }
        }
        Self {
            stores,
            index: Arc::new(RwLock::new(index)),
        }
    }

    async fn insert(&self, invite: &SpaceInvite) -> Result<()> {
        self.stores.get(&invite.space_id)?.insert_invite(invite)?;
        self.index
            .write()
            .await
            .insert(invite.id.clone(), invite.space_id.clone());
        Ok(())
    }

    async fn store_of(&self, id: &str) -> Result<Arc<SpaceStore>> {
        let space_id = self
            .index
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("Invite {} not found", id))?;
        self.stores.get(&space_id)
    }

    /// Create a new invite. Caller must have verified inviter has permission.
    pub async fn create(
        &self,
        space_id: String,
        inviter_node: String,
        target_node: String,
        role: SpaceRole,
    ) -> Result<SpaceInvite> {
        let now = Utc::now();
        self.create_with_expiry(
            space_id,
            inviter_node,
            target_node,
            role,
            now + Duration::hours(24),
        )
        .await
    }

    /// Create with explicit expiry (for testing)
    pub async fn create_with_expiry(
        &self,
        space_id: String,
        inviter_node: String,
        target_node: String,
        role: SpaceRole,
        expires_at: DateTime<Utc>,
    ) -> Result<SpaceInvite> {
        let invite = SpaceInvite {
            id: ulid::Ulid::new().to_string(),
            space_id,
            inviter_node,
            target_node,
            role,
            created_at: Utc::now(),
            expires_at,
            signature: None,
            revoked: false,
        };
        self.insert(&invite).await?;
        Ok(invite)
    }

    /// Retrieve an invite by id
    pub async fn get(&self, id: &str) -> Result<SpaceInvite> {
        self.store_of(id)
            .await?
            .get_invite(id)?
            .ok_or_else(|| anyhow!("Invite {} not found", id))
    }

    /// Validate an invite (checks revoked + expiry). Returns the invite if valid.
    pub async fn validate(&self, id: &str) -> Result<SpaceInvite> {
        let invite = self.get(id).await?;
        if invite.revoked {
            return Err(anyhow!("Invite {} revoked", id));
        }
        if invite.is_expired() {
            return Err(anyhow!("Invite {} expired", id));
        }
        Ok(invite)
    }

    /// Validate and verify the signature against a trusted key. A present
    /// signature must verify; an unsigned invite is rejected when
    /// `require_signed` is set.
    pub async fn validate_trusted(
        &self,
        id: &str,
        trusted: &VerifyingKey,
        require_signed: bool,
    ) -> Result<SpaceInvite> {
        let invite = self.validate(id).await?;
        if invite.is_signed() {
            invite.verify(trusted)?;
        } else if require_signed {
            return Err(anyhow!("Invite {} is unsigned", id));
        }
        Ok(invite)
    }

    /// Redeem an invite: validate it, add the member with the invite role and
    /// issue one `xsp_` token (returned once).
    ///
    /// * With `trusted_key` the signature is verified (`validate_trusted`);
    ///   `require_signed` without a key is refused because it cannot be met.
    /// * The invite is single-use: it is revoked atomically before anything
    ///   is issued, so concurrent or repeated accepts yield one token at most.
    /// * An existing member id is never overwritten or upgraded.
    pub async fn accept_invite(
        &self,
        invite_id: &str,
        member_display: &str,
        trusted_key: Option<&VerifyingKey>,
        require_signed: bool,
    ) -> Result<AcceptedInvite> {
        let member_id = member_display.trim();
        if member_id.is_empty()
            || member_id.len() > 128
            || member_id.chars().any(|c| c.is_control())
        {
            return Err(anyhow!("invalid member display name"));
        }
        let invite = match trusted_key {
            Some(key) => {
                self.validate_trusted(invite_id, key, require_signed)
                    .await?
            }
            None if require_signed => {
                return Err(anyhow!("signature required but no trusted key supplied"))
            }
            None => self.validate(invite_id).await?,
        };
        let store = self.stores.get(&invite.space_id)?;
        if store.member(member_id)?.is_some() {
            return Err(anyhow!("member {member_id} already exists in the space"));
        }
        // Claim the invite (atomic); only the winner proceeds.
        match store.revoke_invite(invite_id)? {
            RevokeOutcome::Revoked => {}
            _ => return Err(anyhow!("Invite {} already used", invite_id)),
        }
        if !store.add_member_if_absent(member_id, invite.role)? {
            return Err(anyhow!("member {member_id} already exists in the space"));
        }
        let issued = store.issue_token(&invite.space_id, member_id, invite.role, None)?;
        Ok(AcceptedInvite {
            space_id: invite.space_id,
            member_id: member_id.to_string(),
            role: invite.role,
            token_id: issued.token_id,
            token: issued.raw,
        })
    }

    /// Revoke an invite (admin only)
    pub async fn revoke(&self, id: &str) -> Result<()> {
        match self.store_of(id).await?.revoke_invite(id)? {
            RevokeOutcome::Revoked => Ok(()),
            RevokeOutcome::AlreadyRevoked => Err(anyhow!("Invite {} already revoked", id)),
            RevokeOutcome::NotFound => Err(anyhow!("Invite {} not found", id)),
        }
    }

    /// List invites for a space
    pub async fn list_for_space(&self, space_id: &str) -> Vec<SpaceInvite> {
        let res = self
            .stores
            .get_existing(space_id)
            .and_then(|s| s.map(|s| s.list_invites()).transpose());
        match res {
            Ok(v) => v.unwrap_or_default(),
            Err(e) => {
                tracing::error!("espacio: cannot list invites of {space_id}: {e}");
                Vec::new()
            }
        }
    }

    /// Attach a signature to an existing invite (Ed25519 hex over canonical payload).
    /// Not verified here; use `validate_trusted` on the accept path.
    pub async fn attach_signature(&self, id: &str, signature_hex: String) -> Result<()> {
        if self
            .store_of(id)
            .await?
            .set_invite_signature(id, &signature_hex)?
        {
            Ok(())
        } else {
            Err(anyhow!("Invite {} not found", id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    #[tokio::test]
    async fn create_and_validate() {
        let mgr = InviteManager::new();
        let inv = mgr
            .create(
                "esp_a".into(),
                "xv1_admin".into(),
                "xv1_bob".into(),
                SpaceRole::Member,
            )
            .await
            .unwrap();
        assert!(inv.is_valid());
        assert_eq!(inv.role, SpaceRole::Member);
        let fetched = mgr.validate(&inv.id).await.unwrap();
        assert_eq!(fetched.id, inv.id);
    }

    #[tokio::test]
    async fn expired_is_rejected() {
        let mgr = InviteManager::new();
        let past = Utc::now() - ChronoDuration::hours(1);
        let inv = mgr
            .create_with_expiry(
                "esp_a".into(),
                "xv1_admin".into(),
                "xv1_bob".into(),
                SpaceRole::Reader,
                past,
            )
            .await
            .unwrap();
        assert!(inv.is_expired());
        assert!(mgr.validate(&inv.id).await.is_err());
    }

    #[tokio::test]
    async fn revoke_blocks_validate() {
        let mgr = InviteManager::new();
        let inv = mgr
            .create(
                "esp_a".into(),
                "xv1_admin".into(),
                "xv1_bob".into(),
                SpaceRole::Moderator,
            )
            .await
            .unwrap();
        mgr.revoke(&inv.id).await.unwrap();
        assert!(mgr.validate(&inv.id).await.is_err());
        assert!(mgr.get(&inv.id).await.unwrap().revoked);
    }

    #[tokio::test]
    async fn list_per_space() {
        let mgr = InviteManager::new();
        mgr.create("esp_a".into(), "n1".into(), "n2".into(), SpaceRole::Member)
            .await
            .unwrap();
        mgr.create("esp_a".into(), "n1".into(), "n3".into(), SpaceRole::Reader)
            .await
            .unwrap();
        mgr.create("esp_b".into(), "n1".into(), "n2".into(), SpaceRole::Member)
            .await
            .unwrap();
        assert_eq!(mgr.list_for_space("esp_a").await.len(), 2);
        assert_eq!(mgr.list_for_space("esp_b").await.len(), 1);
    }

    #[test]
    fn canonical_payload_deterministic() {
        let inv = SpaceInvite {
            id: "01H".into(),
            space_id: "esp_a".into(),
            inviter_node: "xv1_admin".into(),
            target_node: "xv1_bob".into(),
            role: SpaceRole::Admin,
            created_at: Utc::now(),
            expires_at: Utc::now() + ChronoDuration::hours(24),
            signature: None,
            revoked: false,
        };
        assert_eq!(inv.canonical_payload(), "01H:esp_a:xv1_admin:xv1_bob:admin");
    }

    fn signed_fixture() -> (InviteManager, ed25519_dalek::SigningKey) {
        (
            InviteManager::new(),
            ed25519_dalek::SigningKey::from_bytes(&[3u8; 32]),
        )
    }

    async fn make(mgr: &InviteManager) -> SpaceInvite {
        mgr.create(
            "esp_a".into(),
            "adm".into(),
            "bob".into(),
            SpaceRole::Member,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn valid_signature_accepted() {
        use ed25519_dalek::Signer;
        let (mgr, sk) = signed_fixture();
        let inv = make(&mgr).await;
        let sig = sk.sign(inv.canonical_payload().as_bytes());
        mgr.attach_signature(&inv.id, crate::crypto::hex_encode(sig.to_bytes()))
            .await
            .unwrap();
        let got = mgr
            .validate_trusted(&inv.id, &sk.verifying_key(), true)
            .await
            .unwrap();
        assert!(got.is_signed());
        assert!(got.verify(&sk.verifying_key()).is_ok());
    }

    #[tokio::test]
    async fn forged_signature_rejected() {
        use ed25519_dalek::Signer;
        let (mgr, sk) = signed_fixture();
        let attacker = ed25519_dalek::SigningKey::from_bytes(&[4u8; 32]);
        let inv = make(&mgr).await;
        let sig = attacker.sign(inv.canonical_payload().as_bytes());
        mgr.attach_signature(&inv.id, crate::crypto::hex_encode(sig.to_bytes()))
            .await
            .unwrap();
        assert!(mgr
            .validate_trusted(&inv.id, &sk.verifying_key(), false)
            .await
            .is_err());
        mgr.attach_signature(&inv.id, "zz".into()).await.unwrap();
        assert!(mgr
            .validate_trusted(&inv.id, &sk.verifying_key(), false)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn unsigned_policy() {
        let (mgr, sk) = signed_fixture();
        let inv = make(&mgr).await;
        assert!(!inv.is_signed());
        assert!(inv.verify(&sk.verifying_key()).is_err());
        assert!(mgr
            .validate_trusted(&inv.id, &sk.verifying_key(), false)
            .await
            .is_ok());
        assert!(mgr
            .validate_trusted(&inv.id, &sk.verifying_key(), true)
            .await
            .is_err());
    }
    #[tokio::test]
    async fn accept_issues_one_token_with_invite_role_and_is_single_use() {
        let (mgr, sk) = signed_fixture();
        let inv = make(&mgr).await;
        let acc = mgr
            .accept_invite(&inv.id, "bob", Some(&sk.verifying_key()), false)
            .await
            .unwrap();
        assert_eq!(acc.space_id, "esp_a");
        assert_eq!(acc.role, SpaceRole::Member);
        assert!(acc.token.starts_with("xsp_esp_a_"));
        let auth = crate::espacio::tokens::verify_in_stores(&mgr.stores, &acc.token).unwrap();
        assert_eq!(auth.member_id, "bob");
        assert_eq!(auth.role, SpaceRole::Member);
        // Second redemption fails (invite consumed), no second token.
        assert!(mgr
            .accept_invite(&inv.id, "eve", None, false)
            .await
            .is_err());
        assert_eq!(
            mgr.stores
                .get("esp_a")
                .unwrap()
                .list_tokens()
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn accept_rejects_forged_signature_revoked_expired_and_existing_member() {
        use ed25519_dalek::Signer;
        let (mgr, sk) = signed_fixture();
        let key = sk.verifying_key();
        // Forged signature with a key supplied.
        let inv = make(&mgr).await;
        let attacker = ed25519_dalek::SigningKey::from_bytes(&[4u8; 32]);
        let sig = attacker.sign(inv.canonical_payload().as_bytes());
        mgr.attach_signature(&inv.id, crate::crypto::hex_encode(sig.to_bytes()))
            .await
            .unwrap();
        assert!(mgr
            .accept_invite(&inv.id, "bob", Some(&key), false)
            .await
            .is_err());
        // Unsigned but signature required.
        let inv2 = make(&mgr).await;
        assert!(mgr
            .accept_invite(&inv2.id, "bob", Some(&key), true)
            .await
            .is_err());
        // require_signed without a key cannot be satisfied.
        assert!(mgr
            .accept_invite(&inv2.id, "bob", None, true)
            .await
            .is_err());
        // Revoked.
        mgr.revoke(&inv2.id).await.unwrap();
        assert!(mgr
            .accept_invite(&inv2.id, "bob", None, false)
            .await
            .is_err());
        // Expired.
        let exp = mgr
            .create_with_expiry(
                "esp_a".into(),
                "adm".into(),
                "bob".into(),
                SpaceRole::Member,
                Utc::now() - Duration::seconds(1),
            )
            .await
            .unwrap();
        assert!(mgr
            .accept_invite(&exp.id, "bob", None, false)
            .await
            .is_err());
        // Existing member (e.g. the admin) cannot be impersonated or demoted.
        let store = mgr.stores.get("esp_a").unwrap();
        store.add_member("owner", SpaceRole::Admin).unwrap();
        let inv3 = make(&mgr).await;
        assert!(mgr
            .accept_invite(&inv3.id, "owner", None, false)
            .await
            .is_err());
        assert_eq!(
            store.member("owner").unwrap().unwrap().role,
            SpaceRole::Admin
        );
        // Invalid display names.
        assert!(mgr
            .accept_invite(&inv3.id, "  ", None, false)
            .await
            .is_err());
        assert!(mgr
            .accept_invite(&inv3.id, "a\nb", None, false)
            .await
            .is_err());
        // Nothing was issued by any failed attempt.
        assert!(store.list_tokens().unwrap().is_empty());
    }
}
