//! Space manager — creates and isolates Spaces (T-01)
//!
//! Each Space is a dedicated directory `data/spaces/{space_id}/` with its own
//! SQLite_vec store, belief graph and timeline. Isolation is enforced by
//! distinct WorkspaceConfig.id and separate storage paths.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::invite::SpaceRole;
use super::permissions::SpaceMembership;
use super::store::{self, SpaceStores};

/// Payload for creating a new Space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSpaceRequest {
    /// Unique space identifier (e.g., esp_01H...)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    #[serde(default)]
    pub description: String,
    /// Owner node id (admin)
    pub owner_node: String,
    /// Whether the space is public
    #[serde(default)]
    pub is_public: bool,
}

/// Information about a Space (Telegram-like group)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceInfo {
    /// Unique space identifier (e.g., esp_01H...)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Owner node id (admin)
    pub owner_node: String,
    /// Whether the space is public (listed in Directory Chain)
    pub is_public: bool,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Namespace: xavier://{space_id}/{appId}/{instanceId}
    pub namespace: String,
    /// Storage path
    pub storage_path: PathBuf,
}

/// On-disk descriptor (`space.json`). No secrets, no absolute paths: the
/// directory is self-contained and the storage path is derived at load time.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpaceDescriptor {
    version: u32,
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    owner_node: String,
    #[serde(default)]
    is_public: bool,
    created_at: DateTime<Utc>,
    namespace: String,
}

const DESCRIPTOR_VERSION: u32 = 1;

impl SpaceDescriptor {
    fn from_info(info: &SpaceInfo) -> Self {
        Self {
            version: DESCRIPTOR_VERSION,
            id: info.id.clone(),
            name: info.name.clone(),
            description: info.description.clone(),
            owner_node: info.owner_node.clone(),
            is_public: info.is_public,
            created_at: info.created_at,
            namespace: info.namespace.clone(),
        }
    }

    fn into_info(self, storage_path: PathBuf) -> SpaceInfo {
        SpaceInfo {
            id: self.id,
            name: self.name,
            description: self.description,
            owner_node: self.owner_node,
            is_public: self.is_public,
            created_at: self.created_at,
            namespace: self.namespace,
            storage_path,
        }
    }
}

/// Errors for Space operations
#[derive(Debug, thiserror::Error)]
pub enum SpaceError {
    #[error("Space {0} already exists")]
    AlreadyExists(String),
    #[error("Space {0} not found")]
    NotFound(String),
    #[error("Invalid space id: {0}")]
    InvalidId(String),
    #[error("Storage error: {0}")]
    Storage(String),
}

/// Manages isolated Spaces. Wraps WorkspaceRegistry concept but keeps
/// Spaces separate from legacy single-workspace flow until migration.
#[derive(Debug, Default)]
pub struct SpaceManager {
    spaces: Arc<RwLock<HashMap<String, SpaceInfo>>>,
    /// Spaces whose `space.json` could not be loaded: id -> reason. Their
    /// data is never touched.
    unavailable: Arc<RwLock<HashMap<String, String>>>,
    base_dir: PathBuf,
    stores: Arc<SpaceStores>,
}

impl SpaceManager {
    /// Create a non-persistent manager rooted at `base_dir` (e.g., data/spaces).
    /// The registry and the per-space members live in memory only; use
    /// [`SpaceManager::open`] for durability.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            spaces: Arc::new(RwLock::new(HashMap::new())),
            unavailable: Arc::new(RwLock::new(HashMap::new())),
            base_dir: base_dir.into(),
            stores: SpaceStores::in_memory(),
        }
    }

    /// Open a persistent manager rooted at `root`. Spaces live in
    /// `{root}/spaces/{space_id}/` and the registry is rebuilt by scanning
    /// every `space.json`. A directory whose descriptor is missing is
    /// ignored; one whose descriptor is corrupt is listed as unavailable and
    /// left untouched.
    pub fn open(root: impl AsRef<Path>) -> Self {
        let stores = SpaceStores::open(root.as_ref());
        Self::open_with_stores(stores)
    }

    /// Like [`SpaceManager::open`] but sharing an existing store registry
    /// (so channel and invite managers see the same open databases).
    pub fn open_with_stores(stores: Arc<SpaceStores>) -> Self {
        let base_dir = stores
            .spaces_dir()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let (spaces, unavailable) = Self::scan(&base_dir);
        Self {
            spaces: Arc::new(RwLock::new(spaces)),
            unavailable: Arc::new(RwLock::new(unavailable)),
            base_dir,
            stores,
        }
    }

    /// Shared per-space store registry (hand it to `ChannelManager` /
    /// `InviteManager` via their `with_stores` constructors).
    pub fn stores(&self) -> Arc<SpaceStores> {
        self.stores.clone()
    }

    fn scan(base_dir: &Path) -> (HashMap<String, SpaceInfo>, HashMap<String, String>) {
        let mut spaces = HashMap::new();
        let mut unavailable = HashMap::new();
        let Ok(rd) = std::fs::read_dir(base_dir) else {
            return (spaces, unavailable);
        };
        for entry in rd.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if Self::validate_id(&name).is_err() {
                tracing::warn!("espacio: ignoring directory with invalid space id {name:?}");
                continue;
            }
            let desc_path = dir.join(store::DESCRIPTOR_FILE);
            if !desc_path.exists() {
                // Incomplete creation or foreign dir: not a registered space.
                continue;
            }
            let loaded = std::fs::read(&desc_path)
                .map_err(|e| e.to_string())
                .and_then(|b| {
                    serde_json::from_slice::<SpaceDescriptor>(&b).map_err(|e| e.to_string())
                })
                .and_then(|d| {
                    if d.id == name {
                        Ok(d)
                    } else {
                        Err(format!("descriptor id {:?} does not match directory", d.id))
                    }
                });
            match loaded {
                Ok(d) => {
                    spaces.insert(name, d.into_info(dir));
                }
                Err(reason) => {
                    tracing::error!(
                        "espacio: space {name} unavailable (corrupt {}): {reason}; data left untouched",
                        store::DESCRIPTOR_FILE
                    );
                    unavailable.insert(name, reason);
                }
            }
        }
        (spaces, unavailable)
    }

    /// Spaces that exist on disk but whose descriptor failed to load.
    pub async fn unavailable(&self) -> Vec<(String, String)> {
        let guard = self.unavailable.read().await;
        let mut v: Vec<_> = guard.iter().map(|(k, r)| (k.clone(), r.clone())).collect();
        v.sort();
        v
    }

    /// Validate space id format (allowlist: alphanumeric, dash, underscore, max 64)
    fn validate_id(id: &str) -> Result<()> {
        store::validate_space_id(id)
    }

    // ---- members (persisted in the space's espacio.sqlite) ----

    async fn ensure_available(&self, id: &str) -> Result<()> {
        Self::validate_id(id)?;
        if self.unavailable.read().await.contains_key(id) {
            return Err(anyhow!(SpaceError::Storage(format!(
                "space {id} is unavailable (corrupt descriptor)"
            ))));
        }
        if !self.spaces.read().await.contains_key(id) {
            return Err(anyhow!(SpaceError::NotFound(id.to_string())));
        }
        Ok(())
    }

    /// Add a member (or change its role) in a registered space.
    pub async fn add_member(
        &self,
        space_id: &str,
        node_id: &str,
        role: SpaceRole,
    ) -> Result<SpaceMembership> {
        self.ensure_available(space_id).await?;
        self.stores.get(space_id)?.add_member(node_id, role)
    }

    /// Remove a member. Returns whether it existed.
    pub async fn remove_member(&self, space_id: &str, node_id: &str) -> Result<bool> {
        self.ensure_available(space_id).await?;
        self.stores.get(space_id)?.remove_member(node_id)
    }

    /// Members of a space, oldest first.
    pub async fn members(&self, space_id: &str) -> Result<Vec<SpaceMembership>> {
        self.ensure_available(space_id).await?;
        self.stores.get(space_id)?.members()
    }

    /// Build namespace for a space
    pub fn namespace_for(space_id: &str, app_id: &str, instance_id: &str) -> String {
        format!("xavier://{}/{}/{}", space_id, app_id, instance_id)
    }

    /// Create a new isolated Space
    pub async fn create(
        &self,
        id: String,
        name: String,
        description: String,
        owner_node: String,
        is_public: bool,
    ) -> Result<SpaceInfo> {
        Self::validate_id(&id)?;
        let mut guard = self.spaces.write().await;
        if guard.contains_key(&id) || self.unavailable.read().await.contains_key(&id) {
            return Err(anyhow!(SpaceError::AlreadyExists(id)));
        }
        let storage_path = self.base_dir.join(&id);
        tokio::fs::create_dir_all(&storage_path)
            .await
            .map_err(|e| anyhow!(SpaceError::Storage(e.to_string())))?;

        let info = SpaceInfo {
            id: id.clone(),
            name,
            description,
            owner_node,
            is_public,
            created_at: Utc::now(),
            namespace: Self::namespace_for(&id, "xavier", "default"),
            storage_path,
        };
        // Database first (creates the schema and the owner as admin member),
        // descriptor last: space.json is the commit marker the scan keys on.
        let persist = || -> Result<()> {
            self.stores
                .get(&id)?
                .add_member(&info.owner_node, SpaceRole::Admin)?;
            if self.stores.is_persistent() {
                let bytes = serde_json::to_vec_pretty(&SpaceDescriptor::from_info(&info))?;
                store::write_atomic(&info.storage_path.join(store::DESCRIPTOR_FILE), &bytes)?;
            }
            Ok(())
        };
        if let Err(e) = persist() {
            self.stores.evict(&id);
            return Err(anyhow!(SpaceError::Storage(e.to_string())));
        }
        guard.insert(id, info.clone());
        Ok(info)
    }

    /// Get a Space by id
    pub async fn get(&self, id: &str) -> Result<SpaceInfo> {
        if let Some(reason) = self.unavailable.read().await.get(id) {
            return Err(anyhow!(SpaceError::Storage(format!(
                "space {id} is unavailable: {reason}"
            ))));
        }
        let guard = self.spaces.read().await;
        guard
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!(SpaceError::NotFound(id.to_string())))
    }

    /// List all Spaces
    pub async fn list(&self) -> Vec<SpaceInfo> {
        let guard = self.spaces.read().await;
        let mut v: Vec<_> = guard.values().cloned().collect();
        v.sort_by_key(|a| a.created_at);
        v
    }

    /// Delete a Space (removes from registry and deletes storage dir)
    pub async fn delete(&self, id: &str) -> Result<()> {
        let mut guard = self.spaces.write().await;
        if self.unavailable.read().await.contains_key(id) {
            return Err(anyhow!(SpaceError::Storage(format!(
                "space {id} is unavailable; refusing to delete its data"
            ))));
        }
        let info = guard
            .remove(id)
            .ok_or_else(|| anyhow!(SpaceError::NotFound(id.to_string())))?;
        // Close the database handle, then best-effort delete the directory
        self.stores.evict(id);
        let _ = tokio::fs::remove_dir_all(&info.storage_path).await;
        Ok(())
    }

    /// Check isolation: two spaces never share storage path
    pub async fn are_isolated(&self, a: &str, b: &str) -> bool {
        let guard = self.spaces.read().await;
        match (guard.get(a), guard.get(b)) {
            (Some(sa), Some(sb)) => sa.storage_path != sb.storage_path && sa.id != sb.id,
            _ => false,
        }
    }

    /// Storage path for a space. An id that fails validation (e.g. a
    /// traversal attempt) never maps outside `base_dir`.
    pub fn storage_path(&self, id: &str) -> PathBuf {
        if Self::validate_id(id).is_err() {
            return self.base_dir.join("_invalid_space_id");
        }
        self.base_dir.join(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_isolate() {
        let mgr = SpaceManager::new(std::env::temp_dir().join("xavier_spaces_test"));
        let a = mgr
            .create(
                "esp_a".into(),
                "A".into(),
                "desc".into(),
                "xv1_owner".into(),
                false,
            )
            .await
            .unwrap();
        let b = mgr
            .create(
                "esp_b".into(),
                "B".into(),
                "desc".into(),
                "xv1_owner".into(),
                true,
            )
            .await
            .unwrap();
        assert_ne!(a.storage_path, b.storage_path);
        assert!(mgr.are_isolated("esp_a", "esp_b").await);
        assert!(mgr.get("esp_a").await.is_ok());
        assert_eq!(mgr.list().await.len(), 2);
        mgr.delete("esp_a").await.unwrap();
        assert!(mgr.get("esp_a").await.is_err());
        let _ = mgr.delete("esp_b").await;
    }

    #[tokio::test]
    async fn reject_duplicate_and_invalid() {
        let mgr = SpaceManager::new(std::env::temp_dir().join("xavier_spaces_test2"));
        mgr.create("esp_x".into(), "X".into(), "".into(), "n1".into(), false)
            .await
            .unwrap();
        assert!(mgr
            .create("esp_x".into(), "X".into(), "".into(), "n1".into(), false)
            .await
            .is_err());
        assert!(mgr
            .create("bad/id".into(), "X".into(), "".into(), "n1".into(), false)
            .await
            .is_err());
        let _ = mgr.delete("esp_x").await;
    }

    #[test]
    fn namespace_format() {
        let ns = SpaceManager::namespace_for("esp_01H", "myapp", "inst1");
        assert_eq!(ns, "xavier://esp_01H/myapp/inst1");
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::espacio::channel::ChannelManager;
    use crate::espacio::invite::{InviteManager, SpaceRole};

    async fn mk(mgr: &SpaceManager, id: &str) -> SpaceInfo {
        mgr.create(
            id.into(),
            format!("name-{id}"),
            "d".into(),
            "xv1_owner".into(),
            true,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn space_and_member_survive_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let created = {
            let mgr = SpaceManager::open(tmp.path());
            let info = mk(&mgr, "esp_a").await;
            mgr.add_member("esp_a", "xv1_bob", SpaceRole::Reader)
                .await
                .unwrap();
            info
        };
        assert!(tmp.path().join("spaces/esp_a/space.json").is_file());
        assert!(tmp.path().join("spaces/esp_a/espacio.sqlite").is_file());

        let mgr = SpaceManager::open(tmp.path());
        let got = mgr.get("esp_a").await.unwrap();
        assert_eq!(got.name, "name-esp_a");
        assert_eq!(got.owner_node, "xv1_owner");
        assert!(got.is_public);
        assert_eq!(got.created_at, created.created_at);
        assert_eq!(got.namespace, created.namespace);
        assert_eq!(got.storage_path, tmp.path().join("spaces/esp_a"));
        let members = mgr.members("esp_a").await.unwrap();
        let roles: Vec<_> = members
            .iter()
            .map(|m| (m.node_id.as_str(), m.role))
            .collect();
        assert!(roles.contains(&("xv1_owner", SpaceRole::Admin)));
        assert!(roles.contains(&("xv1_bob", SpaceRole::Reader)));
        // duplicates still rejected after reload
        assert!(mgr
            .create("esp_a".into(), "x".into(), "".into(), "n".into(), false)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn invite_revoked_and_signature_survive_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let (revoked_id, signed_id, sig) = {
            let spaces = SpaceManager::open(tmp.path());
            mk(&spaces, "esp_a").await;
            let inv = InviteManager::with_stores(spaces.stores());
            let r = inv
                .create(
                    "esp_a".into(),
                    "adm".into(),
                    "bob".into(),
                    SpaceRole::Member,
                )
                .await
                .unwrap();
            inv.revoke(&r.id).await.unwrap();
            let s = inv
                .create(
                    "esp_a".into(),
                    "adm".into(),
                    "eve".into(),
                    SpaceRole::Reader,
                )
                .await
                .unwrap();
            use ed25519_dalek::Signer;
            let sk = ed25519_dalek::SigningKey::from_bytes(&[3u8; 32]);
            let sig =
                crate::crypto::hex_encode(sk.sign(s.canonical_payload().as_bytes()).to_bytes());
            inv.attach_signature(&s.id, sig.clone()).await.unwrap();
            (r.id, s.id, sig)
        };

        let inv = InviteManager::open(tmp.path());
        assert!(inv.get(&revoked_id).await.unwrap().revoked);
        assert!(inv.validate(&revoked_id).await.is_err());
        assert!(inv.revoke(&revoked_id).await.is_err(), "stays revoked");
        let s = inv.get(&signed_id).await.unwrap();
        assert_eq!(s.signature.as_deref(), Some(sig.as_str()));
        let sk = ed25519_dalek::SigningKey::from_bytes(&[3u8; 32]);
        assert!(inv
            .validate_trusted(&signed_id, &sk.verifying_key(), true)
            .await
            .is_ok());
        assert_eq!(inv.list_for_space("esp_a").await.len(), 2);
    }

    #[tokio::test]
    async fn messages_keep_order_and_seq_after_restart() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let ch = ChannelManager::open(tmp.path());
            for i in 0..5 {
                let m = ch
                    .post("esp_a".into(), format!("n{i}"), format!("msg{i}"))
                    .await;
                assert_eq!(m.seq, i);
            }
        }
        let ch = ChannelManager::open(tmp.path());
        let all = ch.list_all("esp_a").await;
        let texts: Vec<_> = all.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(texts, ["msg0", "msg1", "msg2", "msg3", "msg4"]);
        assert_eq!(
            all.iter().map(|m| m.seq).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );
        assert_eq!(ch.len("esp_a").await, 5);
        assert_eq!(ch.list_since("esp_a", 3).await.len(), 1);
        let next = ch.post("esp_a".into(), "n".into(), "after".into()).await;
        assert_eq!(next.seq, 5);
    }

    #[tokio::test]
    async fn corrupt_descriptor_is_unavailable_and_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mgr = SpaceManager::open(tmp.path());
            mk(&mgr, "esp_ok").await;
            mk(&mgr, "esp_bad").await;
        }
        let bad = tmp.path().join("spaces/esp_bad");
        std::fs::write(bad.join("space.json"), b"{ not json").unwrap();
        let db_before = std::fs::read(bad.join("espacio.sqlite")).unwrap();

        let mgr = SpaceManager::open(tmp.path());
        assert!(mgr.get("esp_ok").await.is_ok());
        assert!(mgr.get("esp_bad").await.is_err());
        assert_eq!(mgr.list().await.len(), 1);
        let un = mgr.unavailable().await;
        assert_eq!(un.len(), 1);
        assert_eq!(un[0].0, "esp_bad");
        assert!(mgr.delete("esp_bad").await.is_err());
        assert!(mgr
            .create("esp_bad".into(), "x".into(), "".into(), "n".into(), false)
            .await
            .is_err());
        assert!(mgr.members("esp_bad").await.is_err());
        assert_eq!(
            std::fs::read(bad.join("space.json")).unwrap(),
            b"{ not json"
        );
        assert_eq!(
            std::fs::read(bad.join("espacio.sqlite")).unwrap(),
            db_before
        );
    }

    #[tokio::test]
    async fn descriptor_id_mismatch_is_unavailable_and_dir_without_json_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mgr = SpaceManager::open(tmp.path());
            mk(&mgr, "esp_a").await;
        }
        let spaces = tmp.path().join("spaces");
        std::fs::create_dir_all(spaces.join("esp_orphan")).unwrap();
        std::fs::create_dir_all(spaces.join("not.valid")).unwrap();
        std::fs::create_dir_all(spaces.join("esp_copy")).unwrap();
        std::fs::copy(
            spaces.join("esp_a/space.json"),
            spaces.join("esp_copy/space.json"),
        )
        .unwrap();
        let mgr = SpaceManager::open(tmp.path());
        assert_eq!(mgr.list().await.len(), 1);
        assert!(mgr.get("esp_orphan").await.is_err());
        assert_eq!(mgr.unavailable().await[0].0, "esp_copy");
    }

    #[tokio::test]
    async fn traversal_ids_rejected_everywhere() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let mgr = SpaceManager::open(&root);
        for bad in ["../escape", "..", ".", "a/b", "", "a\0b", "a.b"] {
            assert!(mgr
                .create(bad.into(), "x".into(), "".into(), "n".into(), false)
                .await
                .is_err());
            assert!(mgr.add_member(bad, "n", SpaceRole::Member).await.is_err());
            assert!(mgr.storage_path(bad).starts_with(root.join("spaces")));
        }
        let ch = ChannelManager::open(&root);
        assert!(ch
            .try_post("../escape".into(), "a".into(), "x".into())
            .await
            .is_err());
        let _ = ch.post("../escape".into(), "a".into(), "x".into()).await;
        let inv = InviteManager::open(&root);
        assert!(inv
            .create(
                "../escape".into(),
                "a".into(),
                "b".into(),
                SpaceRole::Member
            )
            .await
            .is_err());
        assert!(!tmp.path().join("escape").exists());
        assert!(!root.join("escape").exists());
    }

    #[tokio::test]
    async fn two_spaces_sqlite_files_hold_only_their_rows() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let spaces = SpaceManager::open(tmp.path());
            mk(&spaces, "esp_a").await;
            mk(&spaces, "esp_b").await;
            let ch = ChannelManager::with_stores(spaces.stores());
            let inv = InviteManager::with_stores(spaces.stores());
            ch.post("esp_a".into(), "a".into(), "only-a".into()).await;
            ch.post("esp_b".into(), "b".into(), "only-b".into()).await;
            inv.create("esp_a".into(), "adm".into(), "t".into(), SpaceRole::Member)
                .await
                .unwrap();
            spaces
                .add_member("esp_b", "xv1_b_only", SpaceRole::Member)
                .await
                .unwrap();
        }
        let count = |space: &str, sql: &str| -> i64 {
            let c = rusqlite::Connection::open(
                tmp.path().join("spaces").join(space).join("espacio.sqlite"),
            )
            .unwrap();
            c.query_row(sql, [], |r| r.get(0)).unwrap()
        };
        assert_eq!(
            count(
                "esp_a",
                "SELECT COUNT(*) FROM messages WHERE content = 'only-b' OR space_id <> 'esp_a'"
            ),
            0
        );
        assert_eq!(
            count(
                "esp_b",
                "SELECT COUNT(*) FROM messages WHERE content = 'only-a' OR space_id <> 'esp_b'"
            ),
            0
        );
        assert_eq!(count("esp_a", "SELECT COUNT(*) FROM messages"), 1);
        assert_eq!(count("esp_b", "SELECT COUNT(*) FROM messages"), 1);
        assert_eq!(count("esp_a", "SELECT COUNT(*) FROM invites"), 1);
        assert_eq!(count("esp_b", "SELECT COUNT(*) FROM invites"), 0);
        assert_eq!(
            count(
                "esp_a",
                "SELECT COUNT(*) FROM members WHERE node_id = 'xv1_b_only'"
            ),
            0
        );
        assert_eq!(
            count(
                "esp_b",
                "SELECT COUNT(*) FROM members WHERE node_id = 'xv1_b_only'"
            ),
            1
        );
    }

    #[tokio::test]
    async fn delete_removes_directory_and_stays_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = SpaceManager::open(tmp.path());
        mk(&mgr, "esp_a").await;
        mgr.delete("esp_a").await.unwrap();
        assert!(!tmp.path().join("spaces/esp_a").exists());
        assert!(SpaceManager::open(tmp.path()).list().await.is_empty());
    }
}
