//! Phase 1 access-instrumentation tests.
//!
//! The four mandatory behaviours, plus the bias guard for the visibility trap:
//!
//! 1. A user-facing read increments the counter.
//! 2. An internal read (GC, consolidation, backup, `store.get`) does NOT.
//! 3. `observation_started_at` is written once and stays readable.
//! 4. `get_last_accessed()` resolves from the new storage, not empty metadata.
//!
//! Every test uses a tempdir or an isolated in-memory store. Nothing here
//! touches the production database.
//!
//! ## How the mutation guard works
//!
//! `internal_reads_do_not_count_as_access` is the test that pins the whole
//! design. It asserts that GC and direct store reads leave the counter at zero.
//! If the recording call is moved from the HTTP/MCP handlers into
//! `MemoryStore::get` — the mutation this suite exists to catch — the counter
//! becomes non-zero and the test fails.

use std::sync::Arc;

use serial_test::serial;
use tokio::sync::RwLock as AsyncRwLock;

use super::*;
use crate::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};

/// Build an isolated vec store in a tempdir with instrumentation enabled.
async fn instrumented_store(tag: &str) -> (tempfile::TempDir, VecSqliteMemoryStore) {
    let dir = tempfile::tempdir().expect("test assertion");
    let path = dir.path().join(format!("{}.sqlite3", tag));
    let store = VecSqliteMemoryStore::new(VecSqliteStoreConfig {
        path,
        embedding_dimensions: 128,
    })
    .await
    .expect("store construction");
    store
        .ensure_access_instrumentation()
        .await
        .expect("instrumentation");
    (dir, store)
}

/// An isolated QmdMemory backed by a real instrumented store.
async fn workspace_with_store(
    workspace_id: &str,
    store: &VecSqliteMemoryStore,
) -> (Arc<QmdMemory>, MemoryManager) {
    let memory = Arc::new(QmdMemory::new_with_workspace(
        Arc::new(AsyncRwLock::new(Vec::new())),
        workspace_id,
    ));
    memory.set_store(Arc::new(store.clone())).await;
    let manager = MemoryManager::new(Arc::clone(&memory), None);
    (memory, manager)
}

fn record(id: &str, workspace_id: &str) -> MemoryRecord {
    MemoryRecord {
        id: id.to_string(),
        workspace_id: workspace_id.to_string(),
        path: format!("notes/{}", id),
        content: format!("content of {}", id),
        embedding: vec![0.1; 4],
        embedding_status: "completed".to_string(),
        ..Default::default()
    }
}

fn doc(id: &str) -> MemoryDocument {
    MemoryDocument {
        id: Some(id.to_string()),
        path: format!("notes/{}", id),
        content: format!("content of {}", id),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// 1. A user-facing read counts
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn user_read_increments_access_count() {
    RECORDER.reset();
    let (_dir, store) = instrumented_store("access_user_read").await;
    let (memory, manager) = workspace_with_store("ws-access", &store).await;

    assert_eq!(RECORDER.access_count("ws-access", "doc-a"), 0);

    let n = record_user_access(&manager, &memory, &[doc("doc-a"), doc("doc-b")]).await;
    assert_eq!(n, 2, "both returned documents must be recorded");
    assert_eq!(RECORDER.access_count("ws-access", "doc-a"), 1);
    assert_eq!(RECORDER.access_count("ws-access", "doc-b"), 1);

    // The counter must be durable, not just in RAM.
    flush_pending(&store).await.expect("flush");
    let durable = store.load_access_stats("ws-access").await.expect("load");
    let a = durable
        .iter()
        .find(|(id, _)| id == "doc-a")
        .expect("doc-a persisted");
    assert_eq!(a.1.access_count, 1);
    assert!(a.1.last_accessed_at.is_some(), "recency must be recorded");
    assert!(a.1.first_accessed_at.is_some());
    RECORDER.reset();
}

#[tokio::test]
#[serial]
async fn repeated_reads_accumulate_not_overwrite() {
    RECORDER.reset();
    let (_dir, store) = instrumented_store("access_repeat").await;
    let (memory, manager) = workspace_with_store("ws-repeat", &store).await;

    // Well below DEFAULT_FLUSH_BATCH so the background flush cannot race this
    // test and drain the buffer early.
    for _ in 0..3 {
        record_user_access_ids(&manager, &memory, "ws-repeat", &["doc-x".to_string()]).await;
    }
    assert_eq!(
        RECORDER.access_count("ws-repeat", "doc-x"),
        3,
        "counter must accumulate across reads"
    );
    assert_eq!(
        RECORDER.pending_count(),
        1,
        "repeat reads of one record buffer as a single pending delta"
    );

    flush_pending(&store).await.expect("flush");
    let durable = store.load_access_stats("ws-repeat").await.expect("load");
    assert_eq!(
        durable
            .iter()
            .find(|(id, _)| id == "doc-x")
            .expect("persisted")
            .1
            .access_count,
        3,
        "the durable counter must accumulate too, not overwrite"
    );
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// 2. An internal read does NOT count  (the mutation target)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn internal_reads_do_not_count_as_access() {
    RECORDER.reset();
    let (_dir, store) = instrumented_store("access_internal").await;
    let workspace_id = "ws-internal";

    store
        .put(record("doc-hot", workspace_id))
        .await
        .expect("put");
    store
        .put(record("doc-cold", workspace_id))
        .await
        .expect("put");

    let (memory, manager) = workspace_with_store(workspace_id, &store).await;

    // (a) A direct store read — exactly what consolidation / backup / the
    //     ingestion path does. Must not be treated as utility.
    let fetched = store.get(workspace_id, "doc-hot").await.expect("get");
    assert!(fetched.is_some(), "record must exist");

    // (b) A full internal enumeration — what `garbage_collect` and the TGD
    //     pruner do when they walk candidates.
    let listed = store.list(workspace_id).await.expect("list");
    assert_eq!(listed.len(), 2);

    // (c) GC itself, which also runs the utility pruner.
    manager.garbage_collect().await.expect("gc");

    // (d) The workspace document pool, an in-memory internal read.
    let _ = memory.all_documents().await;

    assert_eq!(
        RECORDER.access_count(workspace_id, "doc-hot"),
        0,
        "MUTATION GUARD: an internal read must never count as utility. If this \
         fails, the recording call was moved into MemoryStore::get."
    );
    assert_eq!(
        RECORDER.access_count(workspace_id, "doc-cold"),
        0,
        "an internal enumeration must never count as utility"
    );
    assert_eq!(
        RECORDER.pending_count(),
        0,
        "internal reads must not enqueue anything for flushing"
    );
    RECORDER.reset();
}

#[tokio::test]
#[serial]
async fn gc_leaves_the_observation_signal_untouched() {
    RECORDER.reset();
    let (_dir, store) = instrumented_store("access_gc").await;
    let workspace_id = "ws-gc";
    let (_memory, manager) = workspace_with_store(workspace_id, &store).await;

    // Several GC passes, as a scheduled maintenance loop would run. None of
    // them may make a record look "used".
    for _ in 0..3 {
        manager.garbage_collect().await.expect("gc");
    }

    assert_eq!(RECORDER.pending_count(), 0);
    assert_eq!(RECORDER.access_count(workspace_id, "anything"), 0);
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// 3. The observation clock
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn observation_started_at_is_written_once_and_readable() {
    let (_dir, store) = instrumented_store("access_clock").await;

    let first = observation_started_at(&store).await.expect("read");
    let before = first.expect("ensure_instrumentation must seed the clock");

    // Re-running must not move the clock: a restart that re-seeded it would
    // silently restart every candidate's observation window.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    store
        .ensure_access_instrumentation()
        .await
        .expect("re-instrument");
    let after = observation_started_at(&store)
        .await
        .expect("read")
        .expect("still seeded");

    assert_eq!(
        before, after,
        "observation_started_at must be written exactly once"
    );
    assert!(
        (Utc::now() - before).num_days().abs() <= 1,
        "the seeded clock must be ~now, got {before}"
    );
    assert!(before.timestamp() > 0);
}

#[test]
fn prune_is_blocked_until_the_observation_window_elapses() {
    let now = Utc::now();

    // No clock at all: absence of evidence is not evidence of calm.
    assert_eq!(
        prune_block_reason(None, now),
        PruneBlockReason::ObservationClockMissing
    );

    // Clock present but young.
    assert_eq!(
        prune_block_reason(Some(now - chrono::Duration::days(3)), now),
        PruneBlockReason::ObservationWindowOpen {
            elapsed_days: 3,
            required_days: MIN_OBSERVATION_DAYS
        }
    );

    // Exactly at the boundary the requirement is satisfied: "30 days of
    // observation before deleting" means after 30 full days, not more.
    assert_eq!(
        prune_block_reason(
            Some(now - chrono::Duration::days(MIN_OBSERVATION_DAYS)),
            now
        ),
        PruneBlockReason::Eligible,
        "the window is satisfied once MIN_OBSERVATION_DAYS full days elapsed"
    );

    // One day short of it, the delete is still blocked.
    assert_eq!(
        prune_block_reason(
            Some(now - chrono::Duration::days(MIN_OBSERVATION_DAYS - 1)),
            now
        ),
        PruneBlockReason::ObservationWindowOpen {
            elapsed_days: MIN_OBSERVATION_DAYS - 1,
            required_days: MIN_OBSERVATION_DAYS
        }
    );

    // A clock in the future (clock skew, restore from a later host) must never
    // grant eligibility.
    assert!(matches!(
        prune_block_reason(Some(now + chrono::Duration::days(5)), now),
        PruneBlockReason::ObservationWindowOpen {
            elapsed_days: 0,
            ..
        }
    ));
}

#[tokio::test]
#[serial]
async fn prune_precondition_reports_the_binding_constraint() {
    RECORDER.reset();
    let (_dir, store) = instrumented_store("access_precondition").await;
    let now = Utc::now();

    // Clock seeded at construction -> window just opened, not eligible.
    let observable = record("doc-vec", "ws-pre");
    assert!(record_is_observable(&observable));
    assert!(matches!(
        prune_precondition(&store, &observable, now)
            .await
            .expect("precondition"),
        PruneBlockReason::ObservationWindowOpen { .. }
    ));

    // A record with no embedding can never enter a top-k, so its zero count
    // says nothing about usefulness. It must be blocked regardless of the clock.
    let mut invisible = record("doc-lexical", "ws-pre");
    invisible.embedding = Vec::new();
    invisible.embedding_status = "pending".to_string();
    assert!(!record_is_observable(&invisible));
    assert_eq!(
        prune_precondition(&store, &invisible, now)
            .await
            .expect("precondition"),
        PruneBlockReason::RecordNotObservable,
        "unembedded records must never be prune candidates: they lost their \
         embedding, they did not stop being useful"
    );
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// 4. get_last_accessed reads the new storage
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn get_last_accessed_resolves_from_the_access_signal_not_metadata() {
    RECORDER.reset();
    let (_dir, store) = instrumented_store("access_recency").await;
    let workspace_id = "ws-recency";
    let (memory, manager) = workspace_with_store(workspace_id, &store).await;

    let mut stored = record("doc-rec", workspace_id);
    // This is the production shape: metadata carries only `encrypted`.
    stored.metadata = serde_json::json!({ "encrypted": "deadbeef" });
    // updated_at is deliberately old, so a metadata-only resolver would report
    // a stale age and the decay/prune math would be wrong.
    stored.updated_at = Utc::now() - chrono::Duration::days(90);
    store.put(stored).await.expect("put");

    let loaded = store
        .get(workspace_id, "doc-rec")
        .await
        .expect("get")
        .expect("record");

    // Before any observation the legacy fallback applies.
    assert_eq!(
        get_last_accessed(&loaded),
        loaded.updated_at,
        "with no observation the resolver must fall back to updated_at"
    );

    // Now a user reads it.
    record_user_access(&manager, &memory, &[doc("doc-rec")]).await;

    let resolved = get_last_accessed(&loaded);
    assert!(
        resolved > loaded.updated_at,
        "ADVERSARY: the access signal must win over metadata/updated_at. Got \
         {resolved}; a metadata-only resolver would give {}",
        loaded.updated_at
    );
    assert!(
        (Utc::now() - resolved).num_days().abs() <= 1,
        "resolved recency must be ~now, got {resolved}"
    );

    // And the same must hold for the age the TGD pruner actually computes.
    let age_days = crate::memory::tgd::get_record_age_days(&loaded, Utc::now());
    assert!(
        age_days < 1.0,
        "the pruner's age must come from the access signal, not from the \
         90-day-old updated_at. Got {age_days}"
    );
    RECORDER.reset();
}

#[tokio::test]
#[serial]
async fn access_signal_survives_a_recorder_reset_and_rehydrate() {
    RECORDER.reset();
    let (_dir, store) = instrumented_store("access_hydrate").await;
    let workspace_id = "ws-hydrate";

    store
        .record_accesses(workspace_id, &[("doc-h".to_string(), 5)])
        .await
        .expect("record");

    // A fresh process starts with an empty in-memory view; hydration must
    // restore the durable counter rather than losing the history.
    RECORDER.reset();
    let (memory, _manager) = workspace_with_store(workspace_id, &store).await;
    ensure_hydrated(&memory).await;

    assert_eq!(
        RECORDER.access_count(workspace_id, "doc-h"),
        5,
        "durable access history must be restored into the in-memory view"
    );

    // Hydration is once-per-workspace: a second call must not overwrite the
    // live buffer with the older durable snapshot.
    record_user_access_ids(&_manager, &memory, workspace_id, &["doc-h".to_string()]).await;
    assert_eq!(RECORDER.access_count(workspace_id, "doc-h"), 6);
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// Write amplification
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn many_reads_batch_into_one_flush() {
    RECORDER.reset();
    let (_dir, store) = instrumented_store("access_batch").await;
    let (memory, manager) = workspace_with_store("ws-batch", &store).await;

    // 200 distinct records touched by a handful of top-k reads: one batch, not
    // 200 statements. Kept under DEFAULT_FLUSH_BATCH so the background flush
    // cannot race this test.
    let ids: Vec<String> = (0..200).map(|i| format!("doc-{}", i)).collect();
    record_user_access_ids(&manager, &memory, "ws-batch", &ids).await;

    assert_eq!(
        RECORDER.pending_count(),
        200,
        "each distinct record must appear exactly once in the buffer"
    );

    // A second read of the same records adds no new pending entries.
    record_user_access_ids(&manager, &memory, "ws-batch", &ids).await;
    assert_eq!(RECORDER.pending_count(), 200);
    assert_eq!(RECORDER.access_count("ws-batch", "doc-0"), 2);

    let written = flush_pending(&store).await.expect("flush");
    assert_eq!(written, 200, "one flush drains the whole batch");
    assert_eq!(RECORDER.pending_count(), 0);

    let durable = store.load_access_stats("ws-batch").await.expect("load");
    assert_eq!(durable.len(), 200);
    assert_eq!(
        durable
            .iter()
            .find(|(id, _)| id == "doc-0")
            .expect("doc-0")
            .1
            .access_count,
        2
    );
    RECORDER.reset();
}

#[test]
fn flush_is_due_once_the_batch_threshold_is_reached() {
    let recorder = AccessRecorder::default();
    assert!(!recorder.should_flush(), "an empty buffer is never due");

    let ids: Vec<String> = (0..DEFAULT_FLUSH_BATCH)
        .map(|i| format!("id-{i}"))
        .collect();
    recorder.record("ws-threshold", &ids, Utc::now());
    assert_eq!(recorder.pending_count(), DEFAULT_FLUSH_BATCH);
    assert!(
        recorder.should_flush(),
        "{} distinct records must trigger a flush regardless of elapsed time",
        DEFAULT_FLUSH_BATCH
    );
}

#[test]
fn a_restored_flush_keeps_the_recorded_accesses() {
    // The failure path in `flush_pending`: whatever was drained goes back into
    // the buffer, so a failed write never destroys evidence.
    let recorder = AccessRecorder::default();
    recorder.record(
        "ws-restore",
        &["a".to_string(), "b".to_string()],
        Utc::now(),
    );

    let drained = recorder.drain_pending();
    assert_eq!(recorder.pending_count(), 0);
    assert_eq!(AccessRecorder::flatten(&drained).len(), 2);

    recorder.restore_pending(AccessRecorder::flatten(&drained));
    assert_eq!(
        recorder.pending_count(),
        2,
        "a failed flush must not lose the recorded access"
    );
    assert_eq!(
        recorder.access_count("ws-restore", "a"),
        1,
        "the in-memory view stays correct even when persistence fails"
    );
}

// ---------------------------------------------------------------------------
// Bias guards on eligibility
// ---------------------------------------------------------------------------

#[test]
fn only_observable_records_are_prune_eligible() {
    let with_status = MemoryRecord {
        embedding: Vec::new(),
        embedding_status: "completed".to_string(),
        ..Default::default()
    };
    assert!(
        record_is_observable(&with_status),
        "a completed embedding status is observable even if the blob is absent \
         from this projection"
    );

    let with_vector = MemoryRecord {
        embedding: vec![0.1, 0.2],
        embedding_status: "pending".to_string(),
        ..Default::default()
    };
    assert!(record_is_observable(&with_vector));

    let neither = MemoryRecord {
        embedding: Vec::new(),
        embedding_status: "pending".to_string(),
        ..Default::default()
    };
    assert!(
        !record_is_observable(&neither),
        "a record with no embedding can never enter a top-k; its zero access \
         count is uninformative and it must never become a prune candidate"
    );
}

#[test]
fn recorder_reports_accumulated_totals_independently_of_flushes() {
    let recorder = AccessRecorder::default();
    let now = Utc::now();
    let ids = vec!["a".to_string(), "b".to_string()];

    recorder.record("ws", &ids, now);
    recorder.record("ws", &ids, now);

    assert_eq!(recorder.access_count("ws", "a"), 2);
    assert_eq!(
        recorder.pending_count(),
        2,
        "buffer holds distinct ids only"
    );

    let stats = recorder.stats("ws", "a").expect("stats");
    assert_eq!(stats.access_count, 2);
    assert!(stats.first_accessed_at.is_some());
    assert!(stats.last_accessed_at.is_some());
    assert_eq!(recorder.access_count("other-ws", "a"), 0);
}
