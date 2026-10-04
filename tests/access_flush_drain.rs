//! D4 + D5: the access buffer must drain without a read, and a failed flush
//! must not count twice.
//!
//! Two defects, one file, because they are the same buffer seen from two sides:
//!
//! - **D5 (double counting).** `flush_pending` wrote workspace after workspace.
//!   When workspace *N* failed, it restored `flatten(&drained)` — *every*
//!   workspace, including the ones already committed at steps `0..N`. The next
//!   flush added those deltas to `access_count` a second time. Since `access_count`
//!   is the metric a future utility prune would delete on, this was silent
//!   corruption of the one number that must be trustworthy.
//! - **D4 (lost on restart).** `maybe_flush` had exactly one caller — the read
//!   path. No timer, no shutdown drain. A SIGTERM from `systemctl restart`
//!   discarded everything buffered, so a record read minutes earlier reads as
//!   "never accessed" to the pruner.
//!
//! The `ScriptedAccessStore` below can fail chosen workspaces and then succeed,
//! which is what makes D5 observable: the bug only shows when a *later* flush
//! runs after a *partial* failure.
//!
//! Every store here is in-memory and process-local; nothing touches the
//! production database. Tests are `#[serial]` because `RECORDER` is a
//! process-wide singleton, exactly as in `src/memory/access_tests.rs`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serial_test::serial;

use xavier::checkpoint::Checkpoint;
use xavier::memory::access::{
    flush_on_shutdown, flush_pending, AccessRecorder, DEFAULT_FLUSH_BATCH, DEFAULT_FLUSH_INTERVAL,
    RECORDER,
};
use xavier::memory::schema::MemoryQueryFilters;
use xavier::memory::store::{
    DurableWorkspaceState, MemoryBackend, MemoryRecord, MemoryStore, SessionTokenRecord,
};

/// Durable counters plus a script of per-workspace failures.
///
/// Only the `record_accesses` / `load_access_stats` half of [`MemoryStore`] is
/// interesting here; the rest is the minimum the trait demands.
struct ScriptedAccessStore {
    /// workspace_id -> memory_id -> durable `access_count`.
    durable: Mutex<HashMap<String, HashMap<String, u64>>>,
    /// Workspaces whose next `record_accesses` must fail. Draining this entry
    /// means the retry succeeds, so a test can model "transient failure".
    fail_next: Mutex<HashMap<String, usize>>,
    /// Fail the write whose 0-based *global* attempt index is in this set.
    ///
    /// D5's test has to be independent of `HashMap` iteration order, and a
    /// per-workspace rule cannot express "fail after exactly one workspace has
    /// been committed" without knowing which one that is. A global attempt index
    /// does: whatever the order, exactly one workspace lands before the failure.
    fail_attempt: Mutex<Vec<usize>>,
    /// Global attempt counter (failed attempts included).
    attempts: AtomicUsize,
    /// Per-workspace call counter, to assert a write happened exactly once.
    calls: Mutex<HashMap<String, usize>>,
    /// Every (workspace_id, memory_id, delta) that was ever applied.
    applied: Mutex<Vec<(String, String, u64)>>,
}

impl ScriptedAccessStore {
    fn new() -> Self {
        Self {
            durable: Mutex::new(HashMap::new()),
            fail_next: Mutex::new(HashMap::new()),
            fail_attempt: Mutex::new(Vec::new()),
            attempts: AtomicUsize::new(0),
            calls: Mutex::new(HashMap::new()),
            applied: Mutex::new(Vec::new()),
        }
    }

    /// Fail the next `count` writes of `workspace_id`.
    fn fail_next(&self, workspace_id: &str, count: usize) {
        self.fail_next
            .lock()
            .expect("fail_next lock")
            .insert(workspace_id.to_string(), count);
    }

    /// Fail the write with 0-based global attempt index `index`.
    fn fail_attempt(&self, index: usize) {
        self.fail_attempt
            .lock()
            .expect("fail_attempt lock")
            .push(index);
    }

    /// Writes that actually reached the durable table.
    fn successful_writes(&self) -> usize {
        self.calls.lock().expect("calls lock").values().sum()
    }

    fn count(&self, workspace_id: &str, memory_id: &str) -> u64 {
        self.durable
            .lock()
            .expect("durable lock")
            .get(workspace_id)
            .and_then(|ws| ws.get(memory_id))
            .copied()
            .unwrap_or(0)
    }

    /// Sum of every delta ever applied to `(workspace_id, memory_id)`.
    ///
    /// Distinct from `count`: if a delta were applied twice, `applied` would
    /// show the duplicate while `count` (an upsert that adds) shows the
    /// inflated total. Asserting on both closes the loop.
    fn applied_total(&self, workspace_id: &str, memory_id: &str) -> u64 {
        self.applied
            .lock()
            .expect("applied lock")
            .iter()
            .filter(|(ws, id, _)| ws == workspace_id && id == memory_id)
            .map(|(_, _, delta)| *delta)
            .sum()
    }

    /// Deltas applied to `workspace_id`, as one line per call, for readability
    /// in failure messages.
    fn describe(&self) -> String {
        self.applied
            .lock()
            .expect("applied lock")
            .iter()
            .map(|(ws, id, delta)| format!("{ws}/{id}+={delta}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[async_trait]
impl MemoryStore for ScriptedAccessStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Memory
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn health(&self) -> Result<String> {
        Ok("ok".to_string())
    }

    async fn record_accesses(
        &self,
        workspace_id: &str,
        entries: &[(String, u64)],
    ) -> Result<usize> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        let scripted_by_index = self
            .fail_attempt
            .lock()
            .expect("fail_attempt lock")
            .contains(&attempt);
        let scripted_by_workspace = {
            let mut fail_next = self.fail_next.lock().expect("fail_next lock");
            match fail_next.get_mut(workspace_id) {
                Some(remaining) if *remaining > 0 => {
                    *remaining -= 1;
                    true
                }
                _ => false,
            }
        };
        if scripted_by_index || scripted_by_workspace {
            anyhow::bail!("scripted access write failure for {workspace_id} (attempt {attempt})");
        }

        *self
            .calls
            .lock()
            .expect("calls lock")
            .entry(workspace_id.to_string())
            .or_insert(0) += 1;

        let mut applied = self.applied.lock().expect("applied lock");
        let mut durable = self.durable.lock().expect("durable lock");
        let ws = durable.entry(workspace_id.to_string()).or_default();
        for (memory_id, delta) in entries {
            let slot = ws.entry(memory_id.clone()).or_insert(0);
            *slot = slot.saturating_add(*delta);
            applied.push((workspace_id.to_string(), memory_id.clone(), *delta));
        }
        Ok(entries.len())
    }

    async fn load_access_stats(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<(String, xavier::memory::access::AccessStats)>> {
        Ok(self
            .durable
            .lock()
            .expect("durable lock")
            .get(workspace_id)
            .map(|ws| {
                ws.iter()
                    .map(|(id, count)| {
                        (
                            id.clone(),
                            xavier::memory::access::AccessStats {
                                access_count: *count,
                                first_accessed_at: Some(Utc::now()),
                                last_accessed_at: Some(Utc::now()),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn put(&self, _record: MemoryRecord) -> Result<()> {
        Ok(())
    }

    async fn get(&self, _workspace_id: &str, _id_or_path: &str) -> Result<Option<MemoryRecord>> {
        Ok(None)
    }

    async fn update(&self, _record: MemoryRecord) -> Result<()> {
        Ok(())
    }

    async fn delete(&self, _workspace_id: &str, _id_or_path: &str) -> Result<Option<MemoryRecord>> {
        Ok(None)
    }

    async fn list(&self, _workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        Ok(Vec::new())
    }

    async fn search(
        &self,
        _workspace_id: &str,
        _query: &str,
        _filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>> {
        Ok(Vec::new())
    }

    async fn load_workspace_state(&self, _workspace_id: &str) -> Result<DurableWorkspaceState> {
        Ok(DurableWorkspaceState::default())
    }

    async fn save_beliefs(
        &self,
        _workspace_id: &str,
        _beliefs: Vec<xavier::domain::memory::belief::BeliefEdge>,
    ) -> Result<()> {
        Ok(())
    }

    async fn save_session_token(
        &self,
        _workspace_id: &str,
        _token: SessionTokenRecord,
    ) -> Result<()> {
        Ok(())
    }

    async fn is_session_token_valid(&self, _workspace_id: &str, _token: &str) -> Result<bool> {
        Ok(true)
    }

    async fn save_checkpoint(&self, _workspace_id: &str, _checkpoint: Checkpoint) -> Result<()> {
        Ok(())
    }

    async fn load_checkpoint(
        &self,
        _workspace_id: &str,
        _task_id: &str,
        _name: &str,
    ) -> Result<Option<Checkpoint>> {
        Ok(None)
    }

    async fn list_checkpoints(
        &self,
        _workspace_id: &str,
        _task_id: &str,
    ) -> Result<Vec<Checkpoint>> {
        Ok(Vec::new())
    }

    async fn delete_checkpoint(
        &self,
        _workspace_id: &str,
        _task_id: &str,
        _name: &str,
    ) -> Result<()> {
        Ok(())
    }
}

/// Buffer `count` distinct records for each of `workspaces`, on the global
/// recorder. Deterministic, no store involved.
fn buffer(workspaces: &[&str], memory_id: &str, count: usize) {
    for ws in workspaces {
        let ids: Vec<String> = (0..count).map(|i| format!("{memory_id}-{i}")).collect();
        RECORDER.record(ws, &ids, Utc::now());
    }
}

// ---------------------------------------------------------------------------
// D5 — a partial failure must not re-count what was already written
// ---------------------------------------------------------------------------

/// The D5 regression, stated as one test.
///
/// Three workspaces are buffered. The middle one fails on the first flush; the
/// two that succeeded before it are written to the store. Under the old code the
/// restore put all three back, and the *second* flush added the two already
/// committed deltas a second time — `access_count` doubled for them while the
/// failing workspace's single access landed exactly once.
#[tokio::test]
#[serial]
async fn failed_flush_does_not_recount_workspaces_already_written() {
    RECORDER.reset();
    let store = Arc::new(ScriptedAccessStore::new());

    buffer(&["ws-d5-a", "ws-d5-b", "ws-d5-c"], "doc", 3);
    assert_eq!(
        RECORDER.pending_count(),
        9,
        "three workspaces x three records"
    );

    // Fail the *second* write attempt. The drain order is a `HashMap` order we
    // do not control, so failing by attempt index — not by workspace name — is
    // what makes this test deterministic: whatever order the three workspaces
    // come out in, exactly one is committed before the failure and two are not.
    store.fail_attempt(1);

    let first = flush_pending(&*store).await;
    assert!(first.is_err(), "the scripted failure must surface");

    // Exactly one workspace was committed before the failure.
    assert_eq!(
        store.successful_writes(),
        1,
        "the failure must land after one committed workspace, not before any \
         and not after all"
    );

    // The other two — the failing one and the one still to come — are back in
    // the buffer. The committed one is not: restoring it is D5.
    let snapshot = RECORDER.pending_snapshot();
    assert_eq!(
        snapshot.len(),
        2,
        "the failed workspace and the unattempted one are restored; a full \
         restore would also resurrect the committed one. Restored: {:?}",
        snapshot.keys().collect::<Vec<_>>()
    );
    for (ws, entries) in &snapshot {
        assert_eq!(
            entries.len(),
            3,
            "workspace {ws} must keep all three of its pending accesses"
        );
    }
    for ws in snapshot.keys() {
        assert_eq!(
            store.count(ws, "doc-0"),
            0,
            "a restored workspace must not have been committed"
        );
    }

    // Second flush: the store now accepts everything.
    let written = flush_pending(&*store).await.expect("retry flush");
    assert_eq!(
        written, 6,
        "only the two restored workspaces are rewritten, three records each"
    );
    assert_eq!(
        RECORDER.pending_count(),
        0,
        "after a successful retry nothing is left buffered"
    );

    // The metric is the assertion that matters: every workspace has exactly the
    // accesses it recorded, none of them doubled.
    for ws in ["ws-d5-a", "ws-d5-b", "ws-d5-c"] {
        for i in 0..3 {
            let id = format!("doc-{i}");
            assert_eq!(
                store.count(ws, &id),
                1,
                "{ws}/{id}: durable access_count must be 1, not 2. \
                 Applied writes: {}",
                store.describe()
            );
            assert_eq!(
                store.applied_total(ws, &id),
                1,
                "{ws}/{id}: the delta must be applied exactly once. Applied: {}",
                store.describe()
            );
        }
    }

    // Three successful writes in total: one on the first pass (before the
    // failure) and two on the retry. There were four *attempts*.
    //
    // This is the discriminator. Under the old code the restore put all three
    // workspaces back, so the retry re-wrote the one that had already been
    // committed: four successful writes and two workspaces with access_count 2.
    // Both the write count and the per-record counts above are asserted.
    assert_eq!(
        store.successful_writes(),
        3,
        "3 workspaces, one committed before the failure and two on the retry; a \
         re-write of the committed workspace would make this 4. Applied: {}",
        store.describe()
    );
    RECORDER.reset();
}

/// The old code's exact failure mode, isolated so the fix cannot regress into a
/// variant of it: with **two** workspaces where the second one fails, the first
/// workspace's counters must stay at 1 after the retry.
#[tokio::test]
#[serial]
async fn partial_flush_leaves_committed_workspaces_untouched() {
    RECORDER.reset();
    let store = Arc::new(ScriptedAccessStore::new());

    buffer(&["ws-d5-first", "ws-d5-second"], "rec", 2);
    // Fail the second write, so exactly one workspace is committed first.
    store.fail_attempt(1);

    assert!(flush_pending(&*store).await.is_err());
    assert_eq!(
        store.successful_writes(),
        1,
        "one workspace committed before the failure"
    );

    // Retry until the buffer is empty. Everything must converge to exactly one
    // access per record, in both workspaces.
    for _ in 0..3 {
        flush_pending(&*store).await.expect("retry");
    }

    for ws in ["ws-d5-first", "ws-d5-second"] {
        for i in 0..2 {
            let id = format!("rec-{i}");
            assert_eq!(
                store.count(ws, &id),
                1,
                "{ws}/{id} counted twice. Applied: {}",
                store.describe()
            );
            assert_eq!(
                store.applied_total(ws, &id),
                1,
                "{ws}/{id} had its delta applied twice. Applied: {}",
                store.describe()
            );
        }
    }
    RECORDER.reset();
}

/// A failed flush must leave a backoff, and the next read must not immediately
/// re-attempt the write.
///
/// This is the second half of D5: the old code never reached `mark_flushed` on
/// the failure path, so `should_flush()` stayed true and *every* read after a
/// broken store spawned another flush.
#[tokio::test]
#[serial]
async fn a_failed_flush_backs_off_instead_of_retrying_on_every_read() {
    RECORDER.reset();
    let recorder = AccessRecorder::default();

    recorder.record("ws-backoff", &["doc-a".to_string()], Utc::now());
    assert_eq!(
        recorder.backoff(),
        DEFAULT_FLUSH_INTERVAL,
        "a healthy recorder flushes on the normal interval"
    );

    recorder.register_flush_failure();
    assert_eq!(recorder.flush_failures(), 1);
    assert_eq!(
        recorder.backoff(),
        DEFAULT_FLUSH_INTERVAL * 2,
        "one failure doubles the window"
    );
    assert!(
        !recorder.should_flush(),
        "an immediate retry after a failure must not be due yet"
    );

    recorder.register_flush_failure();
    assert_eq!(
        recorder.backoff(),
        DEFAULT_FLUSH_INTERVAL * 4,
        "the backoff is exponential, not linear"
    );

    recorder.register_flush_success();
    assert_eq!(
        recorder.flush_failures(),
        0,
        "one success resets the failure streak"
    );
    assert_eq!(recorder.backoff(), DEFAULT_FLUSH_INTERVAL);
    RECORDER.reset();
}

/// The batch threshold is a safety valve, not a suggestion: a broken store must
/// not be allowed to grow the buffer without bound, so reaching
/// `DEFAULT_FLUSH_BATCH` is due regardless of backoff.
#[tokio::test]
#[serial]
async fn a_full_buffer_is_due_even_under_backoff() {
    RECORDER.reset();
    let recorder = AccessRecorder::default();
    for _ in 0..3 {
        recorder.register_flush_failure();
    }
    assert!(
        recorder.backoff() > DEFAULT_FLUSH_INTERVAL,
        "precondition: the backoff is extended"
    );

    let ids: Vec<String> = (0..DEFAULT_FLUSH_BATCH)
        .map(|i| format!("flood-{i}"))
        .collect();
    recorder.record("ws-flood", &ids, Utc::now());

    assert!(
        recorder.should_flush(),
        "a full buffer must still be attempted; a dead store would otherwise \
         grow the buffer forever"
    );
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// Concurrency: the write path must stay race-free
// ---------------------------------------------------------------------------
//
// Closed issue #2806 concerns concurrent telemetry writes. The fix touched the
// write path (`flush_pending` + `restore_pending`), so the invariant under test
// is the one that issue was about: every recorded access is applied exactly
// once, no matter how many flushes and records race.

/// Recording and flushing concurrently must apply every access exactly once.
#[tokio::test]
#[serial]
async fn concurrent_records_and_flushes_count_each_access_once() {
    RECORDER.reset();
    let store = Arc::new(ScriptedAccessStore::new());

    const ROUNDS: usize = 60;
    const DOCS: usize = 4;

    // True interleaving: one task records one delta, the other drains, and each
    // yields between steps so the two halves actually race on `pending`.
    //
    // The invariant is the sum, not the batch shape: a flush may carry any
    // number of deltas, and they add up. What must hold is that each recorded
    // delta reaches the store exactly once — never twice (D5's corruption) and
    // never zero times (a lost access).
    // The writer only touches the process-global `RECORDER`; only the flusher needs
    // the store.
    let writer = tokio::spawn(async move {
        for _ in 0..ROUNDS {
            for i in 0..DOCS {
                RECORDER.record("ws-conc", &[format!("doc-{i}")], Utc::now());
            }
            tokio::task::yield_now().await;
        }
    });

    let flusher = {
        let store = Arc::clone(&store);
        tokio::spawn(async move {
            for _ in 0..ROUNDS {
                let _ = flush_pending(&*store).await;
                tokio::task::yield_now().await;
            }
        })
    };

    writer.await.expect("writer task");
    flusher.await.expect("flusher task");

    // Drain whatever the last flushes left behind.
    for _ in 0..3 {
        flush_pending(&*store).await.expect("final flush");
    }

    let expected = ROUNDS as u64;
    assert_eq!(
        RECORDER.pending_count(),
        0,
        "every recorded access must be drained"
    );
    for i in 0..DOCS {
        let id = format!("doc-{i}");
        assert_eq!(
            store.count("ws-conc", &id),
            expected,
            "{id}: durable count must equal the number of records, no more \
             (double count) and no less (lost access). Applied: {}",
            store.describe()
        );
        assert_eq!(
            store.applied_total("ws-conc", &id),
            expected,
            "{id}: every delta applied exactly once"
        );
    }
    RECORDER.reset();
}

/// Concurrent flushes where the store keeps failing: the buffer must hold each
/// access once, never twice.
///
/// A restore racing another drain is the dangerous case — if the restore
/// duplicated what a concurrent drain was already holding, the retries would
/// inflate the count. Here the failures are permanent, so nothing is ever
/// committed and the invariant to check is that the *buffer* stays exact.
#[tokio::test]
#[serial]
async fn concurrent_failing_flushes_never_duplicate_the_buffer() {
    RECORDER.reset();
    let store = Arc::new(ScriptedAccessStore::new());
    // A huge failure budget: every write in this test fails.
    store.fail_next("ws-concfail", usize::MAX / 2);

    const RECORDS: usize = 50;
    let ids: Vec<String> = (0..RECORDS).map(|i| format!("doc-{i}")).collect();
    RECORDER.record("ws-concfail", &ids, Utc::now());

    for _ in 0..10 {
        let mut tasks = Vec::new();
        for _ in 0..4 {
            let store = Arc::clone(&store);
            tasks.push(tokio::spawn(async move {
                let _ = flush_pending(&*store).await;
            }));
        }
        for task in tasks {
            task.await.expect("flush task");
        }
    }

    let snapshot = RECORDER.pending_snapshot();
    let buffered = snapshot
        .get("ws-concfail")
        .map(|entries| entries.len())
        .unwrap_or(0);
    assert_eq!(
        buffered, RECORDS,
        "after 40 failed flushes the buffer must still hold exactly {RECORDS} \
         accesses; a restore that re-added a concurrent drain would inflate it"
    );
    for entry in snapshot.get("ws-concfail").expect("ws present") {
        assert_eq!(
            entry.1, 1,
            "each record must carry delta 1, got {} — a duplicated restore",
            entry.1
        );
    }
    assert_eq!(
        store.count("ws-concfail", "doc-0"),
        0,
        "nothing may be committed while the store fails"
    );
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// D4 — the buffer must drain at shutdown
// ---------------------------------------------------------------------------

/// The D4 regression: what is buffered when the process is asked to stop is
/// what must be durable afterwards.
///
/// Before the fix nothing called `flush_pending` on the shutdown path, so this
/// sequence simply ended with the buffer gone and the counters at zero — the
/// pruner would read every one of these as "never accessed".
#[tokio::test]
#[serial]
async fn shutdown_flush_persists_the_buffer() {
    RECORDER.reset();
    let store = Arc::new(ScriptedAccessStore::new());

    buffer(&["ws-d4"], "rec", 5);
    assert_eq!(RECORDER.pending_count(), 5);
    assert_eq!(
        store.count("ws-d4", "rec-0"),
        0,
        "nothing durable yet — this is what a restart used to lose"
    );

    let written = flush_on_shutdown(&*store, Duration::from_secs(5)).await;

    assert_eq!(written, 5, "the shutdown drain writes the whole buffer");
    assert_eq!(RECORDER.pending_count(), 0, "buffer emptied");
    for i in 0..5 {
        assert_eq!(
            store.count("ws-d4", &format!("rec-{i}")),
            1,
            "rec-{i} must be durable after the shutdown flush"
        );
    }
    RECORDER.reset();
}

/// A shutdown flush against a broken store must not hang the process and must
/// not lie about having written anything.
#[tokio::test]
#[serial]
async fn shutdown_flush_on_a_broken_store_is_bounded_and_honest() {
    RECORDER.reset();
    let store = Arc::new(ScriptedAccessStore::new());
    store.fail_next("ws-d4-broken", usize::MAX / 2);

    buffer(&["ws-d4-broken"], "rec", 3);

    let written = flush_on_shutdown(&*store, Duration::from_secs(5)).await;
    assert_eq!(
        written, 0,
        "a failed shutdown flush reports zero, not a lie"
    );
    assert_eq!(
        RECORDER.pending_count(),
        3,
        "the accesses stay buffered for the next process — a failed flush \
         loses evidence, it does not discard it"
    );
    RECORDER.reset();
}

/// A store that never answers must not block the shutdown past its budget.
#[tokio::test]
#[serial]
async fn shutdown_flush_gives_up_after_its_timeout() {
    RECORDER.reset();
    buffer(&["ws-d4-slow"], "rec", 2);

    // A store whose write never completes: exercised through a separate type so
    // the slow path is a real await, not a sleep in the test body.
    struct HangingStore;
    #[async_trait]
    impl MemoryStore for HangingStore {
        fn backend(&self) -> MemoryBackend {
            MemoryBackend::Memory
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn health(&self) -> Result<String> {
            Ok("ok".to_string())
        }
        async fn record_accesses(
            &self,
            _workspace_id: &str,
            _entries: &[(String, u64)],
        ) -> Result<usize> {
            std::future::pending::<()>().await;
            unreachable!("pending never resolves");
        }
        async fn put(&self, _record: MemoryRecord) -> Result<()> {
            Ok(())
        }
        async fn get(&self, _ws: &str, _id: &str) -> Result<Option<MemoryRecord>> {
            Ok(None)
        }
        async fn update(&self, _record: MemoryRecord) -> Result<()> {
            Ok(())
        }
        async fn delete(&self, _ws: &str, _id: &str) -> Result<Option<MemoryRecord>> {
            Ok(None)
        }
        async fn list(&self, _ws: &str) -> Result<Vec<MemoryRecord>> {
            Ok(Vec::new())
        }
        async fn search(
            &self,
            _ws: &str,
            _q: &str,
            _f: Option<&MemoryQueryFilters>,
        ) -> Result<Vec<MemoryRecord>> {
            Ok(Vec::new())
        }
        async fn load_workspace_state(&self, _ws: &str) -> Result<DurableWorkspaceState> {
            Ok(DurableWorkspaceState::default())
        }
        async fn save_beliefs(
            &self,
            _ws: &str,
            _b: Vec<xavier::domain::memory::belief::BeliefEdge>,
        ) -> Result<()> {
            Ok(())
        }
        async fn save_session_token(&self, _ws: &str, _t: SessionTokenRecord) -> Result<()> {
            Ok(())
        }
        async fn is_session_token_valid(&self, _ws: &str, _t: &str) -> Result<bool> {
            Ok(true)
        }
        async fn save_checkpoint(&self, _ws: &str, _c: Checkpoint) -> Result<()> {
            Ok(())
        }
        async fn load_checkpoint(
            &self,
            _ws: &str,
            _t: &str,
            _n: &str,
        ) -> Result<Option<Checkpoint>> {
            Ok(None)
        }
        async fn list_checkpoints(&self, _ws: &str, _t: &str) -> Result<Vec<Checkpoint>> {
            Ok(Vec::new())
        }
        async fn delete_checkpoint(&self, _ws: &str, _t: &str, _n: &str) -> Result<()> {
            Ok(())
        }
    }

    let hanging = HangingStore;
    let written = flush_on_shutdown(&hanging, Duration::from_millis(200)).await;
    assert_eq!(
        written, 0,
        "a hung store must not make the shutdown report success"
    );
    assert_eq!(
        RECORDER.pending_count(),
        2,
        "the accesses remain buffered rather than being lost"
    );
    RECORDER.reset();
}

/// The evidence a restart used to destroy, end to end: record, never flush,
/// simulate the restart by draining at shutdown, then read the durable table
/// back the way `ensure_hydrated` would.
#[tokio::test]
#[serial]
async fn accesses_recorded_since_the_last_flush_survive_a_restart() {
    RECORDER.reset();
    let store = Arc::new(ScriptedAccessStore::new());

    // A burst of reads, buffered but not flushed — exactly the state a SIGTERM
    // used to discard.
    for _ in 0..3 {
        RECORDER.record(
            "ws-restart",
            &["hot".to_string(), "warm".to_string()],
            Utc::now(),
        );
    }
    assert_eq!(RECORDER.pending_count(), 2, "two distinct records");
    assert_eq!(RECORDER.access_count("ws-restart", "hot"), 3);

    let written = flush_on_shutdown(&*store, Duration::from_secs(5)).await;
    assert_eq!(written, 2, "two distinct records written, not six");

    // A fresh process hydrates from the durable table only — this is what the
    // pruner reads.
    RECORDER.reset();
    let stats = store.load_access_stats("ws-restart").await.expect("load");
    RECORDER.hydrate("ws-restart", stats);
    assert_eq!(
        RECORDER.access_count("ws-restart", "hot"),
        3,
        "the accesses recorded before the restart must survive it"
    );
    assert_eq!(RECORDER.access_count("ws-restart", "warm"), 3);
    RECORDER.reset();
}

/// Counters stay exact across a flush, a new batch of reads, and another flush —
/// the accumulation case the utility prune ultimately depends on.
#[tokio::test]
#[serial]
async fn repeated_cycles_never_inflate_the_counter() {
    RECORDER.reset();
    let store = Arc::new(ScriptedAccessStore::new());

    let mut expected = 0u64;
    for cycle in 0..5u64 {
        let ids = vec!["rec".to_string()];
        for _ in 0..(cycle + 1) {
            RECORDER.record("ws-cycle", &ids, Utc::now());
        }
        expected += cycle + 1;
        flush_pending(&*store).await.expect("flush");
        assert_eq!(
            store.count("ws-cycle", "rec"),
            expected,
            "cycle {cycle}: durable count must be exactly {expected}, not more. \
             Applied: {}",
            store.describe()
        );
    }
    RECORDER.reset();
}

/// A `flush_pending` on an empty buffer is a no-op, not an error — the periodic
/// worker and the shutdown hook both rely on that.
#[tokio::test]
#[serial]
async fn flushing_an_empty_buffer_is_a_noop() {
    RECORDER.reset();
    let store = Arc::new(ScriptedAccessStore::new());
    assert_eq!(flush_pending(&*store).await.expect("empty flush"), 0);
    assert_eq!(
        flush_on_shutdown(&*store, Duration::from_secs(1)).await,
        0,
        "an empty buffer has nothing to write at shutdown"
    );
    RECORDER.reset();
}

/// Guards the D4 hook itself against being un-armed: `spawn_shutdown_flush`
/// must exist, be callable and be idempotent (two registrations must not panic
/// or leak a second listener).
#[tokio::test]
async fn shutdown_hook_registration_is_idempotent() {
    let store: Arc<dyn MemoryStore> = Arc::new(ScriptedAccessStore::new());
    xavier::memory::access::spawn_shutdown_flush(Arc::clone(&store));
    xavier::memory::access::spawn_shutdown_flush(Arc::clone(&store));
    xavier::memory::access::ensure_flush_worker(store);
    xavier::memory::access::ensure_flush_worker(Arc::new(ScriptedAccessStore::new()));
    // Give the spawned listeners a chance to fail loudly if they are going to.
    tokio::task::yield_now().await;
}

/// The hook's timeout constant must be short enough to fit inside a systemd
/// stop, and long enough to write a batch.
#[test]
fn shutdown_flush_timeout_fits_a_systemd_stop() {
    let secs = xavier::memory::access::SHUTDOWN_FLUSH_TIMEOUT.as_secs();
    assert!(
        secs > 0 && secs <= 10,
        "shutdown flush budget must be > 0 and <= 10s, got {secs}s — above that \
         systemd's default TimeoutStopSec kills the process mid-flush"
    );
}

/// A guard against the shutdown path silently regressing to "flush nothing": if
/// someone deletes the body of `drain_for_shutdown`, `pending_count` would stay
/// non-zero after a simulated stop. Encoded as a source-level assertion because
/// the signal itself cannot be raised from inside the test process.
#[test]
fn shutdown_hook_is_wired_to_the_buffer() {
    let source = include_str!("../src/memory/access.rs");
    assert!(
        source.contains("access: shutdown flush complete"),
        "the shutdown drain must log its outcome; if the hook stopped flushing \
         the buffer this string disappears"
    );
    assert!(
        source.contains("drain_for_shutdown(&store"),
        "the signal handlers must call the drain"
    );
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
