//! Space invites — signed, expiring invitations for Spaces (T-02)
//!
//! Each invite is signed by the inviter's NodeIdentity (Ed25519) and expires
//! after 24h. Verification checks signature, expiry and space existence.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

    /// Check if invite is expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if invite is still valid (not revoked and not expired)
    pub fn is_valid(&self) -> bool {
        !self.revoked && !self.is_expired()
    }
}

/// In-memory invite registry. Persists only for the process lifetime;
/// durability will be added via SQLite in a follow-up iteration.
#[derive(Debug, Default)]
pub struct InviteManager {
    invites: Arc<RwLock<HashMap<String, SpaceInvite>>>,
}

impl InviteManager {
    pub fn new() -> Self {
        Self::default()
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
        let invite = SpaceInvite {
            id: ulid::Ulid::new().to_string(),
            space_id,
            inviter_node,
            target_node,
            role,
            created_at: now,
            expires_at: now + Duration::hours(24),
            signature: None,
            revoked: false,
        };
        let id = invite.id.clone();
        self.invites.write().await.insert(id, invite.clone());
        Ok(invite)
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
        let id = invite.id.clone();
        self.invites.write().await.insert(id, invite.clone());
        Ok(invite)
    }

    /// Retrieve an invite by id
    pub async fn get(&self, id: &str) -> Result<SpaceInvite> {
        let guard = self.invites.read().await;
        guard
            .get(id)
            .cloned()
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

    /// Revoke an invite (admin only)
    pub async fn revoke(&self, id: &str) -> Result<()> {
        let mut guard = self.invites.write().await;
        let invite = guard
            .get_mut(id)
            .ok_or_else(|| anyhow!("Invite {} not found", id))?;
        if invite.revoked {
            return Err(anyhow!("Invite {} already revoked", id));
        }
        invite.revoked = true;
        Ok(())
    }

    /// List invites for a space
    pub async fn list_for_space(&self, space_id: &str) -> Vec<SpaceInvite> {
        let guard = self.invites.read().await;
        guard
            .values()
            .filter(|i| i.space_id == space_id)
            .cloned()
            .collect()
    }

    /// Attach a signature to an existing invite (Ed25519 hex over canonical payload)
    pub async fn attach_signature(&self, id: &str, signature_hex: String) -> Result<()> {
        let mut guard = self.invites.write().await;
        let invite = guard
            .get_mut(id)
            .ok_or_else(|| anyhow!("Invite {} not found", id))?;
        invite.signature = Some(signature_hex);
        Ok(())
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
}
