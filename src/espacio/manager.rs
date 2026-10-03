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
use super::keys::{NodeKek, RecoveryCode, UnlockMode};
use super::migrate::{self, AdoptableDb, LegacySpace, LegacyWorkspace};
use super::permissions::SpaceMembership;
use super::store::{self, SpaceStores};

/// Payload for creating a new Space
#[derive(Clone, Serialize, Deserialize)]
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
    /// `node_unlock` (default) or `password_required`.
    #[serde(default)]
    pub unlock_mode: Option<UnlockMode>,
    /// Required for `password_required`. Never serialized, logged or returned.
    #[serde(default, skip_serializing)]
    pub password: Option<String>,
    /// Encrypt records with the space key. Default true; root may pass false
    /// to get a plaintext-memory space (the choice is persisted in `space.json`).
    #[serde(default)]
    pub encrypt_records: Option<bool>,
}

impl std::fmt::Debug for CreateSpaceRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateSpaceRequest")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("owner_node", &self.owner_node)
            .field("is_public", &self.is_public)
            .field("unlock_mode", &self.unlock_mode)
            .field("encrypt_records", &self.encrypt_records)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Directory (under the spaces dir) that holds deleted spaces.
pub const TRASH_DIR: &str = ".trash";

/// Key options for a new space (only used when the manager has a key ring).
#[derive(Debug, Clone)]
pub struct KeyOptions {
    pub mode: UnlockMode,
    /// Required for `password_required`, optional for `node_unlock`.
    pub password: Option<String>,
    /// Encrypt message content with the space key (default true).
    pub encrypt_records: bool,
}

impl Default for KeyOptions {
    fn default() -> Self {
        Self {
            mode: UnlockMode::NodeUnlock,
            password: None,
            encrypt_records: true,
        }
    }
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
    /// Encryption decision, persisted outside the keystore so that losing
    /// `keystore.json` can never downgrade the space to plaintext. Absent in
    /// legacy descriptors (plaintext spaces).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    encryption: Option<EncryptionMeta>,
    /// WP-13o: descriptor of the pre-espacio workspace, kept by reference.
    /// Such a descriptor is never registered as a space.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    legacy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_store: Option<LegacyWorkspace>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct EncryptionMeta {
    enabled: bool,
    keystore_version: u32,
}

const DESCRIPTOR_VERSION: u32 = 1;
const KEYSTORE_VERSION: u32 = 1;

impl SpaceDescriptor {
    fn from_info(info: &SpaceInfo, encryption: Option<EncryptionMeta>) -> Self {
        Self {
            version: DESCRIPTOR_VERSION,
            id: info.id.clone(),
            name: info.name.clone(),
            description: info.description.clone(),
            owner_node: info.owner_node.clone(),
            is_public: info.is_public,
            created_at: info.created_at,
            namespace: info.namespace.clone(),
            encryption,
            legacy: false,
            legacy_store: None,
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
    /// The legacy default workspace, recorded by reference (WP-13o). Not a
    /// registered space: root keeps serving it exactly as before.
    legacy: Option<LegacySpace>,
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
            legacy: None,
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
    ///
    /// When the stores carry a key ring, every `node_unlock` space is
    /// unlocked here; `password_required` spaces stay locked.
    pub fn open_with_stores(stores: Arc<SpaceStores>) -> Self {
        Self::open_with_stores_and_legacy(stores, None)
    }

    /// Like [`SpaceManager::open_with_stores`], additionally recording the
    /// legacy workspace (WP-13o): when `legacy` is given and no descriptor
    /// exists for its id, `{spaces}/{id}/space.json` is written with
    /// `legacy: true`. Legacy data is referenced, never touched.
    pub fn open_with_stores_and_legacy(
        stores: Arc<SpaceStores>,
        legacy: Option<LegacyWorkspace>,
    ) -> Self {
        if let Some(ring) = stores.key_ring() {
            ring.auto_unlock_all();
        }
        let base_dir = stores
            .spaces_dir()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        if let (Some(legacy), true) = (&legacy, stores.is_persistent()) {
            match migrate::ensure_default_space(&base_dir, legacy) {
                Ok(true) => tracing::info!(
                    "espacio: recorded legacy workspace {:?} by reference (no data moved)",
                    legacy.id
                ),
                Ok(false) => {}
                Err(e) => tracing::warn!("espacio: legacy descriptor not written: {e}"),
            }
        }
        let (spaces, unavailable, encrypted, legacy_found) = Self::scan(&base_dir);
        if let Some(ring) = stores.key_ring() {
            for id in spaces.keys() {
                stores.prime_encryption(id);
            }
            for id in encrypted {
                ring.require_encryption(&id);
                if ring.is_locked(&id) {
                    tracing::error!("espacio: encrypted space {id} is locked or has no keystore");
                }
            }
        }
        Self {
            spaces: Arc::new(RwLock::new(spaces)),
            unavailable: Arc::new(RwLock::new(unavailable)),
            base_dir,
            stores,
            legacy: legacy_found,
        }
    }

    /// Open a persistent manager with per-space keys (WP-13j). `node`
    /// supplies the node-bound KEK; production passes
    /// [`super::keys::MasterNodeKek`], tests inject a static one. New spaces
    /// get a keystore and encrypted messages; `node_unlock` spaces unlock at
    /// open.
    pub fn open_with_keys(root: impl AsRef<Path>, node: Arc<dyn NodeKek>) -> Self {
        Self::open_with_keys_and_legacy(root, node, Some(LegacyWorkspace::from_settings()))
    }

    /// [`SpaceManager::open_with_keys`] with an explicit legacy workspace
    /// reference (`None` skips the legacy record). Used by tests.
    pub fn open_with_keys_and_legacy(
        root: impl AsRef<Path>,
        node: Arc<dyn NodeKek>,
        legacy: Option<LegacyWorkspace>,
    ) -> Self {
        Self::open_with_stores_and_legacy(SpaceStores::with_key_ring(root.as_ref(), node), legacy)
    }

    /// The legacy default workspace record, when this node has one.
    pub fn legacy(&self) -> Option<&LegacySpace> {
        self.legacy.as_ref()
    }

    /// Legacy `*.sqlite` files that could be adopted as spaces later (listing
    /// only, nothing is moved). Blocking filesystem read.
    pub fn adoptable(&self) -> Vec<AdoptableDb> {
        let Some(db_dir) = self.legacy.as_ref().and_then(|l| l.store.db_dir.as_ref()) else {
            return Vec::new();
        };
        let mut taken: Vec<String> = self
            .spaces
            .try_read()
            .map(|g| g.keys().cloned().collect())
            .unwrap_or_default();
        if let Ok(g) = self.unavailable.try_read() {
            taken.extend(g.keys().cloned());
        }
        migrate::list_adoptable(db_dir, &taken)
    }

    /// Per-space key ring, when enabled (unlock / change password).
    pub fn key_ring(&self) -> Option<Arc<super::keys::KeyRing>> {
        self.stores.key_ring()
    }

    /// Shared per-space store registry (hand it to `ChannelManager` /
    /// `InviteManager` via their `with_stores` constructors).
    pub fn stores(&self) -> Arc<SpaceStores> {
        self.stores.clone()
    }

    /// Returns the registered spaces, the unavailable ones and the ids whose
    /// descriptor says they are encrypted.
    #[allow(clippy::type_complexity)]
    fn scan(
        base_dir: &Path,
    ) -> (
        HashMap<String, SpaceInfo>,
        HashMap<String, String>,
        Vec<String>,
        Option<LegacySpace>,
    ) {
        let mut legacy_found = None;
        let mut spaces = HashMap::new();
        let mut unavailable = HashMap::new();
        let mut encrypted = Vec::new();
        let Ok(rd) = std::fs::read_dir(base_dir) else {
            return (spaces, unavailable, encrypted, legacy_found);
        };
        for entry in rd.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                // `.trash` and other housekeeping directories.
                continue;
            }
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
                Ok(d) if d.legacy => {
                    // Recorded by reference only: not a registered space.
                    if let Some(store) = d.legacy_store {
                        legacy_found = Some(LegacySpace {
                            id: d.id,
                            name: d.name,
                            legacy: true,
                            created_at: d.created_at,
                            store,
                        });
                    }
                }
                Ok(d) => {
                    if d.encryption.is_some_and(|e| e.enabled) {
                        encrypted.push(name.clone());
                    }
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
        (spaces, unavailable, encrypted, legacy_found)
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

    /// Create a new isolated Space. With a key ring the space gets the
    /// default keys (`node_unlock`, encrypted records) and the recovery code
    /// is discarded; use [`SpaceManager::create_with_keys`] to receive it.
    pub async fn create(
        &self,
        id: String,
        name: String,
        description: String,
        owner_node: String,
        is_public: bool,
    ) -> Result<SpaceInfo> {
        self.create_with_keys(
            id,
            name,
            description,
            owner_node,
            is_public,
            KeyOptions::default(),
        )
        .await
        .map(|(info, _)| info)
    }

    /// Create a Space and return its one-time recovery code (`None` when the
    /// manager has no key ring). The code is not stored anywhere.
    pub async fn create_with_keys(
        &self,
        id: String,
        name: String,
        description: String,
        owner_node: String,
        is_public: bool,
        opts: KeyOptions,
    ) -> Result<(SpaceInfo, Option<RecoveryCode>)> {
        Self::validate_id(&id)?;
        if self.legacy.as_ref().is_some_and(|l| l.id == id) {
            // The id of the legacy default workspace stays reserved.
            return Err(anyhow!(SpaceError::AlreadyExists(id)));
        }
        let mut guard = self.spaces.write().await;
        if guard.contains_key(&id) || self.unavailable.read().await.contains_key(&id) {
            return Err(anyhow!(SpaceError::AlreadyExists(id)));
        }
        let storage_path = self.base_dir.join(&id);
        tokio::fs::create_dir_all(&storage_path)
            .await
            .map_err(|e| anyhow!(SpaceError::Storage(e.to_string())))?;
        // Keys first: nothing is ever written to the database unencrypted.
        let mut recovery = None;
        let mut encryption = None;
        if let Some(ring) = self.stores.key_ring() {
            // A keystore without a descriptor is the leftover of a crashed
            // create: move it aside (never delete) before writing a new one.
            ring.quarantine_orphan_keystore(&id)?;
            let (_, code) = ring.create_keys_with(
                &id,
                opts.mode,
                opts.password.as_deref(),
                opts.encrypt_records,
            )?;
            recovery = Some(code);
            encryption = Some(EncryptionMeta {
                enabled: opts.encrypt_records,
                keystore_version: KEYSTORE_VERSION,
            });
        }

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
            let store = self.stores.get(&id)?;
            if let Some(e) = encryption {
                store.mark_encryption(e.enabled)?;
            }
            store.add_member(&info.owner_node, SpaceRole::Admin)?;
            if self.stores.is_persistent() {
                let bytes =
                    serde_json::to_vec_pretty(&SpaceDescriptor::from_info(&info, encryption))?;
                store::write_atomic(&info.storage_path.join(store::DESCRIPTOR_FILE), &bytes)?;
            }
            Ok(())
        };
        if let Err(e) = persist() {
            self.stores.evict(&id);
            if let Some(ring) = self.stores.key_ring() {
                ring.discard_keys(&id);
            }
            return Err(anyhow!(SpaceError::Storage(e.to_string())));
        }
        guard.insert(id, info.clone());
        Ok((info, recovery))
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

    /// Delete a Space: removes it from the registry and MOVES its directory
    /// (data and keystore) to `{spaces}/.trash/{id}-{unix_ts}` with an atomic
    /// rename. Nothing is ever removed from disk. Tokens stop working at once
    /// because the space is no longer registered.
    pub async fn delete(&self, id: &str) -> Result<()> {
        Self::validate_id(id)?;
        let mut guard = self.spaces.write().await;
        if self.unavailable.read().await.contains_key(id) {
            return Err(anyhow!(SpaceError::Storage(format!(
                "space {id} is unavailable; refusing to delete its data"
            ))));
        }
        let info = guard
            .remove(id)
            .ok_or_else(|| anyhow!(SpaceError::NotFound(id.to_string())))?;
        // Close the database handle before moving the directory.
        self.stores.evict(id);
        if let Some(ring) = self.stores.key_ring() {
            ring.forget(id);
        }
        if info.storage_path.exists() {
            let trash = self.base_dir.join(TRASH_DIR);
            let mut dest = trash.join(format!("{id}-{}", Utc::now().timestamp()));
            if dest.exists() {
                let nanos = Utc::now().timestamp_nanos_opt().unwrap_or(0);
                dest = trash.join(format!("{id}-{}-{nanos}", Utc::now().timestamp()));
            }
            let moved = match tokio::fs::create_dir_all(&trash).await {
                Ok(()) => tokio::fs::rename(&info.storage_path, &dest).await,
                Err(e) => Err(e),
            };
            if let Err(e) = moved {
                // Put it back: a failed move must not orphan a live space.
                guard.insert(id.to_string(), info);
                return Err(anyhow!(SpaceError::Storage(format!(
                    "could not move space {id} to trash: {e}"
                ))));
            }
        }
        // No inheritance by id reuse: links of other spaces that name the
        // deleted space as grantee are revoked now.
        let others: Vec<String> = guard.keys().cloned().collect();
        let revoked = super::link::revoke_links_naming(&self.stores, &others, id);
        if revoked > 0 {
            tracing::info!("espacio: revoked {revoked} inbound link(s) of deleted space {id}");
        }
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

    // ---- WP-13j: keyed spaces ----

    use crate::espacio::keys::{KeysError, StaticNodeKek};

    const SECRET_TEXT: &str = "top-secret-plaintext-marker-42";
    const PW: &str = "correct horse battery";

    fn node() -> Arc<dyn NodeKek> {
        Arc::new(StaticNodeKek::new([9u8; 32]))
    }

    fn raw_messages(root: &Path, id: &str) -> Vec<String> {
        let conn = rusqlite::Connection::open(root.join("spaces").join(id).join("espacio.sqlite"))
            .unwrap();
        let mut stmt = conn
            .prepare("SELECT content FROM messages ORDER BY seq")
            .unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows
    }

    fn is_locked_err(e: &anyhow::Error) -> bool {
        matches!(e.downcast_ref::<KeysError>(), Some(KeysError::Locked(_)))
    }

    async fn keyed(root: &Path, id: &str, opts: KeyOptions) -> Option<RecoveryCode> {
        let mgr = SpaceManager::open_with_keys(root, node());
        let (_, code) = mgr
            .create_with_keys(
                id.into(),
                "n".into(),
                "".into(),
                "xv1_o".into(),
                false,
                opts,
            )
            .await
            .unwrap();
        let ch = ChannelManager::with_stores(mgr.stores());
        ch.try_post(id.into(), "xv1_o".into(), SECRET_TEXT.into())
            .await
            .unwrap();
        code
    }

    #[tokio::test]
    async fn encrypted_messages_not_plaintext_on_disk_and_node_unlock_reopens() {
        let tmp = tempfile::tempdir().unwrap();
        let code = keyed(tmp.path(), "esp_a", KeyOptions::default()).await;
        assert!(code.is_some());

        let rows = raw_messages(tmp.path(), "esp_a");
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].contains(SECRET_TEXT));
        assert!(rows[0].starts_with("xr1:"));
        // no secrets in the descriptor
        let desc = std::fs::read_to_string(tmp.path().join("spaces/esp_a/space.json")).unwrap();
        assert!(!desc.contains("xr1:") && !desc.contains("wrappers"));

        // node_unlock opens automatically after reopen
        let mgr = SpaceManager::open_with_keys(tmp.path(), node());
        assert!(!mgr.key_ring().unwrap().is_locked("esp_a"));
        let ch = ChannelManager::with_stores(mgr.stores());
        let all = ch.list_all("esp_a").await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].content, SECRET_TEXT);
    }

    #[tokio::test]
    async fn password_required_space_locked_after_reopen_until_unlocked() {
        let tmp = tempfile::tempdir().unwrap();
        let code = keyed(
            tmp.path(),
            "esp_p",
            KeyOptions {
                mode: UnlockMode::PasswordRequired,
                password: Some(PW.into()),
                encrypt_records: true,
            },
        )
        .await
        .unwrap();

        let mgr = SpaceManager::open_with_keys(tmp.path(), node());
        let ring = mgr.key_ring().unwrap();
        assert!(ring.is_locked("esp_p"));
        let ch = ChannelManager::with_stores(mgr.stores());
        let e = ch
            .try_post("esp_p".into(), "x".into(), "must not store".into())
            .await
            .unwrap_err();
        assert!(is_locked_err(&e));
        assert!(mgr.stores().get("esp_p").is_err());
        assert!(mgr
            .add_member("esp_p", "xv1_m", SpaceRole::Member)
            .await
            .is_err());
        // locked reads yield nothing, never plaintext
        assert!(ch.list_all("esp_p").await.is_empty());
        assert_eq!(raw_messages(tmp.path(), "esp_p").len(), 1);

        assert!(ring
            .unlock_with_password("esp_p", "wrong password!!")
            .is_err());
        assert!(ring.is_locked("esp_p"));

        // recovery code opens it and resets the password
        let h = ring.unlock_with_recovery("esp_p", code.expose()).unwrap();
        ring.change_password("esp_p", &h, "fresh password 123")
            .unwrap();
        assert_eq!(ch.list_all("esp_p").await[0].content, SECRET_TEXT);

        let mgr2 = SpaceManager::open_with_keys(tmp.path(), node());
        let ring2 = mgr2.key_ring().unwrap();
        assert!(ring2.is_locked("esp_p"));
        assert!(ring2.unlock_with_password("esp_p", PW).is_err());
        ring2
            .unlock_with_password("esp_p", "fresh password 123")
            .unwrap();
        let ch2 = ChannelManager::with_stores(mgr2.stores());
        assert_eq!(ch2.list_all("esp_p").await[0].content, SECRET_TEXT);
    }

    #[tokio::test]
    async fn encrypt_records_false_stores_plain_and_wrong_node_key_stays_locked() {
        let tmp = tempfile::tempdir().unwrap();
        keyed(
            tmp.path(),
            "esp_plain",
            KeyOptions {
                encrypt_records: false,
                ..KeyOptions::default()
            },
        )
        .await;
        assert_eq!(raw_messages(tmp.path(), "esp_plain")[0], SECRET_TEXT);

        keyed(tmp.path(), "esp_enc", KeyOptions::default()).await;
        let other: Arc<dyn NodeKek> = Arc::new(StaticNodeKek::new([1u8; 32]));
        let mgr = SpaceManager::open_with_keys(tmp.path(), other);
        assert!(mgr.key_ring().unwrap().is_locked("esp_enc"));
        assert!(mgr.stores().get("esp_enc").is_err());
    }

    #[tokio::test]
    async fn merged_messages_are_encrypted_too_and_delete_removes_keystore() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = SpaceManager::open_with_keys(tmp.path(), node());
        mgr.create("esp_m".into(), "n".into(), "".into(), "xv1_o".into(), false)
            .await
            .unwrap();
        let ch = ChannelManager::with_stores(mgr.stores());
        ch.merge(
            "esp_m".into(),
            vec![crate::espacio::channel::ChannelMessage {
                seq: 1,
                space_id: "esp_m".into(),
                author: "a".into(),
                content: SECRET_TEXT.into(),
                created_at: Utc::now(),
            }],
        )
        .await;
        assert!(!raw_messages(tmp.path(), "esp_m")[0].contains(SECRET_TEXT));
        assert_eq!(ch.list_all("esp_m").await[0].content, SECRET_TEXT);
        mgr.delete("esp_m").await.unwrap();
        assert!(!tmp.path().join("spaces/esp_m").exists());
    }

    #[tokio::test]
    async fn manager_without_ring_keeps_legacy_plaintext() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = SpaceManager::open(tmp.path());
        mgr.create("esp_l".into(), "n".into(), "".into(), "xv1_o".into(), false)
            .await
            .unwrap();
        ChannelManager::with_stores(mgr.stores())
            .try_post("esp_l".into(), "a".into(), "hi".into())
            .await
            .unwrap();
        assert_eq!(raw_messages(tmp.path(), "esp_l")[0], "hi");
        assert!(!tmp.path().join("spaces/esp_l/keystore.json").exists());
    }

    // ---- WP-13j fix: fail closed ----

    fn is_missing_err(e: &anyhow::Error) -> bool {
        matches!(
            e.downcast_ref::<KeysError>(),
            Some(KeysError::KeystoreMissing(_))
        )
    }

    fn sql(root: &Path, id: &str, stmt: &str) {
        let conn = rusqlite::Connection::open(root.join("spaces").join(id).join("espacio.sqlite"))
            .unwrap();
        conn.execute_batch(stmt).unwrap();
    }

    fn disk_has_plaintext(root: &Path, needle: &str) -> bool {
        fn walk(p: &Path, needle: &[u8]) -> bool {
            let Ok(rd) = std::fs::read_dir(p) else {
                return false;
            };
            rd.flatten().any(|e| {
                let path = e.path();
                if path.is_dir() {
                    walk(&path, needle)
                } else {
                    std::fs::read(&path)
                        .map(|b| b.windows(needle.len()).any(|w| w == needle))
                        .unwrap_or(false)
                }
            })
        }
        walk(root, needle.as_bytes())
    }

    async fn assert_closed(root: &Path, id: &str) {
        let mgr = SpaceManager::open_with_keys(root, node());
        let ring = mgr.key_ring().unwrap();
        assert!(ring.is_locked(id), "space must be locked");
        let ch = ChannelManager::with_stores(mgr.stores());
        let e = ch
            .try_post(id.into(), "x".into(), "NEW-WRITE-MARKER".into())
            .await
            .unwrap_err();
        assert!(
            is_missing_err(&e),
            "write must fail with KeystoreMissing: {e}"
        );
        assert!(
            ch.list_all(id).await.is_empty(),
            "reads must not serve rows"
        );
        assert!(is_missing_err(&mgr.stores().get(id).unwrap_err()));
        assert!(mgr
            .add_member(id, "xv1_m", SpaceRole::Member)
            .await
            .is_err());
        // a plaintext-only registry over the same root refuses too
        let plain = ChannelManager::open(root);
        assert!(plain
            .try_post(id.into(), "x".into(), "NEW-WRITE-MARKER".into())
            .await
            .is_err());
        assert!(plain.list_all(id).await.is_empty());
        assert!(!root.join("spaces").join(id).join("keystore.json").exists());
        assert!(!disk_has_plaintext(root, "NEW-WRITE-MARKER"));
        assert!(!disk_has_plaintext(root, SECRET_TEXT));
    }

    #[tokio::test]
    async fn deleting_keystore_of_encrypted_space_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        keyed(tmp.path(), "esp_a", KeyOptions::default()).await;
        let desc = std::fs::read_to_string(tmp.path().join("spaces/esp_a/space.json")).unwrap();
        assert!(desc.contains("\"encryption\"") && desc.contains("\"enabled\": true"));
        std::fs::remove_file(tmp.path().join("spaces/esp_a/keystore.json")).unwrap();
        assert_closed(tmp.path(), "esp_a").await;
        assert_eq!(raw_messages(tmp.path(), "esp_a").len(), 1);
    }

    #[tokio::test]
    async fn encryption_evidence_survives_removed_descriptor_flag() {
        let tmp = tempfile::tempdir().unwrap();
        keyed(tmp.path(), "esp_a", KeyOptions::default()).await;
        let spath = tmp.path().join("spaces/esp_a/space.json");
        let mut v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&spath).unwrap()).unwrap();
        v.as_object_mut().unwrap().remove("encryption");
        std::fs::write(&spath, serde_json::to_vec(&v).unwrap()).unwrap();
        std::fs::remove_file(tmp.path().join("spaces/esp_a/keystore.json")).unwrap();
        // meta row + xr1 rows still say encrypted
        assert_closed(tmp.path(), "esp_a").await;
        // meta row gone too: the xr1 rows alone are enough
        sql(tmp.path(), "esp_a", "DELETE FROM meta");
        assert_closed(tmp.path(), "esp_a").await;
    }

    #[tokio::test]
    async fn descriptor_flag_alone_keeps_empty_space_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = SpaceManager::open_with_keys(tmp.path(), node());
        mgr.create("esp_e".into(), "n".into(), "".into(), "xv1_o".into(), false)
            .await
            .unwrap();
        drop(mgr);
        sql(tmp.path(), "esp_e", "DELETE FROM meta");
        std::fs::remove_file(tmp.path().join("spaces/esp_e/keystore.json")).unwrap();
        assert_closed(tmp.path(), "esp_e").await;
    }

    #[tokio::test]
    async fn swapped_message_rows_fail_authentication() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mgr = SpaceManager::open_with_keys(tmp.path(), node());
            mgr.create("esp_s".into(), "n".into(), "".into(), "xv1_o".into(), false)
                .await
                .unwrap();
            let ch = ChannelManager::with_stores(mgr.stores());
            for t in ["first", "second"] {
                ch.try_post("esp_s".into(), "a".into(), t.into())
                    .await
                    .unwrap();
            }
        }
        let rows = raw_messages(tmp.path(), "esp_s");
        assert_eq!(rows.len(), 2);
        sql(
            tmp.path(),
            "esp_s",
            &format!(
                "UPDATE messages SET content = '{}' WHERE seq = 1;
                 UPDATE messages SET content = '{}' WHERE seq = 0;",
                rows[0], rows[1]
            ),
        );
        let mgr = SpaceManager::open_with_keys(tmp.path(), node());
        let store = mgr.stores().get("esp_s").unwrap();
        assert!(
            store.messages_since(None).is_err(),
            "swapped rows must not open"
        );
    }

    #[tokio::test]
    async fn legacy_plaintext_space_still_works_with_key_ring() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mgr = SpaceManager::open(tmp.path());
            mgr.create("esp_l".into(), "n".into(), "".into(), "xv1_o".into(), false)
                .await
                .unwrap();
            ChannelManager::with_stores(mgr.stores())
                .try_post("esp_l".into(), "a".into(), "legacy hi".into())
                .await
                .unwrap();
        }
        let desc = std::fs::read_to_string(tmp.path().join("spaces/esp_l/space.json")).unwrap();
        assert!(!desc.contains("encryption"));
        let mgr = SpaceManager::open_with_keys(tmp.path(), node());
        let ring = mgr.key_ring().unwrap();
        assert!(!ring.is_locked("esp_l"));
        let ch = ChannelManager::with_stores(mgr.stores());
        assert_eq!(ch.list_all("esp_l").await[0].content, "legacy hi");
        ch.try_post("esp_l".into(), "a".into(), "more".into())
            .await
            .unwrap();
        assert_eq!(raw_messages(tmp.path(), "esp_l"), ["legacy hi", "more"]);
    }

    #[tokio::test]
    async fn orphan_keystore_is_moved_not_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("spaces/esp_o");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("keystore.json"), b"orphan-bytes").unwrap();
        let mgr = SpaceManager::open_with_keys(tmp.path(), node());
        mgr.create("esp_o".into(), "n".into(), "".into(), "xv1_o".into(), false)
            .await
            .unwrap();
        let orphans: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("keystore.json.orphan-")
            })
            .collect();
        assert_eq!(orphans.len(), 1);
        assert_eq!(std::fs::read(orphans[0].path()).unwrap(), b"orphan-bytes");
        assert_ne!(
            std::fs::read(dir.join("keystore.json")).unwrap(),
            b"orphan-bytes"
        );

        // with a descriptor present the keystore is NOT an orphan
        let ring = mgr.key_ring().unwrap();
        assert!(ring.quarantine_orphan_keystore("esp_o").unwrap().is_none());
        assert!(dir.join("keystore.json").exists());
    }
}
