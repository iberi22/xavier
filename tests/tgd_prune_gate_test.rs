//! The prune gate applied to the production pruner (D3).
//!
//! `TgdUtilityPruner::prune_memories` runs unattended every 24h via
//! `MemoryDaemon`, and before this gate was connected it deleted on heuristic
//! utility alone — while `prune_precondition` / `record_is_observable` sat
//! unused in `crate::memory::access`. These tests pin the gate's contract
//! against a **real** instrumented SQLite store, because the interesting
//! failure modes (missing clock, unobservable record, never-observed workspace)
//! are exactly the ones an in-memory double would paper over.
//!
//! The invariant under test, stated once: **every rule in the gate may only
//! keep a memory. Nothing is ever deleted on absent evidence.**
//!
//! Each test owns a tempdir, and the ones that need an *elapsed* observation
//! window pre-seed `maintenance_meta` before the store is built. The production
//! seed is `INSERT OR IGNORE` precisely so the clock is immutable, which means a
//! pre-existing row survives construction untouched — no test has to reach into
//! a live database to age it.

use serial_test::serial;
use xavier::memory::access::{self, RECORDER};
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::store::{MemoryRecord, MemoryStore};
use xavier::memory::tgd::{TgdPruneConfig, TgdUtilityPruner};

/// An isolated instrumented store in a tempdir.
///
/// `age_days` pre-seeds `observation_started_at` that many days in the past,
/// i.e. `None` leaves a freshly opened window, `Some(45)` starts with the window
/// already elapsed.
async fn store_named(
    tag: &str,
    age_days: Option<i64>,
) -> (tempfile::TempDir, VecSqliteMemoryStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("{tag}.sqlite3"));

    if let Some(days) = age_days {
        seed_aged_clock(&path, days);
    }

    let store = VecSqliteMemoryStore::new(VecSqliteStoreConfig {
        path,
        embedding_dimensions: 8,
    })
    .await
    .expect("store construction");
    (dir, store)
}

/// Clear a record's embedding directly in the database.
///
/// Necessary because `VecSqliteMemoryStore::put` calls `put_embed`, which
/// generates an embedding for any record that lacks one. An "unobservable"
/// record therefore cannot be produced through the public write path — it only
/// exists in the wild (failed embedder, model change, pending reindex), so the
/// fixture has to reproduce that state the same way: by writing the row.
async fn null_the_embedding(
    dir: &tempfile::TempDir,
    tag: &str,
    workspace_id: &str,
    record_id: &str,
) {
    let path = dir.path().join(format!("{tag}.sqlite3"));
    let conn = rusqlite::Connection::open(&path).expect("open store db");
    let changed = conn
        .execute(
            "UPDATE memory_records SET embedding = NULL, embedding_status = 'pending'
             WHERE workspace_id = ?1 AND path = ?2",
            rusqlite::params![
                workspace_id.to_string(),
                format!("session/cron_{record_id}")
            ],
        )
        .expect("null embedding");
    assert_eq!(changed, 1, "the row to unblind must exist exactly once");
    conn.close().expect("close");
}

/// Create the DB with a back-dated observation clock, before the store exists.
///
/// The store's own seeding uses `INSERT OR IGNORE`, so this row is what survives.
fn seed_aged_clock(path: &std::path::Path, days: i64) {
    let started = chrono::Utc::now() - chrono::Duration::days(days);
    let conn = rusqlite::Connection::open(path).expect("open new db");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS maintenance_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );",
    )
    .expect("create maintenance_meta");
    conn.execute(
        "INSERT OR IGNORE INTO maintenance_meta (key, value, updated_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            access::OBSERVATION_STARTED_AT_KEY,
            started.to_rfc3339(),
            started.to_rfc3339()
        ],
    )
    .expect("seed clock");
    conn.close().expect("close");
}

/// A low-utility, clearly-old candidate. `updated_at` drives
/// `get_record_age_days` through the `get_last_accessed` fallback chain, so it
/// must be well past `min_age_days` for the record to be a candidate at all.
///
/// Path `session/cron_*` keeps the heuristic utility at 0.15, below the 0.3
/// threshold, without relying on a metadata score.
fn low_utility_old(id: &str, workspace_id: &str) -> MemoryRecord {
    let now = chrono::Utc::now();
    MemoryRecord {
        id: id.to_string(),
        workspace_id: workspace_id.to_string(),
        path: format!("session/cron_{id}"),
        content: format!("transient low utility artifact {id}"),
        embedding: vec![0.1; 8],
        embedding_status: "completed".to_string(),
        created_at: now - chrono::Duration::days(60),
        updated_at: now - chrono::Duration::days(60),
        ..Default::default()
    }
}

fn pruner(floor: usize) -> TgdUtilityPruner {
    TgdUtilityPruner::new(TgdPruneConfig {
        utility_threshold: 0.3,
        min_age_days: 7.0,
        safety_retention_floor: floor,
    })
}

// ---------------------------------------------------------------------------
// 1. The gate holds the line while the observation window is open
// ---------------------------------------------------------------------------

/// A fresh store has its clock seeded `now`, so the window is open. Candidates
/// the heuristic alone would happily delete must all survive.
#[tokio::test]
#[serial]
async fn open_observation_window_deletes_nothing() {
    RECORDER.reset();
    let (_dir, store) = store_named("gate_open_window", None).await;
    let ws = "ws-gate-open";

    for i in 0..4 {
        store
            .put(low_utility_old(&format!("cand-{i}"), ws))
            .await
            .expect("put");
    }

    let summary = pruner(0).prune_memories(&store, ws).await.expect("prune");

    assert_eq!(
        summary.pruned_count, 0,
        "an open observation window must delete nothing, pruned {:?}",
        summary.pruned_ids
    );
    assert_eq!(
        summary.gate_block_reason.as_deref(),
        Some("ObservationWindowOpen { elapsed_days: 0, required_days: 30 }"),
        "the gate must report WHY it blocked"
    );
    assert_eq!(summary.total_processed, 4, "records were still evaluated");

    for i in 0..4 {
        assert!(
            store
                .get(ws, &format!("cand-{i}"))
                .await
                .expect("get")
                .is_some(),
            "cand-{i} must survive an open window"
        );
    }
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// 2. No evidence anywhere in the workspace => no prune at all
// ---------------------------------------------------------------------------

/// The exact hazard the review flagged: a restart leaves the in-memory buffer
/// empty, so every record looks "never read". Deleting on that basis would
/// remove precisely the unobserved memories. With an elapsed clock and zero
/// access rows, the gate must refuse the whole workspace.
#[tokio::test]
#[serial]
async fn elapsed_window_but_no_evidence_prunes_nothing() {
    RECORDER.reset();
    let (_dir, store) = store_named("gate_no_evidence", Some(45)).await;
    let ws = "ws-gate-no-evidence";

    for i in 0..4 {
        store
            .put(low_utility_old(&format!("cand-{i}"), ws))
            .await
            .expect("put");
    }

    let summary = pruner(0).prune_memories(&store, ws).await.expect("prune");

    assert_eq!(
        summary.gate_block_reason.as_deref(),
        Some("no_access_evidence_for_workspace"),
        "an unobserved workspace must be refused wholesale"
    );
    assert_eq!(
        summary.pruned_count, 0,
        "nothing may be deleted when nothing was ever observed"
    );

    for i in 0..4 {
        assert!(
            store
                .get(ws, &format!("cand-{i}"))
                .await
                .expect("get")
                .is_some(),
            "cand-{i} must survive: unobserved is not the same as unused"
        );
    }
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// 3. Evidence of use protects, even against a passing gate
// ---------------------------------------------------------------------------

/// An observed record is demonstrably needed and must be shielded even though
/// the heuristic selects it and the clock has elapsed.
#[tokio::test]
#[serial]
async fn observed_candidate_is_shielded() {
    RECORDER.reset();
    let (_dir, store) = store_named("gate_observed", Some(45)).await;
    let ws = "ws-gate-observed";

    store
        .put(low_utility_old("read-me", ws))
        .await
        .expect("put");
    store
        .put(low_utility_old("nobody-read", ws))
        .await
        .expect("put");

    // Durable access evidence for exactly one of them — which also makes the
    // workspace "observed", so gate 4 does not short-circuit the run.
    store
        .record_accesses(ws, &[("read-me".to_string(), 7u64)])
        .await
        .expect("record_accesses");

    let summary = pruner(0).prune_memories(&store, ws).await.expect("prune");

    assert!(
        store.get(ws, "read-me").await.expect("get").is_some(),
        "a record that was actually read must never be pruned"
    );
    assert_eq!(
        summary.pruned_ids,
        vec!["nobody-read".to_string()],
        "only the unobserved candidate may go"
    );
    assert_eq!(summary.pruned_count, 1);
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// 4. An unflushed in-memory access also shields
// ---------------------------------------------------------------------------

/// `maybe_flush` batches every 256 records or 30s, so an access recorded
/// moments ago may still be only in RAM. The gate reads the live buffer in
/// addition to the durable table precisely so that window is not a hole.
#[tokio::test]
#[serial]
async fn unflushed_buffered_access_shields_the_record() {
    RECORDER.reset();
    let (_dir, store) = store_named("gate_buffered", Some(45)).await;
    let ws = "ws-gate-buffered";

    store
        .put(low_utility_old("just-read", ws))
        .await
        .expect("put");
    store.put(low_utility_old("stale", ws)).await.expect("put");

    // Buffer-only: deliberately NOT flushed.
    RECORDER.record(ws, &["just-read".to_string()], chrono::Utc::now());
    assert_eq!(
        store.load_access_stats(ws).await.expect("load").len(),
        0,
        "precondition: the durable table is empty for this workspace"
    );

    let summary = pruner(0).prune_memories(&store, ws).await.expect("prune");

    assert!(
        store.get(ws, "just-read").await.expect("get").is_some(),
        "a buffered-but-unflushed access must still shield its record"
    );
    assert_eq!(summary.pruned_ids, vec!["stale".to_string()]);
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// 5. Unobservable records are never candidates
// ---------------------------------------------------------------------------

/// A record with no embedding can never enter a top-k, so its zero access
/// count says nothing about utility. Pruning it would delete exactly the
/// memories that lost their embedding — the opposite of the intent.
#[tokio::test]
#[serial]
async fn unobservable_record_is_never_a_candidate() {
    RECORDER.reset();
    let (dir, store) = store_named("gate_unobservable", Some(45)).await;
    let ws = "ws-gate-unobservable";

    store
        .put(low_utility_old("embedded", ws))
        .await
        .expect("put");

    let mut blind = low_utility_old("no-embedding", ws);
    blind.embedding = Vec::new();
    blind.embedding_status = "pending".to_string();
    store.put(blind).await.expect("put");

    // `put_embed` regenerates any missing embedding, so blind the row the way
    // the wild state actually arises: by writing the column directly.
    null_the_embedding(&dir, "gate_unobservable", ws, "no-embedding").await;

    // Observable workspace evidence for someone, but not for `no-embedding`.
    store
        .record_accesses(ws, &[("embedded".to_string(), 3u64)])
        .await
        .expect("record_accesses");

    let summary = pruner(0).prune_memories(&store, ws).await.expect("prune");

    assert!(
        store.get(ws, "no-embedding").await.expect("get").is_some(),
        "a record with no embedding must be preserved regardless of utility"
    );
    // And it was the *observable* one that was shielded, so the run did proceed.
    assert_eq!(summary.pruned_count, 0, "the embedded record is in use");
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// 6. Pinned records and the retention floor still apply with the gate open
// ---------------------------------------------------------------------------

/// The gate is additive: it must not have rewritten the selection logic. A
/// pinned record survives even with window elapsed and evidence present, and
/// the safety floor still caps the deletions.
#[tokio::test]
#[serial]
async fn pinned_and_floor_still_apply_under_the_gate() {
    RECORDER.reset();
    let (_dir, store) = store_named("gate_pinned", Some(45)).await;
    let ws = "ws-gate-pinned";

    let mut pinned = low_utility_old("pinned", ws);
    pinned.metadata = serde_json::json!({"pinned": true});
    store.put(pinned).await.expect("put");

    for i in 0..5 {
        store
            .put(low_utility_old(&format!("plain-{i}"), ws))
            .await
            .expect("put");
    }
    store
        .record_accesses(ws, &[("someone".to_string(), 1u64)])
        .await
        .expect("record_accesses");

    // 6 records, floor of 3 => at most 3 deletions.
    let summary = pruner(3).prune_memories(&store, ws).await.expect("prune");

    assert!(
        summary.pruned_count <= 3,
        "safety floor violated: pruned {}",
        summary.pruned_count
    );
    assert!(
        store.get(ws, "pinned").await.expect("get").is_some(),
        "a pinned record must survive even with the gate fully open"
    );
    assert!(
        !summary.pruned_ids.iter().any(|id| id == "pinned"),
        "pinned must not appear in pruned_ids"
    );
    RECORDER.reset();
}

// ---------------------------------------------------------------------------
// 7. The original threshold matrix, now reachable behind an open gate
// ---------------------------------------------------------------------------

/// The full utility/age/floor matrix, with the observation window elapsed and
/// the workspace genuinely observed. This is the coverage the in-module
/// `InMemoryMemoryStore` test can no longer provide: that store has no clock, so
/// it can only ever reach the blocked path.
///
/// Matrix:
/// - r1 pinned + critical  -> preserved (metadata)
/// - r2..r6 low utility, old -> candidates
/// - r7 high utility, old   -> retained (utility)
/// - r8 low utility, fresh  -> retained (min_age_days)
#[tokio::test]
#[serial]
async fn threshold_matrix_behind_an_open_gate() {
    RECORDER.reset();
    let (_dir, store) = store_named("gate_matrix", Some(45)).await;
    let ws = "ws-gate-matrix";
    let now = chrono::Utc::now();

    let mut pinned = low_utility_old("r1_critical", ws);
    pinned.metadata = serde_json::json!({
        "pinned": true,
        "last_accessed_at": (now - chrono::Duration::days(30)).to_rfc3339(),
    });
    store.put(pinned).await.expect("put");

    for i in 2..=6 {
        let mut rec = low_utility_old(&format!("r{i}"), ws);
        rec.metadata = serde_json::json!({
            "last_accessed_at": (now - chrono::Duration::days(15)).to_rfc3339(),
        });
        store.put(rec).await.expect("put");
    }

    // High utility but old: `docs/` heuristic scores 0.80.
    let mut high = low_utility_old("r7_high_utility", ws);
    high.path = "docs/manual.md".to_string();
    high.metadata = serde_json::json!({
        "last_accessed_at": (now - chrono::Duration::days(20)).to_rfc3339(),
    });
    store.put(high).await.expect("put");

    // Low utility but fresh: below min_age_days.
    let mut fresh = low_utility_old("r8_fresh", ws);
    fresh.metadata = serde_json::json!({
        "last_accessed_at": (now - chrono::Duration::days(1)).to_rfc3339(),
    });
    store.put(fresh).await.expect("put");

    // Workspace is observed, but none of the candidates above were read.
    store
        .record_accesses(ws, &[("observer".to_string(), 1u64)])
        .await
        .expect("record_accesses");

    let summary = pruner(6).prune_memories(&store, ws).await.expect("prune");

    assert_eq!(summary.total_processed, 8, "the eight matrix records");
    // Candidates are r2..r6 (low utility + old): 5 of them. The retention floor
    // of 6 over 8 records caps deletions at 2 regardless.
    assert_eq!(
        summary.pruned_count, 2,
        "the safety floor must cap the candidate list"
    );
    assert!(
        summary.gate_block_reason.is_none(),
        "the gate cleared the run; any cap must come from the floor, not the gate"
    );

    for id in ["r1_critical", "r7_high_utility", "r8_fresh"] {
        assert!(
            store.get(ws, id).await.expect("get").is_some(),
            "{id} must be preserved by its own rule, not by the floor"
        );
    }

    // With the floor lifted, the three un-read candidates that survived the
    // first pass become prunable, and the three protected ones still stand.
    let summary = pruner(0).prune_memories(&store, ws).await.expect("prune");
    assert_eq!(
        summary.pruned_count, 3,
        "the remaining 3 of the 5 candidates go once the floor is lifted"
    );
    assert!(store.get(ws, "r2").await.expect("get").is_none());
    assert!(store.get(ws, "r3").await.expect("get").is_none());
    for id in ["r1_critical", "r7_high_utility", "r8_fresh"] {
        assert!(
            store.get(ws, id).await.expect("get").is_some(),
            "{id} must survive even with no retention floor at all"
        );
    }
    RECORDER.reset();
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
