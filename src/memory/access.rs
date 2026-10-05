//! Phase 1 access instrumentation: the evidence a utility prune needs.
//!
//! ## Why this module exists
//!
//! `TgdUtilityPruner` (`src/memory/tgd.rs`) asks "was this ever needed?" and
//! `DecayManager` (`src/memory/decay.rs`) asks "when was this last touched?".
//! Both used to answer from `MemoryRecord::metadata["last_accessed_at"]`. On the
//! production database that key does not exist: the only key present in all
//! 16,771 rows is `{"encrypted": ...}`. So the pruner could not tell
//! "nobody needs this" apart from "nobody has asked yet", and pruning on that
//! basis is a guess — an irreversible one.
//!
//! This module supplies the missing signal:
//!
//! 1. [`record_user_access`] is called from the **HTTP/MCP read handlers**
//!    only — never from `MemoryStore::get`. Internal reads (consolidation,
//!    GC, backup, ingestion, the pruner itself) must not count as "use", or
//!    the signal is born biased towards whatever the system itself touches.
//! 2. Accesses land in a dedicated `memory_access` table (counter + first/last
//!    timestamps) instead of the encrypted metadata blob.
//! 3. [`get_last_accessed_observed`] resolves recency from that table and only
//!    falls back to metadata for records that predate instrumentation.
//! 4. [`ensure_instrumentation`] writes `observation_started_at` into
//!    `maintenance_meta`, so a prune can require a full observation window
//!    ([`MIN_OBSERVATION_DAYS`]) before anything irreversible is even
//!    *considered*.
//!
//! ## Write amplification
//!
//! A top-k search touches up to `k` records. One `INSERT` per read would put
//! the instrumented path at one write per row returned, which is worse than
//! the read itself and would serialise searches behind the write lock.
//! Instead, accesses accumulate in a process-local buffer
//! ([`RECORDER`]) and are flushed as a single multi-row upsert when either
//! [`DEFAULT_FLUSH_BATCH`] distinct records are pending or
//! [`DEFAULT_FLUSH_INTERVAL`] has elapsed — one statement per flush, not per
//! read. The in-memory [`AccessStats`] totals are updated synchronously on
//! record, so recency queries are correct even before a flush lands.
//!
//! ## Bias to keep in mind when this signal is finally used for pruning
//!
//! Utility pruning rewards what is already visible. A record that never enters
//! a top-k looks unused; a record with no embedding can never enter one. See
//! [`record_is_observable`] — only records carrying an embedding may ever
//! become prune candidates, and archive must always be preferred over delete.
//! This module deliberately stops at instrumentation and the clock: it performs
//! no deletion and exposes no route that could.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::memory::decay::get_last_accessed;
use crate::memory::manager::MemoryManager;
use crate::memory::qmd_memory::{MemoryDocument, QmdMemory};
use crate::memory::store::{MemoryRecord, MemoryStore};

#[cfg(test)]
#[path = "access_tests.rs"]
mod tests;

/// `maintenance_meta` key holding the instant the access observation window opened.
pub const OBSERVATION_STARTED_AT_KEY: &str = "observation_started_at";

/// Calendar days of observation required before a utility prune may delete anything.
///
/// The point is not "30 days is enough" — it is that a prune must never run
/// against a signal that did not exist when the oldest candidate was written.
/// Without a clock, "never accessed" and "never observable" are the same string.
pub const MIN_OBSERVATION_DAYS: i64 = 30;

/// Pending distinct records that trigger a flush.
pub const DEFAULT_FLUSH_BATCH: usize = 256;

/// Maximum time an access may sit in the buffer before a flush.
pub const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Stored access statistics
// ---------------------------------------------------------------------------

/// Durable access signal for one memory record.
///
/// Deliberately narrow: an id, a counter and two timestamps. No content, no
/// metadata, nothing that would need the at-rest encryption columns of
/// `memory_records`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessStats {
    pub access_count: u64,
    pub first_accessed_at: Option<DateTime<Utc>>,
    pub last_accessed_at: Option<DateTime<Utc>>,
}

impl AccessStats {
    fn apply_delta(&mut self, delta: u64, now: DateTime<Utc>) {
        self.access_count = self.access_count.saturating_add(delta);
        if self.first_accessed_at.is_none() {
            self.first_accessed_at = Some(now);
        }
        self.last_accessed_at = Some(now);
    }
}

type AccessKey = (String, String);

/// Process-local buffer plus in-memory view of the durable access signal.
#[derive(Debug)]
pub struct AccessRecorder {
    pending: Mutex<HashMap<AccessKey, u64>>,
    totals: Mutex<HashMap<AccessKey, AccessStats>>,
    hydrated: Mutex<HashSet<String>>,
    last_flush: Mutex<Instant>,
    /// Consecutive flush failures. Drives the backoff in `should_flush`.
    flush_failures: AtomicU32,
    /// Instant of the last flush *attempt*, successful or not.
    last_attempt: Mutex<Instant>,
    hydrated_at_least_once: AtomicBool,
}

/// Backoff ceiling after repeated flush failures: 2^7 * interval.
const MAX_FLUSH_BACKOFF_SHIFT: u32 = 7;

/// Process-wide recorder. One per process, keyed by `(workspace_id, memory_id)`.
pub static RECORDER: LazyLock<AccessRecorder> = LazyLock::new(|| AccessRecorder {
    pending: Mutex::new(HashMap::new()),
    totals: Mutex::new(HashMap::new()),
    hydrated: Mutex::new(HashSet::new()),
    last_flush: Mutex::new(Instant::now()),
    flush_failures: AtomicU32::new(0),
    last_attempt: Mutex::new(Instant::now()),
    hydrated_at_least_once: AtomicBool::new(false),
});

impl Default for AccessRecorder {
    fn default() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            totals: Mutex::new(HashMap::new()),
            hydrated: Mutex::new(HashSet::new()),
            last_flush: Mutex::new(Instant::now()),
            flush_failures: AtomicU32::new(0),
            last_attempt: Mutex::new(Instant::now()),
            hydrated_at_least_once: AtomicBool::new(false),
        }
    }
}

impl AccessRecorder {
    /// Buffer one access per id for `workspace_id` and update the in-memory view.
    ///
    /// Returns the number of distinct records recorded.
    pub fn record(&self, workspace_id: &str, ids: &[String], now: DateTime<Utc>) -> usize {
        if ids.is_empty() {
            return 0;
        }
        {
            let mut pending = self.pending.lock().expect("access: pending lock poisoned");
            for id in ids {
                *pending
                    .entry((workspace_id.to_string(), id.clone()))
                    .or_insert(0) += 1;
            }
        }
        {
            let mut totals = self.totals.lock().expect("access: totals lock poisoned");
            for id in ids {
                totals
                    .entry((workspace_id.to_string(), id.clone()))
                    .or_default()
                    .apply_delta(1, now);
            }
        }
        ids.len()
    }

    /// Current in-memory access count for a record (0 when never observed).
    pub fn access_count(&self, workspace_id: &str, memory_id: &str) -> u64 {
        self.totals
            .lock()
            .expect("access: totals lock poisoned")
            .get(&(workspace_id.to_string(), memory_id.to_string()))
            .map(|stats| stats.access_count)
            .unwrap_or(0)
    }

    /// Current in-memory access statistics for a record.
    pub fn stats(&self, workspace_id: &str, memory_id: &str) -> Option<AccessStats> {
        self.totals
            .lock()
            .expect("access: totals lock poisoned")
            .get(&(workspace_id.to_string(), memory_id.to_string()))
            .cloned()
    }

    /// Seed the in-memory view from durable storage (once per workspace).
    ///
    /// Returns the number of records seeded; 0 means this workspace was already
    /// hydrated, in which case the caller's data is discarded on purpose so a
    /// restart cannot double-count into a snapshot older than the live buffer.
    pub fn hydrate(&self, workspace_id: &str, stats: Vec<(String, AccessStats)>) -> usize {
        let mut hydrated = self
            .hydrated
            .lock()
            .expect("access: hydrated lock poisoned");
        if !hydrated.insert(workspace_id.to_string()) {
            return 0;
        }
        self.hydrated_at_least_once.store(true, Ordering::Release);
        let mut totals = self.totals.lock().expect("access: totals lock poisoned");
        let mut seeded = 0;
        for (memory_id, stats) in stats {
            totals.insert((workspace_id.to_string(), memory_id), stats);
            seeded += 1;
        }
        seeded
    }

    /// Number of distinct records with an unflushed access.
    pub fn pending_count(&self) -> usize {
        self.pending
            .lock()
            .expect("access: pending lock poisoned")
            .len()
    }

    /// Whether the buffer is due for a flush.
    ///
    /// Two gates. The batch gate is traffic-driven and unaffected by failures —
    /// a full buffer must still be attempted, otherwise a dead store would grow
    /// the buffer without bound. The time gate is multiplied by an exponential
    /// backoff derived from `flush_failures`, so a persistently failing store
    /// stops turning every single read into another write attempt (the D5
    /// symptom: `mark_flushed` was never reached on the failure path, so the
    /// interval never advanced and the next read retried immediately).
    pub fn should_flush(&self) -> bool {
        let pending = self.pending_count();
        if pending == 0 {
            return false;
        }
        if pending >= DEFAULT_FLUSH_BATCH {
            return true;
        }
        self.last_attempt
            .lock()
            .expect("access: last_attempt lock poisoned")
            .elapsed()
            >= self.backoff()
    }

    /// Current backoff window: `DEFAULT_FLUSH_INTERVAL` doubled per consecutive
    /// failure, capped at 128x (so ~64 min at the default interval).
    pub fn backoff(&self) -> Duration {
        let failures = self.flush_failures.load(Ordering::Acquire);
        if failures == 0 {
            return DEFAULT_FLUSH_INTERVAL;
        }
        let shift = failures.min(MAX_FLUSH_BACKOFF_SHIFT);
        DEFAULT_FLUSH_INTERVAL * 2u32.saturating_pow(shift)
    }

    /// Consecutive flush failures so far (0 right after a success).
    pub fn flush_failures(&self) -> u32 {
        self.flush_failures.load(Ordering::Acquire)
    }

    /// A flush succeeded: reset the backoff and move the interval clock.
    pub fn register_flush_success(&self) {
        self.flush_failures.store(0, Ordering::Release);
        self.mark_flushed();
    }

    /// A flush failed: move the attempt clock so the next read waits out the
    /// backoff instead of retrying immediately.
    pub fn register_flush_failure(&self) {
        self.flush_failures.fetch_add(1, Ordering::AcqRel);
        self.mark_attempt();
    }

    /// Take every pending access, grouped by workspace.
    pub fn drain_pending(&self) -> HashMap<String, Vec<(String, u64)>> {
        let mut pending = self.pending.lock().expect("access: pending lock poisoned");
        let drained = std::mem::take(&mut *pending);
        let mut grouped: HashMap<String, Vec<(String, u64)>> = HashMap::new();
        for ((workspace_id, memory_id), delta) in drained {
            grouped
                .entry(workspace_id)
                .or_default()
                .push((memory_id, delta));
        }
        grouped
    }

    /// Take the pending accesses of the workspaces `keep` accepts; every other
    /// workspace stays queued untouched.
    fn drain_pending_where(
        &self,
        mut keep: impl FnMut(&str) -> bool,
    ) -> HashMap<String, Vec<(String, u64)>> {
        let mut pending = self.pending.lock().expect("access: pending lock poisoned");
        let mut grouped: HashMap<String, Vec<(String, u64)>> = HashMap::new();
        pending.retain(|(workspace_id, memory_id), delta| {
            if keep(workspace_id) {
                grouped
                    .entry(workspace_id.clone())
                    .or_default()
                    .push((memory_id.clone(), *delta));
                false
            } else {
                true
            }
        });
        grouped
    }

    /// Distinct workspace ids that have an unflushed access.
    fn pending_workspaces(&self) -> Vec<String> {
        let pending = self.pending.lock().expect("access: pending lock poisoned");
        let set: HashSet<&String> = pending
            .keys()
            .map(|(workspace_id, _)| workspace_id)
            .collect();
        set.into_iter().cloned().collect()
    }

    /// Drop the unflushed accesses of one workspace (its store is gone).
    fn discard_workspace(&self, workspace_id: &str) {
        self.pending
            .lock()
            .expect("access: pending lock poisoned")
            .retain(|(ws, _), _| ws != workspace_id);
    }

    /// Put drained accesses back (flush failed; they must not be lost).
    pub fn restore_pending(&self, drained: Vec<(String, String, u64)>) {
        let mut pending = self.pending.lock().expect("access: pending lock poisoned");
        for (workspace_id, memory_id, delta) in drained {
            *pending.entry((workspace_id, memory_id)).or_insert(0) += delta;
        }
    }

    /// Flatten the buffer back into `(workspace_id, memory_id, delta)` triples.
    pub fn flatten(drained: &HashMap<String, Vec<(String, u64)>>) -> Vec<(String, String, u64)> {
        drained
            .iter()
            .flat_map(|(workspace_id, entries)| {
                entries.iter().map(move |(memory_id, delta)| {
                    (workspace_id.clone(), memory_id.clone(), *delta)
                })
            })
            .collect()
    }

    /// Read-only copy of the buffer, grouped by workspace.
    ///
    /// Diagnostic/test helper: it never drains, so a test can assert *which*
    /// accesses survived a failed flush without perturbing the state it asserts
    /// on.
    pub fn pending_snapshot(&self) -> HashMap<String, Vec<(String, u64)>> {
        let pending = self.pending.lock().expect("access: pending lock poisoned");
        let mut grouped: HashMap<String, Vec<(String, u64)>> = HashMap::new();
        for ((workspace_id, memory_id), delta) in pending.iter() {
            grouped
                .entry(workspace_id.clone())
                .or_default()
                .push((memory_id.clone(), *delta));
        }
        grouped
    }

    fn mark_flushed(&self) {
        let now = Instant::now();
        *self
            .last_flush
            .lock()
            .expect("access: last_flush lock poisoned") = now;
        *self
            .last_attempt
            .lock()
            .expect("access: last_attempt lock poisoned") = now;
    }

    /// Move the attempt clock without touching the failure counter.
    fn mark_attempt(&self) {
        *self
            .last_attempt
            .lock()
            .expect("access: last_attempt lock poisoned") = Instant::now();
    }

    /// Test/diagnostic helper: whether this recorder has ever been hydrated.
    pub fn has_been_hydrated(&self) -> bool {
        self.hydrated_at_least_once.load(Ordering::Acquire)
    }

    /// Test/diagnostic helper: forget every buffered and in-memory access.
    pub fn reset(&self) {
        self.pending
            .lock()
            .expect("access: pending lock poisoned")
            .clear();
        self.totals
            .lock()
            .expect("access: totals lock poisoned")
            .clear();
        self.hydrated
            .lock()
            .expect("access: hydrated lock poisoned")
            .clear();
        self.flush_failures.store(0, Ordering::Release);
        self.mark_flushed();
        STORES
            .lock()
            .expect("access: store registry poisoned")
            .clear();
    }
}

/// Store that owns each workspace's access rows. The recorder is process-wide,
/// so a flush must write a workspace's accesses ONLY into that workspace's own
/// store (a space store never receives another workspace's rows). Weak: a
/// registered store that was dropped (evicted/trashed space) is not kept alive.
static STORES: LazyLock<Mutex<HashMap<String, Weak<dyn MemoryStore>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn register_store(workspace_id: &str, store: &Arc<dyn MemoryStore>) {
    STORES
        .lock()
        .expect("access: store registry poisoned")
        .insert(workspace_id.to_string(), Arc::downgrade(store));
}

/// Where a workspace's pending accesses go.
enum FlushTarget {
    /// Its registered, still-alive store.
    Own(Arc<dyn MemoryStore>),
    /// Never registered: the caller's store (legacy single-store path).
    Caller,
    /// Registered but the store is gone: nothing can receive them.
    Gone,
}

fn flush_target(workspace_id: &str) -> FlushTarget {
    match STORES
        .lock()
        .expect("access: store registry poisoned")
        .get(workspace_id)
    {
        Some(weak) => match weak.upgrade() {
            Some(store) => FlushTarget::Own(store),
            None => FlushTarget::Gone,
        },
        None => FlushTarget::Caller,
    }
}

// ---------------------------------------------------------------------------
// Recording from the read handlers
// ---------------------------------------------------------------------------

/// Record a user-facing read of `docs` for `workspace_id`.
///
/// Called from HTTP and MCP read handlers **only**. Never from
/// `MemoryStore::get` — internal reads are not evidence of utility.
pub async fn record_user_access(
    manager: &MemoryManager,
    memory: &QmdMemory,
    docs: &[MemoryDocument],
) -> usize {
    let workspace_id = memory.workspace_id().to_string();
    let mut ids: Vec<String> = Vec::with_capacity(docs.len());
    for doc in docs {
        if let Some(id) = doc.id.as_deref() {
            if !id.trim().is_empty() && !ids.iter().any(|seen| seen == id) {
                ids.push(id.to_string());
            }
        }
    }
    record_user_access_ids(manager, memory, &workspace_id, &ids).await
}

/// Record a user-facing read of the given ids.
///
/// Same contract as [`record_user_access`] but for handlers that already
/// extracted ids (id-only search modes, single-memory GET).
pub async fn record_user_access_ids(
    manager: &MemoryManager,
    memory: &QmdMemory,
    workspace_id: &str,
    ids: &[String],
) -> usize {
    if ids.is_empty() {
        return 0;
    }

    // Hydrate BEFORE recording. `hydrate` inserts durable rows into the
    // in-memory map, so doing it afterwards would silently overwrite the counts
    // this very call is recording.
    ensure_hydrated(memory).await;
    if let Some(store) = memory.store().await {
        register_store(workspace_id, &store);
    }

    let recorded = RECORDER.record(workspace_id, ids, Utc::now());
    for id in ids {
        manager.record_access(id);
    }

    maybe_flush(memory).await;
    recorded
}

/// Load durable access statistics into the in-memory view once per workspace.
pub async fn ensure_hydrated(memory: &QmdMemory) {
    let workspace_id = memory.workspace_id().to_string();
    {
        let hydrated = RECORDER
            .hydrated
            .lock()
            .expect("access: hydrated lock poisoned");
        if hydrated.contains(&workspace_id) {
            return;
        }
    }
    let Some(store) = memory.store().await else {
        return;
    };
    if let Ok(stats) = store.load_access_stats(&workspace_id).await {
        RECORDER.hydrate(&workspace_id, stats);
    }
}

/// Flush the buffer in the background when it is due.
///
/// This is the *only* read-path caller of [`flush_pending`]. It also guarantees
/// the periodic worker exists (see [`ensure_flush_worker`]): a buffer that can
/// only be drained by a read loses everything when the process is stopped by a
/// signal instead of by a user request.
pub async fn maybe_flush(memory: &QmdMemory) {
    let Some(store) = memory.store().await else {
        return;
    };
    ensure_flush_worker(Arc::clone(&store));
    if !RECORDER.should_flush() {
        return;
    }
    tokio::spawn(async move {
        if let Err(error) = flush_pending(&*store).await {
            tracing::warn!(%error, "access: failed to flush access buffer");
        }
    });
}

/// Timer that drains the buffer even when nothing reads.
///
/// `maybe_flush` is only reachable from a read handler, so between two reads the
/// buffer can only age. A SIGTERM from `systemctl restart` is not a read, and
/// before this worker existed the buffered accesses were lost with the process —
/// which for a utility prune means "never accessed" for records that were read
/// hours earlier. The worker makes the loss window bounded by
/// [`DEFAULT_FLUSH_INTERVAL`] instead of unbounded by traffic.
///
/// Idempotent and process-wide: the first caller spawns it, later ones are no-ops
/// even when they pass a different store, because [`RECORDER`] is process-global
/// and flushing it through two stores concurrently would be the race this module
/// must not have.
pub fn ensure_flush_worker(store: Arc<dyn MemoryStore>) {
    static WORKER: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    // Weak: the first caller may be a space store that is later evicted.
    let store = Arc::downgrade(&store);
    WORKER.get_or_init(|| {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(DEFAULT_FLUSH_INTERVAL);
            // The first tick fires immediately; skip it so the worker does not
            // race the `maybe_flush` call that spawned it.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if RECORDER.pending_count() == 0 {
                    continue;
                }
                let caller = store.upgrade();
                match flush_inner(caller.as_deref()).await {
                    Ok(written) if written > 0 => {
                        tracing::debug!(written, "access: periodic flush drained buffer");
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, "access: periodic access flush failed");
                    }
                }
            }
        });
    });
}

/// Flush everything buffered, right now, and report how many accesses landed.
///
/// This is the shutdown hook (D4). Call it from the graceful-shutdown path —
/// *before* the store is dropped — so a `systemctl restart` persists the
/// accesses of the last [`DEFAULT_FLUSH_INTERVAL`] instead of discarding them.
///
/// Bounded by `timeout` because a shutdown must not hang on a wedged database:
/// on timeout the buffer is left intact and the periodic worker (or the next
/// process) is the fallback, which is strictly better than blocking the exit.
pub async fn flush_on_shutdown(store: &dyn MemoryStore, timeout: Duration) -> usize {
    match tokio::time::timeout(timeout, flush_pending(store)).await {
        Ok(Ok(written)) => written,
        Ok(Err(error)) => {
            tracing::warn!(%error, "access: shutdown flush failed; buffer kept for retry");
            0
        }
        Err(_) => {
            tracing::warn!(
                timeout_secs = timeout.as_secs(),
                "access: shutdown flush timed out; buffer kept for retry"
            );
            0
        }
    }
}

/// How long the shutdown hook waits for the final flush.
pub const SHUTDOWN_FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// Flush the buffer when the process is asked to stop (D4).
///
/// Tokio delivers SIGTERM/SIGINT to *every* registered stream, so installing
/// this listener alongside the HTTP server's graceful shutdown neither steals the
/// signal nor requires touching the server: the server still stops accepting
/// connections, and this task drains the access buffer on its own.
///
/// Registered once per process, like [`ensure_flush_worker`].
pub fn spawn_shutdown_flush(store: Arc<dyn MemoryStore>) {
    static SHUTDOWN_HOOK: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    SHUTDOWN_HOOK.get_or_init(|| {
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = match signal(SignalKind::terminate()) {
                    Ok(stream) => stream,
                    Err(error) => {
                        tracing::warn!(%error, "access: SIGTERM listener unavailable");
                        return;
                    }
                };
                let mut sigint = match signal(SignalKind::interrupt()) {
                    Ok(stream) => stream,
                    Err(error) => {
                        tracing::warn!(%error, "access: SIGINT listener unavailable");
                        return;
                    }
                };
                let reason = tokio::select! {
                    _ = sigterm.recv() => "SIGTERM",
                    _ = sigint.recv() => "SIGINT",
                };
                drain_for_shutdown(&store, reason).await;
            }
            #[cfg(not(unix))]
            {
                if let Ok(mut rx) = tokio::signal::ctrl_c().await {
                    if rx.recv().await.is_some() {
                        drain_for_shutdown(&store, "Ctrl+C").await;
                    }
                }
            }
        });
    });
}

async fn drain_for_shutdown(store: &Arc<dyn MemoryStore>, reason: &str) {
    let pending = RECORDER.pending_count();
    if pending == 0 {
        tracing::debug!(reason, "access: shutdown flush skipped, buffer empty");
        return;
    }
    let written = flush_on_shutdown(&**store, SHUTDOWN_FLUSH_TIMEOUT).await;
    tracing::info!(
        reason,
        pending,
        written,
        "access: shutdown flush complete (D4)"
    );
}

/// Guard that puts drained-but-unwritten accesses back into the buffer.
///
/// Two failure modes have to restore the same set, and getting them wrong is the
/// whole of D5:
///
/// - an error mid-loop, and
/// - the future being **cancelled** while a write is in flight (which is what
///   `flush_on_shutdown`'s timeout does, and what any task abort does).
///
/// Workspaces are removed from `drained` only *after* their write succeeds, so
/// whatever is left at `Drop` time is exactly what was never committed — the
/// already-written workspaces are gone from the map and cannot be resurrected.
struct Unwritten {
    drained: HashMap<String, Vec<(String, u64)>>,
}

impl Drop for Unwritten {
    fn drop(&mut self) {
        if self.drained.is_empty() {
            return;
        }
        RECORDER.restore_pending(AccessRecorder::flatten(&self.drained));
    }
}

/// Flush buffered accesses, each into its workspace's OWN store; `store` only
/// receives workspaces that were never registered (see [`flush_inner`]).
///
/// One `record_accesses` call per workspace, i.e. one multi-row upsert per
/// workspace rather than one statement per read.
///
/// ## Only what was not written is restored (D5)
///
/// Workspaces are written in turn. When workspace *N* fails, the accesses of
/// the workspaces already committed must **not** go back into the buffer: the
/// next flush would add them to `access_count` a second time — a silent
/// inflation of the one metric a future utility prune would delete on. So the
/// restore is scoped to the failing workspace and everything after it, which is
/// what [`Unwritten`] holds.
///
/// Concurrency: [`drain_pending`] empties the buffer under its lock, so two
/// concurrent flushes hold disjoint access sets and never write the same delta
/// twice. A restore may land while a concurrent flush is draining, in which case
/// those accesses simply join the next batch and are still written exactly once.
pub async fn flush_pending(store: &dyn MemoryStore) -> Result<usize> {
    flush_inner(Some(store)).await
}

/// Flush into each workspace's OWN store. Workspaces never registered use
/// `caller` when given (otherwise they stay queued); workspaces whose store is
/// gone are dropped.
async fn flush_inner(caller: Option<&dyn MemoryStore>) -> Result<usize> {
    let mut targets: HashMap<String, Option<Arc<dyn MemoryStore>>> = HashMap::new();
    for workspace_id in RECORDER.pending_workspaces() {
        match flush_target(&workspace_id) {
            FlushTarget::Own(store) => {
                targets.insert(workspace_id, Some(store));
            }
            FlushTarget::Caller if caller.is_some() => {
                targets.insert(workspace_id, None);
            }
            FlushTarget::Caller => {}
            FlushTarget::Gone => RECORDER.discard_workspace(&workspace_id),
        }
    }
    let mut unwritten = Unwritten {
        drained: RECORDER.drain_pending_where(|ws| targets.contains_key(ws)),
    };
    if unwritten.drained.is_empty() {
        return Ok(0);
    }

    // Own the iteration order: the `Unwritten` guard has to see each workspace
    // removed once it is committed, and iterating a map while mutating it is a
    // borrow error. The drain order is already arbitrary, so nothing is lost.
    let workspaces: Vec<String> = unwritten.drained.keys().cloned().collect();
    let mut written = 0;
    for workspace_id in workspaces {
        let result = match targets.get(&workspace_id) {
            Some(Some(own)) => {
                own.record_accesses(&workspace_id, &unwritten.drained[&workspace_id])
                    .await
            }
            Some(None) => match caller {
                Some(store) => {
                    store
                        .record_accesses(&workspace_id, &unwritten.drained[&workspace_id])
                        .await
                }
                None => Ok(0),
            },
            None => Ok(0),
        };
        match result {
            Ok(_) => {
                written += unwritten
                    .drained
                    .remove(&workspace_id)
                    .map(|entries| entries.len())
                    .unwrap_or(0);
            }
            Err(error) => {
                // `unwritten` still holds this workspace and every later one;
                // the Drop impl puts exactly those back.
                RECORDER.register_flush_failure();
                return Err(error);
            }
        }
    }
    RECORDER.register_flush_success();
    Ok(written)
}

// ---------------------------------------------------------------------------
// Recency resolution
// ---------------------------------------------------------------------------
//
// `get_last_accessed()` (in `crate::memory::decay`) is the single resolution
// point: it consults `RECORDER` first and falls back to metadata, then
// `updated_at`. `DecayManager` and `TgdUtilityPruner::get_record_age_days` both
// route through it, so rewiring it there fixed every consumer at once rather
// than leaving two disagreeing notions of "last accessed".

/// Whether a record carries the minimum signal a utility prune may ever use.
///
/// Only records **with an embedding** are observable. A record without one can
/// never enter a vector top-k, so its zero access count carries no information:
/// pruning it would delete exactly the memories that lost their embedding, not
/// the ones that stopped being useful.
///
/// This is a predicate, not a prune. It is the eligibility gate a future
/// `/maintenance/*` apply step must consult, alongside [`MIN_OBSERVATION_DAYS`],
/// and archive must be preferred over delete when it finally runs.
pub fn record_is_observable(record: &MemoryRecord) -> bool {
    record.embedding_status == "completed" || !record.embedding.is_empty()
}

// ---------------------------------------------------------------------------
// Observation clock
// ---------------------------------------------------------------------------

/// Why a utility prune may not delete anything yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneBlockReason {
    /// Nothing blocks the prune; the observation window has elapsed.
    Eligible,
    /// The observation clock is missing, so no candidate's age can be trusted.
    ObservationClockMissing,
    /// The observation window has not elapsed yet.
    ObservationWindowOpen {
        /// Whole days observed so far (floor).
        elapsed_days: i64,
        /// Whole days required.
        required_days: i64,
    },
    /// The record is not observable and must never be a prune candidate.
    RecordNotObservable,
}

/// Evaluate the observation window.
///
/// Pure so the guard is testable without a database and auditable before any
/// destructive code exists. With `observation_started_at` missing, the answer is
/// always "not yet" — absence of a clock is not evidence of 30 days of calm.
pub fn prune_block_reason(
    observation_started_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> PruneBlockReason {
    let Some(started) = observation_started_at else {
        return PruneBlockReason::ObservationClockMissing;
    };
    let elapsed_days = (now - started).num_days().max(0);
    if elapsed_days < MIN_OBSERVATION_DAYS {
        return PruneBlockReason::ObservationWindowOpen {
            elapsed_days,
            required_days: MIN_OBSERVATION_DAYS,
        };
    }
    PruneBlockReason::Eligible
}

/// Create the instrumentation tables and seed the observation clock.
///
/// The seed uses `INSERT OR IGNORE`, so the very first caller fixes
/// `observation_started_at` forever: every later start would otherwise push the
/// window forward and silently extend every candidate's lease.
pub async fn ensure_instrumentation(store: &dyn MemoryStore) -> Result<()> {
    store.ensure_access_instrumentation().await
}

/// Read the instant the access observation window opened.
pub async fn observation_started_at(store: &dyn MemoryStore) -> Result<Option<DateTime<Utc>>> {
    store
        .read_maintenance_meta(OBSERVATION_STARTED_AT_KEY)
        .await
}

/// Days elapsed since the observation clock started (0 when absent).
pub async fn observation_days_elapsed(store: &dyn MemoryStore, now: DateTime<Utc>) -> Result<f64> {
    let Some(started) = observation_started_at(store).await? else {
        return Ok(0.0);
    };
    Ok((now - started).num_days().max(0) as f64)
}

/// Combine the clock and the per-record gate into one verdict.
///
/// Two independent reasons a delete must not happen: the observation window may
/// still be open, or the specific record may not be observable at all.
pub async fn prune_precondition(
    store: &dyn MemoryStore,
    record: &MemoryRecord,
    now: DateTime<Utc>,
) -> Result<PruneBlockReason> {
    if !record_is_observable(record) {
        return Ok(PruneBlockReason::RecordNotObservable);
    }
    Ok(prune_block_reason(
        observation_started_at(store).await?,
        now,
    ))
}

#[cfg(test)]
mod flush_scope_tests {
    use super::*;
    use crate::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
    use serial_test::serial;
    use tokio::sync::RwLock as AsyncRwLock;

    async fn ws_store(
        dir: &tempfile::TempDir,
        tag: &str,
        ws: &str,
    ) -> (Arc<QmdMemory>, MemoryManager, Arc<dyn MemoryStore>) {
        let store = VecSqliteMemoryStore::new(VecSqliteStoreConfig {
            path: dir.path().join(format!("{tag}.sqlite3")),
            embedding_dimensions: 128,
        })
        .await
        .expect("store");
        store.ensure_access_instrumentation().await.expect("instr");
        let store: Arc<dyn MemoryStore> = Arc::new(store);
        let memory = Arc::new(QmdMemory::new_with_workspace(
            Arc::new(AsyncRwLock::new(Vec::new())),
            ws,
        ));
        memory.set_store(Arc::clone(&store)).await;
        let manager = MemoryManager::new(Arc::clone(&memory), None);
        (memory, manager, store)
    }

    fn one(id: &str) -> MemoryDocument {
        MemoryDocument {
            id: Some(id.to_string()),
            path: format!("notes/{id}"),
            content: id.to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    #[serial]
    async fn flush_never_writes_another_workspaces_entries_into_a_store() {
        RECORDER.reset();
        let dir = tempfile::tempdir().unwrap();
        let (mem_a, man_a, store_a) = ws_store(&dir, "a", "ws-scope-a").await;
        let (mem_b, man_b, store_b) = ws_store(&dir, "b", "ws-scope-b").await;
        record_user_access(&man_a, &mem_a, &[one("only-a")]).await;
        record_user_access(&man_b, &mem_b, &[one("only-b")]).await;

        // Flushing through B's store must not put A's rows into B's database.
        flush_pending(&*store_b).await.expect("flush");

        let in_a = store_a.load_access_stats("ws-scope-a").await.unwrap();
        let a_in_b = store_b.load_access_stats("ws-scope-a").await.unwrap();
        let in_b = store_b.load_access_stats("ws-scope-b").await.unwrap();
        let b_in_a = store_a.load_access_stats("ws-scope-b").await.unwrap();
        assert_eq!(in_a.len(), 1, "A's entry lands in A's store");
        assert_eq!(in_b.len(), 1, "B's entry lands in B's store");
        assert!(a_in_b.is_empty(), "A's entry never written into B's store");
        assert!(b_in_a.is_empty(), "B's entry never written into A's store");
        RECORDER.reset();
    }

    #[tokio::test]
    #[serial]
    async fn entries_of_a_dropped_store_are_not_flushed_elsewhere() {
        RECORDER.reset();
        let dir = tempfile::tempdir().unwrap();
        let (mem_a, man_a, store_a) = ws_store(&dir, "gone", "ws-scope-gone").await;
        let (_mem_b, _man_b, store_b) = ws_store(&dir, "kept", "ws-scope-kept").await;
        record_user_access(&man_a, &mem_a, &[one("x")]).await;
        drop((mem_a, man_a, store_a));

        flush_pending(&*store_b).await.expect("flush");
        assert!(store_b
            .load_access_stats("ws-scope-gone")
            .await
            .unwrap()
            .is_empty());
        assert_eq!(RECORDER.pending_count(), 0, "orphaned entries are dropped");
        RECORDER.reset();
    }
}
