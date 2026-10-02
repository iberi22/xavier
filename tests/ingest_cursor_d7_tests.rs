//! D7: the three ingestion-cursor defects that lose data silently.
//!
//! All three are the same class of bug — the cursor advances on a *failure*
//! instead of on a success — seen from three sources:
//!
//! - **D7a (Hermes).** `remember()` ran BEFORE the import, so one failing
//!   `store.put()` left the file permanently marked as ingested. Hermes
//!   `request_dump_*.json` files never change, so their fingerprint never moved
//!   and the un-imported remainder was never retried. The rest of the transcript
//!   was gone, silently, until someone cleared the marker by hand or the process
//!   restarted. Fix: `remember` runs after a successful import.
//!
//! - **D7b (OpenCode).** `observed_max` was not bounded by the wall clock, so
//!   one row with a future `time_updated` — clock drift, wrong unit, a restored
//!   database stamped ahead — raised the watermark past `now`, and the query
//!   floor (`watermark - REPLAY_WINDOW_MS`) landed above *every* real session.
//!   Ingestion then reported "nothing to do" forever, with no error. Fix: the
//!   watermark is clamped to `now_ms` on the way in and on the way out.
//!
//! - **D7c (OpenCode).** `read_turns(&conn, &cand.id)?` returned from `sync`
//!   BEFORE `seen` and `watermark_ms` were folded back, so a single permanently
//!   unreadable row made every cycle restart from watermark 0 — the full scan
//!   the cursor exists to avoid, forever, with no signal. Fix: the session is
//!   skipped for this pass, counted in `read_errors`, and retried next pass.
//!
//! Every store here is in-memory and every database is a tempdir fixture.
//! Nothing touches the production corpus.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use async_trait::async_trait;

use xavier::checkpoint::Checkpoint;
use xavier::memory::hermes_importer::HermesImporter;
use xavier::memory::opencode_importer::OpenCodeImporter;
use xavier::memory::schema::MemoryQueryFilters;
use xavier::memory::store::{
    DurableWorkspaceState, MemoryBackend, MemoryRecord, MemoryStore, SessionTokenRecord,
};

/// Milliseconds since the Unix epoch, matching the unit the importers read.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Wall-clock ceiling for any single `sync` call made by these tests.
///
/// "The cycle does not get stuck" is not provable by a test that is itself
/// allowed to hang, so every pass below runs inside this timeout. A regression
/// that reintroduces an unbounded loop fails here with `Elapsed`, instead of
/// hanging CI: that is what the first run of this file did (both threads in
/// `futex_wait` on a self-held `std::sync::Mutex`, 0 % CPU, 41 minutes).
const PASS_DEADLINE_SECS: u64 = 30;

/// `sync` under a hard deadline, so a stuck pass fails instead of hanging.
async fn sync_bounded(
    importer: &OpenCodeImporter,
    store: &dyn MemoryStore,
) -> Result<xavier::memory::opencode_importer::OpenCodeSyncStats> {
    let fut = importer.sync(store);
    match tokio::time::timeout(std::time::Duration::from_secs(PASS_DEADLINE_SECS), fut).await {
        Ok(result) => result,
        Err(_) => anyhow::bail!(
            "sync() did not return within {}s: the pass is stuck, not slow",
            PASS_DEADLINE_SECS
        ),
    }
}

/// [`HermesImporter::sync`] under the same hard deadline, for the same reason.
async fn hermes_sync_bounded(
    importer: &HermesImporter,
    store: &dyn MemoryStore,
) -> Result<xavier::memory::hermes_importer::IngestStats> {
    let fut = importer.sync(store);
    match tokio::time::timeout(std::time::Duration::from_secs(PASS_DEADLINE_SECS), fut).await {
        Ok(result) => result,
        Err(_) => anyhow::bail!(
            "Hermes sync() did not return within {}s: the pass is stuck, not slow",
            PASS_DEADLINE_SECS
        ),
    }
}

// ───────────────────────────── a store that can fail ─────────────────────────

/// In-memory store whose `put()` can be made to fail a scripted number of
/// times.
///
/// This is what makes D7a observable. The defect is "the file was marked seen
/// even though the write failed", so the test needs a write that fails and then
/// succeeds, and needs to observe that the second pass actually reaches the
/// store. `put_attempts` counts every call, failed ones included, so a pass
/// that silently skipped the file shows up as an unchanged counter.
struct FailOnceStore {
    records: Mutex<HashMap<(String, String), MemoryRecord>>,
    /// 1-based index of the `put()` call that must fail, if any.
    ///
    /// Targeting an index rather than "the next N" is what lets a test fail the
    /// write *in the middle* of a file, which is the case D7a describes: one
    /// `store.put(...)` erroring halfway through the message loop.
    fail_on_put: Mutex<Option<usize>>,
    /// Total `put()` calls, failures included.
    put_attempts: AtomicUsize,
}

impl Default for FailOnceStore {
    fn default() -> Self {
        Self::healthy()
    }
}

impl FailOnceStore {
    fn healthy() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            fail_on_put: Mutex::new(None),
            put_attempts: AtomicUsize::new(0),
        }
    }

    /// Every write succeeds.
    fn fail_next_puts(_n: usize) -> Self {
        Self::healthy()
    }

    /// The `n`-th `put()` call (1-based) fails; all others succeed.
    fn fail_put_number(n: usize) -> Self {
        Self {
            fail_on_put: Mutex::new(Some(n)),
            ..Self::healthy()
        }
    }

    fn attempts(&self) -> usize {
        self.put_attempts.load(Ordering::SeqCst)
    }

    /// Snapshot of a workspace, as owned values.
    ///
    /// The guard is dropped before anything else can touch the map: this once
    /// held it across a `filter_map` that re-locked the same non-reentrant
    /// `std::sync::Mutex`, which deadlocks the test thread on its own guard
    /// (both threads in `futex_wait`, 0 % CPU, forever). Owning the values
    /// removes the re-entrancy entirely.
    fn rows(&self, workspace_id: &str) -> Vec<MemoryRecord> {
        self.records
            .lock()
            .expect("records lock")
            .iter()
            .filter(|((ws, _), _)| ws == workspace_id)
            .map(|(_, record)| record.clone())
            .collect()
    }
}

#[async_trait]
impl MemoryStore for FailOnceStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Memory
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn health(&self) -> Result<String> {
        Ok("ok".to_string())
    }

    async fn put(&self, record: MemoryRecord) -> Result<()> {
        let attempt = self.put_attempts.fetch_add(1, Ordering::SeqCst) + 1;
        if *self.fail_on_put.lock().expect("fail_on_put lock") == Some(attempt) {
            anyhow::bail!("scripted put failure on call {attempt}");
        }
        let key = (record.workspace_id.clone(), record.id.clone());
        self.records.lock().expect("records lock").insert(key, record);
        Ok(())
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        Ok(self
            .records
            .lock()
            .expect("records lock")
            .get(&(workspace_id.to_string(), id_or_path.to_string()))
            .cloned())
    }

    async fn update(&self, record: MemoryRecord) -> Result<()> {
        self.put(record).await
    }

    async fn delete(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        Ok(self
            .records
            .lock()
            .expect("records lock")
            .remove(&(workspace_id.to_string(), id_or_path.to_string())))
    }

    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        Ok(self.rows(workspace_id))
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

// ─────────────────────────────── Hermes fixture ───────────────────────────────

/// Writes one `request_dump_*.json` shaped like a real Hermes transcript: the
/// name never changes and neither does the content, which is exactly why the
/// fingerprint cannot rescue a file that was marked seen too early.
fn write_request_dump(dir: &std::path::Path, messages: &[(&str, &str)]) -> Result<()> {
    let msgs: Vec<serde_json::Value> = messages
        .iter()
        .map(|(role, content)| serde_json::json!({ "role": role, "content": content }))
        .collect();
    let val = serde_json::json!({
        "session_id": "req-dump-d7a",
        "request": { "body": { "model": "hermes-test", "messages": msgs } }
    });
    std::fs::write(
        dir.join("request_dump_d7a.json"),
        serde_json::to_string(&val)?,
    )?;
    Ok(())
}

/// **D7a**: a failed import must not mark the file as ingested.
///
/// The store fails the *second* `put()`, i.e. the failure lands mid-file, after
/// the first message was already written — the exact case in the defect report
/// ("falla un `store.put(...)` a mitad de línea 356"). Before the fix, the file
/// was already in `seen`, so the second pass skipped it and the second message
/// was never ingested: permanently lost, because the dump file's fingerprint
/// never changes.
#[tokio::test]
async fn d7a_failed_import_leaves_hermes_file_unseen_and_it_is_retried() -> Result<()> {
    let dir = tempfile::tempdir()?;
    write_request_dump(
        dir.path(),
        &[("user", "first message"), ("assistant", "second message")],
    )?;

    // The 2nd put fails: message 0 is already stored, message 1 is not.
    let store = FailOnceStore::fail_put_number(2);
    let importer = HermesImporter::with_dir(dir.path());

    // Pass 1: message 0 lands, message 1 fails. The pass must not report the
    // file as read, because it was only half ingested.
    let first = hermes_sync_bounded(&importer, &store).await?;
    assert_eq!(first.read, 0, "a file whose import failed is not 'read'");
    assert_eq!(store.rows("agent:hermes").len(), 1, "the first message landed");

    // Pass 2: the store is healthy again. The file must be re-read, not skipped.
    let second = hermes_sync_bounded(&importer, &store).await?;
    assert_eq!(
        second.skipped, 0,
        "a file whose import FAILED must not be marked as seen (skipped={}, read={})",
        second.skipped,
        second.read
    );
    assert_eq!(second.read, 1, "the failed file is retried on the next pass");
    assert_eq!(
        store.attempts(),
        3,
        "put() must be called a third time: the retried pass reached the store"
    );

    let rows = store.rows("agent:hermes");
    assert_eq!(
        rows.len(),
        2,
        "both messages must be in the store; the second one is recovered only \
         because the file was never marked as seen"
    );
    assert!(
        rows.iter().any(|r| r.content == "second message"),
        "the message lost to the mid-file failure must be recovered"
    );

    // And the cursor still works once the file really is fully ingested.
    let third = hermes_sync_bounded(&importer, &store).await?;
    assert_eq!(third.skipped, 1, "a fully ingested file is skipped afterwards");
    assert_eq!(third.read, 0);

    Ok(())
}

// ─────────────────────────────── OpenCode fixture ─────────────────────────────

/// OpenCode's real session/message/part schema, `time_updated` included.
fn create_oc_db(db_file: &std::path::Path) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(db_file)?;
    conn.execute_batch(
        "CREATE TABLE session (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            workspace_id TEXT,
            parent_id TEXT,
            slug TEXT NOT NULL,
            directory TEXT NOT NULL,
            path TEXT,
            title TEXT NOT NULL,
            version TEXT NOT NULL,
            share_url TEXT,
            metadata TEXT,
            cost REAL DEFAULT 0 NOT NULL,
            agent TEXT,
            model TEXT,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            time_compacting INTEGER,
            time_archived INTEGER
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT
        );
        CREATE TABLE part (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT
        );
        CREATE INDEX message_session_idx ON message (session_id, time_created, id);
        CREATE INDEX part_message_idx ON part (message_id, id);
        ",
    )?;
    Ok(conn)
}

fn insert_session(
    conn: &rusqlite::Connection,
    id: &str,
    title: &str,
    time_created: i64,
    time_updated: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO session (id, project_id, slug, directory, title, version, \
         time_created, time_updated) VALUES (?1, 'p1', ?1, '/tmp', ?2, '0.1.0', ?3, ?4)",
        rusqlite::params![id, title, time_created, time_updated],
    )?;
    Ok(())
}

fn insert_turn(
    conn: &rusqlite::Connection,
    session_id: &str,
    seq: u32,
    role: &str,
    text: &str,
    at_ms: i64,
) -> Result<()> {
    let msg_id = format!("{session_id}_m{seq}");
    let prt_id = format!("{session_id}_p{seq}");
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) \
         VALUES (?1, ?2, ?3, ?3, ?4)",
        rusqlite::params![
            msg_id,
            session_id,
            at_ms,
            serde_json::json!({ "role": role }).to_string()
        ],
    )?;
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) \
         VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
        rusqlite::params![
            prt_id,
            msg_id,
            session_id,
            at_ms,
            serde_json::json!({ "type": "text", "text": text }).to_string()
        ],
    )?;
    Ok(())
}

fn opencode_rows(store: &FailOnceStore) -> Vec<MemoryRecord> {
    store.rows("agent:opencode")
}

/// One session whose only message row carries a BLOB in `data`.
///
/// `read_turns` does `row.get::<_, String>(1)?`, which fails on a BLOB. That is
/// a faithful stand-in for a corrupt or partially written row: unreadable, and
/// unreadable *permanently*, which is what turns a transient error into a loop.
fn insert_unreadable_session(conn: &rusqlite::Connection, id: &str, at_ms: i64) -> Result<()> {
    insert_session(conn, id, "Corrupt", at_ms, at_ms)?;
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) \
         VALUES (?1, ?2, ?3, ?3, ?4)",
        rusqlite::params![
            format!("{id}_m0"),
            id,
            at_ms,
            rusqlite::types::Value::Blob(vec![0x00, 0x01, 0x02])
        ],
    )?;
    Ok(())
}

/// **D7b**: a future `time_updated` must not be able to raise the cursor.
///
/// The poisoned row is ingested normally (it has a real message), but it is
/// dated 30 days ahead. Before the fix it set `watermark_ms` to that future
/// value, so the next pass queried from `watermark - 24h`, which is 29 days in
/// the future — above every real session. A brand new session, created right
/// then, was never a candidate again: ingestion reported "0 candidates, nothing
/// to do" on every cycle, silently, until a restart.
#[tokio::test]
async fn d7b_future_time_updated_cannot_poison_the_watermark() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_file = dir.path().join("opencode.db");

    let ten_days_ms: i64 = 10 * 86_400_000;
    {
        let conn = create_oc_db(&db_file)?;
        let future_at = now_ms() + ten_days_ms;
        insert_session(&conn, "ses_future", "Clock drift", future_at - 1000, future_at)?;
        insert_turn(&conn, "ses_future", 0, "user", "future dated row", future_at)?;
    }

    let store = FailOnceStore::fail_next_puts(0);
    let importer = OpenCodeImporter::with_path(&db_file).with_hot_window(0);

    let first = sync_bounded(&importer, &store).await?;
    assert_eq!(first.read, 1, "the future-dated session is ingested");
    assert_eq!(opencode_rows(&store).len(), 1);

    // A normal session appears now. Its timestamp is far BELOW the poisoned
    // watermark, so only the replay window keeps it reachable — and the replay
    // window only helps if the watermark itself is sane.
    {
        let conn = rusqlite::Connection::open(&db_file)?;
        let now = now_ms();
        insert_session(&conn, "ses_real", "Real", now - 5000, now - 1000)?;
        insert_turn(&conn, "ses_real", 0, "user", "a normal new session", now)?;
    }

    let second = sync_bounded(&importer, &store).await?;
    assert_eq!(
        second.read, 1,
        "the real session must be deep-read; a future watermark put the query \
         floor 29 days ahead and starved the whole corpus (read={}, candidates={})",
        second.read,
        second.candidates
    );
    assert_eq!(
        second.skipped, 1,
        "only the future-dated session is skipped, on its fingerprint"
    );
    assert_eq!(second.store_writes, 1);
    assert!(
        opencode_rows(&store)
            .iter()
            .any(|r| r.content.contains("a normal new session")),
        "the session that arrived after the poisoned row must reach the store"
    );

    Ok(())
}

/// **D7c**: an unreadable session must not abort the pass or reset the cursor.
///
/// Two properties, both required:
///
/// 1. The good sessions in the same pass are ingested (`sync` returns `Ok`, not
///    `Err`) — one broken row must not cost the other 1722 sessions.
/// 2. The cursor is still folded back, so the NEXT pass does not rescan
///    everything. Before the fix the `?` returned before `seen` and
///    `watermark_ms` were written, so every cycle restarted at watermark 0 and
///    re-read the whole corpus — the busy loop the cursor exists to prevent.
///
/// The corrupt session is deliberately NOT recorded in `seen`: a read failure
/// is often transient (half-written row, locked db), and marking it seen would
/// lose it permanently — the same silent loss as D7a. So it is retried, but
/// only that one row, and every retry is counted in `read_errors`.
#[tokio::test]
async fn d7c_unreadable_session_does_not_abort_or_reset_the_cycle() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_file = dir.path().join("opencode.db");

    let old_at: i64 = 1_600_000_000_000;
    {
        let conn = create_oc_db(&db_file)?;
        insert_session(&conn, "ses_ok1", "Fine", old_at, old_at)?;
        insert_turn(&conn, "ses_ok1", 0, "user", "healthy one", old_at + 1)?;
        insert_unreadable_session(&conn, "ses_broken", old_at + 1_000)?;
        insert_session(&conn, "ses_ok2", "Fine too", old_at + 2_000, old_at + 2_000)?;
        insert_turn(&conn, "ses_ok2", 0, "user", "healthy two", old_at + 2_001)?;
    }

    let store = FailOnceStore::fail_next_puts(0);
    let importer = OpenCodeImporter::with_path(&db_file).with_hot_window(0);

    // Pass 1: the broken row must not abort the pass.
    let first = sync_bounded(&importer, &store)
        .await
        .expect("an unreadable session must not abort sync");
    assert_eq!(
        first.read_errors, 1,
        "the unreadable session is counted, not silently dropped"
    );
    assert_eq!(
        first.store_writes, 2,
        "both healthy sessions must be ingested despite the broken row"
    );
    let rows = opencode_rows(&store);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|r| r.content.contains("healthy one")));
    assert!(rows.iter().any(|r| r.content.contains("healthy two")));

    // Pass 2: the cursor must have survived. Only the broken session is retried;
    // the healthy ones are skipped by fingerprint, not re-read.
    let second = sync_bounded(&importer, &store)
        .await
        .expect("the second pass must also complete");
    assert_eq!(
        second.skipped, 2,
        "the two healthy sessions must be skipped by fingerprint; skipped={} \
         means the cursor was discarded and the pass restarted from scratch",
        second.skipped
    );
    assert_eq!(
        second.store_reads, 0,
        "an unchanged pass must not touch the store"
    );
    assert_eq!(
        second.read_errors, 1,
        "the unreadable session is retried, once per cycle, and counted"
    );
    assert_eq!(
        second.message_rows, 0,
        "the broken session's message row is never actually read; the healthy \
         ones are skipped on the fingerprint"
    );
    assert_eq!(
        opencode_rows(&store).len(),
        2,
        "retrying the broken session must not duplicate the healthy rows"
    );

    // And the loop is bounded: it does not grow with the number of cycles.
    for _ in 0..3 {
        let stats = sync_bounded(&importer, &store).await?;
        assert_eq!(stats.read, 0, "no healthy session is ever re-read");
        assert_eq!(stats.store_writes, 0);
        assert_eq!(stats.read_errors, 1);
    }
    assert_eq!(
        opencode_rows(&store).len(),
        2,
        "five cycles leave exactly two rows; the broken one is skipped, not \
         retried into the store"
    );

    Ok(())
}

/// **D7c, durability of the claim**: a permanently unreadable row must not turn
/// into a permanently stuck cycle, and the cost of the retry must not grow.
///
/// 100 consecutive passes over a corpus holding one row that can never be read.
/// Every pass is individually bounded by [`PASS_DEADLINE_SECS`]; the whole
/// sequence is bounded by an overall deadline, so "it does not get stuck" is
/// asserted in wall-clock terms and not merely by the fact that the process is
/// still running. The cost that matters is the one the cursor removed: the
/// healthy sessions must be *skipped* every pass, so message rows read stays at
/// the corrupt session alone instead of the whole corpus.
///
/// Before the fix, pass 2 returned `Err` from `read_turns` before `seen` and
/// `watermark_ms` were folded back, so pass 2 onward re-read the entire corpus
/// from watermark 0, forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn d7c_permanently_unreadable_row_never_stalls_or_grows_the_pass() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_file = dir.path().join("opencode.db");

    let old_at: i64 = 1_600_000_000_000;
    let healthy: Vec<String> = (0..25).map(|i| format!("ses_ok{i:02}")).collect();
    {
        let conn = create_oc_db(&db_file)?;
        for id in &healthy {
            insert_session(&conn, id, "Fine", old_at, old_at)?;
            insert_turn(&conn, id, 0, "user", &format!("body of {id}"), old_at + 1)?;
        }
        insert_unreadable_session(&conn, "ses_broken", old_at + 1_000)?;
    }

    let store = FailOnceStore::fail_next_puts(0);
    let importer = OpenCodeImporter::with_path(&db_file).with_hot_window(0);

    const CYCLES: usize = 100;
    let overall = std::time::Duration::from_secs(PASS_DEADLINE_SECS * 4);

    let start = std::time::Instant::now();
    let run = tokio::time::timeout(overall, async {
        // The first pass is the only one that may deep-read the healthy corpus.
        let first = sync_bounded(&importer, &store).await?;
        assert_eq!(first.store_writes, healthy.len());
        assert_eq!(first.read_errors, 1);

        let mut first_steady: Option<(usize, usize, usize)> = None;
        for cycle in 2..=CYCLES {
            let stats = sync_bounded(&importer, &store).await?;

            assert_eq!(
                stats.read_errors, 1,
                "cycle {cycle}: the corrupt row is still reported, not swallowed"
            );
            assert_eq!(
                stats.skipped, healthy.len(),
                "cycle {cycle}: every healthy session must be skipped on the \
                 fingerprint; skipped={} of {} means the cursor was reset and the \
                 pass degenerated into a full rescan",
                stats.skipped,
                healthy.len()
            );
            assert_eq!(
                stats.read, 0,
                "cycle {cycle}: no healthy session may be deep-read again"
            );
            assert_eq!(
                stats.store_reads, 0,
                "cycle {cycle}: an unchanged pass must not touch the store"
            );
            assert_eq!(
                stats.store_writes, 0,
                "cycle {cycle}: nothing changed, so nothing may be written"
            );
            assert_eq!(
                stats.candidates, healthy.len() + 1,
                "cycle {cycle}: the candidate set is index rows only and stays flat"
            );

            let cost = (stats.message_rows, stats.store_reads, stats.store_writes);
            match first_steady {
                None => first_steady = Some(cost),
                Some(first) => assert_eq!(
                    cost, first,
                    "cycle {cycle}: per-cycle cost grew from {first:?} to {cost:?}; a \
                     stuck cursor makes the cost climb every cycle"
                ),
            }
        }
        Ok::<(), anyhow::Error>(())
    })
    .await;

    match run {
        Ok(inner) => inner?,
        Err(_) => anyhow::bail!(
            "{CYCLES} cycles over {} sessions did not finish within {overall:?}: the \
             cycle is stuck, not slow",
            healthy.len()
        ),
    }

    let elapsed = start.elapsed();
    assert_eq!(
        opencode_rows(&store).len(),
        healthy.len(),
        "100 cycles leave exactly the healthy rows: the corrupt session is \
         skipped, never retried into the store, and no duplicate appears"
    );
    println!(
        "d7c: {CYCLES} cycles over {} sessions (1 permanently unreadable) in {:?}",
        healthy.len() + 1,
        elapsed
    );

    Ok(())
}
