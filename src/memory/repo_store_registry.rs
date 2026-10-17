//! Per-repository memory store registry (XAV-REPO T7).
//!
//! Opens and caches one [`VecSqliteMemoryStore`] per repository that has opted
//! in to its own physical store, on top of the process-wide
//! [`ConnectionManager`](crate::codebase::connection_manager::ConnectionManager).
//! The registry is a thin domain layer: it decides *whether* and *where* a repo
//! gets a store, and leaves pool sizing and eviction to the connection manager.
//!
//! # Checks, in the order they run
//!
//! 1. **Canonical root.** The root must exist and is canonicalised, so two
//!    spellings of one checkout (`a/./b`, `a/c/..`, a symlink) share one store.
//!    A root that does not exist is refused without creating anything.
//! 2. **Allowlist.** The canonical root must be inside one of the entries of
//!    `XAVIER_REPO_STORE_ROOTS` ([`RepoStoreRegistry::from_env`]). An empty list
//!    turns the feature off: every root is refused.
//! 3. **Opt-in.** `<root>/.xavier/config.toml` must declare
//!    `memory_store = "repo"`. A missing or malformed file is a refusal.
//! 4. **Open once.** Concurrent callers for the same store share one open
//!    (single-flight). The migrations run once per store per process.
//!
//! Nothing is created on disk until checks 1–3 pass. Refusals are typed
//! ([`RepoStoreError`]) and counted in [`RepoStoreStats`].
//!
//! # What is NOT done here
//!
//! - Routing reads and writes per repo (XAV-REPO T10). Nothing calls this yet.
//! - Federated search (T11), the admin endpoint and health section (T13, T14).
//! - Pool-level fixes such as non-blocking eviction (T5).
//!
//! # Record key (T8)
//!
//! Every repo store is sealed with its own record key, derived from the node
//! master key over the repo's project id
//! ([`derive_repo_record_key`]). The key is derived **before** the store file
//! is touched: a repo whose key cannot be derived gets no file and no store,
//! so rows can never fall back to the node record key or to plaintext.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use thiserror::Error;
use tokio::sync::OnceCell;

use crate::codebase::connection_manager::PoolProfile;
use crate::codebase::repo_config::{MemoryStoreMode, RepoConfig};
use crate::codebase::repo_identity::derive_repo_identity;
use crate::codebase::repo_package_crypto::derive_repo_record_key;
use crate::keystore::MasterKeyManager;
use crate::memory::connection_provider::{ConnectionProvider, GlobalConnectionProvider};
use crate::memory::repo_memory_store_path::memory_store_path_for_root;
use crate::memory::sqlite_vec_store::config::VecSqliteStoreConfig;
use crate::memory::sqlite_vec_store::VecSqliteMemoryStore;
use crate::settings::XavierSettings;

/// Environment variable listing the roots under which per-repo stores may live.
/// Entries use the platform `PATH` separator (`:` on Linux).
pub const REPO_STORE_ROOTS_ENV: &str = "XAVIER_REPO_STORE_ROOTS";

/// Supplies the record key of a repo store (XAV-REPO T8).
pub trait RepoRecordKeySource: Send + Sync {
    /// Record key for the repo whose project id is `project_id`.
    fn record_key(&self, project_id: &str) -> anyhow::Result<[u8; 32]>;
}

/// Production source: HKDF over the node master key, read from the keyring or
/// the fallback file without creating one. A node without a master key has no
/// repo stores.
pub struct MasterRecordKeySource;

impl RepoRecordKeySource for MasterRecordKeySource {
    fn record_key(&self, project_id: &str) -> anyhow::Result<[u8; 32]> {
        let master = MasterKeyManager::load_existing()?
            .ok_or_else(|| anyhow::anyhow!("the node master key is not initialised"))?;
        derive_repo_record_key(&master, project_id)
    }
}

/// Maximum entries in the root → store resolution cache.
const RESOLVE_CACHE_CAPACITY: usize = 256;

/// How long a successful resolution is trusted before `config.toml` is read again.
const RESOLVE_CACHE_TTL: Duration = Duration::from_secs(60);

/// Why a repo did not get a per-repo store.
#[derive(Debug, Error)]
pub enum RepoStoreError {
    /// The feature is off (empty allowlist) or the root is not under an allowed root.
    #[error("repo {} is outside {REPO_STORE_ROOTS_ENV}", .root.display())]
    OutsideAllowlist { root: PathBuf },
    /// The repo did not declare `memory_store = "repo"`, or its config is unreadable.
    #[error("repo {} has not opted in to a per-repo store: {reason}", .root.display())]
    NotOptedIn { root: PathBuf, reason: String },
    /// The root does not exist or cannot be canonicalised.
    #[error("repo root {} is not accessible: {reason}", .root.display())]
    RootUnavailable { root: PathBuf, reason: String },
    /// The record key for the repo could not be derived, so no store was opened.
    #[error("repo {} has no record key: {reason}", .root.display())]
    RecordKeyUnavailable { root: PathBuf, reason: String },
    /// The store file exists but SQLite does not accept it as a database.
    #[error("repo memory store {} is corrupt: {reason}", .path.display())]
    StoreCorrupt { path: PathBuf, reason: String },
    /// Any other failure while opening the store.
    #[error("failed to open repo memory store {}: {reason}", .path.display())]
    Open { path: PathBuf, reason: String },
}

/// Counters for the registry. Serialisable so the admin view can expose them (T13).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RepoStoreStats {
    /// Stores currently open in the registry.
    pub open_stores: usize,
    /// Successful store opens. Each runs the migrations once.
    pub opens: u64,
    /// Failed open attempts.
    pub open_failures: u64,
    /// Resolutions answered from the resolution cache.
    pub resolve_hits: u64,
    /// Resolutions that read the allowlist and `config.toml` again.
    pub resolve_misses: u64,
    /// Refusals because the root is outside the allowlist.
    pub rejected_outside_allowlist: u64,
    /// Refusals because the repo has not opted in.
    pub rejected_not_opted_in: u64,
}

#[derive(Default)]
struct Counters {
    opens: AtomicU64,
    open_failures: AtomicU64,
    resolve_hits: AtomicU64,
    resolve_misses: AtomicU64,
    rejected_outside_allowlist: AtomicU64,
    rejected_not_opted_in: AtomicU64,
}

/// A repo that passed checks 1–3: its canonical root and the file its store uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRepoStore {
    /// Canonical repository (or product) root.
    pub root: PathBuf,
    /// `<root>/.xavier/memory.sqlite3`.
    pub store_path: PathBuf,
}

struct CachedResolution {
    resolved: ResolvedRepoStore,
    inserted: Instant,
    last_used: u64,
}

/// Opt-in, allowlisted, single-flight cache of per-repo memory stores.
pub struct RepoStoreRegistry {
    allowed_roots: Vec<PathBuf>,
    provider: Arc<dyn ConnectionProvider>,
    keys: Arc<dyn RepoRecordKeySource>,
    /// Raw root as given by the caller → resolution. Bounded and TTL-limited.
    resolved: Mutex<ResolveCache>,
    /// Canonical store path → single-flight cell holding the open store.
    stores: Mutex<HashMap<PathBuf, Arc<OnceCell<Arc<VecSqliteMemoryStore>>>>>,
    counters: Counters,
}

#[derive(Default)]
struct ResolveCache {
    entries: HashMap<PathBuf, CachedResolution>,
    tick: u64,
}

impl ResolveCache {
    fn get(&mut self, raw_root: &Path) -> Option<ResolvedRepoStore> {
        let entry = self.entries.get_mut(raw_root)?;
        if entry.inserted.elapsed() > RESOLVE_CACHE_TTL {
            self.entries.remove(raw_root);
            return None;
        }
        self.tick += 1;
        entry.last_used = self.tick;
        Some(entry.resolved.clone())
    }

    fn insert(&mut self, raw_root: &Path, resolved: ResolvedRepoStore) {
        if !self.entries.contains_key(raw_root) && self.entries.len() >= RESOLVE_CACHE_CAPACITY {
            self.entries
                .retain(|_, entry| entry.inserted.elapsed() <= RESOLVE_CACHE_TTL);
            if self.entries.len() >= RESOLVE_CACHE_CAPACITY {
                let least_recent = self
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(key, _)| key.clone());
                if let Some(key) = least_recent {
                    self.entries.remove(&key);
                }
            }
        }
        self.tick += 1;
        self.entries.insert(
            raw_root.to_path_buf(),
            CachedResolution {
                resolved,
                inserted: Instant::now(),
                last_used: self.tick,
            },
        );
    }
}

impl RepoStoreRegistry {
    /// Registry with an explicit allowlist, connection provider and record key source.
    ///
    /// Allowlist entries are canonicalised once here. Entries that do not exist
    /// are ignored with a warning, so a typo cannot widen the allowlist.
    pub fn new(
        allowed_roots: &[PathBuf],
        provider: Arc<dyn ConnectionProvider>,
        keys: Arc<dyn RepoRecordKeySource>,
    ) -> Self {
        let allowed_roots = allowed_roots
            .iter()
            .filter_map(|root| match root.canonicalize() {
                Ok(canonical) => Some(canonical),
                Err(err) => {
                    tracing::warn!(
                        root = %root.display(),
                        error = %err,
                        "ignoring unusable {REPO_STORE_ROOTS_ENV} entry"
                    );
                    None
                }
            })
            .collect();
        Self {
            allowed_roots,
            provider,
            keys,
            resolved: Mutex::new(ResolveCache::default()),
            stores: Mutex::new(HashMap::new()),
            counters: Counters::default(),
        }
    }

    /// Allowlist from `XAVIER_REPO_STORE_ROOTS`, connections from the process-wide
    /// manager, record keys from the node master key.
    pub fn from_env() -> Self {
        let raw = std::env::var_os(REPO_STORE_ROOTS_ENV).unwrap_or_default();
        let roots: Vec<PathBuf> = std::env::split_paths(&raw)
            .filter(|path| !path.as_os_str().is_empty())
            .collect();
        Self::new(
            &roots,
            Arc::new(GlobalConnectionProvider::new()),
            Arc::new(MasterRecordKeySource),
        )
    }

    /// Open (or reuse) the per-repo store for `repo_root`.
    ///
    /// Concurrent calls for one store wait for a single open. A failed open is
    /// not cached, so the next call tries again.
    pub async fn open(
        &self,
        repo_root: &Path,
    ) -> Result<Arc<VecSqliteMemoryStore>, RepoStoreError> {
        let resolved = self.resolve(repo_root)?;
        let cell = {
            let mut stores = self.stores.lock();
            Arc::clone(
                stores
                    .entry(resolved.store_path.clone())
                    .or_insert_with(|| Arc::new(OnceCell::new())),
            )
        };
        let store = cell.get_or_try_init(|| self.open_store(&resolved)).await?;
        Ok(Arc::clone(store))
    }

    /// Where `repo_root`'s store lives, if the repo passes checks 1–3.
    ///
    /// Creates nothing on disk. Successful answers are cached for
    /// [`RESOLVE_CACHE_TTL`], so a change to `memory_store` takes effect within
    /// that window. Refusals are not cached.
    pub fn resolve(&self, repo_root: &Path) -> Result<ResolvedRepoStore, RepoStoreError> {
        if let Some(hit) = self.resolved.lock().get(repo_root) {
            self.counters.resolve_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(hit);
        }
        self.counters.resolve_misses.fetch_add(1, Ordering::Relaxed);
        let resolved = self.check(repo_root)?;
        self.resolved.lock().insert(repo_root, resolved.clone());
        Ok(resolved)
    }

    /// Snapshot of the counters and of how many stores are open.
    pub fn stats(&self) -> RepoStoreStats {
        let open_stores = self
            .stores
            .lock()
            .values()
            .filter(|cell| cell.get().is_some())
            .count();
        RepoStoreStats {
            open_stores,
            opens: self.counters.opens.load(Ordering::Relaxed),
            open_failures: self.counters.open_failures.load(Ordering::Relaxed),
            resolve_hits: self.counters.resolve_hits.load(Ordering::Relaxed),
            resolve_misses: self.counters.resolve_misses.load(Ordering::Relaxed),
            rejected_outside_allowlist: self
                .counters
                .rejected_outside_allowlist
                .load(Ordering::Relaxed),
            rejected_not_opted_in: self.counters.rejected_not_opted_in.load(Ordering::Relaxed),
        }
    }

    /// Checks 1–3. Reads the filesystem but never writes to it.
    fn check(&self, repo_root: &Path) -> Result<ResolvedRepoStore, RepoStoreError> {
        let root = repo_root
            .canonicalize()
            .map_err(|err| RepoStoreError::RootUnavailable {
                root: repo_root.to_path_buf(),
                reason: err.to_string(),
            })?;

        let allowed = self
            .allowed_roots
            .iter()
            .any(|allowed| root.starts_with(allowed));
        if !allowed {
            self.counters
                .rejected_outside_allowlist
                .fetch_add(1, Ordering::Relaxed);
            return Err(RepoStoreError::OutsideAllowlist { root });
        }

        let config = match RepoConfig::load(&root) {
            Ok(config) => config,
            Err(err) => {
                self.counters
                    .rejected_not_opted_in
                    .fetch_add(1, Ordering::Relaxed);
                return Err(RepoStoreError::NotOptedIn {
                    root,
                    reason: format!("config.toml unreadable: {err}"),
                });
            }
        };
        if config.memory_store != MemoryStoreMode::Repo {
            self.counters
                .rejected_not_opted_in
                .fetch_add(1, Ordering::Relaxed);
            return Err(RepoStoreError::NotOptedIn {
                root,
                reason: "memory_store is not \"repo\"".to_string(),
            });
        }

        Ok(ResolvedRepoStore {
            store_path: memory_store_path_for_root(&root),
            root,
        })
    }

    async fn open_store(
        &self,
        resolved: &ResolvedRepoStore,
    ) -> Result<Arc<VecSqliteMemoryStore>, RepoStoreError> {
        // Key first: without it the store file is never created. The project
        // id is the one the repo's package uses, so both share a key domain.
        let project_id = derive_repo_identity(&resolved.root).project_id;
        let record_key = match self.keys.record_key(&project_id) {
            Ok(key) => key,
            Err(err) => {
                self.counters.open_failures.fetch_add(1, Ordering::Relaxed);
                return Err(RepoStoreError::RecordKeyUnavailable {
                    root: resolved.root.clone(),
                    reason: format!("{err:#}"),
                });
            }
        };
        let mut config = VecSqliteStoreConfig::from_env();
        config.path = resolved.store_path.clone();
        let opened = VecSqliteMemoryStore::new_with_record_key(
            config,
            Arc::clone(&self.provider),
            PoolProfile::RepoStore,
            record_key,
        )
        .await;
        let store = match opened {
            Ok(store) => store,
            Err(err) => {
                self.counters.open_failures.fetch_add(1, Ordering::Relaxed);
                return Err(classify_open_error(&resolved.store_path, &err));
            }
        };
        // Same dedup settings the global store gets in `from_env`.
        *store.dedup_config.write().await = XavierSettings::current().memory.dedup.clone();
        self.counters.opens.fetch_add(1, Ordering::Relaxed);
        Ok(Arc::new(store))
    }
}

/// Map an open failure to [`RepoStoreError::StoreCorrupt`] when SQLite says the
/// file is not a database, and to [`RepoStoreError::Open`] otherwise.
fn classify_open_error(path: &Path, err: &anyhow::Error) -> RepoStoreError {
    let corrupt = err.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::SqliteFailure(code, _))
                if matches!(
                    code.code,
                    rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt
                )
        )
    });
    let reason = format!("{err:#}");
    if corrupt {
        RepoStoreError::StoreCorrupt {
            path: path.to_path_buf(),
            reason,
        }
    } else {
        RepoStoreError::Open {
            path: path.to_path_buf(),
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::connection_provider::InMemoryProvider;
    use crate::memory::store::{MemoryRecord, MemoryStore as _};

    xavier::isolate_test_process!();

    const OPT_IN: &str = "memory_store = \"repo\"\n";

    /// A repo directory under `base`, with `.xavier/config.toml` when `config` is `Some`.
    fn make_repo(base: &Path, name: &str, config: Option<&str>) -> PathBuf {
        let root = base.join(name);
        std::fs::create_dir_all(root.join(".git")).expect("mkdir .git");
        if let Some(body) = config {
            std::fs::create_dir_all(root.join(".xavier")).expect("mkdir .xavier");
            std::fs::write(root.join(".xavier/config.toml"), body).expect("write config");
        }
        root.canonicalize().expect("canonical repo root")
    }

    const TEST_MASTER: [u8; 32] = [0x3C; 32];
    /// Node record key used by the global store; a repo store must never accept it.
    const NODE_KEY: [u8; 32] = [0xEF; 32];

    /// Per-repo keys derived from a fixed test master, never from the keyring.
    struct TestKeys;

    impl RepoRecordKeySource for TestKeys {
        fn record_key(&self, project_id: &str) -> anyhow::Result<[u8; 32]> {
            crate::codebase::repo_package_crypto::derive_repo_record_key_from_master(
                &TEST_MASTER,
                project_id,
            )
        }
    }

    /// A key source with no master key behind it.
    struct NoKeys;

    impl RepoRecordKeySource for NoKeys {
        fn record_key(&self, _project_id: &str) -> anyhow::Result<[u8; 32]> {
            Err(anyhow::anyhow!("the node master key is not initialised"))
        }
    }

    fn registry(allowed: &[&Path]) -> RepoStoreRegistry {
        let roots: Vec<PathBuf> = allowed.iter().map(|p| p.to_path_buf()).collect();
        RepoStoreRegistry::new(
            &roots,
            Arc::new(InMemoryProvider::new()),
            Arc::new(TestKeys),
        )
    }

    fn record(id: &str, content: &str) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            workspace_id: "ws".to_string(),
            path: format!("notes/{id}"),
            content: content.to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn two_repos_two_files_no_cross_read() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let a = make_repo(tmp.path(), "repo-a", Some(OPT_IN));
        let b = make_repo(tmp.path(), "repo-b", Some(OPT_IN));
        let reg = registry(&[tmp.path()]);

        let store_a = reg.open(&a).await.expect("open a");
        let store_b = reg.open(&b).await.expect("open b");
        assert_ne!(
            store_a.connection_project_id(),
            store_b.connection_project_id()
        );

        store_a
            .put(record("only-a", "alpha secret"))
            .await
            .expect("put into a");
        assert!(store_a.get("ws", "only-a").await.unwrap().is_some());
        assert!(
            store_b.get("ws", "only-a").await.unwrap().is_none(),
            "a record written to repo A must not be readable from repo B"
        );
        assert!(a.join(".xavier/memory.sqlite3").exists());
        assert!(b.join(".xavier/memory.sqlite3").exists());
    }

    #[tokio::test]
    async fn repo_store_rows_unreadable_with_node_key() {
        use crate::memory::sqlite_vec_store::at_rest::unwrap_dek;

        let tmp = tempfile::tempdir().expect("tempdir");
        let root = make_repo(tmp.path(), "repo-keyed", Some(OPT_IN));
        let reg = registry(&[tmp.path()]);
        let store = reg.open(&root).await.expect("open");
        store
            .put(record("keyed", "repo-only words"))
            .await
            .expect("put");

        let conn = rusqlite::Connection::open(root.join(".xavier/memory.sqlite3"))
            .expect("raw connection");
        let wrapped: Vec<u8> = conn
            .query_row(
                "SELECT encrypted_dek FROM memory_records WHERE path = ?1",
                ["notes/keyed"],
                |row| row.get(0),
            )
            .expect("raw row");
        assert!(
            unwrap_dek(&wrapped, &NODE_KEY).is_err(),
            "the node record key must not open a repo row"
        );
        let repo_key = TestKeys
            .record_key(&derive_repo_identity(&root).project_id)
            .expect("repo key");
        assert!(unwrap_dek(&wrapped, &repo_key).is_ok());
        assert_eq!(
            store
                .get("ws", "keyed")
                .await
                .expect("get")
                .expect("present")
                .content,
            "repo-only words"
        );
    }

    #[tokio::test]
    async fn missing_record_key_fails_closed_and_creates_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = make_repo(tmp.path(), "repo-nokey", Some(OPT_IN));
        let reg = RepoStoreRegistry::new(
            &[tmp.path().to_path_buf()],
            Arc::new(InMemoryProvider::new()),
            Arc::new(NoKeys),
        );
        assert!(matches!(
            reg.open(&root).await,
            Err(RepoStoreError::RecordKeyUnavailable { .. })
        ));
        assert!(!root.join(".xavier/memory.sqlite3").exists());
        assert_eq!(reg.stats().open_failures, 1);
        assert_eq!(reg.stats().open_stores, 0);
    }

    #[tokio::test]
    async fn store_opened_once_migrations_run_once() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = make_repo(tmp.path(), "repo-once", Some(OPT_IN));
        let reg = registry(&[tmp.path()]);

        let first = reg.open(&root).await.expect("first open");
        for _ in 0..3 {
            let again = reg.open(&root).await.expect("reopen");
            assert!(
                Arc::ptr_eq(&first, &again),
                "same repo must reuse the store"
            );
        }
        let stats = reg.stats();
        assert_eq!(stats.opens, 1, "the store is opened and migrated once");
        assert_eq!(stats.open_stores, 1);
        assert!(
            stats.resolve_hits >= 3,
            "repeat opens hit the resolution cache"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_open_same_repo_yields_one_pool() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = make_repo(tmp.path(), "repo-race", Some(OPT_IN));
        let reg = Arc::new(registry(&[tmp.path()]));

        let tasks: Vec<_> = (0..32)
            .map(|_| {
                let reg = Arc::clone(&reg);
                let root = root.clone();
                tokio::spawn(async move { reg.open(&root).await })
            })
            .collect();
        let mut stores = Vec::new();
        for task in tasks {
            stores.push(task.await.expect("join").expect("open"));
        }
        for store in &stores[1..] {
            assert!(Arc::ptr_eq(&stores[0], store), "single-flight: one store");
        }
        assert_eq!(
            reg.stats().opens,
            1,
            "32 concurrent opens run one migration"
        );
    }

    #[tokio::test]
    async fn spelling_variants_share_one_pool() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = make_repo(tmp.path(), "repo-spelling", Some(OPT_IN));
        std::fs::create_dir_all(root.join("sub")).expect("mkdir sub");
        let reg = registry(&[tmp.path()]);

        let plain = reg.open(&root).await.expect("plain");
        let dotted = reg.open(&root.join(".")).await.expect("dotted");
        let via_parent = reg
            .open(&root.join("sub").join(".."))
            .await
            .expect("parent");
        assert!(Arc::ptr_eq(&plain, &dotted));
        assert!(Arc::ptr_eq(&plain, &via_parent));
        assert_eq!(reg.stats().opens, 1);
    }

    #[tokio::test]
    async fn root_without_opt_in_is_rejected_and_creates_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plain = make_repo(tmp.path(), "repo-plain", None);
        let global = make_repo(
            tmp.path(),
            "repo-global",
            Some("memory_store = \"global\"\n"),
        );
        let reg = registry(&[tmp.path()]);

        for root in [&plain, &global] {
            match reg.open(root).await {
                Err(RepoStoreError::NotOptedIn { .. }) => {}
                Err(other) => panic!("expected NotOptedIn for {}, got {other:?}", root.display()),
                Ok(_) => panic!("expected NotOptedIn for {}, got Ok", root.display()),
            }
        }
        assert!(!plain.join(".xavier").exists(), "no package dir created");
        assert!(!plain.join(".xavier/memory.sqlite3").exists());
        assert_eq!(reg.stats().rejected_not_opted_in, 2);
        assert_eq!(reg.stats().opens, 0);
    }

    #[tokio::test]
    async fn root_outside_allowlist_is_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let allowed = tmp.path().join("allowed");
        std::fs::create_dir_all(&allowed).expect("mkdir allowed");
        let inside = make_repo(&allowed, "inside", Some(OPT_IN));
        // A sibling whose name only shares the prefix must not pass.
        let sibling = make_repo(tmp.path(), "allowed-evil", Some(OPT_IN));
        let outside = make_repo(tmp.path(), "outside", Some(OPT_IN));
        let reg = registry(&[&allowed]);

        assert!(reg.open(&inside).await.is_ok());
        for root in [&sibling, &outside] {
            match reg.open(root).await {
                Err(RepoStoreError::OutsideAllowlist { .. }) => {}
                Err(other) => panic!(
                    "expected OutsideAllowlist for {}, got {other:?}",
                    root.display()
                ),
                Ok(_) => panic!("expected OutsideAllowlist for {}, got Ok", root.display()),
            }
            assert!(!root.join(".xavier/memory.sqlite3").exists());
        }
        assert_eq!(reg.stats().rejected_outside_allowlist, 2);
    }

    #[tokio::test]
    async fn empty_allowlist_disables_the_feature() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = make_repo(tmp.path(), "repo-disabled", Some(OPT_IN));
        let reg = registry(&[]);
        assert!(matches!(
            reg.open(&root).await,
            Err(RepoStoreError::OutsideAllowlist { .. })
        ));
        assert!(!root.join(".xavier/memory.sqlite3").exists());
    }

    #[tokio::test]
    async fn missing_root_is_unavailable_and_creates_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let reg = registry(&[tmp.path()]);
        let missing = tmp.path().join("does-not-exist");
        assert!(matches!(
            reg.open(&missing).await,
            Err(RepoStoreError::RootUnavailable { .. })
        ));
        assert!(!missing.exists());
    }

    #[tokio::test]
    async fn corrupt_store_file_is_reported_and_other_stores_keep_working() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bad = make_repo(tmp.path(), "repo-corrupt", Some(OPT_IN));
        let good = make_repo(tmp.path(), "repo-healthy", Some(OPT_IN));
        std::fs::write(
            bad.join(".xavier/memory.sqlite3"),
            b"this is not a sqlite database, not even close\n".repeat(128),
        )
        .expect("write garbage store");
        let reg = registry(&[tmp.path()]);

        match reg.open(&bad).await {
            Err(RepoStoreError::StoreCorrupt { .. }) => {}
            Err(other) => panic!("expected StoreCorrupt, got {other:?}"),
            Ok(_) => panic!("expected StoreCorrupt, got Ok"),
        }
        assert!(
            reg.open(&good).await.is_ok(),
            "one bad file must not poison the registry"
        );
        assert_eq!(reg.stats().open_failures, 1);
        assert_eq!(reg.stats().open_stores, 1);
    }

    #[test]
    fn resolve_cache_is_bounded_and_evicts_least_recent() {
        let mut cache = ResolveCache::default();
        let resolved = |i: usize| ResolvedRepoStore {
            root: PathBuf::from(format!("/r/{i}")),
            store_path: PathBuf::from(format!("/r/{i}/.xavier/memory.sqlite3")),
        };
        for i in 0..RESOLVE_CACHE_CAPACITY {
            cache.insert(Path::new(&format!("/r/{i}")), resolved(i));
        }
        // Touch entry 0 so entry 1 becomes the least recently used.
        assert!(cache.get(Path::new("/r/0")).is_some());
        cache.insert(Path::new("/r/new"), resolved(999));
        assert_eq!(cache.entries.len(), RESOLVE_CACHE_CAPACITY);
        assert!(cache.get(Path::new("/r/0")).is_some());
        assert!(cache.get(Path::new("/r/1")).is_none());
    }
}
