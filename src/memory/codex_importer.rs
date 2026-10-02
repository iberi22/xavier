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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Fingerprint of each file as last ingested successfully. Process-local:
    /// a restart costs one full pass, never a missed import.
    seen: Mutex<HashMap<PathBuf, FileFingerprint>>,
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
        }
    }

    pub fn with_embedder(embedder: Arc<dyn Embedder>) -> Self {
        Self {
            sessions_dir: None,
            embedder: Some(embedder),
            seen: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_dir<P: AsRef<Path>>(path: P) -> Self {
        Self {
            sessions_dir: Some(path.as_ref().to_path_buf()),
            embedder: None,
            seen: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_dir_and_embedder<P: AsRef<Path>>(path: P, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            sessions_dir: Some(path.as_ref().to_path_buf()),
            embedder: Some(embedder),
            seen: Mutex::new(HashMap::new()),
        }
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
            if let Ok(m) = serde_json::from_str::<serde_json::Value>(line) {
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
}
