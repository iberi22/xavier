//! Codex Session Importer
//!
//! Scans ~/.codex/sessions (or `CODEX_SESSIONS_DIR` env var) for JSON / JSONL session files
//! and indexes them into Xavier `MemoryStore` under `codex://` paths.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::fs;
use tracing::{debug, info, warn};

use crate::embedding::Embedder;
use crate::memory::ingest_cursor;
use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};

/// A turn or message in a Codex session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexMessage {
    pub role: Option<String>,
    pub content: Option<String>,
    pub timestamp: Option<String>,
}

/// A Codex session parsed from disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSession {
    pub session_id: String,
    pub created_at: Option<String>,
    pub topic: Option<String>,
    pub messages: Vec<CodexMessage>,
}

/// Size and mtime of a session file as last ingested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct FileFingerprint {
    mtime_secs: i64,
    len: u64,
}

/// What one `sync` pass did.
#[derive(Debug, Default, Clone, Copy)]
pub struct CodexSyncStats {
    /// Session files found on disk.
    pub candidates: usize,
    /// Files read and parsed this pass.
    pub read: usize,
    /// Files skipped because their fingerprint was unchanged.
    pub skipped: usize,
    /// Files that failed to read, parse or store; retried next pass.
    pub errors: usize,
    /// Records returned by the imports of this pass.
    pub records: usize,
}

pub struct CodexImporter {
    /// Explicit override; `None` re-resolves from the environment each pass.
    sessions_dir: Option<PathBuf>,
    embedder: Option<Arc<dyn Embedder>>,
    /// Fingerprint of each file as last ingested successfully. Saved to
    /// `cursor_path` after each pass when one is set; otherwise process-local,
    /// and a restart costs one full pass, never a missed import.
    seen: Mutex<HashMap<PathBuf, FileFingerprint>>,
    /// Where `seen` is saved after each pass. `None` = memory only.
    cursor_path: Option<PathBuf>,
}

impl Default for CodexImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexImporter {
    pub fn new() -> Self {
        Self {
            sessions_dir: None,
            embedder: None,
            seen: Mutex::new(HashMap::new()),
            cursor_path: None,
        }
    }

    pub fn with_embedder(embedder: Arc<dyn Embedder>) -> Self {
        Self {
            sessions_dir: None,
            embedder: Some(embedder),
            seen: Mutex::new(HashMap::new()),
            cursor_path: None,
        }
    }

    pub fn with_dir<P: AsRef<Path>>(path: P) -> Self {
        Self {
            sessions_dir: Some(path.as_ref().to_path_buf()),
            embedder: None,
            seen: Mutex::new(HashMap::new()),
            cursor_path: None,
        }
    }

    pub fn with_dir_and_embedder<P: AsRef<Path>>(path: P, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            sessions_dir: Some(path.as_ref().to_path_buf()),
            embedder: Some(embedder),
            seen: Mutex::new(HashMap::new()),
            cursor_path: None,
        }
    }

    /// Persist the fingerprints at `path` and restore them now. A missing,
    /// corrupt or foreign-version file means a full first pass.
    pub fn with_cursor_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        let path = path.as_ref().to_path_buf();
        if let Some(loaded) = ingest_cursor::load::<HashMap<PathBuf, FileFingerprint>>(&path) {
            if let Ok(seen) = self.seen.get_mut() {
                *seen = loaded;
            }
        }
        self.cursor_path = Some(path);
        self
    }

    fn current_dir(&self) -> PathBuf {
        self.sessions_dir
            .clone()
            .unwrap_or_else(Self::resolve_sessions_dir)
    }

    fn resolve_sessions_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("CODEX_SESSIONS_DIR") {
            return PathBuf::from(dir);
        }
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home.clone()).join(".codex").join("sessions");
            if path.exists() {
                return path;
            }
            let path_archived = PathBuf::from(home.clone()).join(".codex").join("archived");
            if path_archived.exists() {
                return path_archived;
            }
            let path_root = PathBuf::from(home).join(".codex");
            if path_root.exists() {
                return path_root;
            }
        }
        PathBuf::from(".codex/sessions")
    }

    /// List every candidate session file under the sessions dir (recursive).
    async fn list_session_files(&self) -> Vec<PathBuf> {
        let sessions_dir = self.current_dir();
        let mut files = Vec::new();

        if !fs::try_exists(&sessions_dir).await.unwrap_or(false) {
            debug!("Codex sessions directory {:?} does not exist", sessions_dir);
            return files;
        }

        let mut dirs_to_visit = vec![sessions_dir];
        while let Some(current_dir) = dirs_to_visit.pop() {
            let mut entries = match fs::read_dir(&current_dir).await {
                Ok(e) => e,
                Err(e) => {
                    warn!("Could not read directory {:?}: {}", current_dir, e);
                    continue;
                }
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_dir() {
                    dirs_to_visit.push(path);
                    continue;
                }
                if !path.is_file() {
                    continue;
                }

                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if ext != "json" && ext != "jsonl" && !file_name.starts_with("rollout-") {
                    continue;
                }
                files.push(path);
            }
        }
        files
    }

    async fn parse_file(path: &Path) -> Option<CodexSession> {
        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let content = fs::read_to_string(path).await.ok()?;
        Self::parse_session_content(&file_stem, &content).ok()
    }

    /// Scan `sessions_dir` and parse all `.json` and `.jsonl` session files recursively.
    ///
    /// Full scan: never consults the cursor. Use [`Self::sync`] for the
    /// periodic pass.
    pub async fn scan_sessions(&self) -> Result<Vec<CodexSession>> {
        let mut sessions = Vec::new();
        for path in self.list_session_files().await {
            if let Some(session) = Self::parse_file(&path).await {
                sessions.push(session);
            }
        }

        info!("✅ Discovered {} Codex sessions", sessions.len());
        Ok(sessions)
    }

    /// Periodic pass: a file whose size and mtime are unchanged since it was
    /// last imported successfully is skipped with a single `stat`, instead of
    /// being re-read (135 files / ~158 MB every cycle on the live node). A file
    /// is remembered only AFTER its import succeeded, so a failure is retried.
    pub async fn sync(&self, store: &dyn MemoryStore) -> Result<CodexSyncStats> {
        let files = self.list_session_files().await;
        let mut stats = CodexSyncStats {
            candidates: files.len(),
            ..Default::default()
        };

        for path in files {
            let fingerprint = match file_fingerprint(&path).await {
                Ok(f) => f,
                Err(e) => {
                    warn!("Failed to stat Codex session file {:?}: {}", path, e);
                    stats.errors += 1;
                    continue;
                }
            };
            let unchanged = self
                .seen
                .lock()
                .map(|m| m.get(&path) == Some(&fingerprint))
                .unwrap_or(false);
            if unchanged {
                stats.skipped += 1;
                continue;
            }

            let Some(session) = Self::parse_file(&path).await else {
                warn!("Failed to read Codex session file {:?}; will retry", path);
                stats.errors += 1;
                continue;
            };
            stats.read += 1;
            match self.import_session(&session, store).await {
                Ok(recs) => {
                    stats.records += recs.len();
                    if let Ok(mut m) = self.seen.lock() {
                        m.insert(path, fingerprint);
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to import Codex session {}: {}; will retry",
                        session.session_id, e
                    );
                    stats.errors += 1;
                }
            }
        }

        // Save only after the pass folded its results back: a file whose read or
        // store write failed never reached `seen`, so it is never persisted as
        // done. A save error costs one full pass after restart, not data.
        if let Some(path) = self.cursor_path.as_deref() {
            if let Ok(seen) = self.seen.lock() {
                if let Err(e) = ingest_cursor::save(path, &*seen) {
                    warn!("Could not persist Codex ingest cursor: {}", e);
                }
            }
        }

        info!(
            "✅ CodexImporter sync: {} candidates, {} read, {} skipped, {} errors",
            stats.candidates, stats.read, stats.skipped, stats.errors
        );
        Ok(stats)
    }

    fn parse_session_content(file_stem: &str, content: &str) -> Result<CodexSession> {
        // Try parsing full JSON object
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
            let session_id = v["session_id"]
                .as_str()
                .or_else(|| v["id"].as_str())
                .unwrap_or(file_stem)
                .to_string();
            let created_at = v["created_at"]
                .as_str()
                .or_else(|| v["timestamp"].as_str())
                .map(|s| s.to_string());
            let topic = v["topic"]
                .as_str()
                .or_else(|| v["title"].as_str())
                .map(|s| s.to_string());

            let mut messages = Vec::new();
            if let Some(msg_arr) = v["messages"].as_array() {
                for m in msg_arr {
                    let role = m["role"].as_str().map(|s| s.to_string());
                    let text = m["content"]
                        .as_str()
                        .or_else(|| m["text"].as_str())
                        .map(|s| s.to_string());
                    let ts = m["timestamp"].as_str().map(|s| s.to_string());
                    if role.is_some() || text.is_some() {
                        messages.push(CodexMessage {
                            role,
                            content: text,
                            timestamp: ts,
                        });
                    }
                }
            }
            return Ok(CodexSession {
                session_id,
                created_at,
                topic,
                messages,
            });
        }

        // Fallback JSONL line-by-line parsing
        let mut messages = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                messages.extend(jsonl_message(&v));
            }
        }

        Ok(CodexSession {
            session_id: file_stem.to_string(),
            created_at: None,
            topic: None,
            messages,
        })
    }

    /// Convert parsed session into `MemoryRecord` entries and store them.
    pub async fn import_session(
        &self,
        session: &CodexSession,
        store: &dyn MemoryStore,
    ) -> Result<Vec<MemoryRecord>> {
        let mut records = Vec::new();
        let path = format!("codex://sessions/{}", session.session_id);
        let workspace_id = "agent:codex".to_string();

        let mut full_text = String::new();
        if let Some(t) = &session.topic {
            full_text.push_str(&format!("# Session Topic: {}\n\n", t));
        }

        for msg in &session.messages {
            let role = msg.role.as_deref().unwrap_or("user");
            let content = msg.content.as_deref().unwrap_or("");
            full_text.push_str(&format!("### [{}]\n{}\n\n", role, content));
        }

        if full_text.trim().is_empty() {
            full_text = format!("Codex session {}", session.session_id);
        }

        let mut record = MemoryRecord {
            workspace_id: workspace_id.clone(),
            path: path.clone(),
            content: full_text.clone(),
            metadata: json!({
                "source_app": "codex",
                "session_id": session.session_id,
                "created_at": session.created_at,
                "topic": session.topic,
                "message_count": session.messages.len(),
            }),
            ..Default::default()
        };

        record.id = stable_key("memory", &[&workspace_id, &path]);

        // Incremental skip (stability S1.08): identical content already
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

    /// Scan and index all Codex sessions into the given `MemoryStore`.
    pub async fn import_all(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>> {
        let sessions = self.scan_sessions().await?;
        let mut imported = Vec::new();

        for s in sessions {
            match self.import_session(&s, store).await {
                Ok(recs) => imported.extend(recs),
                Err(e) => warn!("Failed to import Codex session {}: {}", s.session_id, e),
            }
        }

        info!(
            "✅ Successfully imported {} Codex session records",
            imported.len()
        );
        Ok(imported)
    }
}

/// One JSONL line as a message. Accepts the flat `{role, content}` shape and the
/// Codex CLI rollout item, whose text sits in `payload.content[]` parts. Tool
/// calls, tool output and reasoning carry no message text and yield `None`.
fn jsonl_message(line: &serde_json::Value) -> Option<CodexMessage> {
    let item = if line["payload"]["type"].as_str() == Some("message") {
        &line["payload"]
    } else {
        line
    };
    let role = item["role"].as_str().map(|s| s.to_string());
    // Only user and assistant turns are memory. Developer and system items carry
    // injected instructions (AGENTS.md, system prompts): noise, and an injection
    // channel for agents that later read Xavier.
    if matches!(role.as_deref(), Some(r) if r != "user" && r != "assistant") {
        return None;
    }
    let content = message_text(item);
    if role.is_none() && content.is_none() {
        return None;
    }
    let timestamp = item["timestamp"]
        .as_str()
        .or_else(|| line["timestamp"].as_str())
        .map(|s| s.to_string());
    Some(CodexMessage {
        role,
        content,
        timestamp,
    })
}

/// Text of a message item: a `content` or `text` string, else the joined
/// `input_text` / `output_text` parts of a `content` array. Parts count only on
/// message items, never on reasoning or tool-call items.
fn message_text(item: &serde_json::Value) -> Option<String> {
    if let Some(s) = item["content"].as_str().or_else(|| item["text"].as_str()) {
        return Some(s.to_string());
    }
    if !matches!(item["type"].as_str(), None | Some("message")) {
        return None;
    }
    let parts: Vec<&str> = item["content"]
        .as_array()?
        .iter()
        .filter(|p| matches!(p["type"].as_str(), Some("input_text" | "output_text")))
        .filter_map(|p| p["text"].as_str())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

async fn file_fingerprint(path: &Path) -> std::io::Result<FileFingerprint> {
    let meta = fs::metadata(path).await?;
    let mtime_secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(FileFingerprint {
        mtime_secs,
        len: meta.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::{Embedder, EmbeddingError, NoopEmbedder};
    use crate::memory::store::InMemoryMemoryStore;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_scan_and_import_codex_session_json() -> Result<()> {
        let dir = tempdir()?;
        let session_file = dir.path().join("sess_001.json");
        let content = json!({
            "session_id": "codex-123",
            "created_at": "2026-08-15T12:00:00Z",
            "topic": "Refactoring module",
            "messages": [
                { "role": "user", "content": "Refactor codebase" },
                { "role": "assistant", "content": "Refactored successfully" }
            ]
        });
        fs::write(&session_file, serde_json::to_string(&content)?).await?;

        let importer = CodexImporter::with_dir_and_embedder(dir.path(), Arc::new(NoopEmbedder));
        let sessions = importer.scan_sessions().await?;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "codex-123");
        assert_eq!(sessions[0].messages.len(), 2);

        let store = InMemoryMemoryStore::new();
        let records = importer.import_all(&store).await?;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, "codex://sessions/codex-123");
        assert_eq!(records[0].metadata["source_app"], "codex");
        assert!(records[0].content.contains("Refactored successfully"));

        Ok(())
    }

    #[tokio::test]
    async fn test_scan_codex_jsonl() -> Result<()> {
        let dir = tempdir()?;
        let session_file = dir.path().join("sess_002.jsonl");
        let lines = format!(
            "{}\n{}\n",
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "assistant", "content": "Hi there"})
        );
        fs::write(&session_file, lines).await?;

        let importer = CodexImporter::with_dir(dir.path());
        let sessions = importer.scan_sessions().await?;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "sess_002");
        assert_eq!(sessions[0].messages.len(), 2);

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

    /// Stability S1.08: a second identical import must not re-encode.
    #[tokio::test]
    async fn test_codex_second_pass_no_reencode() -> Result<()> {
        let dir = tempdir()?;
        let session_file = dir.path().join("sess_001.json");
        let content = json!({
            "session_id": "codex-123",
            "created_at": "2026-08-15T12:00:00Z",
            "topic": "Refactoring module",
            "messages": [
                { "role": "user", "content": "Refactor codebase" },
                { "role": "assistant", "content": "Refactored successfully" }
            ]
        });
        fs::write(&session_file, serde_json::to_string(&content)?).await?;

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = CodexImporter::with_dir_and_embedder(dir.path(), embedder.clone());

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

    /// D8: the periodic pass must not re-read or re-encode unchanged files, and
    /// must pick up a rewritten one.
    #[tokio::test]
    async fn sync_skips_unchanged_and_rereads_changed() -> Result<()> {
        let dir = tempdir()?;
        let file = dir.path().join("sess_001.json");
        let write = |text: &str| {
            serde_json::to_string(&json!({
                "session_id": "codex-123",
                "messages": [{ "role": "user", "content": text }]
            }))
        };
        fs::write(&file, write("one")?).await?;

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = CodexImporter::with_dir_and_embedder(dir.path(), embedder.clone());

        let first = importer.sync(&store).await?;
        assert_eq!((first.read, first.skipped), (1, 0));
        let second = importer.sync(&store).await?;
        assert_eq!((second.read, second.skipped), (0, 1));
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 1);

        // Different length => different fingerprint, whatever the mtime tick.
        fs::write(&file, write("one plus more text")?).await?;
        let third = importer.sync(&store).await?;
        assert_eq!((third.read, third.skipped), (1, 0));
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 2);
        Ok(())
    }

    /// D13: a directory that does not exist at construction time is picked up
    /// once it appears.
    #[tokio::test]
    async fn sync_resolves_the_dir_each_pass() -> Result<()> {
        let dir = tempdir()?;
        let late = dir.path().join("late");
        std::env::set_var("CODEX_SESSIONS_DIR", &late);
        let importer = CodexImporter::new();
        let store = InMemoryMemoryStore::new();
        assert_eq!(importer.sync(&store).await?.candidates, 0);
        std::fs::create_dir_all(&late)?;
        std::fs::write(
            late.join("s.json"),
            r#"{"session_id":"x","messages":[{"role":"user","content":"hi"}]}"#,
        )?;
        let stats = importer.sync(&store).await?;
        std::env::remove_var("CODEX_SESSIONS_DIR");
        assert_eq!((stats.candidates, stats.read), (1, 1));
        Ok(())
    }

    fn cursor_file(dir: &Path) -> PathBuf {
        dir.join("ingest-cursors").join("codex.json")
    }

    fn write_session(dir: &Path, name: &str, text: &str) -> Result<()> {
        let body = json!({"session_id": name, "messages": [{"role": "user", "content": text}]});
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_string(&body)?,
        )?;
        Ok(())
    }

    /// Restart replay: a fresh importer on the same cursor file reads only the
    /// files that changed since the previous instance's last successful pass.
    #[tokio::test]
    async fn restart_replays_only_changed() -> Result<()> {
        let dir = tempdir()?;
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions)?;
        for name in ["a", "b", "c"] {
            write_session(&sessions, name, "hello")?;
        }
        let cursor = cursor_file(dir.path());
        let store = InMemoryMemoryStore::new();

        let first = CodexImporter::with_dir(&sessions).with_cursor_path(&cursor);
        let stats = first.sync(&store).await?;
        assert_eq!((stats.read, stats.skipped), (3, 0));
        drop(first);

        let second = CodexImporter::with_dir(&sessions).with_cursor_path(&cursor);
        let stats = second.sync(&store).await?;
        assert_eq!(
            (stats.read, stats.skipped),
            (0, 3),
            "restart must not re-read unchanged files"
        );
        drop(second);

        write_session(&sessions, "b", "hello, changed")?;
        let third = CodexImporter::with_dir(&sessions).with_cursor_path(&cursor);
        let stats = third.sync(&store).await?;
        assert_eq!(
            (stats.read, stats.skipped),
            (1, 2),
            "only the changed file is replayed"
        );
        Ok(())
    }

    /// A file rewritten while the process was down is re-read after restart,
    /// and its new content reaches the store.
    #[tokio::test]
    async fn changed_file_is_reread_after_restart() -> Result<()> {
        let dir = tempdir()?;
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions)?;
        write_session(&sessions, "a", "before")?;
        let cursor = cursor_file(dir.path());
        let store = InMemoryMemoryStore::new();
        CodexImporter::with_dir(&sessions)
            .with_cursor_path(&cursor)
            .sync(&store)
            .await?;

        write_session(&sessions, "a", "after the restart, with longer text")?;
        let restarted = CodexImporter::with_dir(&sessions).with_cursor_path(&cursor);
        let stats = restarted.sync(&store).await?;
        assert_eq!((stats.read, stats.skipped), (1, 0));
        let records = store.list("agent:codex").await?;
        assert_eq!(records.len(), 1);
        assert!(records[0].content.contains("after the restart"));
        Ok(())
    }

    /// No cursor, a corrupt cursor, or one from another format version means a
    /// full pass: every file is read and nothing panics.
    #[tokio::test]
    async fn missing_corrupt_or_foreign_cursor_is_a_full_pass() -> Result<()> {
        let dir = tempdir()?;
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions)?;
        for name in ["a", "b", "c"] {
            write_session(&sessions, name, "hello")?;
        }
        let cursor = cursor_file(dir.path());
        let store = InMemoryMemoryStore::new();

        let stats = CodexImporter::with_dir(&sessions)
            .with_cursor_path(&cursor)
            .sync(&store)
            .await?;
        assert_eq!(stats.read, 3, "missing cursor");

        for garbage in [
            "{truncated",
            r#"{"version":999,"state":{}}"#,
            r#"{"version":1,"state":"wrong shape"}"#,
            "",
        ] {
            std::fs::create_dir_all(cursor.parent().unwrap())?;
            std::fs::write(&cursor, garbage)?;
            let stats = CodexImporter::with_dir(&sessions)
                .with_cursor_path(&cursor)
                .sync(&store)
                .await?;
            assert_eq!(stats.read, 3, "cursor {garbage:?} must mean a full pass");
        }
        Ok(())
    }

    /// A file that fails to read is not recorded in the saved cursor, so the
    /// restarted importer retries it and skips only the file that succeeded.
    #[tokio::test]
    async fn failed_read_is_not_persisted_as_done() -> Result<()> {
        let dir = tempdir()?;
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions)?;
        write_session(&sessions, "a", "ok")?;
        // Invalid UTF-8: `read_to_string` fails, so b.json is an error this pass.
        std::fs::write(sessions.join("b.json"), [0xff_u8, 0xfe, 0xfd])?;
        let cursor = cursor_file(dir.path());
        let store = InMemoryMemoryStore::new();

        let first = CodexImporter::with_dir(&sessions).with_cursor_path(&cursor);
        let stats = first.sync(&store).await?;
        assert_eq!((stats.read, stats.errors), (1, 1));
        drop(first);

        let saved: HashMap<PathBuf, FileFingerprint> =
            ingest_cursor::load(&cursor).expect("the pass saved a cursor");
        assert!(saved.contains_key(&sessions.join("a.json")));
        assert!(
            !saved.contains_key(&sessions.join("b.json")),
            "a failed file must not be persisted as done"
        );

        write_session(&sessions, "b", "now readable")?;
        let second = CodexImporter::with_dir(&sessions).with_cursor_path(&cursor);
        let stats = second.sync(&store).await?;
        assert_eq!(
            (stats.read, stats.skipped),
            (1, 1),
            "only the failed file is retried"
        );
        Ok(())
    }

    /// Flat `{role, content}` lines keep working; `text` and `timestamp` are read.
    #[tokio::test]
    async fn jsonl_flat_shape_still_parses() -> Result<()> {
        let dir = tempdir()?;
        let lines = format!(
            "{}\n{}\n",
            json!({"role": "user", "content": "Hello", "timestamp": "2026-10-08T10:00:00Z"}),
            json!({"role": "assistant", "text": "Hi there"})
        );
        std::fs::write(dir.path().join("flat.jsonl"), lines)?;

        let sessions = CodexImporter::with_dir(dir.path()).scan_sessions().await?;
        let msgs = &sessions[0].messages;
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role.as_deref(), Some("user"));
        assert_eq!(msgs[0].content.as_deref(), Some("Hello"));
        assert_eq!(msgs[0].timestamp.as_deref(), Some("2026-10-08T10:00:00Z"));
        assert_eq!(msgs[1].role.as_deref(), Some("assistant"));
        assert_eq!(msgs[1].content.as_deref(), Some("Hi there"));
        Ok(())
    }

    /// Codex CLI rollout: message text sits in `payload.content[]` parts. Session
    /// metadata, tool calls, tool output, reasoning and event mirrors add nothing.
    #[tokio::test]
    async fn jsonl_rollout_shape_reads_message_parts_only() -> Result<()> {
        let dir = tempdir()?;
        let lines = [
            json!({"timestamp": "2026-10-08T10:00:00Z", "type": "session_meta", "payload": {"id": "abc"}}),
            json!({"timestamp": "2026-10-08T10:00:01Z", "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Fix the parser"}]}}),
            json!({"timestamp": "2026-10-08T10:00:02Z", "type": "response_item", "payload": {"type": "reasoning", "summary": [], "content": [{"type": "reasoning_text", "text": "private chain"}]}}),
            json!({"timestamp": "2026-10-08T10:00:03Z", "type": "response_item", "payload": {"type": "function_call", "name": "shell", "arguments": "{\"cmd\":\"ls\"}"}}),
            json!({"timestamp": "2026-10-08T10:00:04Z", "type": "response_item", "payload": {"type": "function_call_output", "call_id": "c1", "output": "file list"}}),
            json!({"timestamp": "2026-10-08T10:00:05Z", "type": "event_msg", "payload": {"type": "user_message", "message": "Fix the parser"}}),
            json!({"timestamp": "2026-10-08T10:00:06Z", "type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Parser fixed"}]}}),
            json!({"type": "reasoning", "content": [{"type": "input_text", "text": "not a message"}]}),
        ];
        let body = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(
            dir.path().join("rollout-2026-10-08T10-00-00-abc.jsonl"),
            body,
        )?;

        let sessions = CodexImporter::with_dir(dir.path()).scan_sessions().await?;
        let got: Vec<(Option<&str>, Option<&str>, Option<&str>)> = sessions[0]
            .messages
            .iter()
            .map(|m| {
                (
                    m.role.as_deref(),
                    m.content.as_deref(),
                    m.timestamp.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    Some("user"),
                    Some("Fix the parser"),
                    Some("2026-10-08T10:00:01Z")
                ),
                (
                    Some("assistant"),
                    Some("Parser fixed"),
                    Some("2026-10-08T10:00:06Z")
                ),
            ]
        );
        Ok(())
    }

    /// Developer and system items carry injected instructions, not session
    /// content: they are skipped, while the user and assistant parts around them
    /// are kept.
    #[tokio::test]
    async fn jsonl_developer_and_system_items_are_skipped() -> Result<()> {
        let dir = tempdir()?;
        let lines = [
            json!({"type": "response_item", "payload": {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "AGENTS.md: always run cargo fmt"}]}}),
            json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Fix the parser"}]}}),
            json!({"role": "system", "content": "You are a coding agent"}),
            json!({"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Parser fixed"}]}}),
        ];
        let body = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(dir.path().join("rollout-roles.jsonl"), body)?;

        let sessions = CodexImporter::with_dir(dir.path()).scan_sessions().await?;
        let got: Vec<(Option<&str>, Option<&str>)> = sessions[0]
            .messages
            .iter()
            .map(|m| (m.role.as_deref(), m.content.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![
                (Some("user"), Some("Fix the parser")),
                (Some("assistant"), Some("Parser fixed")),
            ]
        );
        Ok(())
    }
}
