//! Google Antigravity Session Importer
//!
//! Scans ~/.gemini/antigravity/brain/ (or `ANTIGRAVITY_BRAIN_DIR` env var) for session
//! transcript JSONL files and indexes them into Xavier `MemoryStore` under `antigravity://` paths.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::fs;
use tracing::{debug, info, warn};

use crate::embedding::Embedder;
use crate::kernel::filters::strip_ansi;
use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};

/// A single turn in an Antigravity conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityTurn {
    pub role: String,
    pub content: String,
    pub tool_calls: Vec<String>,
}

/// A parsed Antigravity session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravitySession {
    pub session_id: String,
    pub transcript_path: PathBuf,
    pub turns: Vec<AntigravityTurn>,
}

/// Size and mtime of a transcript as last ingested.
///
/// The importer skips a transcript whose fingerprint is unchanged, so an idle
/// corpus costs one `stat` per session and no transcript read at all. Deciding
/// whether to re-read must never require reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptFingerprint {
    /// Whole seconds of the modification time. Second resolution is
    /// deliberate: nanoseconds would miss a rewrite landing in the same tick.
    pub mtime_secs: i64,
    pub len: u64,
}

/// What one `sync` pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AntigravitySyncStats {
    /// Transcripts found on disk, changed or not.
    pub candidates: usize,
    /// Transcripts deep-read (parsed and imported).
    pub read: usize,
    /// Transcripts skipped because their fingerprint was unchanged.
    pub skipped: usize,
    /// Records written to the store by this pass.
    pub records: usize,
    /// `store.get()` calls issued.
    pub store_reads: usize,
    /// `store.put()` calls issued.
    pub store_writes: usize,
    /// Whether this pass skipped anything as unchanged.
    pub had_skips: bool,
}

pub struct AntigravityImporter {
    /// Explicit override; `None` re-resolves from the environment each pass.
    brain_dir: Option<PathBuf>,
    embedder: Option<Arc<dyn Embedder>>,
    /// Fingerprint of each transcript as last ingested, keyed by path.
    ///
    /// Process-local by design: a restart costs one full pass, which is the
    /// same as the pre-cursor behaviour, never a missed import.
    seen: Mutex<HashMap<PathBuf, TranscriptFingerprint>>,
}

impl Default for AntigravityImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl AntigravityImporter {
    pub fn new() -> Self {
        Self {
            brain_dir: None,
            embedder: None,
            seen: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    pub fn with_dir<P: AsRef<Path>>(path: P) -> Self {
        Self {
            brain_dir: Some(path.as_ref().to_path_buf()),
            embedder: None,
            seen: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_dir_and_embedder<P: AsRef<Path>>(path: P, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            brain_dir: Some(path.as_ref().to_path_buf()),
            embedder: Some(embedder),
            seen: Mutex::new(HashMap::new()),
        }
    }

    fn current_dir(&self) -> PathBuf {
        self.brain_dir
            .clone()
            .unwrap_or_else(Self::resolve_brain_dir)
    }

    fn resolve_brain_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("ANTIGRAVITY_BRAIN_DIR") {
            return PathBuf::from(dir);
        }
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home)
                .join(".gemini")
                .join("antigravity")
                .join("brain");
            if path.exists() {
                return path;
            }
        }
        PathBuf::from(".gemini/antigravity/brain")
    }

    /// Scan `brain_dir` for session directories containing transcript.jsonl.
    pub async fn scan_sessions(&self) -> Result<Vec<AntigravitySession>> {
        let mut sessions = Vec::new();
        let brain_dir = self.current_dir();

        for (session_id, tpath) in locate_transcripts(&brain_dir).await? {
            match self.parse_transcript(&session_id, &tpath).await {
                Ok(session) => {
                    if !session.turns.is_empty() {
                        sessions.push(session);
                    }
                }
                Err(e) => {
                    debug!("Failed to parse Antigravity transcript {:?}: {}", tpath, e);
                }
            }
        }

        info!(
            "🔍 AntigravityImporter found {} valid sessions in {:?}",
            sessions.len(),
            brain_dir
        );
        Ok(sessions)
    }

    /// Parse a JSONL transcript file into an `AntigravitySession`.
    pub async fn parse_transcript(
        &self,
        session_id: &str,
        path: &Path,
    ) -> Result<AntigravitySession> {
        let content = fs::read_to_string(path).await?;
        let mut turns = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(val) => val,
                Err(_) => continue,
            };

            let step_type = v["type"].as_str().unwrap_or("");

            if step_type == "USER_INPUT" {
                if let Some(raw_content) = v["content"].as_str() {
                    let cleaned = strip_ansi(raw_content).trim().to_string();
                    if !cleaned.is_empty() {
                        turns.push(AntigravityTurn {
                            role: "user".to_string(),
                            content: cleaned,
                            tool_calls: Vec::new(),
                        });
                    }
                }
            } else if step_type == "PLANNER_RESPONSE" {
                let mut tool_names = Vec::new();
                if let Some(tool_calls) = v["tool_calls"].as_array() {
                    for tc in tool_calls {
                        if let Some(name) = tc["name"].as_str() {
                            let summary = tc["arguments"]["toolSummary"]
                                .as_str()
                                .or_else(|| tc["args"]["toolSummary"].as_str())
                                .unwrap_or("");
                            if summary.is_empty() {
                                tool_names.push(name.to_string());
                            } else {
                                tool_names.push(format!("{}({})", name, summary));
                            }
                        }
                    }
                }

                let response_text = v["content"]
                    .as_str()
                    .map(|s| strip_ansi(s).trim().to_string())
                    .unwrap_or_default();

                if !response_text.is_empty() || !tool_names.is_empty() {
                    turns.push(AntigravityTurn {
                        role: "assistant".to_string(),
                        content: response_text,
                        tool_calls: tool_names,
                    });
                }
            }
        }

        Ok(AntigravitySession {
            session_id: session_id.to_string(),
            transcript_path: path.to_path_buf(),
            turns,
        })
    }

    /// Import a single parsed Antigravity session into Xavier `MemoryStore`.
    pub async fn import_session(
        &self,
        session: &AntigravitySession,
        store: &dyn MemoryStore,
    ) -> Result<Vec<MemoryRecord>> {
        let mut stats = AntigravitySyncStats::default();
        self.import_session_counted(session, store, &mut stats)
            .await
    }

    /// `import_session` with the counters filled in, so `sync` can prove how
    /// many store round-trips a pass cost.
    async fn import_session_counted(
        &self,
        session: &AntigravitySession,
        store: &dyn MemoryStore,
        stats: &mut AntigravitySyncStats,
    ) -> Result<Vec<MemoryRecord>> {
        let mut records = Vec::new();
        let path = format!("antigravity://sessions/{}", session.session_id);
        let workspace_id = "agent:antigravity".to_string();

        let mut full_text = format!("# Google Antigravity Session: {}\n\n", session.session_id);

        for turn in &session.turns {
            full_text.push_str(&format!("### [{}]\n", turn.role));
            if !turn.tool_calls.is_empty() {
                full_text.push_str(&format!("🔧 **Tools**: {}\n\n", turn.tool_calls.join(", ")));
            }
            if !turn.content.is_empty() {
                full_text.push_str(&format!("{}\n\n", turn.content));
            }
        }

        let mut record = MemoryRecord {
            workspace_id: workspace_id.clone(),
            path: path.clone(),
            content: full_text,
            metadata: json!({
                "source_app": "antigravity",
                "agent_id": "antigravity",
                "session_id": session.session_id,
                "transcript_path": session.transcript_path.to_string_lossy(),
                "turn_count": session.turns.len(),
                "dedup": true,
            }),
            ..Default::default()
        };

        record.id = stable_key("memory", &[&workspace_id, &path]);

        // Incremental skip (stability S1.06): identical content already
        // stored reuses the existing record without re-embedding. Fail-open
        // on store errors.
        let lookup = store.get(&workspace_id, &record.id).await;
        // Count the round-trip whatever it returned: a miss (`None`) or an
        // error cost a store read too, and this counter is the production
        // proof that the cursor works.
        stats.store_reads += 1;
        if let Ok(Some(existing)) = lookup {
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
        stats.store_writes += 1;
        records.push(record);

        Ok(records)
    }

    /// Scan and import all Antigravity sessions into the given `MemoryStore`.
    ///
    /// Full scan: this is the explicit re-index path and never consults the
    /// cursor. Use [`Self::sync`] for the periodic background pass.
    pub async fn import_all(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>> {
        let sessions = self.scan_sessions().await?;
        let mut imported = Vec::new();

        for s in sessions {
            match self.import_session(&s, store).await {
                Ok(recs) => imported.extend(recs),
                Err(e) => warn!(
                    "Failed to import Antigravity session {}: {}",
                    s.session_id, e
                ),
            }
        }

        info!(
            "✅ Successfully imported {} Antigravity session records",
            imported.len()
        );
        Ok(imported)
    }

    /// Periodic pass: skips transcripts whose size and mtime are unchanged.
    ///
    /// The previous code parsed every transcript of every session on every
    /// cycle. Deciding whether a session changed costs one `stat`; the
    /// transcript is only opened for a session that actually moved. Like
    /// [`HermesImporter::sync`](crate::memory::hermes_importer::HermesImporter::sync)
    /// this is the path the background ingestion loop calls.
    ///
    /// The cursor lives in this instance, so the loop must build the importer
    /// once, outside its cycle loop. A constructor inside the loop throws the
    /// fingerprints away every cycle and silently degrades `sync` back into a
    /// full scan — which is exactly the bug this method exists to prevent.
    pub async fn sync(&self, store: &dyn MemoryStore) -> Result<AntigravitySyncStats> {
        let mut stats = AntigravitySyncStats::default();

        // Locate first, parse second: parsing is exactly the cost the
        // fingerprint is meant to avoid.
        let transcripts = locate_transcripts(&self.current_dir()).await?;
        stats.candidates = transcripts.len();

        // Snapshot under the lock, do the I/O unlocked, fold results back at
        // the end. Holding the lock across the scan would serialise concurrent
        // passes for no benefit.
        let mut seen = self.seen.lock().map(|s| s.clone()).unwrap_or_default();

        for (session_id, tpath) in transcripts {
            let fingerprint = match transcript_fingerprint(&tpath).await {
                Ok(f) => f,
                Err(e) => {
                    debug!("Failed to stat Antigravity transcript {:?}: {}", tpath, e);
                    continue;
                }
            };

            if is_unchanged(&tpath, fingerprint, &seen) {
                stats.skipped += 1;
                continue;
            }

            let session = match self.parse_transcript(&session_id, &tpath).await {
                Ok(s) => s,
                Err(e) => {
                    debug!("Failed to parse Antigravity transcript {:?}: {}", tpath, e);
                    continue;
                }
            };
            if session.turns.is_empty() {
                // Nothing to store, but the transcript is accounted for.
                seen.insert(tpath, fingerprint);
                continue;
            }

            stats.read += 1;
            match self
                .import_session_counted(&session, store, &mut stats)
                .await
            {
                Ok(_) => {
                    seen.insert(tpath, fingerprint);
                }
                Err(e) => warn!("Failed to import Antigravity session {}: {}", session_id, e),
            }
        }

        if let Ok(mut state) = self.seen.lock() {
            *state = seen;
        }

        stats.records = stats.store_writes;
        stats.had_skips = stats.skipped > 0;
        info!(
            "✅ AntigravityImporter sync: {} candidates, {} read, {} skipped, {} records, {} store reads",
            stats.candidates, stats.read, stats.skipped, stats.records, stats.store_reads
        );
        Ok(stats)
    }

    /// Reconciliation escape hatch: re-read every transcript, cursor ignored.
    ///
    /// `rsync -t`, `tar` extraction and `mv` all preserve an old mtime, so a
    /// transcript rewritten that way would otherwise be skipped forever.
    pub async fn force_reindex(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>> {
        self.import_all(store).await
    }
}

/// Whether a transcript's fingerprint matches what the cursor recorded.
fn is_unchanged(
    path: &Path,
    fingerprint: TranscriptFingerprint,
    snapshot: &HashMap<PathBuf, TranscriptFingerprint>,
) -> bool {
    snapshot.get(path) == Some(&fingerprint)
}

/// Every `(session_id, transcript_path)` on disk, without parsing anything.
///
/// `scan_sessions` parses; `sync` must not (deciding whether a session changed
/// may never require reading it), so both start from this shared locator and
/// the two paths cannot drift apart on which file counts as a transcript.
async fn locate_transcripts(brain_dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    if !brain_dir.exists() {
        debug!(
            "Antigravity brain dir {:?} does not exist. Skipping.",
            brain_dir
        );
        return Ok(out);
    }

    let mut read_dir = match fs::read_dir(brain_dir).await {
        Ok(rd) => rd,
        Err(e) => {
            warn!("Failed to read Antigravity brain dir: {}", e);
            return Ok(out);
        }
    };

    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // .system_generated/logs/transcript.jsonl, else the legacy flat path.
        let primary = path
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        let fallback = path.join("transcript.jsonl");
        let transcript = if primary.exists() {
            Some(primary)
        } else if fallback.exists() {
            Some(fallback)
        } else {
            None
        };
        if let Some(t) = transcript {
            out.push((entry.file_name().to_string_lossy().to_string(), t));
        }
    }
    Ok(out)
}

/// Size and mtime of a transcript, read with one `stat`.
async fn transcript_fingerprint(path: &Path) -> Result<TranscriptFingerprint> {
    let meta = fs::metadata(path).await?;
    let mtime_secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(TranscriptFingerprint {
        mtime_secs,
        len: meta.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::{Embedder, EmbeddingError};
    use crate::memory::store::InMemoryMemoryStore;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_parse_and_import_antigravity_transcript() -> Result<()> {
        let dir = tempdir()?;
        let session_dir = dir.path().join("sess_test_123");
        let logs_dir = session_dir.join(".system_generated").join("logs");
        fs::create_dir_all(&logs_dir).await?;

        let transcript_file = logs_dir.join("transcript.jsonl");
        let lines = vec![
            json!({
                "step_index": 0,
                "type": "USER_INPUT",
                "content": "\x1B[31mHello Antigravity!\x1B[0m",
            }),
            json!({
                "step_index": 1,
                "type": "PLANNER_RESPONSE",
                "content": "I am ready to assist you.",
                "tool_calls": [
                    {
                        "name": "view_file",
                        "arguments": { "toolSummary": "Read config" }
                    }
                ]
            }),
        ];

        let mut content = String::new();
        for l in lines {
            content.push_str(&l.to_string());
            content.push('\n');
        }
        fs::write(&transcript_file, content).await?;

        let importer = AntigravityImporter::with_dir(dir.path());
        let store = InMemoryMemoryStore::new();

        let imported = importer.import_all(&store).await?;
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].path, "antigravity://sessions/sess_test_123");
        assert!(imported[0].content.contains("Hello Antigravity!"));
        assert!(!imported[0].content.contains("\x1B[31m")); // ANSI stripped!
        assert!(imported[0].content.contains("view_file(Read config)"));

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

    async fn write_fixture(dir: &std::path::Path) -> Result<()> {
        let logs_dir = dir
            .join("sess_test_123")
            .join(".system_generated")
            .join("logs");
        fs::create_dir_all(&logs_dir).await?;
        let lines = vec![
            json!({"step_index": 0, "type": "USER_INPUT", "content": "Hello Antigravity!"}),
            json!({"step_index": 1, "type": "PLANNER_RESPONSE", "content": "I am ready."}),
        ];
        let mut content = String::new();
        for l in lines {
            content.push_str(&l.to_string());
            content.push('\n');
        }
        fs::write(logs_dir.join("transcript.jsonl"), content).await?;
        Ok(())
    }

    /// Stability S1.06: a second identical import must not re-encode.
    #[tokio::test]
    async fn test_antigravity_second_pass_no_reencode() -> Result<()> {
        let dir = tempdir()?;
        write_fixture(dir.path()).await?;

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = AntigravityImporter::with_dir_and_embedder(dir.path(), embedder.clone());

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

    // ── Cursor tests ───────────────────────────────────────────────────
    // The cursor lives in the importer instance. These tests all build the
    // importer ONCE and call `sync` repeatedly, which is the shape of the
    // production loop. If the loop ever moves the constructor back inside the
    // cycle loop, `cursor_survives_across_cycles` and the two tests below it go
    // red, which is exactly the regression the move had to prevent.

    /// Write a session dir with a transcript of `turns` user inputs.
    async fn write_session(root: &std::path::Path, session_id: &str, texts: &[&str]) -> Result<()> {
        let logs_dir = root.join(session_id).join(".system_generated").join("logs");
        fs::create_dir_all(&logs_dir).await?;
        let mut content = String::new();
        for (i, t) in texts.iter().enumerate() {
            content.push_str(
                &json!({"step_index": i, "type": "USER_INPUT", "content": t}).to_string(),
            );
            content.push('\n');
        }
        fs::write(logs_dir.join("transcript.jsonl"), content).await?;
        Ok(())
    }

    /// A cycle with no changes reads no transcript at all: `read == 0`.
    #[tokio::test]
    async fn sync_second_cycle_without_changes_reads_nothing() -> Result<()> {
        let dir = tempdir()?;
        write_session(dir.path(), "sess_a", &["one", "two"]).await?;

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = AntigravityImporter::with_dir_and_embedder(dir.path(), embedder.clone());

        let first = importer.sync(&store).await?;
        assert_eq!(first.read, 1);
        assert_eq!(first.records, 1);
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 1);

        let second = importer.sync(&store).await?;
        assert_eq!(second.candidates, 1);
        assert_eq!(second.skipped, 1);
        assert_eq!(second.read, 0, "unchanged transcript must not be re-read");
        assert_eq!(second.records, 0);
        assert_eq!(
            second.store_reads, 0,
            "skipped pass must not touch the store"
        );
        assert_eq!(
            embedder.calls.load(Ordering::SeqCst),
            1,
            "unchanged transcript must not be re-encoded"
        );

        Ok(())
    }

    /// With a new session on disk, only that one is read.
    #[tokio::test]
    async fn sync_new_session_is_the_only_one_read() -> Result<()> {
        let dir = tempdir()?;
        write_session(dir.path(), "sess_a", &["first"]).await?;

        let store = InMemoryMemoryStore::new();
        let importer = AntigravityImporter::with_dir(dir.path());
        assert_eq!(importer.sync(&store).await?.read, 1);

        write_session(dir.path(), "sess_b", &["second"]).await?;

        let second = importer.sync(&store).await?;
        assert_eq!(second.candidates, 2);
        assert_eq!(second.skipped, 1, "the old session must be skipped");
        assert_eq!(second.read, 1, "only the new session may be read");

        Ok(())
    }

    /// The cursor lives in the importer, so reusing one instance across cycles
    /// is what makes cycle 2 cheaper than cycle 1. A fresh instance per cycle —
    /// the mutation this test guards against — reads the corpus every time.
    #[tokio::test]
    async fn cursor_survives_across_cycles() -> Result<()> {
        let dir = tempdir()?;
        write_session(dir.path(), "sess_a", &["first"]).await?;
        write_session(dir.path(), "sess_b", &["second"]).await?;

        let store = InMemoryMemoryStore::new();
        let importer = AntigravityImporter::with_dir(dir.path());

        let first = importer.sync(&store).await?;
        assert_eq!(first.read, 2);

        // Same instance, no changes on disk: the fingerprints must still be
        // there.
        let second = importer.sync(&store).await?;
        assert_eq!(second.skipped, 2);
        assert_eq!(
            second.read, 0,
            "cursor state must survive across sync cycles on one instance"
        );

        // Control: an instance built fresh has no cursor and reads everything.
        // If this ever also reads 0, the fingerprint is not the thing deciding
        // the skip and the test above would pass for the wrong reason.
        let fresh = AntigravityImporter::with_dir(dir.path());
        let cold = fresh.sync(&store).await?;
        assert_eq!(
            cold.read, 2,
            "a fresh instance must have an empty cursor and read the corpus"
        );

        Ok(())
    }

    /// A grown transcript is re-read; the skip is fingerprint-driven, so a
    /// size change alone must be enough.
    #[tokio::test]
    async fn sync_grown_transcript_is_reread() -> Result<()> {
        let dir = tempdir()?;
        write_session(dir.path(), "sess_a", &["one"]).await?;

        let store = InMemoryMemoryStore::new();
        let importer = AntigravityImporter::with_dir(dir.path());
        assert_eq!(importer.sync(&store).await?.read, 1);

        write_session(dir.path(), "sess_a", &["one", "two", "three"]).await?;

        let second = importer.sync(&store).await?;
        assert_eq!(second.read, 1, "a grown transcript must be re-read");
        assert_eq!(second.skipped, 0);

        Ok(())
    }

    /// `import_all` stays the explicit full-scan path: it ignores the cursor.
    #[tokio::test]
    async fn import_all_ignores_the_cursor() -> Result<()> {
        let dir = tempdir()?;
        write_session(dir.path(), "sess_a", &["one"]).await?;

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = AntigravityImporter::with_dir_and_embedder(dir.path(), embedder.clone());

        importer.sync(&store).await?;
        // force_reindex is import_all: the store short-circuits on identical
        // content, so nothing is re-encoded, but the transcript is re-read.
        let forced = importer.force_reindex(&store).await?;
        assert_eq!(forced.len(), 1);
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 1);

        Ok(())
    }
}
