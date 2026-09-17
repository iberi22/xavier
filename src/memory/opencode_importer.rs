//! OpenCode Sessions Importer
//!
//! Connects to ~/.local/share/opencode/opencode.db (or `OPENCODE_DB_PATH` env var)
//! and imports OpenCode sessions, messages, and text parts into Xavier `MemoryStore`
//! under `opencode://` paths.

use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::embedding::Embedder;
use crate::kernel::filters::strip_ansi;
use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};

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
    pub turns: Vec<OpenCodeTurn>,
}

pub struct OpenCodeImporter {
    db_path: PathBuf,
    embedder: Option<Arc<dyn Embedder>>,
}

impl Default for OpenCodeImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenCodeImporter {
    pub fn new() -> Self {
        let db_path = Self::resolve_db_path();
        Self {
            db_path,
            embedder: None,
        }
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    pub fn with_path<P: AsRef<Path>>(path: P) -> Self {
        Self {
            db_path: path.as_ref().to_path_buf(),
            embedder: None,
        }
    }

    pub fn with_path_and_embedder<P: AsRef<Path>>(path: P, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            db_path: path.as_ref().to_path_buf(),
            embedder: Some(embedder),
        }
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

    /// Read sessions, messages, and parts from `opencode.db`.
    pub fn read_sessions(&self) -> Result<Vec<OpenCodeSession>> {
        if !self.db_path.exists() {
            debug!(
                "OpenCode db path {:?} does not exist. Skipping.",
                self.db_path
            );
            return Ok(Vec::new());
        }

        let conn = Connection::open_with_flags(
            &self.db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;

        let mut stmt = conn.prepare(
            "SELECT id, title, agent, model, time_created FROM session ORDER BY time_created DESC",
        )?;

        let mut sessions = Vec::new();
        let session_rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?;

        for s_res in session_rows {
            let (session_id, title, agent, model_raw, time_created) = s_res?;

            // Extract model name from JSON string if needed
            let model_name = model_raw.as_deref().and_then(|m| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(m) {
                    v["id"].as_str().map(|s| s.to_string())
                } else {
                    Some(m.to_string())
                }
            });

            // Fetch messages for this session
            let mut msg_stmt = conn.prepare(
                "SELECT id, data FROM message WHERE session_id = ? ORDER BY time_created ASC",
            )?;

            let msg_rows = msg_stmt.query_map([&session_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;

            let mut turns = Vec::new();
            for m_res in msg_rows {
                let (msg_id, msg_data_str) = m_res?;
                let role = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg_data_str) {
                    v["role"].as_str().unwrap_or("user").to_string()
                } else {
                    "user".to_string()
                };

                // Fetch text parts for this message
                let mut part_stmt = conn.prepare(
                    "SELECT data FROM part WHERE message_id = ? ORDER BY time_created ASC",
                )?;

                let part_rows = part_stmt.query_map([&msg_id], |row| row.get::<_, String>(0))?;

                let mut combined_text = String::new();
                for p_res in part_rows {
                    let part_data_str = p_res?;
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&part_data_str) {
                        let p_type = v["type"].as_str().unwrap_or("");
                        if p_type == "text" {
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

            if !turns.is_empty() {
                sessions.push(OpenCodeSession {
                    session_id,
                    title,
                    agent,
                    model: model_name,
                    time_created,
                    turns,
                });
            }
        }

        info!(
            "🔍 OpenCodeImporter read {} valid sessions from {:?}",
            sessions.len(),
            self.db_path
        );
        Ok(sessions)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::InMemoryMemoryStore;
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
}
