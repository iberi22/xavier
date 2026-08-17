//! Codex Session Importer
//!
//! Scans ~/.codex/sessions (or `CODEX_SESSIONS_DIR` env var) for JSON / JSONL session files
//! and indexes them into Xavier `MemoryStore` under `codex://` paths.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

pub struct CodexImporter {
    sessions_dir: PathBuf,
    embedder: Option<Arc<dyn Embedder>>,
}

impl Default for CodexImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexImporter {
    pub fn new() -> Self {
        let sessions_dir = Self::resolve_sessions_dir();
        Self {
            sessions_dir,
            embedder: None,
        }
    }

    pub fn with_embedder(embedder: Arc<dyn Embedder>) -> Self {
        let sessions_dir = Self::resolve_sessions_dir();
        Self {
            sessions_dir,
            embedder: Some(embedder),
        }
    }

    pub fn with_dir<P: AsRef<Path>>(path: P) -> Self {
        Self {
            sessions_dir: path.as_ref().to_path_buf(),
            embedder: None,
        }
    }

    pub fn with_dir_and_embedder<P: AsRef<Path>>(path: P, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            sessions_dir: path.as_ref().to_path_buf(),
            embedder: Some(embedder),
        }
    }

    fn resolve_sessions_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("CODEX_SESSIONS_DIR") {
            return PathBuf::from(dir);
        }
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home).join(".codex").join("sessions");
            if path.exists() {
                return path;
            }
        }
        PathBuf::from(".codex/sessions")
    }

    /// Scan `sessions_dir` and parse all `.json` and `.jsonl` session files.
    pub async fn scan_sessions(&self) -> Result<Vec<CodexSession>> {
        let mut sessions = Vec::new();

        if !fs::try_exists(&self.sessions_dir).await.unwrap_or(false) {
            debug!(
                "Codex sessions directory {:?} does not exist",
                self.sessions_dir
            );
            return Ok(sessions);
        }

        let mut entries = match fs::read_dir(&self.sessions_dir).await {
            Ok(e) => e,
            Err(e) => {
                warn!("Could not read directory {:?}: {}", self.sessions_dir, e);
                return Ok(sessions);
            }
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext != "json" && ext != "jsonl" {
                continue;
            }

            let file_stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            if let Ok(content) = fs::read_to_string(&path).await {
                if let Ok(session) = Self::parse_session_content(&file_stem, &content) {
                    sessions.push(session);
                }
            }
        }

        info!("✅ Discovered {} Codex sessions", sessions.len());
        Ok(sessions)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::NoopEmbedder;
    use crate::memory::store::InMemoryMemoryStore;
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
}
