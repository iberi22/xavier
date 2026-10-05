//! Codebase connection manager
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::domain::cycle_breaks::w30_10::ConnectionTuning;
use anyhow::{Context, Result};
use parking_lot::{Mutex, RwLock};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, ErrorCode};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub static INSTANCE: std::sync::OnceLock<ConnectionManager> = std::sync::OnceLock::new();

/// Hard cap on concurrently registered pools, enforced by
/// [`ConnectionManager::evict_if_needed`]. Public so it can be reported by
/// [`ConnectionManager::snapshot`] instead of being a hardcoded literal at
/// every call site.
pub const MAX_POOLS: usize = 16;
const WAL_INIT_ATTEMPTS: usize = 10;
/// `max_size` given to every r2d2 pool. Unchanged from today's value; named so
/// the snapshot reports the real number instead of a duplicate literal.
pub const POOL_MAX_SIZE: u32 = 10;
/// `max_size` of a per-repository memory store pool (T4).
///
/// 4 is the value from design §2.2: a repo store is queried by one or two
/// concurrent requests at a time, so 10 connections per file would just be 10
/// file-descriptor pairs and 10 page caches held open.
pub const REPO_POOL_MAX_SIZE: u32 = 4;
/// `cache_size` of a per-repository store pool: negative means KiB, so -2000
/// is ~2 MiB per connection instead of the daemon-wide -8000 (~7.8 MiB).
pub const REPO_POOL_CACHE_SIZE: i64 = -2000;
const WAL_INIT_RETRY_DELAY: Duration = Duration::from_millis(50);
/// How many eviction records [`ConnectionManager::snapshot`] keeps.
const EVICTION_RING_CAPACITY: usize = 32;

static WAL_INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Unified SQLite connection manager for Xavier.
/// Manages connection pools by project_id with LRU eviction and PRAGMA optimizations.
pub struct ConnectionManager {
    pools: RwLock<std::collections::HashMap<String, ProjectPool>>,
    /// Real db path used the first time a `project_id` was connected, so an
    /// evicted pool is resurrected against the file it actually owns instead
    /// of a guess (see [`ConnectionManager::get_or_reconnect_pool`]).
    known_paths: RwLock<std::collections::HashMap<String, PathBuf>>,
    active: Arc<tokio::sync::RwLock<Option<String>>>,
    idle_timeout_secs: u64,
    /// Purely observational counters (T3). Nothing reads these to decide
    /// anything, so eviction behaviour is unaffected by their presence.
    metrics: PoolMetrics,
    /// Start of the in-process monotonic clock used for `at_ms` in eviction
    /// records. Observational only.
    epoch: Instant,
}

/// Observability state for [`ConnectionManager::snapshot`].
///
/// Every field here is write-only bookkeeping: no eviction, connection or
/// path decision reads it. It exists so pool and file-descriptor pressure can
/// be *measured* before any change is made to how pools are opened or closed.
#[derive(Debug, Default)]
struct PoolMetrics {
    /// Number of times a pool was registered for a `project_id` that had no
    /// live pool (fresh pool or resurrection after eviction).
    opened_total: AtomicU64,
    /// Pools currently registered in `pools`, maintained in lockstep with the
    /// map so the snapshot can cross-check it.
    registered_total: AtomicU64,
    /// Evictions broken down by cause. `lru` = over `MAX_POOLS`,
    /// `idle` = past `idle_timeout_secs`, `manual` = `disconnect`.
    evicted_lru: AtomicU64,
    evicted_idle: AtomicU64,
    evicted_manual: AtomicU64,
    /// Eviction where a connection was available and
    /// `PRAGMA wal_checkpoint(TRUNCATE)` actually ran.
    evicted_checkpointed: AtomicU64,
    /// Evictions where no connection was available, so no checkpoint ran.
    evicted_uncheckpointed: AtomicU64,
    /// `get_or_reconnect_pool` calls that had to rebuild an absent pool.
    resurrected_total: AtomicU64,
    /// Bounded history of recent evictions, newest last.
    ring: Mutex<VecDeque<EvictionRecord>>,
}

/// One eviction, as recorded in the bounded ring of [`PoolMetrics::ring`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvictionRecord {
    /// The `project_id` whose pool was dropped.
    pub id: String,
    /// Why it was dropped.
    pub reason: EvictionReason,
    /// Milliseconds since this `ConnectionManager` was created. A monotonic,
    /// in-process clock, not a wall-clock timestamp: it is only meaningful for
    /// ordering and spacing, never for correlating with logs.
    pub at_ms: u64,
    /// Whether `PRAGMA wal_checkpoint(TRUNCATE)` ran before removal.
    pub checkpointed: bool,
}

/// Why a pool left the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictionReason {
    /// Dropped because `pools.len() >= MAX_POOLS` (least recently used).
    Lru,
    /// Dropped because `activated_at` was older than `idle_timeout_secs`.
    Idle,
    /// Dropped by an explicit [`ConnectionManager::disconnect`].
    Manual,
}

impl EvictionReason {
    fn as_str(self) -> &'static str {
        match self {
            EvictionReason::Lru => "lru",
            EvictionReason::Idle => "idle",
            EvictionReason::Manual => "manual",
        }
    }
}

struct ProjectPool {
    pool: Arc<Pool<SqliteConnectionManager>>,
    activated_at: Instant,
    /// Shape of this pool: `max_size`, `min_idle`, `cache_size`, `mmap_size`.
    profile: PoolProfile,
    /// Whether the LRU / idle branches may evict this pool (T4).
    class: PoolClass,
}

/// Per-class pool sizing and PRAGMA overrides (T4).
///
/// Two shapes exist so a per-repository memory store does not cost the same as
/// a daemon-wide infrastructure store:
///
/// | Profile | `max_size` | `min_idle` | `cache_size` | `mmap_size` |
/// |---|---|---|---|---|
/// | [`PoolProfile::Default`] | 10 | 10 (eager) | -8000 KiB | 256 MiB |
/// | [`PoolProfile::RepoStore`] | 4 | 0 (lazy) | -2000 KiB | 0 |
///
/// [`PoolProfile::Default`] is byte-identical to the behaviour before T4: the
/// same 10 connections opened eagerly at pool build, the same pragmas. Only
/// [`PoolProfile::RepoStore`] changes, and only callers that ask for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PoolProfile {
    /// Today's daemon-wide pool: `max_size = 10`, filled to `min_idle = 10` at
    /// build time, `cache_size = -8000` (≈7.8 MiB per connection) and
    /// `mmap_size = 256 MiB`.
    #[default]
    Default,
    /// A per-repository memory store: `max_size = 4`, `min_idle = 0` so r2d2
    /// opens connections on demand instead of all four up front,
    /// `cache_size = -2000` (≈2 MiB per connection) and no mmap.
    RepoStore,
}

impl PoolProfile {
    /// `max_size` given to the r2d2 builder.
    pub fn max_size(self) -> u32 {
        match self {
            PoolProfile::Default => POOL_MAX_SIZE,
            PoolProfile::RepoStore => REPO_POOL_MAX_SIZE,
        }
    }

    /// `min_idle` given to the r2d2 builder.
    ///
    /// `Some(0)` is what makes connections lazy: r2d2 0.8 defaults `min_idle`
    /// to `max_size` and fills the pool at build time, so omitting it opens
    /// every connection the moment the pool is registered.
    pub fn min_idle(self) -> Option<u32> {
        match self {
            PoolProfile::Default => Some(POOL_MAX_SIZE),
            PoolProfile::RepoStore => Some(0),
        }
    }

    /// `PRAGMA cache_size` for this profile. Negative values are KiB.
    pub fn cache_size(self) -> i64 {
        match self {
            PoolProfile::Default => -8000,
            PoolProfile::RepoStore => REPO_POOL_CACHE_SIZE,
        }
    }

    /// `PRAGMA mmap_size` for this profile, in bytes.
    pub fn mmap_size(self) -> i64 {
        match self {
            PoolProfile::Default => 268_435_456,
            PoolProfile::RepoStore => 0,
        }
    }

    /// Class a caller most likely wants for this profile.
    ///
    /// `Default` → `Pinned` (daemon infrastructure and the global store),
    /// `RepoStore` → `Evictable`. It is *not* applied automatically by
    /// [`ConnectionManager::connect_with_path_profile`] (which keeps today's
    /// "everything is evictable" behaviour); pass it explicitly, or flip an
    /// open pool later with [`ConnectionManager::set_pool_class`].
    pub fn suggested_class(self) -> PoolClass {
        match self {
            PoolProfile::Default => PoolClass::Pinned,
            PoolProfile::RepoStore => PoolClass::Evictable,
        }
    }

    /// Apply only the settings this profile overrides, on top of
    /// [`ConnectionTuning::apply_connection_pragmas`].
    ///
    /// Deliberately narrow: `journal_mode`, `wal_autocheckpoint`,
    /// `journal_size_limit`, `synchronous` and `temp_store` stay exactly as
    /// `apply_connection_pragmas` sets them for every profile, because those
    /// are file-level durability settings and not part of the pool budget.
    /// Neither layer checkpoints the WAL.
    pub fn apply_overrides(&self, conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(&format!(
            "PRAGMA cache_size={}; \
             PRAGMA mmap_size={};",
            self.cache_size(),
            self.mmap_size()
        ))
    }
}

/// Whether the LRU and idle branches may evict a pool (T4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolClass {
    /// Infrastructure the daemon cannot lose: `memory`, `metrics`, `security`,
    /// `default`, `auth`, `vec_store_*`, `conv_*`. Never chosen by the LRU or
    /// the idle branch; only `disconnect` / `shutdown` drop it.
    Pinned,
    /// Per-project / per-repository pools. These are the LRU's candidates.
    Evictable,
}

impl PoolClass {
    /// True when the LRU and idle branches may drop this pool.
    pub fn is_evictable(self) -> bool {
        matches!(self, PoolClass::Evictable)
    }

    fn as_str(self) -> &'static str {
        match self {
            PoolClass::Pinned => "pinned",
            PoolClass::Evictable => "evictable",
        }
    }
}

/// Ids of pools the daemon cannot operate without (design §2.2: "Nunca se
/// expulsa un `Pinned` (store global, `memory`, `metrics`, `security`,
/// `default`, `auth`)"), so [`ConnectionManager::connect`] registers them as
/// [`PoolClass::Pinned`].
///
/// Deliberately a closed list of literals, not a prefix rule: `conv_*` is one
/// pool per workspace, so pinning the prefix would let the map grow past
/// `MAX_POOLS` without bound and multiply today's eager 10-connection pools.
/// Per-workspace pools stay [`PoolClass::Evictable`] (and become reclaimable by
/// the idle sweep in T5). `vec_store_*` is pinned too — the global memory store
/// is the one store the process reads on every federated search.
pub fn is_pinned_infrastructure_id(project_id: &str) -> bool {
    matches!(
        project_id,
        "memory" | "vec_store" | "metrics" | "security" | "default" | "auth"
    ) || project_id.starts_with("vec_store_")
}

#[derive(Debug)]
struct PragmaCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for PragmaCustomizer {
    /// Only applies the cheap, connection-local PRAGMA layer because this
    /// callback can run on every acquire.
    ///
    /// The heavy settings are applied once per connection by the
    /// [`SqliteConnectionManager::with_init`] callback installed in
    /// [`ConnectionManager::connect_with_path`], and no layer checkpoints the
    /// WAL. Acquiring a pooled connection therefore never blocks behind another
    /// connection's readers (previously a `wal_checkpoint(TRUNCATE)` could stall
    /// every acquire for up to `busy_timeout`).
    fn on_acquire(&self, conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        conn.apply_acquire_pragmas().map_err(|e| {
            eprintln!("PragmaCustomizer: PRAGMA error: {}", e);
            e
        })
    }
}

/// One-time SQLite/WAL setup for a database file, run when its pool is built.
///
/// Applies the full (heavy) PRAGMA layer and switches the file to WAL mode,
/// retrying briefly when another process holds a conflicting lock. Does not
/// checkpoint the WAL: that is maintenance work, not connection setup (see
/// [`ConnectionTuning`]).
fn initialize_wal_mode(conn: &Connection, db_path: &PathBuf) -> Result<()> {
    let _guard = WAL_INIT_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("SQLite WAL initialization lock was poisoned"))?;

    conn.apply_pragmas()
        .with_context(|| format!("failed to configure SQLite pragmas at {:?}", db_path))?;

    for attempt in 1..=WAL_INIT_ATTEMPTS {
        match conn.execute_batch("PRAGMA journal_mode=WAL;") {
            Ok(()) => break,
            Err(err) if is_sqlite_lock_error(&err) && attempt < WAL_INIT_ATTEMPTS => {
                std::thread::sleep(WAL_INIT_RETRY_DELAY);
            }
            Err(err) if is_sqlite_lock_error(&err) => {
                eprintln!(
                    "ConnectionManager: SQLite WAL initialization skipped for {:?}: {}",
                    db_path, err
                );
                break;
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to initialize SQLite WAL mode at {:?}", db_path)
                });
            }
        }
    }

    // Opportunistic WAL truncation, moved out of the pool *acquire* path: it runs
    // once per pool construction and only when the WAL exceeded the threshold
    // (a `wal_checkpoint(TRUNCATE)` waits for every reader of the file, so running
    // it per acquire could stall every checkout behind a long-running read).
    let _ = conn.maybe_wal_checkpoint();

    Ok(())
}

/// True when `project_id` matches one of the explicit id→path branches in
/// [`ConnectionManager::connect`] (the literal ids `memory`, `vec_store`,
/// `metrics`, `security`, or the `conv_*`/`test_*` prefixes) rather than the
/// catch-all `./.xavier/codebase.db` guess. `get_or_reconnect_pool` uses this
/// to decide whether an id that was never registered via `connect()` /
/// `connect_with_path()` may still be lazily connected, or must be rejected
/// (the catch-all previously let an unrelated id like a hashed
/// `vec_store_<sha>` silently bind to the wrong database file, #2736).
fn is_explicitly_mapped(project_id: &str) -> bool {
    matches!(project_id, "memory" | "vec_store" | "metrics" | "security")
        || project_id.starts_with("conv_")
        || project_id.starts_with("test_")
}

/// True for the ids that only ever exist to serve tests: `conv_test_*` (from
/// `ConversationsDb::open_in_memory` / `ConversationsDb::new_test`) and
/// `test_*` (from `CodebaseDb::open_in_memory`,
/// `RateLimitManager::new_with_project`). See [`test_store_root`].
fn is_test_only_id(project_id: &str) -> bool {
    project_id.starts_with("conv_test_") || project_id.starts_with("test_")
}

/// Root directory under which test-only ids (`conv_test_*`, `test_*`) resolve
/// their database files, as an `Option` so that both cfg variants share one
/// signature.
///
/// Every caller of these ids passes `project_root = "."`, so before this
/// override the path branch in [`ConnectionManager::connect`] produced
/// `<cwd>/.xavier/tests/<id>.db` — i.e. **inside the repository**, because
/// `cargo test` runs with the crate root as cwd. One `cargo test` run therefore
/// left a `conv_test_<ULID>.db` plus its `-shm`/`-wal` trio per test, and
/// `.xavier/tests/` had accumulated 27.618 files (825 MB) before it was cleared.
///
/// This mirrors the `MultiDbManager::with_root` fix (T1): the id→path mapping
/// is unchanged, only the root it resolves against moves out of the repo and
/// into the system temp dir. The root is a process-lifetime `OnceLock` on
/// purpose — a per-call `TempDir` would be dropped (and its files deleted)
/// while a pool built by an earlier test is still open, so the directory is
/// created once and left for the OS to reap. Production ids (`memory`,
/// `metrics`, `security`, `conv_<workspace>`, `vec_store_*`) are untouched.
///
/// Returns `None` in a production build, so the production path resolution is
/// provably unchanged (the `test-utils` feature keeps the override on, which is
/// why its callers are the ones under `#[cfg(test)]`).
#[cfg(any(test, feature = "test-utils"))]
fn test_store_root() -> Option<PathBuf> {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    Some(
        ROOT.get_or_init(|| {
            let root =
                std::env::temp_dir().join(format!("xavier-test-store-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&root);
            root
        })
        .clone(),
    )
}

/// [`test_store_root`] in a production build: always `None`.
#[cfg(not(any(test, feature = "test-utils")))]
fn test_store_root() -> Option<PathBuf> {
    None
}

/// Directory that holds the database files of a test-only id
/// ([`is_test_only_id`]: `conv_test_*`, `test_*`).
///
/// In a test/test-utils build this is always inside [`test_store_root`],
/// never the repo. In a production build `test_store_root` is `None` and this
/// is exactly the historical `<project_root>/.xavier/tests`.
fn test_store_dir(project_root: &str) -> PathBuf {
    let _ = project_root;
    PathBuf::from(project_root).join(".xavier").join("tests")
}

/// The single id→path mapping of [`ConnectionManager::connect`], extracted so
/// it can be unit-tested without opening a pool.
///
/// The mapping itself is byte-for-byte the pre-existing one. The only
/// behavioural difference is the root that the test-only branches
/// (`conv_test_*`, `test_*`) resolve against — see [`test_store_root`]. Branch
/// order matters: `conv_test_*` is tested before `conv_*`, and `memory` /
/// `vec_store` / `metrics` / `security` before both prefixes.
fn resolve_db_path(project_id: &str, project_root: &str) -> Result<PathBuf> {
    let db_path = if project_id == "memory" {
        PathBuf::from(project_root).join("xavier_memory.db")
    } else if project_id == "vec_store" {
        PathBuf::from(project_root).join("vec-store.sqlite3")
    } else if project_id == "metrics" {
        PathBuf::from(project_root).join("metrics.db")
    } else if project_id == "security" {
        PathBuf::from(project_root)
            .join(".xavier")
            .join("security.db")
    } else if project_id.starts_with("conv_test_") {
        test_store_dir(project_root).join(format!("{}.db", project_id))
    } else if project_id.starts_with("conv_") {
        let pid = project_id
            .strip_prefix("conv_")
            .ok_or_else(|| anyhow::anyhow!("invalid conversation prefix"))?;
        dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not find home directory"))?
            .join(".xavier")
            .join("conversations")
            .join(format!("{}.db", pid))
    } else if project_id.starts_with("test_") {
        test_store_dir(project_root).join(format!("{}.db", project_id))
    } else {
        PathBuf::from(project_root)
            .join(".xavier")
            .join("codebase.db")
    };
    Ok(db_path)
}

fn is_sqlite_lock_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(error, _)
            if matches!(error.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionSnapshot {
    /// One entry per live pool, sorted by `id` for stable output.
    pub pools: Vec<PoolSnapshot>,
    /// Aggregated counters, including the eviction totals split by reason.
    pub counters: SnapshotCounters,
    /// Most recent evictions, newest last, capped at
    /// [`EVICTION_RING_CAPACITY`].
    pub evictions: Vec<EvictionRecord>,
}

/// Observable state of a single live pool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PoolSnapshot {
    /// The `project_id` this pool is registered under.
    pub id: String,
    /// The database file the pool actually opens. Taken from
    /// `known_paths`, so it is the resolved path, not a guess.
    pub path: Option<PathBuf>,
    /// Open connections, from `r2d2::Pool::state`.
    pub connections: u32,
    /// Connections currently idle in the pool, from `r2d2::Pool::state`.
    pub idle_connections: u32,
    /// `max_size` the pool was built with, read from its [`PoolProfile`] so a
    /// repo pool reports its real 4 instead of the daemon-wide 10.
    pub max_size: u32,
    /// Shape this pool was built with.
    pub profile: PoolProfile,
    /// Whether the LRU and idle branches may drop this pool.
    pub class: PoolClass,
    /// `min_idle` the pool was built with.
    pub min_idle: Option<u32>,
    /// Milliseconds since `activated_at` was last refreshed.
    pub idle_for_ms: u64,
}

/// Counters exposed by [`ConnectionManager::snapshot`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotCounters {
    /// Pools currently registered.
    pub open_pools: usize,
    /// `MAX_POOLS`, the hard cap on registered pools.
    pub max_pools: usize,
    /// Sum of `connections` across live pools: the lower bound on SQLite
    /// connections this manager holds.
    pub total_connections: u32,
    /// Sum of `idle_connections` across live pools.
    pub total_idle_connections: u32,
    /// `project_id`s in `known_paths`, which survive eviction.
    pub known_paths: usize,
    /// Pools built since creation (fresh plus resurrected).
    pub opened_total: u64,
    /// Pools rebuilt by `get_or_reconnect_pool` after being absent.
    pub resurrected_total: u64,
    /// Every eviction, any reason.
    pub evicted_total: u64,
    /// Evictions from the `MAX_POOLS` LRU branch.
    pub evicted_lru: u64,
    /// Evictions from the idle-timeout branch.
    pub evicted_idle: u64,
    /// Evictions from `disconnect`.
    pub evicted_manual: u64,
    /// Evictions where `PRAGMA wal_checkpoint(TRUNCATE)` ran.
    pub evicted_checkpointed: u64,
    /// Evictions where no connection was free, so no checkpoint ran.
    pub evicted_uncheckpointed: u64,
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionManager {
    /// Create a new connection manager instance.
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(std::collections::HashMap::new()),
            known_paths: RwLock::new(std::collections::HashMap::new()),
            active: Arc::new(tokio::sync::RwLock::new(None)),
            idle_timeout_secs: 300, // 5 minutes
            metrics: PoolMetrics::default(),
            epoch: Instant::now(),
        }
    }

    /// Get the global singleton instance.
    pub fn global() -> &'static Self {
        INSTANCE.get_or_init(Self::new)
    }

    /// Connect to a database by project_id.
    /// If the pool doesn't exist, it is created lazily.
    ///
    /// A pool whose id is daemon infrastructure
    /// ([`is_pinned_infrastructure_id`]) is registered as
    /// [`PoolClass::Pinned`] so the LRU cannot drop it; everything else is
    /// [`PoolClass::Evictable`], as before.
    pub fn connect(&self, project_id: &str, project_root: &str) -> Result<()> {
        if !self.pools.read().contains_key(project_id) {
            let db_path = resolve_db_path(project_id, project_root)?;

            self.connect_with_path_profile(
                project_id,
                db_path,
                PoolProfile::Default,
                if is_pinned_infrastructure_id(project_id) {
                    PoolClass::Pinned
                } else {
                    PoolClass::Evictable
                },
            )
        } else {
            // Update last accessed time
            if let Some(entry) = self.pools.write().get_mut(project_id) {
                entry.activated_at = Instant::now();
            }
            Ok(())
        }
    }

    /// Explicitly connect to a database file with a given project_id.
    ///
    /// Exactly [`ConnectionManager::connect_with_path_profile`] with
    /// [`PoolProfile::Default`] and [`PoolClass::Evictable`], i.e. byte-for-byte
    /// today's behaviour: `max_size = 10`, filled eagerly, and the LRU is free
    /// to drop it.
    pub fn connect_with_path(&self, project_id: &str, db_path: PathBuf) -> Result<()> {
        self.connect_with_path_profile(
            project_id,
            db_path,
            PoolProfile::Default,
            PoolClass::Evictable,
        )
    }

    /// Explicitly connect to a database file with a given project_id, pool
    /// shape and eviction class.
    ///
    /// Registering an id that already has a live pool only refreshes
    /// `activated_at`; the profile and class of the live pool are left alone,
    /// because r2d2 has already opened that pool at its configured size.
    /// Use [`ConnectionManager::set_pool_class`] to change the class.
    pub fn connect_with_path_profile(
        &self,
        project_id: &str,
        db_path: PathBuf,
        profile: PoolProfile,
        class: PoolClass,
    ) -> Result<()> {
        self.known_paths
            .write()
            .insert(project_id.to_string(), db_path.clone());

        if !self.pools.read().contains_key(project_id) {
            if let Some(parent) = db_path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create parent dir for {:?}", db_path)
                    })?;
                }
            }

            let init_conn = Connection::open(&db_path).with_context(|| {
                format!("failed to initialize SQLite database at {:?}", db_path)
            })?;
            initialize_wal_mode(&init_conn, &db_path)?;
            drop(init_conn);

            // Heavy PRAGMAs are applied once per connection, when the pool opens
            // it; `PragmaCustomizer` only re-applies the cheap layer on acquire.
            // The profile's own overrides (`cache_size`, `mmap_size`) run right
            // after, so they win over the shared defaults.
            let builder_profile = profile;
            let manager = SqliteConnectionManager::file(db_path).with_init(move |conn| {
                conn.apply_connection_pragmas()
                    .and_then(|()| builder_profile.apply_overrides(conn))
            });
            let pool = Pool::builder()
                .max_size(profile.max_size())
                // Explicit on both profiles: r2d2 0.8 defaults `min_idle` to
                // `max_size`, which is why every pool used to open its full
                // width the moment it was registered.
                .min_idle(profile.min_idle())
                .connection_customizer(Box::new(PragmaCustomizer))
                .build(manager)
                .context("failed to build r2d2 SQLite pool")?;

            self.evict_if_needed();

            self.pools.write().insert(
                project_id.to_string(),
                ProjectPool {
                    pool: Arc::new(pool),
                    activated_at: Instant::now(),
                    profile,
                    class,
                },
            );
            self.metrics.opened_total.fetch_add(1, Ordering::Relaxed);
        } else if let Some(entry) = self.pools.write().get_mut(project_id) {
            entry.activated_at = Instant::now();
        }
        Ok(())
    }

    /// Change the eviction class of an already-open pool.
    ///
    /// For infrastructure ids (`memory`, `metrics`, `security`, `default`,
    /// `auth`, `vec_store_*`, `conv_*`) registered through the plain
    /// [`ConnectionManager::connect`] / [`ConnectionManager::connect_with_path`]
    /// entry points, this is how the daemon marks them
    /// [`PoolClass::Pinned`] — those two entry points keep today's
    /// "everything is evictable" behaviour so no existing caller changes.
    /// Returns `false` when the id has no live pool.
    pub fn set_pool_class(&self, project_id: &str, class: PoolClass) -> bool {
        let mut pools = self.pools.write();
        match pools.get_mut(project_id) {
            Some(entry) => {
                entry.class = class;
                true
            }
            None => false,
        }
    }

    /// Class of a live pool, if any.
    pub fn pool_class(&self, project_id: &str) -> Option<PoolClass> {
        self.pools.read().get(project_id).map(|e| e.class)
    }

    /// Get or open a cached `CodeGraphDB` instance for a given database path.
    pub fn get_code_graph_db(
        &self,
        db_path: &std::path::Path,
    ) -> Result<code_graph::db::CodeGraphDB> {
        code_graph::db::CodeGraphDB::new(db_path).map_err(|e| anyhow::anyhow!(e))
    }

    /// Unload and checkpoint a specific `CodeGraphDB` from memory.
    pub fn unload_code_graph_db(&self, db_path: &std::path::Path) -> bool {
        code_graph::db::unload_db(db_path)
    }

    /// Shutdown the connection manager and flush all SQLite WAL checkpoints cleanly.
    pub fn shutdown(&self) {
        self.pools.write().clear();
        code_graph::db::flush_and_close_cache();
    }

    /// Manually disconnect and drop a pool.
    ///
    /// Note there is deliberately no WAL checkpoint here (unchanged behaviour,
    /// T3 is instrumentation only): SQLite checkpoints on the close of the last
    /// connection to a WAL file. The eviction record for this drop reports
    /// `checkpointed: false`.
    pub fn disconnect(&self, project_id: &str) {
        if self.pools.write().remove(project_id).is_some() {
            self.record_eviction(project_id.to_string(), EvictionReason::Manual, false);
        }
    }

    /// Set a project as active.
    pub async fn set_active(&self, project_id: &str, project_root: &str) -> Result<()> {
        self.connect(project_id, project_root)?;
        let mut active = self.active.write().await;
        *active = Some(project_id.to_string());
        Ok(())
    }

    /// Execute a query closure using a connection from the active pool.
    pub async fn with_active<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let active_id = {
            let active = self.active.read().await;
            active
                .clone()
                .ok_or_else(|| anyhow::anyhow!("no active project set"))?
        };

        self.with_conn(&active_id, f).await
    }

    /// Get an existing pool or lazily resurrect / reconnect it if evicted.
    pub fn get_or_reconnect_pool(
        &self,
        project_id: &str,
    ) -> Result<Arc<Pool<SqliteConnectionManager>>> {
        {
            let mut pools = self.pools.write();
            if let Some(entry) = pools.get_mut(project_id) {
                entry.activated_at = Instant::now();
                return Ok(entry.pool.clone());
            }
        }

        // Pool was evicted or not yet connected. If we've already resolved a
        // real path for this id (via a prior `connect()`/`connect_with_path()`
        // call), resurrect it against that exact path, never a guess:
        // reconnecting via `connect(project_id, ".")` used to silently bind
        // the pool to the wrong file for ids whose path isn't derivable from
        // `project_id` alone (e.g. a hashed `vec_store_<sha>` id read by a
        // health check before `VecSqliteMemoryStore::new` ever registers it).
        // Once bound, `connect_with_path` became a no-op for that id, so the
        // real store silently kept using the wrong pool (#2736).
        //
        // Ids that `connect()` maps through an explicit branch (literal ids,
        // or the `conv_*`/`test_*` prefixes) have a derivable path even on
        // their very first use, so those may still connect lazily here. Only
        // ids that would fall through to the catch-all guess are rejected.
        let known_path = self.known_paths.read().get(project_id).cloned();
        self.metrics
            .resurrected_total
            .fetch_add(1, Ordering::Relaxed);
        match known_path {
            Some(db_path) => self.connect_with_path(project_id, db_path)?,
            None if is_explicitly_mapped(project_id) => self.connect(project_id, ".")?,
            None => {
                return Err(anyhow::anyhow!(
                    "connection pool for '{}' was never established; call connect() or connect_with_path() before use",
                    project_id
                ))
            }
        }

        let mut pools = self.pools.write();
        let entry = pools.get_mut(project_id).ok_or_else(|| {
            anyhow::anyhow!("pool for {} not found after reconnect attempt", project_id)
        })?;
        entry.activated_at = Instant::now();
        Ok(entry.pool.clone())
    }

    /// Execute a query closure using a connection from a specific project's pool.
    pub async fn with_conn<F, T>(&self, project_id: &str, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let pool = self.get_or_reconnect_pool(project_id)?;

        tokio::task::spawn_blocking(move || {
            let conn = pool.get().context("failed to get connection from pool")?;
            f(&conn)
        })
        .await
        .context("blocking task panicked")?
    }

    /// Access the underlying pool for a specific project.
    pub async fn with_pool<F, T>(&self, project_id: &str, f: F) -> Result<T>
    where
        F: FnOnce(&Pool<SqliteConnectionManager>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let pool = self.get_or_reconnect_pool(project_id)?;

        tokio::task::spawn_blocking(move || f(&pool))
            .await
            .context("blocking task panicked")?
    }

    /// Observable state of every live pool, plus counters.
    ///
    /// Read-only: this takes the pools read lock and never touches a
    /// connection, so calling it cannot block on a checkout or change eviction
    /// timing. The whole point is to make pool and file-descriptor pressure
    /// measurable *before* any change is made to how pools are opened or
    /// closed.
    ///
    /// `PoolSnapshot::connections` comes from `r2d2::Pool::state`, so it
    /// reflects connections r2d2 has actually opened (r2d2 0.8 eagerly fills
    /// to `min_idle` when a pool is built, which defaults to `max_size`).
    pub fn snapshot(&self) -> ConnectionSnapshot {
        let now = Instant::now();
        let known = self.known_paths.read();

        let pools: Vec<PoolSnapshot> = self
            .pools
            .read()
            .iter()
            .map(|(id, entry)| {
                let state = entry.pool.state();
                PoolSnapshot {
                    id: id.clone(),
                    path: known.get(id).cloned(),
                    connections: state.connections,
                    idle_connections: state.idle_connections,
                    max_size: entry.profile.max_size(),
                    profile: entry.profile,
                    class: entry.class,
                    min_idle: entry.profile.min_idle(),
                    idle_for_ms: now.duration_since(entry.activated_at).as_millis() as u64,
                }
            })
            .collect();

        let counters = SnapshotCounters {
            open_pools: pools.len(),
            max_pools: MAX_POOLS,
            total_connections: pools.iter().map(|p| p.connections).sum(),
            total_idle_connections: pools.iter().map(|p| p.idle_connections).sum(),
            known_paths: known.len(),
            opened_total: self.metrics.opened_total.load(Ordering::Relaxed),
            resurrected_total: self.metrics.resurrected_total.load(Ordering::Relaxed),
            evicted_total: self.evicted_total(),
            evicted_lru: self.metrics.evicted_lru.load(Ordering::Relaxed),
            evicted_idle: self.metrics.evicted_idle.load(Ordering::Relaxed),
            evicted_manual: self.metrics.evicted_manual.load(Ordering::Relaxed),
            evicted_checkpointed: self.metrics.evicted_checkpointed.load(Ordering::Relaxed),
            evicted_uncheckpointed: self.metrics.evicted_uncheckpointed.load(Ordering::Relaxed),
        };

        drop(known);

        let mut pools = pools;
        pools.sort_by(|a, b| a.id.cmp(&b.id));

        ConnectionSnapshot {
            pools,
            counters,
            evictions: self.metrics.ring.lock().iter().cloned().collect(),
        }
    }

    fn evicted_total(&self) -> u64 {
        self.metrics.evicted_lru.load(Ordering::Relaxed)
            + self.metrics.evicted_idle.load(Ordering::Relaxed)
            + self.metrics.evicted_manual.load(Ordering::Relaxed)
    }

    /// Record one eviction. Observational only: it bumps counters and pushes
    /// to the bounded ring, and changes no eviction decision.
    fn record_eviction(&self, id: String, reason: EvictionReason, checkpointed: bool) {
        let counter = match reason {
            EvictionReason::Lru => &self.metrics.evicted_lru,
            EvictionReason::Idle => &self.metrics.evicted_idle,
            EvictionReason::Manual => &self.metrics.evicted_manual,
        };
        counter.fetch_add(1, Ordering::Relaxed);

        if checkpointed {
            self.metrics
                .evicted_checkpointed
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.metrics
                .evicted_uncheckpointed
                .fetch_add(1, Ordering::Relaxed);
        }

        let record = EvictionRecord {
            id,
            reason,
            at_ms: self.epoch.elapsed().as_millis() as u64,
            checkpointed,
        };
        let mut ring = self.metrics.ring.lock();
        if ring.len() >= EVICTION_RING_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(record);
    }

    /// Evict pools that are idle for too long or if we exceed the max capacity.
    ///
    /// Only [`PoolClass::Evictable`] pools are candidates (T4): a
    /// [`PoolClass::Pinned`] pool is daemon infrastructure the process cannot
    /// do without, so the LRU steps over it instead of dropping it. When every
    /// registered pool is pinned and `MAX_POOLS` is already exceeded, this
    /// breaks out and the map is allowed to grow past `MAX_POOLS` — losing
    /// infrastructure is worse than one extra pool.
    fn evict_if_needed(&self) {
        let now = Instant::now();
        let mut pools = self.pools.write();

        // 1. Remove expired pools (checkpoint WAL before removal)
        let expired: Vec<String> = pools
            .iter()
            .filter(|(_, pool)| {
                pool.class.is_evictable()
                    && now.duration_since(pool.activated_at).as_secs() >= self.idle_timeout_secs
            })
            .map(|(k, _)| k.clone())
            .collect();
        for key in expired {
            let mut checkpointed = false;
            if let Some(entry) = pools.get(&key) {
                if let Ok(conn) = entry.pool.get() {
                    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                    checkpointed = true;
                }
            }
            pools.remove(&key);
            self.record_eviction(key, EvictionReason::Idle, checkpointed);
        }

        // 2. If still too many, remove least recently used (checkpoint WAL before removal)
        while pools.len() >= MAX_POOLS {
            let oldest = pools
                .iter()
                .filter(|(_, v)| v.class.is_evictable())
                .map(|(k, v)| (k.clone(), v.activated_at))
                .min_by_key(|e| e.1);

            if let Some((key, _)) = oldest {
                let mut checkpointed = false;
                if let Some(entry) = pools.get(&key) {
                    if let Ok(conn) = entry.pool.get() {
                        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                        checkpointed = true;
                    }
                }
                pools.remove(&key);
                self.record_eviction(key, EvictionReason::Lru, checkpointed);
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codebase::conversations_db::ConversationsDb;
    use tempfile::tempdir;

    /// T3: the snapshot must list every live pool with the database file it
    /// actually owns, so pool/file-descriptor pressure can be measured.
    #[tokio::test]
    async fn snapshot_lists_open_pools_with_real_paths() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();

        let path_a = dir.path().join("snap_a.db");
        let path_b = dir.path().join("snap_b.db");
        cm.connect_with_path("snap_a", path_a.clone()).unwrap();
        cm.connect_with_path("snap_b", path_b.clone()).unwrap();

        let snap = cm.snapshot();

        let ids: Vec<&str> = snap.pools.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["snap_a", "snap_b"],
            "every registered pool must be listed, sorted by id"
        );

        for (id, expected) in [("snap_a", &path_a), ("snap_b", &path_b)] {
            let entry = snap
                .pools
                .iter()
                .find(|p| p.id == id)
                .unwrap_or_else(|| panic!("{id} missing from snapshot"));
            assert_eq!(
                entry.path.as_deref(),
                Some(expected.as_path()),
                "{id} must report the real path it was registered with, not a guess"
            );
            assert!(
                expected.exists(),
                "{id} must point at a file that actually exists"
            );
        }

        assert_eq!(snap.counters.open_pools, 2);
        assert_eq!(snap.counters.max_pools, MAX_POOLS);
        assert_eq!(snap.counters.known_paths, 2);
        assert_eq!(snap.counters.opened_total, 2);
        assert_eq!(snap.counters.evicted_total, 0);
    }

    /// T3: evictions must be attributable by cause. `disconnect` and the
    /// `MAX_POOLS` LRU branch are both reachable deterministically; the idle branch
    /// is not (it needs a 300 s clock), so it is asserted structurally.
    #[tokio::test]
    async fn snapshot_counts_evictions_by_reason() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();

        // Manual: disconnect an open pool.
        cm.connect_with_path("manual_victim", dir.path().join("manual_victim.db"))
            .unwrap();
        cm.disconnect("manual_victim");

        // LRU: fill past MAX_POOLS so evict_if_needed runs the LRU branch.
        // Evicting happens inside connect_with_path, so ids are added in order and
        // the oldest (smallest activated_at) goes first.
        for i in 0..MAX_POOLS {
            cm.connect_with_path(
                &format!("filler_{i:02}"),
                dir.path().join(format!("filler_{i:02}.db")),
            )
            .unwrap();
        }
        // One more connect trips the LRU branch exactly once.
        cm.connect_with_path("lru_trigger", dir.path().join("lru_trigger.db"))
            .unwrap();

        let snap = cm.snapshot();

        assert_eq!(
            snap.counters.evicted_manual, 1,
            "disconnect must count as a manual eviction"
        );
        assert_eq!(
            snap.counters.evicted_lru, 1,
            "exceeding MAX_POOLS must count exactly one LRU eviction"
        );
        assert_eq!(
            snap.counters.evicted_total, 2,
            "evicted_total must be the sum by reason"
        );
        assert_eq!(
            snap.counters.evicted_total,
            snap.counters.evicted_lru + snap.counters.evicted_idle + snap.counters.evicted_manual
        );
        assert_eq!(
            snap.counters.evicted_checkpointed + snap.counters.evicted_uncheckpointed,
            snap.counters.evicted_total,
            "every eviction is checkpointed or explicitly not"
        );
        // `disconnect` does not checkpoint (documented, unchanged behaviour).
        assert_eq!(snap.counters.evicted_uncheckpointed, 1);
        assert_eq!(snap.counters.evicted_idle, 0);

        // The ring attributes each eviction by reason.
        let manual: Vec<&EvictionRecord> = snap
            .evictions
            .iter()
            .filter(|e| e.reason == EvictionReason::Manual)
            .collect();
        assert_eq!(manual.len(), 1);
        assert_eq!(manual[0].id, "manual_victim");
        assert!(!manual[0].checkpointed);
        assert_eq!(
            EvictionReason::Manual.as_str(),
            "manual",
            "reasons must serialize to stable slugs"
        );

        let lru: Vec<&EvictionRecord> = snap
            .evictions
            .iter()
            .filter(|e| e.reason == EvictionReason::Lru)
            .collect();
        assert_eq!(lru.len(), 1, "the LRU victim must be in the ring");
        assert_eq!(
            lru[0].id, "filler_00",
            "the LRU branch evicts the oldest registered pool, which is the first filler"
        );

        // The LRU branch keeps the map at MAX_POOLS or below.
        assert!(
            snap.counters.open_pools <= MAX_POOLS,
            "open_pools {} must respect MAX_POOLS {}",
            snap.counters.open_pools,
            MAX_POOLS
        );
    }

    /// T3: the snapshot must report real r2d2 pool state (connections opened,
    /// idle, `idle_for_ms`) and survive serialization.
    #[tokio::test]
    async fn snapshot_reports_pool_state() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("state.db");

        // No pools: an empty snapshot, not a panic.
        let empty = cm.snapshot();
        assert!(empty.pools.is_empty());
        assert_eq!(empty.counters.open_pools, 0);
        assert_eq!(empty.counters.total_connections, 0);

        cm.connect_with_path("state_probe", db_path.clone())
            .unwrap();

        // r2d2 0.8 fills the pool to `min_idle` (defaults to `max_size`) at build
        // time, so this reports connections that are really open, not a
        // theoretical count.
        let snap = cm.snapshot();
        let pool = snap.pools.iter().find(|p| p.id == "state_probe").unwrap();

        assert_eq!(pool.max_size, POOL_MAX_SIZE);
        assert!(
            pool.connections > 0,
            "r2d2 must have opened connections at pool build, got {}",
            pool.connections
        );
        assert!(
            pool.connections <= POOL_MAX_SIZE,
            "connections {} must not exceed max_size {}",
            pool.connections,
            POOL_MAX_SIZE
        );
        assert_eq!(
            pool.idle_connections, pool.connections,
            "nothing checked a connection out, so every connection is idle"
        );
        assert_eq!(snap.counters.total_connections, pool.connections);
        assert_eq!(snap.counters.total_idle_connections, pool.idle_connections);

        // `idle_for_ms` is measured and monotonic-ish, not a placeholder.
        assert!(
            pool.idle_for_ms < 60_000,
            "idle_for_ms must be a real elapsed measure, got {}",
            pool.idle_for_ms
        );
        std::thread::sleep(Duration::from_millis(25));
        let later = cm.snapshot();
        let pool2 = later.pools.iter().find(|p| p.id == "state_probe").unwrap();
        assert!(
            pool2.idle_for_ms >= pool.idle_for_ms,
            "idle_for_ms must not go backwards ({} then {})",
            pool.idle_for_ms,
            pool2.idle_for_ms
        );

        // Serializable, as an admin endpoint would need.
        let json = serde_json::to_string(&snap).expect("snapshot must serialize");
        assert!(json.contains("state_probe"), "json: {json}");
        assert!(
            json.contains(&db_path.display().to_string()),
            "json: {json}"
        );
        let back: ConnectionSnapshot =
            serde_json::from_str(&json).expect("snapshot must round-trip through JSON");
        assert_eq!(back.counters, snap.counters);
        assert_eq!(back.pools, snap.pools);
    }

    /// Number of open file descriptors of this process, or `None` off Linux.
    fn open_fd_count() -> Option<usize> {
        std::fs::read_dir("/proc/self/fd").ok().map(|d| d.count())
    }

    /// T4: a repo profile must reach the connection *and* the pool builder —
    /// small cache, no mmap, `max_size = 4` — and must not open a single
    /// connection at build time (`min_idle = 0`).
    #[tokio::test]
    async fn repo_profile_applies_cache_mmap_and_pool_size() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("repo_profile.db");

        cm.connect_with_path_profile(
            "repo_store_probe",
            db_path.clone(),
            PoolProfile::RepoStore,
            PoolClass::Evictable,
        )
        .unwrap();

        let snap = cm.snapshot();
        let pool = snap
            .pools
            .iter()
            .find(|p| p.id == "repo_store_probe")
            .unwrap();
        assert_eq!(pool.profile, PoolProfile::RepoStore);
        assert_eq!(pool.class, PoolClass::Evictable);
        assert_eq!(
            pool.max_size, 4,
            "a repo pool must be built with max_size 4, not the daemon-wide 10"
        );
        assert_eq!(
            pool.min_idle,
            Some(0),
            "a repo pool must declare min_idle 0 so r2d2 does not fill it at build"
        );
        assert_eq!(
            pool.connections, 0,
            "r2d2 must not open any connection at build time when min_idle is 0"
        );

        // The PRAGMA overrides must reach a real pooled connection, layered on
        // top of the shared connection pragmas.
        let pragmas = cm
            .with_conn("repo_store_probe", |conn| {
                Ok((
                    conn.query_row("PRAGMA cache_size;", [], |r| r.get::<_, i64>(0))?,
                    conn.query_row("PRAGMA mmap_size;", [], |r| r.get::<_, i64>(0))?,
                    // Untouched by the profile, so the shared layer still holds.
                    conn.query_row("PRAGMA temp_store;", [], |r| r.get::<_, i64>(0))?,
                    conn.query_row("PRAGMA wal_autocheckpoint;", [], |r| r.get::<_, i64>(0))?,
                ))
            })
            .await
            .unwrap();
        assert_eq!(pragmas.0, -2000, "cache_size must come from the profile");
        assert_eq!(pragmas.1, 0, "mmap_size must come from the profile");
        assert_eq!(
            pragmas.2, 2,
            "the shared connection layer must still apply temp_store=MEMORY"
        );
        assert_eq!(
            pragmas.3, 1000,
            "the shared connection layer must still apply wal_autocheckpoint=1000"
        );

        // Lazy is not "broken": checking a connection out opens exactly one.
        let after = cm.snapshot();
        let pool_after = after
            .pools
            .iter()
            .find(|p| p.id == "repo_store_probe")
            .unwrap();
        assert!(
            (1..=pool_after.max_size).contains(&pool_after.connections),
            "on-demand opening must land between 1 and max_size, got {}",
            pool_after.connections
        );
    }

    /// T4: `PoolProfile::Default` must be byte-identical to the pre-T4 pool:
    /// same `max_size`, same eagerly-filled `min_idle`, same pragmas, and the
    /// same eviction class for callers of `connect_with_path`.
    #[tokio::test]
    async fn default_profile_is_byte_identical_to_today() {
        // The numbers that were hardcoded before T4.
        assert_eq!(PoolProfile::Default.max_size(), 10);
        assert_eq!(PoolProfile::Default.min_idle(), Some(10));
        assert_eq!(PoolProfile::Default.cache_size(), -8000);
        assert_eq!(PoolProfile::Default.mmap_size(), 268_435_456);
        assert_eq!(PoolProfile::default(), PoolProfile::Default);

        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();

        cm.connect_with_path("legacy_default", dir.path().join("legacy_default.db"))
            .unwrap();

        let snap = cm.snapshot();
        let pool = snap
            .pools
            .iter()
            .find(|p| p.id == "legacy_default")
            .unwrap();
        assert_eq!(pool.profile, PoolProfile::Default);
        assert_eq!(pool.max_size, POOL_MAX_SIZE);
        assert_eq!(pool.min_idle, Some(POOL_MAX_SIZE));
        assert_eq!(
            pool.connections, POOL_MAX_SIZE,
            "today's pools open max_size connections at build time; that must not change"
        );
        assert_eq!(
            pool.class,
            PoolClass::Evictable,
            "connect_with_path must keep today's class so no existing caller changes"
        );

        let pragmas = cm
            .with_conn("legacy_default", |conn| {
                Ok((
                    conn.query_row("PRAGMA cache_size;", [], |r| r.get::<_, i64>(0))?,
                    conn.query_row("PRAGMA mmap_size;", [], |r| r.get::<_, i64>(0))?,
                ))
            })
            .await
            .unwrap();
        assert_eq!(pragmas.0, -8000);
        assert_eq!(pragmas.1, 268_435_456);
    }

    /// T4: the LRU must never choose a pinned pool, no matter how old it is or
    /// how far past `MAX_POOLS` the map is. A daemon infrastructure pool losing
    /// its connections is worse than the map growing.
    #[tokio::test]
    async fn eviction_never_evicts_pinned_pools() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();

        // Infrastructure: oldest pool in the map, but pinned.
        cm.connect_with_path_profile(
            "memory",
            dir.path().join("memory.db"),
            PoolProfile::Default,
            PoolClass::Pinned,
        )
        .unwrap();

        // Fill well past MAX_POOLS with evictable repo pools.
        for i in 0..MAX_POOLS + 4 {
            cm.connect_with_path_profile(
                &format!("repo_{i:02}"),
                dir.path().join(format!("repo_{i:02}.db")),
                PoolProfile::RepoStore,
                PoolClass::Evictable,
            )
            .unwrap();
        }

        let snap = cm.snapshot();
        assert!(
            snap.pools.iter().any(|p| p.id == "memory"),
            "the pinned pool must survive {MAX_POOLS}+4 registrations"
        );
        assert_eq!(cm.pool_class("memory"), Some(PoolClass::Pinned));
        assert!(
            !snap
                .evictions
                .iter()
                .any(|e| e.id == "memory" && e.reason == EvictionReason::Lru),
            "no LRU record may name a pinned pool: {:?}",
            snap.evictions
        );

        // And the LRU still works among the evictable ones.
        assert!(
            snap.counters.evicted_lru >= 4,
            "evictable pools must still be evicted, got {}",
            snap.counters.evicted_lru
        );

        // With only pinned pools registered the map is allowed past MAX_POOLS
        // rather than dropping infrastructure.
        let all_pinned = ConnectionManager::new();
        for i in 0..MAX_POOLS + 2 {
            all_pinned
                .connect_with_path_profile(
                    &format!("infra_{i:02}"),
                    dir.path().join(format!("infra_{i:02}.db")),
                    PoolProfile::Default,
                    PoolClass::Pinned,
                )
                .unwrap();
        }
        assert_eq!(
            all_pinned.snapshot().pools.len(),
            MAX_POOLS + 2,
            "an all-pinned map grows instead of evicting infrastructure"
        );
    }

    /// T4: 16 pools of the repo profile must cost far fewer descriptors than 16
    /// pools of the default profile. This is the before/after measurement for
    /// the r2d2 `min_idle` fix.
    #[tokio::test]
    async fn repo_profile_sixteen_pools_cost_far_fewer_fds_than_default() {
        if open_fd_count().is_none() {
            eprintln!("skipped: /proc/self/fd is not readable on this platform");
            return;
        }

        let dir = tempdir().unwrap();

        // Before: 16 pools of today's profile, filled to min_idle at build.
        let before_cm = ConnectionManager::new();
        for i in 0..MAX_POOLS {
            before_cm
                .connect_with_path(
                    &format!("before_{i:02}"),
                    dir.path().join(format!("before_{i:02}.db")),
                )
                .unwrap();
        }
        let fds_before = open_fd_count().unwrap();
        let snap_before = before_cm.snapshot();
        assert_eq!(snap_before.pools.len(), MAX_POOLS);
        let conns_before = snap_before.counters.total_connections;
        assert_eq!(
            conns_before,
            (MAX_POOLS as u32) * POOL_MAX_SIZE,
            "today's 16 pools hold 16 x POOL_MAX_SIZE connections"
        );
        before_cm.shutdown();
        drop(before_cm);

        // After: 16 repo pools, no connection opened until one is requested.
        let after_cm = ConnectionManager::new();
        for i in 0..MAX_POOLS {
            after_cm
                .connect_with_path_profile(
                    &format!("after_{i:02}"),
                    dir.path().join(format!("after_{i:02}.db")),
                    PoolProfile::RepoStore,
                    PoolClass::Evictable,
                )
                .unwrap();
        }
        let fds_after = open_fd_count().unwrap();
        let snap_after = after_cm.snapshot();
        assert_eq!(snap_after.pools.len(), MAX_POOLS);
        assert_eq!(
            snap_after.counters.total_connections, 0,
            "16 repo pools must hold zero connections until they are used"
        );

        eprintln!(
            "T4 fd measurement with {MAX_POOLS} pools: before={fds_before} fds for {conns_before} connections, after={fds_after} fds for {} connections",
            snap_after.counters.total_connections
        );

        // 16 repo pools that were never queried must hold no database fds, so
        // the descriptor count cannot have grown.
        assert!(
            fds_after <= fds_before,
            "repo profile must not increase descriptors: {fds_before} -> {fds_after}"
        );
        after_cm.shutdown();
    }

    #[tokio::test]
    async fn test_connection_manager() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let project_root = dir.path().to_str().unwrap();

        cm.connect("p1", project_root).unwrap();
        cm.connect("p2", project_root).unwrap();
        assert_eq!(cm.pools.read().len(), 2);
        assert!(cm.pools.read().contains_key("p1"));
        assert!(cm.pools.read().contains_key("p2"));
    }

    /// Pool acquires must still hand out fully configured connections after the
    /// pragma split: the one-time layer is applied when the pool is built and the
    /// cheap acquire layer on every checkout.
    #[tokio::test]
    async fn test_pooled_connections_carry_both_pragma_layers() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("pragma_layers.db");

        cm.connect_with_path("pragma_layers", db_path).unwrap();

        let pragmas = cm
            .with_conn("pragma_layers", |conn| {
                Ok((
                    conn.query_row("PRAGMA busy_timeout;", [], |r| r.get::<_, i64>(0))?,
                    conn.query_row("PRAGMA foreign_keys;", [], |r| r.get::<_, i64>(0))?,
                    conn.query_row("PRAGMA cache_size;", [], |r| r.get::<_, i64>(0))?,
                    conn.query_row("PRAGMA temp_store;", [], |r| r.get::<_, i64>(0))?,
                ))
            })
            .await
            .unwrap();

        // Acquire layer.
        assert_eq!(pragmas.0, 5000, "busy_timeout must be applied on acquire");
        assert_eq!(pragmas.1, 1, "foreign_keys must be applied on acquire");
        // One-time layer.
        assert_eq!(pragmas.2, -8000, "cache_size must be set at pool build");
        assert_eq!(pragmas.3, 2, "temp_store=MEMORY must be set at pool build");
    }

    /// Regression for issue #2544: a *new* write connection opened through
    /// `ConnectionManager` (the same mechanism `VecSqliteMemoryStore` and the
    /// MCP `create_memory` write path use) must have the sqlite-vec extension
    /// (`vec_f32`, `vec_distance_cosine`, ...) available — not just the
    /// connection that happened to be open at boot.
    ///
    /// Mirrors production ordering: register the extension via
    /// `sqlite3_auto_extension` (idempotent, safe to call from every test in
    /// this binary), *then* open a brand-new project pool + connection and
    /// exercise a real `vec_f32(...)` call, matching the embedding INSERT in
    /// `sqlite_vec_store`.
    #[tokio::test]
    async fn test_fresh_write_connection_has_vec_f32() {
        crate::memory::sqlite_vec_store::vector::register_sqlite_vec_extension()
            .expect("sqlite-vec extension registration must succeed");

        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("vec_f32_write_conn.sqlite3");

        cm.connect_with_path("vec_f32_write_conn", db_path).unwrap();

        let version: String = cm
            .with_conn("vec_f32_write_conn", |conn| {
                Ok(conn.query_row("SELECT vec_version()", [], |row| row.get(0))?)
            })
            .await
            .expect("vec_version() must resolve on a fresh connection from a fresh pool");
        assert!(
            version.starts_with('v'),
            "unexpected vec_version() output: {version}"
        );

        // The exact call shape used on the memory-write path
        // (`INSERT ... VALUES (?1, ?2, vec_f32(?3))`): a real vec_f32() call
        // against a freshly opened write connection must not error with
        // "no such function: vec_f32".
        let blob: Vec<u8> = cm
            .with_conn("vec_f32_write_conn", |conn| {
                let embedding_json = serde_json::to_string(&[0.1_f32, 0.2, 0.3]).unwrap();
                conn.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v BLOB)", [])?;
                conn.execute(
                    "INSERT INTO t (v) VALUES (vec_f32(?1))",
                    rusqlite::params![embedding_json],
                )?;
                Ok(conn.query_row("SELECT v FROM t WHERE id = 1", [], |row| row.get(0))?)
            })
            .await
            .expect("vec_f32() insert must succeed on a fresh write connection");
        assert_eq!(blob.len(), 3 * std::mem::size_of::<f32>());
    }

    #[tokio::test]
    async fn test_get_code_graph_db_and_shutdown() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("code_graph_cm.db");

        let cg1 = cm.get_code_graph_db(&db_path).unwrap();
        let cg2 = cm.get_code_graph_db(&db_path).unwrap();

        let sym = code_graph::types::Symbol {
            id: None,
            stable_id: None,
            name: "cm_test_symbol".to_string(),
            kind: code_graph::types::SymbolKind::Function,
            lang: code_graph::types::Language::Rust,
            file_path: "src/cm_test.rs".to_string(),
            start_line: 1,
            end_line: 2,
            start_col: 0,
            end_col: 0,
            signature: None,
            parent: None,
            complexity: None,
        };
        cg1.insert_symbol(&sym).unwrap();
        let found = cg2.find_by_name("cm_test_symbol", 1).unwrap();
        assert_eq!(found.len(), 1);

        cm.shutdown();
    }

    #[tokio::test]
    async fn test_active_project() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let project_root = dir.path().to_str().unwrap();

        cm.set_active("p1", project_root).await.unwrap();

        let res = cm
            .with_active(|conn| {
                conn.execute("CREATE TABLE IF NOT EXISTS t(id INT)", [])?;
                Ok(())
            })
            .await;

        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_eviction_and_resurrection() {
        let cm = ConnectionManager::new();
        let dir = tempdir().unwrap();
        let project_root = dir.path().to_str().unwrap();

        cm.connect("conv_test_resurrect", project_root).unwrap();
        assert!(cm.pools.read().contains_key("conv_test_resurrect"));

        // Force eviction
        cm.disconnect("conv_test_resurrect");
        assert!(!cm.pools.read().contains_key("conv_test_resurrect"));

        // with_conn should automatically reconnect / resurrect
        let res = cm
            .with_conn("conv_test_resurrect", |conn| {
                conn.execute("CREATE TABLE IF NOT EXISTS t_res(id INT)", [])?;
                Ok(())
            })
            .await;

        assert!(res.is_ok());
        assert!(cm.pools.read().contains_key("conv_test_resurrect"));
    }

    /// Regression for #2736: an id that would fall through to the catch-all
    /// `./.xavier/codebase.db` guess (e.g. a hashed `vec_store_<sha>` id) must
    /// never be silently resurrected against a guessed path. It has to be
    /// registered via `connect()`/`connect_with_path()` first.
    #[tokio::test]
    async fn test_get_or_reconnect_pool_rejects_unregistered_catchall_id() {
        let cm = ConnectionManager::new();

        let err = cm
            .get_or_reconnect_pool("vec_store_deadbeefcafe")
            .expect_err("a hashed vec_store id must never be guessed via the catch-all path");
        assert!(
            err.to_string().contains("was never established"),
            "unexpected error: {err}"
        );
    }

    /// Regression for the #2736 REPAIR: ids `connect()` maps through an
    /// explicit branch (here, the `conv_*` prefix used by
    /// `ConversationsDb::new_test()`'s `conv_test_default`) must keep
    /// connecting lazily on first use, without a prior `connect()` /
    /// `connect_with_path()` call.
    ///
    /// `get_or_reconnect_pool`'s lazy branch resurrects against
    /// `project_root = "."` (same as before #2736), so an unregistered id
    /// would write `.xavier/tests/<id>.db` under the crate root like other
    /// `conv_test_*` ids already do at runtime. A test must not leave that
    /// behind, so the id is first bound to a tempdir path; the pool is then
    /// dropped so the resurrection branch really runs, and it must rebuild
    /// from the registered tempdir path, never from the `.` guess.
    #[tokio::test]
    async fn test_get_or_reconnect_pool_lazily_connects_conv_prefixed_id() {
        let cm = ConnectionManager::new();
        let project_id = "conv_test_lazy_default_regression";
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join(format!("{}.db", project_id));

        cm.connect_with_path(project_id, db_path.clone()).unwrap();
        // Drop the pool but keep the `known_paths` registration, so the next
        // call has to take the resurrect branch.
        cm.disconnect(project_id);

        let pool = cm
            .get_or_reconnect_pool(project_id)
            .expect("a conv_*-prefixed id must connect lazily without prior registration");
        assert!(pool.get().is_ok());
        assert!(
            db_path.exists(),
            "the resurrected pool must reopen the registered tempdir path"
        );

        // The repo must not have gained a `.xavier/tests` artefact.
        let leaked = PathBuf::from(".")
            .join(".xavier")
            .join("tests")
            .join(format!("{}.db", project_id));
        assert!(
            !leaked.exists(),
            "the lazy resurrection leaked {} into the repo",
            leaked.display()
        );

        drop(pool);
        cm.shutdown();
    }

    /// Regression for the `.xavier/tests` hygiene failure: the conversations
    /// test store must stay in the temp dir and must never write into the
    /// repository.
    ///
    /// Before the fix, `ConnectionsDb`/`ConversationsDb::open_in_memory` and
    /// `new_test` registered ids `conv_test_*` via `connect(id, ".")`, and the
    /// `conv_test_` branch mapped those to `<cwd>/.xavier/tests/<id>.db`. With
    /// `cargo test` running from the crate root, **cwd is the repository**, so
    /// every run dropped a `conv_test_<ULID>.db` + `-shm` + `-wal` trio in the
    /// working tree — 27.618 files / 825 MB had piled up there.
    ///
    /// This drives the real entry points (not just the path helper) and asserts
    /// three things:
    ///   1. `new_test()`'s id resolves outside the repo, into the temp root.
    ///   2. `open_in_memory()`'s id does too, and actually creating the schema
    ///      (which is what opens the file and the WAL) writes nothing into
    ///      `.xavier/tests`.
    ///   3. A production id (`conv_<workspace>`) is unaffected, so the fix did
    ///      not move real conversations databases into the temp dir.
    #[tokio::test]
    async fn conversations_test_store_stays_in_tempdir() {
        // Resolve against a tempdir repo root so the assertion below is about
        // the *mapping*, not about whatever the crate cwd happens to be.
        let repo_root = tempfile::tempdir().unwrap();
        let project_root = repo_root.path().to_str().unwrap();

        // 1. `ConversationsDb::new_test()`'s id (built by hand here so this
        //    module does not depend on ConversationsDb; the prefix is the
        //    contract, asserted literally).
        let new_test_id = "conv_test_default";
        let new_test_path = resolve_db_path(new_test_id, project_root).unwrap();
        let tmp_root =
            std::fs::canonicalize(test_store_root().expect("test build always has a temp root"))
                .unwrap();
        assert!(
            std::fs::canonicalize(&new_test_path)
                .unwrap_or_else(|_| new_test_path.clone())
                .starts_with(&tmp_root),
            "conversations_test_store_stays_in_tempdir: {} must resolve inside the \
             temp root {}, not into the repo",
            new_test_path.display(),
            tmp_root.display()
        );

        // 2. The real ConversationsDb entry point, driven end to end.
        //    `open_in_memory` appends the `conv_test_` prefix itself and
        //    registers through the *global* manager, so this is the path a
        //    leaked file would actually be written to.
        let workspace_id = format!("storehygiene_{}", ulid::Ulid::new());
        let full_id = format!("conv_test_{}", workspace_id);
        let db = ConversationsDb::open_in_memory(&workspace_id)
            .await
            .unwrap();
        db.create_schema().await.unwrap();

        let registered = crate::codebase::connection_manager::ConnectionManager::global()
            .known_paths
            .read()
            .get(&full_id)
            .cloned()
            .expect("open_in_memory must register its pool path");
        assert!(
            registered.starts_with(&tmp_root),
            "conversations_test_store_stays_in_tempdir: {} resolved to {} which is outside \
             the temp root {} — it would be written inside the repo",
            full_id,
            registered.display(),
            tmp_root.display()
        );
        assert!(
            registered.exists(),
            "the pool did not create {} at all",
            registered.display()
        );

        // Nothing at all may appear under the cwd-relative repo path.
        let leaked = PathBuf::from(".")
            .join(".xavier")
            .join("tests")
            .join(format!("{}.db", full_id));
        assert!(
            !leaked.exists(),
            "conversations_test_store_stays_in_tempdir: ConversationsDb wrote {} into the \
             repo tree",
            leaked.display()
        );

        // 3. A production conversations id must NOT be redirected to temp.
        let prod_id = "conv_some_workspace";
        let prod_path = resolve_db_path(prod_id, project_root).unwrap();
        assert!(
            !std::fs::canonicalize(&prod_path)
                .unwrap_or_else(|_| prod_path.clone())
                .starts_with(&tmp_root),
            "a real conv_<workspace> database must not resolve inside the test temp root"
        );
        assert!(
            prod_path.ends_with("conversations/some_workspace.db"),
            "production conversations path changed unexpectedly: {}",
            prod_path.display()
        );

        // Sanity: the helpers we rely on agree about which ids are test-only.
        assert!(is_test_only_id(new_test_id));
        assert!(is_test_only_id("test_metrics_01ABC"));
        assert!(!is_test_only_id(prod_id));
    }

    /// Guards the *mechanism*, not just one id: any `conv_test_*` or `test_*`
    /// id handed to `connect()` must land in the temp root, never under the
    /// project root's `.xavier/tests`. This is what keeps
    /// `ls .xavier/tests | wc -l` from growing on every `cargo test`.
    #[tokio::test]
    async fn test_only_ids_never_resolve_under_project_root() {
        let repo_root = tempfile::tempdir().unwrap();
        let project_root = repo_root.path().to_str().unwrap();
        let tmp_root =
            std::fs::canonicalize(test_store_root().expect("test build always has a temp root"))
                .unwrap();

        for id in [
            "conv_test_default",
            "conv_test_test-crud",
            "conv_test_add-fastpath-01ABCDEF",
            "test_metrics_01ABCDEF",
            "test_quota_01ABCDEF",
        ] {
            let path = resolve_db_path(id, project_root).unwrap();
            let normalised = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            assert!(
                normalised.starts_with(&tmp_root),
                "id {id} resolved to {} which is outside the temp root {}",
                path.display(),
                tmp_root.display()
            );
            assert!(
                !normalised.starts_with(repo_root.path()),
                "id {id} resolved into the repo at {}",
                normalised.display()
            );
        }
    }

    /// The `connect()` path itself must honour the override end to end, pool
    /// included: this is the code path a real conversations test takes.
    #[tokio::test]
    async fn connect_of_conv_test_id_creates_files_only_in_temp_root() {
        let cm = ConnectionManager::new();
        let repo_root = tempfile::tempdir().unwrap();
        let project_root = repo_root.path().to_str().unwrap();
        let project_id = format!("conv_test_connecthygiene_{}", ulid::Ulid::new());

        cm.connect(&project_id, project_root).unwrap();

        // The registered path is inside the temp root...
        let registered = cm.known_paths.read().get(&project_id).cloned().unwrap();
        let tmp_root =
            std::fs::canonicalize(test_store_root().expect("test build always has a temp root"))
                .unwrap();
        assert!(
            registered.starts_with(&tmp_root),
            "known_paths must hold the temp-root path, got {}",
            registered.display()
        );
        // ...and the pool really created it there, with no file in the repo.
        assert!(registered.exists(), "the pool did not create its db file");
        let leaked_dir = repo_root.path().join(".xavier").join("tests");
        let in_repo: Vec<_> = std::fs::read_dir(&leaked_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            in_repo.is_empty(),
            "connect({project_id}) leaked {:?} into {}",
            in_repo,
            leaked_dir.display()
        );

        cm.shutdown();
    }
}
