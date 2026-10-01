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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
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
    hydrated_at_least_once: AtomicBool,
}

/// Process-wide recorder. One per process, keyed by `(workspace_id, memory_id)`.
pub static RECORDER: LazyLock<AccessRecorder> = LazyLock::new(|| AccessRecorder {
    pending: Mutex::new(HashMap::new()),
    totals: Mutex::new(HashMap::new()),
    hydrated: Mutex::new(HashSet::new()),
    last_flush: Mutex::new(Instant::now()),
    hydrated_at_least_once: AtomicBool::new(false),
});

impl Default for AccessRecorder {
    fn default() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            totals: Mutex::new(HashMap::new()),
            hydrated: Mutex::new(HashSet::new()),
            last_flush: Mutex::new(Instant::now()),
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
    pub fn should_flush(&self) -> bool {
        if self.pending_count() >= DEFAULT_FLUSH_BATCH {
            return true;
        }
        self.last_flush
            .lock()
            .expect("access: last_flush lock poisoned")
            .elapsed()
            >= DEFAULT_FLUSH_INTERVAL
            && self.pending_count() > 0
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

    fn mark_flushed(&self) {
        *self
            .last_flush
            .lock()
            .expect("access: last_flush lock poisoned") = Instant::now();
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
pub async fn maybe_flush(memory: &QmdMemory) {
    if !RECORDER.should_flush() {
        return;
    }
    let Some(store) = memory.store().await else {
        return;
    };
    tokio::spawn(async move {
        if let Err(error) = flush_pending(&*store).await {
            tracing::warn!(%error, "access: failed to flush access buffer");
        }
    });
}

/// Flush every buffered access into `store`.
///
/// One `record_accesses` call per workspace, i.e. one multi-row upsert per
/// workspace rather than one statement per read. Pending accesses are returned
/// to the buffer if the write fails, so a failed flush never loses evidence.
pub async fn flush_pending(store: &dyn MemoryStore) -> Result<usize> {
    let drained = RECORDER.drain_pending();
    if drained.is_empty() {
        return Ok(0);
    }

    let mut written = 0;
    for (workspace_id, entries) in &drained {
        if let Err(error) = store.record_accesses(workspace_id, entries).await {
            // A failed flush must not destroy evidence: put every drained
            // access back so the next flush retries it.
            RECORDER.restore_pending(AccessRecorder::flatten(&drained));
            return Err(error);
        }
        written += entries.len();
    }
    RECORDER.mark_flushed();
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
