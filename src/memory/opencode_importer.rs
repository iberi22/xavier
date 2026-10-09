//! OpenCode Sessions Importer
//!
//! Connects to ~/.local/share/opencode/opencode.db (or `OPENCODE_DB_PATH` env var)
//! and imports OpenCode sessions, messages, and text parts into Xavier `MemoryStore`
//! under `opencode://` paths.

use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

use crate::embedding::Embedder;
use crate::kernel::filters::strip_ansi;
use crate::memory::ingest_cursor;
use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};

/// Default quiet period a session must pass before `sync` will re-import it.
///
/// A session touched within this window is treated as live: it is remembered
/// but not read, because it is still accumulating turns and re-embedding it on
/// every cycle re-writes the whole transcript each time.
pub const DEFAULT_HOT_WINDOW_MS: i64 = 300_000;

/// How far back the cursor always looks, regardless of how high the watermark
/// has climbed.
///
/// A pure "highest `time_updated` seen" watermark is unsound: a session whose
/// `time_updated` fails to advance — a frozen clock, a backdated restore, a
/// writer that only stamps on create — keeps a value below the watermark and
/// would never be looked at again. Replaying a bounded tail costs one indexed
/// row per recent session and makes the skip purely fingerprint-driven, so
/// staleness is bounded by this window rather than unbounded.
pub const REPLAY_WINDOW_MS: i64 = 86_400_000;

/// A single message turn in an OpenCode session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeTurn {
    pub role: String,
    pub content: String,
}

/// A parsed OpenCode session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeSession {
    pub session_id: String,
    pub title: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub time_created: Option<i64>,
    /// `session.time_updated` in milliseconds since the Unix epoch, as written
    /// by OpenCode. Drives the incremental cursor; `None` on legacy schemas
    /// that lack the column.
    pub time_updated: Option<i64>,
    pub turns: Vec<OpenCodeTurn>,
}

/// Change evidence for one session, kept between passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct SessionFingerprint {
    /// Milliseconds since the Unix epoch, or 0 on a schema without the column.
    time_updated: i64,
    /// Number of messages in the session. Second signal, and the only one on a
    /// legacy schema: a session that grows messages is a session that changed.
    message_count: i64,
}

/// Cursor state. Persisted when a cursor path is configured; without one it is
/// process-local and a restart costs one full pass, never a missed import.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct CursorState {
    /// Highest `time_updated` observed. The next pass queries from
    /// `watermark_ms - REPLAY_WINDOW_MS`, not from this value, so a session
    /// whose timestamp stalls cannot fall behind it permanently.
    watermark_ms: i64,
    /// Fingerprint per session as last imported.
    seen: HashMap<String, SessionFingerprint>,
    /// Live sessions held back by the hot window, re-checked by id only.
    deferred: HashMap<String, i64>,
    /// Database the cursor above describes. If the resolved path changes (the
    /// directory appeared late, an env var was fixed) the cursor belongs to a
    /// different file and is discarded rather than trusted.
    path: Option<PathBuf>,
}

/// What one `sync` pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OpenCodeSyncStats {
    /// Session rows returned by the cursor query, including hot-window hits.
    /// These are index rows only: no messages or parts were read for them.
    pub candidates: usize,
    /// Sessions deep-read (messages and parts fetched).
    pub read: usize,
    /// Sessions skipped because their fingerprint was unchanged.
    pub skipped: usize,
    /// Sessions held back because they were still within the hot window.
    pub hot_deferred: usize,
    /// Message rows read from the source db. This is the cost that mattered:
    /// the message and part tables are what the full scan re-read every cycle.
    pub message_rows: usize,
    /// `store.get()` calls issued.
    pub store_reads: usize,
    /// `store.put()` calls issued.
    pub store_writes: usize,
    /// Sessions that could not be read this pass (unparseable/corrupt rows).
    ///
    /// They are skipped for this cycle and retried on the next one. A non-zero
    /// value means part of the source corpus is not reaching the store.
    pub read_errors: usize,
    /// Records returned to the caller.
    pub records: usize,
    /// Whether this pass skipped anything as unchanged.
    pub had_skips: bool,
}

pub struct OpenCodeImporter {
    /// Explicit override. `None` means "resolve from the environment on every
    /// pass", so a database that appears after daemon start is picked up.
    db_path: Option<PathBuf>,
    embedder: Option<Arc<dyn Embedder>>,
    state: Mutex<CursorState>,
    hot_window_ms: i64,
    /// Where the cursor is saved after each successful pass. `None` = memory only.
    cursor_path: Option<PathBuf>,
}

impl Default for OpenCodeImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenCodeImporter {
    pub fn new() -> Self {
        Self::with_defaults(None)
    }

    fn with_defaults(db_path: Option<PathBuf>) -> Self {
        Self {
            db_path,
            embedder: None,
            state: Mutex::new(CursorState::default()),
            hot_window_ms: DEFAULT_HOT_WINDOW_MS,
            cursor_path: None,
        }
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    pub fn with_path<P: AsRef<Path>>(path: P) -> Self {
        Self::with_defaults(Some(path.as_ref().to_path_buf()))
    }

    pub fn with_path_and_embedder<P: AsRef<Path>>(path: P, embedder: Arc<dyn Embedder>) -> Self {
        Self::with_defaults(Some(path.as_ref().to_path_buf())).with_embedder(embedder)
    }

    /// Override the hot window. Zero imports every changed session on every
    /// pass; the default keeps a live session from being re-written each cycle.
    pub fn with_hot_window(mut self, hot_window_ms: i64) -> Self {
        self.hot_window_ms = hot_window_ms.max(0);
        self
    }

    /// Persist the cursor at `path` and restore it now. A missing, corrupt or
    /// foreign-version file means a full first pass. The watermark is clamped to
    /// the wall clock on load, so a poisoned future value cannot skip sessions.
    pub fn with_cursor_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        let path = path.as_ref().to_path_buf();
        if let Some(mut loaded) = ingest_cursor::load::<CursorState>(&path) {
            loaded.watermark_ms = loaded.watermark_ms.min(now_ms());
            if let Ok(state) = self.state.get_mut() {
                *state = loaded;
            }
        }
        self.cursor_path = Some(path);
        self
    }

    /// Database read by this pass: the explicit override, else resolved afresh.
    fn current_db_path(&self) -> PathBuf {
        self.db_path.clone().unwrap_or_else(Self::resolve_db_path)
    }

    fn resolve_db_path() -> PathBuf {
        if let Ok(path) = std::env::var("OPENCODE_DB_PATH") {
            return PathBuf::from(path);
        }
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home.clone())
                .join(".local")
                .join("share")
                .join("opencode")
                .join("opencode.db");
            if path.exists() {
                return path;
            }
            let path_config = PathBuf::from(home)
                .join(".config")
                .join("opencode")
                .join("opencode.db");
            if path_config.exists() {
                return path_config;
            }
        }
        PathBuf::from(".local/share/opencode/opencode.db")
    }

    /// Read every session, messages, and parts from `opencode.db`.
    ///
    /// Full scan: this is the explicit path and never consults the cursor. Use
    /// [`Self::sync`] for the periodic pass.
    pub fn read_sessions(&self) -> Result<Vec<OpenCodeSession>> {
        let db_path = self.current_db_path();
        if !db_path.exists() {
            debug!("OpenCode db path {:?} does not exist. Skipping.", db_path);
            return Ok(Vec::new());
        }

        let conn = self.open_readonly(&db_path)?;
        let has_updated = session_has_time_updated(&conn);
        let order = if has_updated {
            "time_updated"
        } else {
            "time_created"
        };

        // Legacy schemas have no `time_updated` column to select or order by.
        let columns = if has_updated {
            "time_created, time_updated"
        } else {
            "time_created, NULL"
        };
        let sql =
            format!("SELECT id, title, agent, model, {columns} FROM session ORDER BY {order} DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5).ok().flatten(),
            ))
        })?;

        let mut sessions = Vec::new();
        let mut message_rows = 0usize;
        for row in rows {
            let (id, title, agent, model_raw, time_created, time_updated) = row?;
            let (turns, read) = read_turns(&conn, &id)?;
            message_rows += read;
            if turns.is_empty() {
                continue;
            }
            sessions.push(build_session(
                id,
                title,
                agent,
                model_raw,
                time_created,
                time_updated,
                turns,
            ));
        }

        info!(
            "🔍 OpenCodeImporter read {} valid sessions ({} message rows) from {:?}",
            sessions.len(),
            message_rows,
            db_path
        );
        Ok(sessions)
    }

    /// Periodic pass: only sessions whose `time_updated` is at or after the
    /// cursor are considered, and of those only the ones whose fingerprint moved.
    ///
    /// The previous code re-scanned the whole harness database on every cycle.
    /// On the live node that was 1723 sessions, 40 593 messages and 167 557
    /// parts re-read every 600 s, measured at 78-94 % CPU and ~406 000
    /// `syscr`/s with an rchar/syscr ratio of 4090 — 4096-byte SQLite pages
    /// read over and over. Deciding whether a session changed costs one row of
    /// the `session` table; the message and part tables are only touched for a
    /// session that actually moved.
    pub async fn sync(&self, store: &dyn MemoryStore) -> Result<OpenCodeSyncStats> {
        let db_path = self.current_db_path();
        if !db_path.exists() {
            debug!("OpenCode db path {:?} does not exist. Skipping.", db_path);
            return Ok(OpenCodeSyncStats::default());
        }

        let conn = self.open_readonly(&db_path)?;
        // A cursor describes one database. If the resolved path moved, start over.
        if let Ok(mut state) = self.state.lock() {
            if state.path.as_deref() != Some(db_path.as_path()) {
                *state = CursorState {
                    path: Some(db_path.clone()),
                    ..Default::default()
                };
            }
        }
        let has_updated = session_has_time_updated(&conn);

        // Snapshot under the lock, do the I/O unlocked, fold results back at the
        // end. Holding the lock across the scan would serialise concurrent
        // passes for no benefit.
        let (watermark, mut seen) = self
            .state
            .lock()
            .map(|s| (s.watermark_ms, s.seen.clone()))
            .unwrap_or((0, HashMap::new()));

        // The query floor is the watermark minus the replay window, so a session
        // whose `time_updated` never advances stays inside the candidate set
        // instead of falling behind the watermark forever.
        let now_ms = now_ms();
        // The watermark is clamped to the wall clock of this pass. A single row
        // with a `time_updated` in the future — clock drift between writer and
        // reader, seconds written where milliseconds are expected, a restored
        // database stamped ahead — used to raise the watermark without limit, and
        // since the floor is `watermark - REPLAY_WINDOW_MS`, that row pushed the
        // floor above *every* real session. Ingestion then returned "0 candidates,
        // nothing to do" on every cycle, silently, until the process restarted and
        // the in-memory cursor was lost. A future timestamp is evidence of a
        // broken clock, never of work to skip, so it must not move the cursor.
        let watermark = watermark.min(now_ms);
        let floor = if watermark == 0 {
            0
        } else {
            (watermark - REPLAY_WINDOW_MS).max(0)
        };

        let candidates = self.query_candidates(&conn, has_updated, floor)?;
        let mut stats = OpenCodeSyncStats {
            candidates: candidates.len(),
            ..Default::default()
        };
        let mut deferred: HashMap<String, i64> = HashMap::new();
        let mut observed_max = watermark;
        // Earliest `time_updated` among sessions that failed to read this pass.
        // The watermark must not advance past it, or once the replay window
        // elapses the failed session would never be a candidate again.
        let mut min_failed: Option<i64> = None;

        for cand in candidates {
            let fingerprint = SessionFingerprint {
                time_updated: cand.time_updated.unwrap_or(0),
                message_count: cand.message_count,
            };

            // Skip 1: the session moved since it was last imported.
            if seen.get(&cand.id) == Some(&fingerprint) {
                stats.skipped += 1;
                continue;
            }

            // Skip 2: the session is still live. Re-importing a session that
            // changed in the last cycle re-reads its whole transcript and
            // re-embedds the whole record each time; the next pass after it
            // goes quiet picks it up. The cursor never advances past it, so the
            // next pass still sees it.
            if self.hot_window_ms > 0 {
                let age_ms = now_ms.saturating_sub(fingerprint.time_updated);
                if age_ms >= 0 && age_ms < self.hot_window_ms {
                    deferred.insert(cand.id, fingerprint.time_updated);
                    stats.hot_deferred += 1;
                    continue;
                }
            }

            let (turns, message_rows) = match read_turns(&conn, &cand.id) {
                Ok(t) => t,
                Err(e) => {
                    // One unreadable session must not abort the pass.
                    //
                    // `?` here used to return from `sync` BEFORE the cursor was
                    // folded back, so `seen` and `watermark_ms` were thrown away
                    // and the next cycle started again from watermark 0 — i.e. a
                    // permanently broken row re-created the full scan the cursor
                    // exists to avoid, on every cycle, forever.
                    //
                    // Deliberately NOT inserted into `seen`: a read failure is
                    // often transient (a partially written row, a locked db), and
                    // marking it seen would lose that session forever — the same
                    // silent-data-loss class as the Hermes defect. The retry is
                    // bounded to this one session per cycle and now surfaces in
                    // `read_errors` plus the warning log, instead of stalling the
                    // other 1722 sessions.
                    stats.read_errors += 1;
                    let t = cand.time_updated.unwrap_or(0);
                    min_failed = Some(min_failed.map_or(t, |m| m.min(t)));
                    warn!(
                        "Skipping unreadable OpenCode session {} this pass: {}",
                        cand.id, e
                    );
                    continue;
                }
            };
            stats.read += 1;
            stats.message_rows += message_rows;
            observed_max = observed_max.max(fingerprint.time_updated);

            if turns.is_empty() {
                // Nothing to store, but the session is accounted for.
                seen.insert(cand.id, fingerprint);
                continue;
            }

            let session = build_session(
                cand.id.clone(),
                cand.title,
                cand.agent,
                cand.model,
                cand.time_created,
                cand.time_updated,
                turns,
            );
            match self
                .import_session_counted(&session, store, &mut stats)
                .await
            {
                Ok(()) => {
                    seen.insert(cand.id, fingerprint);
                }
                Err(e) => {
                    warn!("Failed to import OpenCode session {}: {}", cand.id, e);
                }
            }
        }

        if let Ok(mut state) = self.state.lock() {
            state.seen = seen;
            state.deferred = deferred;
            if has_updated {
                // Clamp again on the way out: `observed_max` is fed straight
                // from row timestamps, so without this a future-dated row is
                // written back into the cursor and the next pass reads it as the
                // starting watermark. Clamping only the read side would fix one
                // pass and re-poison the next.
                let mut next = observed_max.min(now_ms);
                if let Some(failed) = min_failed {
                    // Hold the cursor at the earliest failure (never moving it
                    // backwards): successful sessions are skipped by `seen`, so
                    // the retry costs one row read, not a full scan.
                    next = next.min(failed).max(watermark);
                }
                state.watermark_ms = next;
            }
        }

        // Save only after the pass folded its results back: a failed read or
        // store write never reaches `seen`, so it is never persisted as done.
        // A save error costs a full pass after restart, not data.
        if let Some(path) = self.cursor_path.as_deref() {
            let snapshot = self.state.lock().map(|s| s.clone()).ok();
            if let Some(snapshot) = snapshot {
                if let Err(e) = ingest_cursor::save(path, &snapshot) {
                    warn!("Could not persist OpenCode ingest cursor: {}", e);
                }
            }
        }

        // `records` counts what this pass actually put in the store, so a caller
        // can distinguish "nothing to do" from "everything re-confirmed".
        stats.records = stats.store_writes;
        stats.had_skips = stats.skipped > 0;
        info!(
            "✅ OpenCodeImporter sync: {} candidates, {} read, {} skipped, {} hot-deferred, {} message rows, {} store reads, {} read errors",
            stats.candidates,
            stats.read,
            stats.skipped,
            stats.hot_deferred,
            stats.message_rows,
            stats.store_reads,
            stats.read_errors
        );
        Ok(stats)
    }

    /// Reconciliation escape hatch: re-read every session, cursor ignored.
    /// Run after a restart that lost the in-memory cursor is unnecessary (the
    /// first `sync` is a full pass anyway), but this exists for an operator who
    /// suspects a wrong fingerprint.
    pub async fn force_reindex(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>> {
        self.import_all(store).await
    }

    /// Session index rows at or after the cursor, with a per-session message
    /// count used as the second change signal.
    fn query_candidates(
        &self,
        conn: &Connection,
        has_updated: bool,
        cursor: i64,
    ) -> Result<Vec<SessionCandidate>> {
        // `time_updated` is milliseconds; the subquery rides the
        // (session_id, ...) index on message and never touches message data.
        let sql = if has_updated {
            "SELECT s.id, s.title, s.agent, s.model, s.time_created, s.time_updated, \
             (SELECT COUNT(*) FROM message m WHERE m.session_id = s.id) AS message_count \
             FROM session s WHERE s.time_updated >= ?1 ORDER BY s.time_updated DESC"
        } else {
            // Legacy schema without time_updated: fall back to every session
            // row, which is still an index-only scan, and let the message count
            // decide.
            "SELECT s.id, s.title, s.agent, s.model, s.time_created, NULL AS time_updated, \
             (SELECT COUNT(*) FROM message m WHERE m.session_id = s.id) AS message_count \
             FROM session s ORDER BY s.time_created DESC"
        };

        let mut stmt = conn.prepare(sql)?;
        let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<SessionCandidate> {
            Ok(SessionCandidate {
                id: row.get(0)?,
                title: row.get(1)?,
                agent: row.get(2)?,
                model: row.get(3)?,
                time_created: row.get(4)?,
                time_updated: row.get::<_, Option<i64>>(5).ok().flatten(),
                message_count: row.get::<_, i64>(6).unwrap_or(0),
            })
        };

        if has_updated {
            Ok(stmt
                .query_map([cursor], mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        } else {
            Ok(stmt
                .query_map([], mapper)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        }
    }

    fn open_readonly(&self, db_path: &Path) -> Result<Connection> {
        Ok(Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?)
    }

    /// `import_session` with the counters filled in, so `sync` can prove how
    /// many store round-trips a pass cost.
    async fn import_session_counted(
        &self,
        session: &OpenCodeSession,
        store: &dyn MemoryStore,
        stats: &mut OpenCodeSyncStats,
    ) -> Result<()> {
        let path = format!("opencode://sessions/{}", session.session_id);
        let workspace_id = "agent:opencode".to_string();

        let mut full_text = format!(
            "# OpenCode Session: {}\n\n",
            session.title.as_deref().unwrap_or(&session.session_id)
        );

        if let Some(m) = &session.model {
            full_text.push_str(&format!("**Model**: {}\n", m));
        }
        if let Some(a) = &session.agent {
            full_text.push_str(&format!("**Agent**: {}\n", a));
        }
        full_text.push('\n');

        for turn in &session.turns {
            full_text.push_str(&format!("### [{}]\n{}\n\n", turn.role, turn.content));
        }

        let mut record = MemoryRecord {
            workspace_id: workspace_id.clone(),
            path: path.clone(),
            content: full_text,
            metadata: json!({
                "source_app": "opencode",
                "agent_id": "opencode",
                "session_id": session.session_id,
                "title": session.title,
                "model": session.model,
                "agent": session.agent,
                "turn_count": session.turns.len(),
                "time_created": session.time_created,
                "time_updated": session.time_updated,
                "dedup": true,
            }),
            ..Default::default()
        };

        record.id = stable_key("memory", &[&workspace_id, &path]);

        // Incremental skip (stability S1.07): identical content already
        // stored reuses the existing record without re-embedding. Fail-open
        // on store errors.
        stats.store_reads += 1;
        if let Ok(Some(existing)) = store.get(&workspace_id, &record.id).await {
            if existing.content == record.content {
                return Ok(());
            }
        }

        if let Some(embedder) = &self.embedder {
            if let Ok(emb) = embedder.encode(&record.content).await {
                record.embedding = emb;
            }
        }

        store.put(record).await?;
        stats.store_writes += 1;
        Ok(())
    }

    /// Import a single OpenCode session into Xavier `MemoryStore`.
    pub async fn import_session(
        &self,
        session: &OpenCodeSession,
        store: &dyn MemoryStore,
    ) -> Result<Vec<MemoryRecord>> {
        let mut records = Vec::new();
        let path = format!("opencode://sessions/{}", session.session_id);
        let workspace_id = "agent:opencode".to_string();

        let mut full_text = format!(
            "# OpenCode Session: {}\n\n",
            session.title.as_deref().unwrap_or(&session.session_id)
        );

        if let Some(m) = &session.model {
            full_text.push_str(&format!("**Model**: {}\n", m));
        }
        if let Some(a) = &session.agent {
            full_text.push_str(&format!("**Agent**: {}\n", a));
        }
        full_text.push('\n');

        for turn in &session.turns {
            full_text.push_str(&format!("### [{}]\n{}\n\n", turn.role, turn.content));
        }

        let mut record = MemoryRecord {
            workspace_id: workspace_id.clone(),
            path: path.clone(),
            content: full_text,
            metadata: json!({
                "source_app": "opencode",
                "agent_id": "opencode",
                "session_id": session.session_id,
                "title": session.title,
                "model": session.model,
                "agent": session.agent,
                "turn_count": session.turns.len(),
                "time_created": session.time_created,
                "dedup": true,
            }),
            ..Default::default()
        };

        record.id = stable_key("memory", &[&workspace_id, &path]);

        // Incremental skip (stability S1.07): identical content already
        // stored reuses the existing record without re-embedding. Fail-open
        // on store errors.
        if let Ok(Some(existing)) = store.get(&workspace_id, &record.id).await {
            if existing.content == record.content {
                records.push(existing);
                return Ok(records);
            }
        }

        if let Some(embedder) = &self.embedder {
            if let Ok(emb) = embedder.encode(&record.content).await {
                record.embedding = emb;
            }
        }

        store.put(record.clone()).await?;
        records.push(record);

        Ok(records)
    }

    /// Import all OpenCode sessions into the given `MemoryStore`.
    pub async fn import_all(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>> {
        let sessions = self.read_sessions()?;
        let mut imported = Vec::new();

        for s in sessions {
            match self.import_session(&s, store).await {
                Ok(recs) => imported.extend(recs),
                Err(e) => warn!("Failed to import OpenCode session {}: {}", s.session_id, e),
            }
        }

        info!(
            "✅ Successfully imported {} OpenCode session records",
            imported.len()
        );
        Ok(imported)
    }
}

/// One row of the `session` table, plus the message count used as the second
/// change signal. Metadata only: reading this must not touch message data.
struct SessionCandidate {
    id: String,
    title: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    time_created: Option<i64>,
    time_updated: Option<i64>,
    message_count: i64,
}

/// Milliseconds since the Unix epoch.
///
/// The crate has no injectable clock, so this reads the system clock. Tests
/// control the hot window through [`OpenCodeImporter::with_hot_window`] and the
/// fixture timestamps, not through a clock seam.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Whether the `session` table carries `time_updated`. OpenCode's real schema
/// does; older or hand-built fixtures may not, and the cursor degrades to the
/// message count rather than failing.
///
/// `table_info` rather than a probe `SELECT`: preparing a statement does not
/// always resolve its column list, so a missing column slipped through and only
/// surfaced when the real query ran.
fn session_has_time_updated(conn: &Connection) -> bool {
    // The block drops `stmt` and its rows together, which the borrow checker
    // needs and clippy's `let_and_return` would otherwise flag.
    {
        let Ok(mut stmt) = conn.prepare("PRAGMA table_info(session)") else {
            return false;
        };
        let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
            return false;
        };
        if rows.flatten().any(|name| name == "time_updated") {
            return true;
        }
    }
    false
}

/// Turns of one session, plus the number of message rows read. The count is
/// what proves a "no change" pass read nothing.
fn read_turns(conn: &Connection, session_id: &str) -> Result<(Vec<OpenCodeTurn>, usize)> {
    let mut msg_stmt = conn
        .prepare("SELECT id, data FROM message WHERE session_id = ? ORDER BY time_created ASC")?;
    let msg_rows = msg_stmt.query_map([session_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut turns = Vec::new();
    let mut read = 0usize;
    for m_res in msg_rows {
        let (msg_id, msg_data_str) = m_res?;
        read += 1;
        let role = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg_data_str) {
            v["role"].as_str().unwrap_or("user").to_string()
        } else {
            "user".to_string()
        };

        let mut part_stmt =
            conn.prepare("SELECT data FROM part WHERE message_id = ? ORDER BY time_created ASC")?;
        let part_rows = part_stmt.query_map([&msg_id], |row| row.get::<_, String>(0))?;

        let mut combined_text = String::new();
        for p_res in part_rows {
            let part_data_str = p_res?;
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&part_data_str) {
                if v["type"].as_str().unwrap_or("") == "text" {
                    if let Some(txt) = v["text"].as_str() {
                        let cleaned = strip_ansi(txt).trim().to_string();
                        if !cleaned.is_empty() {
                            if !combined_text.is_empty() {
                                combined_text.push('\n');
                            }
                            combined_text.push_str(&cleaned);
                        }
                    }
                }
            }
        }

        if !combined_text.is_empty() {
            turns.push(OpenCodeTurn {
                role,
                content: combined_text,
            });
        }
    }

    Ok((turns, read))
}

fn build_session(
    session_id: String,
    title: Option<String>,
    agent: Option<String>,
    model_raw: Option<String>,
    time_created: Option<i64>,
    time_updated: Option<i64>,
    turns: Vec<OpenCodeTurn>,
) -> OpenCodeSession {
    // Model is stored as a JSON string; keep the id when it parses.
    let model = model_raw.as_deref().and_then(|m| {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(m) {
            v["id"].as_str().map(|s| s.to_string())
        } else {
            Some(m.to_string())
        }
    });

    OpenCodeSession {
        session_id,
        title,
        agent,
        model,
        time_created,
        time_updated,
        turns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::{Embedder, EmbeddingError};
    use crate::memory::store::InMemoryMemoryStore;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_read_and_import_opencode_sqlite() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");

        // Create mock opencode schema and rows
        let conn = Connection::open(&db_file)?;
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                title TEXT,
                agent TEXT,
                model TEXT,
                time_created INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                time_created INTEGER,
                data TEXT
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT,
                session_id TEXT,
                time_created INTEGER,
                data TEXT
            );
            INSERT INTO session VALUES ('ses_001', 'Fixing login bug', 'coder', '{\"id\":\"qwen-coder\"}', 1780000000);
            INSERT INTO message VALUES ('msg_001', 'ses_001', 1780000001, '{\"role\":\"user\"}');
            INSERT INTO message VALUES ('msg_002', 'ses_001', 1780000002, '{\"role\":\"assistant\"}');
            ",
        )?;

        let part1_json = serde_json::json!({
            "type": "text",
            "text": "Please fix the auth loop"
        })
        .to_string();
        let part2_json = serde_json::json!({
            "type": "text",
            "text": "\x1B[32mAuth loop fixed!\x1B[0m"
        })
        .to_string();

        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["prt_001", "msg_001", "ses_001", 1780000001, part1_json],
        )?;
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["prt_002", "msg_002", "ses_001", 1780000002, part2_json],
        )?;

        let importer = OpenCodeImporter::with_path(&db_file);
        let store = InMemoryMemoryStore::new();

        let imported = importer.import_all(&store).await?;
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].path, "opencode://sessions/ses_001");
        assert!(imported[0].content.contains("Fixing login bug"));
        assert!(imported[0].content.contains("Please fix the auth loop"));
        assert!(imported[0].content.contains("Auth loop fixed!"));
        assert!(!imported[0].content.contains("\x1B[32m")); // ANSI stripped!

        Ok(())
    }

    struct CountingEmbedder {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Embedder for CountingEmbedder {
        async fn encode(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![0.5; 8])
        }

        fn dimension(&self) -> usize {
            8
        }
    }

    fn write_fixture_db(db_file: &std::path::Path) -> Result<()> {
        let conn = Connection::open(db_file)?;
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                title TEXT,
                agent TEXT,
                model TEXT,
                time_created INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                time_created INTEGER,
                data TEXT
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT,
                session_id TEXT,
                time_created INTEGER,
                data TEXT
            );
            INSERT INTO session VALUES ('ses_001', 'Fixing login bug', 'coder', '{\"id\":\"qwen-coder\"}', 1780000000);
            INSERT INTO message VALUES ('msg_001', 'ses_001', 1780000001, '{\"role\":\"user\"}');
            INSERT INTO message VALUES ('msg_002', 'ses_001', 1780000002, '{\"role\":\"assistant\"}');
            ",
        )?;
        let part1_json =
            serde_json::json!({"type": "text", "text": "Please fix the auth loop"}).to_string();
        let part2_json =
            serde_json::json!({"type": "text", "text": "Auth loop fixed!"}).to_string();
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["prt_001", "msg_001", "ses_001", 1780000001, part1_json],
        )?;
        conn.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["prt_002", "msg_002", "ses_001", 1780000002, part2_json],
        )?;
        Ok(())
    }

    /// Stability S1.07: a second identical import must not re-encode.
    #[tokio::test]
    async fn test_opencode_second_pass_no_reencode() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        write_fixture_db(&db_file)?;

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = OpenCodeImporter::with_path(&db_file).with_embedder(embedder.clone());

        let first = importer.import_all(&store).await?;
        assert_eq!(first.len(), 1);
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 1);

        let second = importer.import_all(&store).await?;
        assert_eq!(second.len(), 1);
        assert_eq!(
            embedder.calls.load(Ordering::SeqCst),
            1,
            "second identical import must not call encode again"
        );

        Ok(())
    }

    // ── incremental cursor ──

    /// Base timestamp in milliseconds, far enough in the past that no fixture
    /// session is ever inside the hot window unless a test asks for it.
    const OLD_MS: i64 = 1_600_000_000_000;

    /// Session table shaped like OpenCode's real schema, `time_updated`
    /// included. `message`/`part` mirror it as well.
    fn create_modern_db(db_file: &Path) -> Result<Connection> {
        let conn = Connection::open(db_file)?;
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
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE INDEX message_session_idx ON message (session_id, time_created, id);
            CREATE INDEX part_message_idx ON part (message_id, id);
            ",
        )?;
        Ok(conn)
    }

    fn insert_session(
        conn: &Connection,
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
        conn: &Connection,
        session_id: &str,
        seq: u32,
        role: &str,
        text: &str,
        at_ms: i64,
    ) -> Result<()> {
        let msg_id = format!("{}_m{}", session_id, seq);
        let prt_id = format!("{}_p{}", session_id, seq);
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data) \
             VALUES (?1, ?2, ?3, ?3, ?4)",
            rusqlite::params![msg_id, session_id, at_ms, json!({"role": role}).to_string()],
        )?;
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) \
             VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
            rusqlite::params![
                prt_id,
                msg_id,
                session_id,
                at_ms,
                json!({"type": "text", "text": text}).to_string()
            ],
        )?;
        Ok(())
    }

    /// Three quiet sessions, two turns each.
    fn seed_three_sessions(db_file: &Path) -> Result<()> {
        let conn = create_modern_db(db_file)?;
        for (i, id) in ["ses_a", "ses_b", "ses_c"].iter().enumerate() {
            let base = OLD_MS + (i as i64) * 1_000;
            insert_session(&conn, id, &format!("Session {}", id), base, base)?;
            insert_turn(&conn, id, 0, "user", &format!("ask {}", id), base + 1)?;
            insert_turn(
                &conn,
                id,
                1,
                "assistant",
                &format!("reply {}", id),
                base + 2,
            )?;
        }
        Ok(())
    }

    fn quiet_importer(db_file: &Path) -> OpenCodeImporter {
        // Window off: the fixtures are old, and the point of most tests is the
        // fingerprint skip, not the hot window.
        OpenCodeImporter::with_path(db_file).with_hot_window(0)
    }

    /// Row count in the shared workspace, i.e. duplicates, not call counts.
    async fn stored_rows(store: &InMemoryMemoryStore) -> Result<Vec<MemoryRecord>> {
        store.list("agent:opencode").await
    }

    /// 1. First `sync` imports every session and pays for its messages.
    #[tokio::test]
    async fn first_sync_imports_every_session() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;

        let store = InMemoryMemoryStore::new();
        let importer = quiet_importer(&db_file);

        let stats = importer.sync(&store).await?;
        assert_eq!(stats.candidates, 3);
        assert_eq!(stats.read, 3);
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.message_rows, 6, "two messages per session");
        assert_eq!(stats.store_reads, 3);
        assert_eq!(stats.store_writes, 3);
        assert_eq!(stats.records, 3);

        let rows = stored_rows(&store).await?;
        assert_eq!(rows.len(), 3);
        assert!(rows
            .iter()
            .all(|r| r.path.starts_with("opencode://sessions/")));

        Ok(())
    }

    /// 2. The regression: an unchanged corpus must read zero message rows and
    /// touch the store zero times.
    ///
    /// This is the busy-loop. The removed code ran
    /// `SELECT ... FROM session ORDER BY time_created DESC` and then read every
    /// message and part of every session, so each 600 s cycle re-read ~40 k
    /// message rows and ~168 k part rows from a 5.5 GB database.
    #[tokio::test]
    async fn second_sync_without_changes_reads_nothing() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = quiet_importer(&db_file).with_embedder(embedder.clone());

        let first = importer.sync(&store).await?;
        assert_eq!(first.message_rows, 6);
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 3);

        let second = importer.sync(&store).await?;
        assert_eq!(second.candidates, 3, "the cursor still sees the rows");
        assert_eq!(second.skipped, 3, "every session fingerprint is unchanged");
        assert_eq!(second.read, 0);
        assert_eq!(
            second.message_rows, 0,
            "an unchanged pass must read no message rows; got {}",
            second.message_rows
        );
        assert_eq!(second.store_reads, 0, "no get() on an unchanged pass");
        assert_eq!(second.store_writes, 0, "no put() on an unchanged pass");
        assert_eq!(second.records, 0);
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 3);
        assert_eq!(stored_rows(&store).await?.len(), 3);

        Ok(())
    }

    /// 3. A new session is imported; the existing ones are not re-read.
    #[tokio::test]
    async fn new_session_is_the_only_one_imported() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;

        let store = InMemoryMemoryStore::new();
        let importer = quiet_importer(&db_file);
        importer.sync(&store).await?;

        let conn = Connection::open(&db_file)?;
        let fresh = OLD_MS + 10_000;
        insert_session(&conn, "ses_new", "Brand new", fresh, fresh)?;
        insert_turn(
            &conn,
            "ses_new",
            0,
            "user",
            "hello from a new session",
            fresh + 1,
        )?;

        let stats = importer.sync(&store).await?;
        assert_eq!(stats.read, 1, "only the new session is deep-read");
        assert_eq!(stats.message_rows, 1, "only the new session's message");
        assert_eq!(stats.skipped, 3, "the three old sessions are untouched");
        assert_eq!(stats.store_writes, 1);

        let rows = stored_rows(&store).await?;
        assert_eq!(rows.len(), 4);
        assert!(rows
            .iter()
            .any(|r| r.content.contains("hello from a new session")));

        Ok(())
    }

    /// 4. One session whose `time_updated` advances is re-read on its own, and
    /// the re-read actually refreshes the stored content.
    #[tokio::test]
    async fn advanced_time_updated_rereads_only_that_session() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = quiet_importer(&db_file).with_embedder(embedder.clone());
        importer.sync(&store).await?;
        let encodes_after_first = embedder.calls.load(Ordering::SeqCst);

        // Only ses_b grows: new turn, new time_updated.
        let conn = Connection::open(&db_file)?;
        let moved = OLD_MS + 50_000;
        insert_turn(&conn, "ses_b", 2, "user", "one more question", moved)?;
        conn.execute(
            "UPDATE session SET time_updated = ?1 WHERE id = 'ses_b'",
            [moved],
        )?;

        let stats = importer.sync(&store).await?;
        assert_eq!(stats.read, 1, "exactly one session is re-read");
        assert_eq!(stats.message_rows, 3, "ses_b now holds three messages");
        assert_eq!(stats.skipped, 2, "ses_a and ses_c are unchanged");
        assert_eq!(
            embedder.calls.load(Ordering::SeqCst),
            encodes_after_first + 1,
            "only the changed session re-embeds"
        );

        let rows = stored_rows(&store).await?;
        assert_eq!(
            rows.len(),
            3,
            "a re-read updates in place, no duplicate row"
        );
        let b = rows
            .iter()
            .find(|r| r.path == "opencode://sessions/ses_b")
            .expect("ses_b row");
        assert!(b.content.contains("one more question"));

        Ok(())
    }

    /// 5. A message added to an existing session without `time_updated` moving
    /// is still caught, by the message-count half of the fingerprint.
    #[tokio::test]
    async fn message_growth_alone_triggers_a_reread() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;

        let store = InMemoryMemoryStore::new();
        let importer = quiet_importer(&db_file);
        importer.sync(&store).await?;

        // Freeze the session timestamp and add a turn underneath it: the
        // worst case for a timestamp-only cursor.
        let conn = Connection::open(&db_file)?;
        insert_turn(&conn, "ses_c", 9, "user", "silent growth", OLD_MS + 3)?;

        let stats = importer.sync(&store).await?;
        assert_eq!(
            stats.read, 1,
            "a session that gained a message must be re-read even with a frozen time_updated"
        );
        assert_eq!(stats.store_writes, 1);

        Ok(())
    }

    /// 6. The cursor is `time_updated`, never `time_created`. A session created
    /// long ago but touched now must be picked up; a `time_created` cursor
    /// would never look at it again.
    #[tokio::test]
    async fn cursor_tracks_time_updated_not_time_created() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");

        // One old session is imported first, which sets the cursor.
        let conn = create_modern_db(&db_file)?;
        insert_session(&conn, "ses_old", "Old", OLD_MS - 500_000, OLD_MS - 500_000)?;
        insert_turn(
            &conn,
            "ses_old",
            0,
            "user",
            "ancient history",
            OLD_MS - 499_999,
        )?;
        drop(conn);

        let store = InMemoryMemoryStore::new();
        let importer = quiet_importer(&db_file);
        importer.sync(&store).await?;

        // A session created BEFORE the cursor was established, but updated now.
        // A time_created cursor at or above `ses_old`'s creation time would
        // exclude this row forever.
        let conn = Connection::open(&db_file)?;
        let created_early = OLD_MS - 400_000;
        let touched_now = now_ms();
        insert_session(
            &conn,
            "ses_revisited",
            "Revisited",
            created_early,
            touched_now,
        )?;
        insert_turn(
            &conn,
            "ses_revisited",
            0,
            "user",
            "resumed much later",
            touched_now + 1,
        )?;

        let stats = importer.sync(&store).await?;
        assert_eq!(
            stats.read, 1,
            "a session touched now is read regardless of age"
        );
        assert_eq!(stats.store_writes, 1);
        assert_eq!(stored_rows(&store).await?.len(), 2);

        Ok(())
    }

    /// 7. No duplicates after many cycles, counted as rows rather than calls.
    #[tokio::test]
    async fn repeated_sync_cycles_never_duplicate() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = quiet_importer(&db_file).with_embedder(embedder.clone());

        let mut total_writes = 0;
        for _ in 0..5 {
            total_writes += importer.sync(&store).await?.store_writes;
        }
        assert_eq!(total_writes, 3, "only the first pass writes");
        assert_eq!(
            stored_rows(&store).await?.len(),
            3,
            "five cycles must leave exactly three rows"
        );
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 3);

        // And one real change across the same cycles still lands in place.
        let conn = Connection::open(&db_file)?;
        let moved = OLD_MS + 70_000;
        insert_turn(&conn, "ses_a", 3, "user", "late arrival", moved)?;
        conn.execute(
            "UPDATE session SET time_updated = ?1 WHERE id = 'ses_a'",
            [moved],
        )?;
        importer.sync(&store).await?;
        importer.sync(&store).await?;

        let rows = stored_rows(&store).await?;
        assert_eq!(rows.len(), 3, "still three rows after the change");
        let a = rows
            .iter()
            .find(|r| r.path == "opencode://sessions/ses_a")
            .expect("ses_a row");
        assert!(a.content.contains("late arrival"));
        let ids: std::collections::HashSet<&String> = rows.iter().map(|r| &r.id).collect();
        assert_eq!(ids.len(), 3, "record ids must be distinct");

        Ok(())
    }

    /// 8. The hot window: a session touched moments ago is held back instead of
    /// being re-written on every single cycle.
    ///
    /// Without it, a live session changes on every pass, so it is re-read and
    /// re-embedded every pass — the same storm the cursor was added to stop, one
    /// session wide.
    #[tokio::test]
    async fn hot_window_defers_a_live_session() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");

        let conn = create_modern_db(&db_file)?;
        let just_now = now_ms();
        insert_session(&conn, "ses_live", "Live", just_now, just_now)?;
        insert_turn(&conn, "ses_live", 0, "user", "still typing", just_now + 1)?;
        drop(conn);

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = OpenCodeImporter::with_path(&db_file)
            .with_hot_window(DEFAULT_HOT_WINDOW_MS)
            .with_embedder(embedder.clone());

        let first = importer.sync(&store).await?;
        assert_eq!(first.hot_deferred, 1, "a brand-new live session waits");
        assert_eq!(first.read, 0);
        assert_eq!(first.message_rows, 0);
        assert_eq!(first.store_writes, 0);
        assert_eq!(stored_rows(&store).await?.len(), 0);

        // Still live after a second turn: deferred again, still no write.
        let conn = Connection::open(&db_file)?;
        insert_turn(
            &conn,
            "ses_live",
            1,
            "assistant",
            "partial answer",
            just_now + 2,
        )?;
        conn.execute(
            "UPDATE session SET time_updated = ?1 WHERE id = 'ses_live'",
            [now_ms()],
        )?;
        drop(conn);

        let second = importer.sync(&store).await?;
        assert_eq!(second.hot_deferred, 1);
        assert_eq!(second.store_writes, 0);
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 0);

        // Once it goes quiet the next pass picks it up, in full.
        let conn = Connection::open(&db_file)?;
        let quiet_at = now_ms() - DEFAULT_HOT_WINDOW_MS - 1_000;
        conn.execute(
            "UPDATE session SET time_updated = ?1 WHERE id = 'ses_live'",
            [quiet_at],
        )?;
        drop(conn);

        let third = importer.sync(&store).await?;
        assert_eq!(third.read, 1, "a quiet session is imported");
        assert_eq!(third.hot_deferred, 0);
        assert_eq!(third.message_rows, 2, "both turns are imported together");
        assert_eq!(third.store_writes, 1);

        let rows = stored_rows(&store).await?;
        assert_eq!(rows.len(), 1);
        assert!(rows[0].content.contains("still typing"));
        assert!(rows[0].content.contains("partial answer"));

        Ok(())
    }

    /// 9. A legacy schema without `time_updated` still syncs, on the message
    /// count alone. The cursor degrades, it does not break.
    #[tokio::test]
    async fn legacy_schema_without_time_updated_still_syncs() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");

        let conn = Connection::open(&db_file)?;
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY, title TEXT, agent TEXT, model TEXT, time_created INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, data TEXT
            );
            INSERT INTO session VALUES ('ses_legacy', 'Legacy', 'coder', '{\"id\":\"m\"}', 1780000000);
            INSERT INTO message VALUES ('lm_1', 'ses_legacy', 1780000001, '{\"role\":\"user\"}');
            ",
        )?;
        conn.execute(
            "INSERT INTO part VALUES ('lp_1', 'lm_1', 'ses_legacy', 1780000001, ?1)",
            [json!({"type": "text", "text": "legacy turn"}).to_string()],
        )?;

        let store = InMemoryMemoryStore::new();
        let importer = OpenCodeImporter::with_path(&db_file).with_hot_window(0);

        let first = importer.sync(&store).await?;
        assert_eq!(first.read, 1);
        assert_eq!(first.store_writes, 1);
        assert_eq!(stored_rows(&store).await?.len(), 1);

        let second = importer.sync(&store).await?;
        assert_eq!(
            second.message_rows, 0,
            "the message count alone must still short-circuit a legacy db"
        );
        assert_eq!(second.skipped, 1);

        Ok(())
    }

    /// 10. `import_all` stays the explicit, complete path: it ignores the cursor
    /// so a caller asking "how much did you index" gets the whole corpus even on
    /// a warm cursor. The `/index` handler reports that count.
    #[tokio::test]
    async fn import_all_ignores_the_cursor() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;

        let store = InMemoryMemoryStore::new();
        let importer = quiet_importer(&db_file);
        importer.sync(&store).await?;

        let records = importer.import_all(&store).await?;
        assert_eq!(
            records.len(),
            3,
            "an explicit re-index reports every session"
        );
        assert_eq!(stored_rows(&store).await?.len(), 3, "still no duplicates");

        // And the cursor is untouched by it: a following sync is still a no-op.
        let stats = importer.sync(&store).await?;
        assert_eq!(stats.read, 0);
        assert_eq!(stats.message_rows, 0);

        Ok(())
    }

    /// 11. Scale check: the shape that produced the busy loop. A wide corpus
    /// costs a proportional first pass and a flat second pass.
    #[tokio::test]
    async fn wide_corpus_second_pass_cost_does_not_scale_with_corpus() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");

        let conn = create_modern_db(&db_file)?;
        for i in 0..200 {
            let id = format!("ses_{:04}", i);
            let base = OLD_MS + (i as i64) * 1_000;
            insert_session(&conn, &id, &format!("Session {}", i), base, base)?;
            for turn in 0..5 {
                insert_turn(
                    &conn,
                    &id,
                    turn,
                    "user",
                    &format!("body {i}-{turn}"),
                    base + 1,
                )?;
            }
        }
        drop(conn);

        let store = InMemoryMemoryStore::new();
        let importer = quiet_importer(&db_file);

        let first = importer.sync(&store).await?;
        assert_eq!(first.read, 200);
        assert_eq!(first.message_rows, 1_000);

        let second = importer.sync(&store).await?;
        assert_eq!(second.candidates, 200);
        assert_eq!(second.skipped, 200);
        assert_eq!(
            second.message_rows, 0,
            "the second pass over 1000 messages must read none of them"
        );
        assert_eq!(second.store_reads, 0);
        assert_eq!(stored_rows(&store).await?.len(), 200);

        Ok(())
    }

    /// 13. A session that grows while sitting *below* the watermark is still
    /// re-read, because the query floor is `watermark - REPLAY_WINDOW_MS`
    /// rather than the watermark itself.
    ///
    /// This is what the replay window buys. A bare "highest `time_updated` seen"
    /// cursor strands any session whose timestamp does not advance past the
    /// mark left by a newer session — the old session here never gets looked at
    /// again, no matter how many messages it gains.
    #[tokio::test]
    async fn session_below_the_watermark_still_gets_reread() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");

        // An old session, and a much newer one that will raise the watermark
        // above the old session's timestamp. Both stay inside the replay window.
        let conn = create_modern_db(&db_file)?;
        let old_at = OLD_MS;
        let new_at = OLD_MS + REPLAY_WINDOW_MS / 2;
        assert!(
            new_at - old_at < REPLAY_WINDOW_MS,
            "both sessions must remain inside the replay window"
        );
        insert_session(&conn, "ses_stale", "Stale", old_at, old_at)?;
        insert_turn(&conn, "ses_stale", 0, "user", "stale turn", old_at + 1)?;
        insert_session(&conn, "ses_fresh", "Fresh", new_at, new_at)?;
        insert_turn(&conn, "ses_fresh", 0, "user", "fresh turn", new_at + 1)?;
        drop(conn);

        let store = InMemoryMemoryStore::new();
        let importer = quiet_importer(&db_file);

        let first = importer.sync(&store).await?;
        assert_eq!(first.read, 2, "both sessions start inside the window");

        // The stale session gains a message without its timestamp moving.
        let conn = Connection::open(&db_file)?;
        insert_turn(
            &conn,
            "ses_stale",
            1,
            "assistant",
            "stale growth",
            old_at + 2,
        )?;
        drop(conn);

        let second = importer.sync(&store).await?;
        assert_eq!(
            second.read, 1,
            "a session below the watermark must still be re-read"
        );
        assert_eq!(
            second.skipped, 1,
            "the session that set the watermark is skipped"
        );

        let rows = stored_rows(&store).await?;
        let stale = rows
            .iter()
            .find(|r| r.path == "opencode://sessions/ses_stale")
            .expect("ses_stale row");
        assert!(
            stale.content.contains("stale growth"),
            "the grown session must be refreshed, not skipped"
        );

        Ok(())
    }

    /// 12. `force_reindex` bypasses the cursor for an operator who suspects a
    /// wrong fingerprint.
    #[tokio::test]
    async fn force_reindex_bypasses_the_cursor() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;

        let store = InMemoryMemoryStore::new();
        let importer = quiet_importer(&db_file);
        importer.sync(&store).await?;
        assert_eq!(importer.sync(&store).await?.read, 0);

        let records = importer.force_reindex(&store).await?;
        assert_eq!(records.len(), 3, "force re-reads every session");
        assert_eq!(stored_rows(&store).await?.len(), 3);

        Ok(())
    }

    fn cursor_file(dir: &Path) -> PathBuf {
        dir.join("ingest-cursors").join("opencode.json")
    }

    /// Restart replay: a fresh instance on the same cursor file reads only what
    /// changed since the previous instance's last successful pass.
    #[tokio::test]
    async fn restart_replays_only_changed() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;
        let cursor = cursor_file(dir.path());
        let store = InMemoryMemoryStore::new();

        let first = quiet_importer(&db_file).with_cursor_path(&cursor);
        assert_eq!(first.sync(&store).await?.read, 3);
        drop(first);

        let second = quiet_importer(&db_file).with_cursor_path(&cursor);
        let stats = second.sync(&store).await?;
        assert_eq!(stats.read, 0, "restart must not re-read unchanged sessions");
        assert_eq!(stats.message_rows, 0);
        assert_eq!(stats.skipped, 3);
        drop(second);

        let conn = Connection::open(&db_file)?;
        conn.execute(
            "UPDATE session SET time_updated = ?1 WHERE id = 'ses_b'",
            [OLD_MS + 500_000],
        )?;
        drop(conn);

        let third = quiet_importer(&db_file).with_cursor_path(&cursor);
        let stats = third.sync(&store).await?;
        assert_eq!(stats.read, 1, "only the modified session is replayed");
        assert_eq!(stats.skipped, 2);
        assert_eq!(stored_rows(&store).await?.len(), 3);
        Ok(())
    }

    /// Corrupt or foreign-version cursor: full pass, no panic.
    #[tokio::test]
    async fn corrupt_cursor_falls_back_to_full_pass() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;
        let cursor = cursor_file(dir.path());
        fs::create_dir_all(cursor.parent().unwrap())?;

        for garbage in ["{truncated", r#"{"version":999,"state":{}}"#, ""] {
            fs::write(&cursor, garbage)?;
            let store = InMemoryMemoryStore::new();
            let importer = quiet_importer(&db_file).with_cursor_path(&cursor);
            let stats = importer.sync(&store).await?;
            assert_eq!(
                stats.read, 3,
                "garbage cursor {:?} must mean a full pass",
                garbage
            );
            assert_eq!(stored_rows(&store).await?.len(), 3);
        }
        Ok(())
    }

    /// A pass with a failing session must not persist a cursor that moved past
    /// it. After restart the failed session is retried, and only it.
    #[tokio::test]
    async fn cursor_not_advanced_on_failure_after_restart() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;
        let cursor = cursor_file(dir.path());
        let store = InMemoryMemoryStore::new();
        // A BLOB where text is expected makes ses_b unreadable for this pass.
        let conn = Connection::open(&db_file)?;
        conn.execute(
            "UPDATE message SET data = X'00FF' WHERE session_id = 'ses_b'",
            [],
        )?;
        drop(conn);

        let first = quiet_importer(&db_file).with_cursor_path(&cursor);
        let stats = first.sync(&store).await?;
        assert_eq!(stats.read, 2);
        assert_eq!(stats.read_errors, 1);
        drop(first);

        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&cursor)?)?;
        let watermark = saved["state"]["watermark_ms"].as_i64().unwrap_or(i64::MAX);
        assert!(
            watermark <= OLD_MS + 1_000,
            "persisted watermark {} moved past the failed session",
            watermark
        );

        let conn = Connection::open(&db_file)?;
        conn.execute(
            "UPDATE message SET data = ?1 WHERE session_id = 'ses_b'",
            [json!({"role": "user"}).to_string()],
        )?;
        drop(conn);

        let second = quiet_importer(&db_file).with_cursor_path(&cursor);
        let stats = second.sync(&store).await?;
        assert_eq!(stats.read, 1, "only the failed session is retried");
        assert_eq!(stats.skipped, 2);
        Ok(())
    }

    /// A persisted watermark in the future must be clamped on load, or the
    /// replay floor lands above every real session and the pass sees nothing.
    #[tokio::test]
    async fn persisted_future_watermark_is_clamped_on_load() -> Result<()> {
        let dir = tempdir()?;
        let db_file = dir.path().join("opencode.db");
        seed_three_sessions(&db_file)?;
        let cursor = cursor_file(dir.path());
        let poisoned = CursorState {
            watermark_ms: now_ms() + 10 * 86_400_000,
            ..Default::default()
        };
        ingest_cursor::save(&cursor, &poisoned)?;

        let store = InMemoryMemoryStore::new();
        let importer = quiet_importer(&db_file).with_cursor_path(&cursor);
        let stats = importer.sync(&store).await?;
        assert_eq!(
            stats.candidates, 3,
            "future watermark must not hide sessions"
        );
        assert_eq!(stats.read, 3);
        Ok(())
    }
}
