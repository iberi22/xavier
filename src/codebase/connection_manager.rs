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
    /// `max_size` the pool was built with.
    pub max_size: u32,
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
    pub fn connect(&self, project_id: &str, project_root: &str) -> Result<()> {
        if !self.pools.read().contains_key(project_id) {
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
                PathBuf::from(project_root)
                    .join(".xavier")
                    .join("tests")
                    .join(format!("{}.db", project_id))
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
                PathBuf::from(project_root)
                    .join(".xavier")
                    .join("tests")
                    .join(format!("{}.db", project_id))
            } else {
                PathBuf::from(project_root)
                    .join(".xavier")
                    .join("codebase.db")
            };

            self.connect_with_path(project_id, db_path)
        } else {
            // Update last accessed time
            if let Some(entry) = self.pools.write().get_mut(project_id) {
                entry.activated_at = Instant::now();
            }
            Ok(())
        }
    }

    /// Explicitly connect to a database file with a given project_id.
    pub fn connect_with_path(&self, project_id: &str, db_path: PathBuf) -> Result<()> {
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
            let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
                conn.apply_connection_pragmas()
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
            });
            let pool = Pool::builder()
                .max_size(POOL_MAX_SIZE)
                .connection_customizer(Box::new(PragmaCustomizer))
                .build(manager)
                .context("failed to build r2d2 SQLite pool")?;

            self.evict_if_needed();

            self.pools.write().insert(
                project_id.to_string(),
                ProjectPool {
                    pool: Arc::new(pool),
                    activated_at: Instant::now(),
                },
            );
            self.metrics.opened_total.fetch_add(1, Ordering::Relaxed);
        } else if let Some(entry) = self.pools.write().get_mut(project_id) {
            entry.activated_at = Instant::now();
        }
        Ok(())
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
                    max_size: POOL_MAX_SIZE,
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
    fn evict_if_needed(&self) {
        let now = Instant::now();
        let mut pools = self.pools.write();

        // 1. Remove expired pools (checkpoint WAL before removal)
        let expired: Vec<String> = pools
            .iter()
            .filter(|(_, pool)| {
                now.duration_since(pool.activated_at).as_secs() >= self.idle_timeout_secs
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
}
