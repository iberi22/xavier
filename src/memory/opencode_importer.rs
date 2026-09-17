//! OpenCode Session & Chat History Importer
//!
//! Scans ~/.local/share/opencode/opencode.db (or OPENCODE_DB_PATH)
//! and imports session transcripts into Xavier MemoryStore under opencode:// paths.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::embedding::Embedder;
use crate::memory::sanitizer::{RawTurn, SanitizeConfig, Sanitizer};
use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};

/// Importer for OpenCode session databases.
pub struct OpenCodeImporter {
    db_path: PathBuf,
    embedder: Option<Arc<dyn Embedder>>,
    sanitizer: Sanitizer,
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
            sanitizer: Sanitizer::new(SanitizeConfig::default()),
        }
    }

    pub fn with_path<P: AsRef<Path>>(path: P) -> Self {
        Self {
            db_path: path.as_ref().to_path_buf(),
            embedder: None,
            sanitizer: Sanitizer::new(SanitizeConfig::default()),
        }
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    fn resolve_db_path() -> PathBuf {
        if let Ok(path) = std::env::var("OPENCODE_DB_PATH") {
            return PathBuf::from(path);
        }
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("opencode")
                .join("opencode.db");
            if path.exists() {
                return path;
            }
        }
        PathBuf::from(".local/share/opencode/opencode.db")
    }

    /// Read raw turns from OpenCode SQLite database via spawn_blocking.
    async fn read_turns(&self) -> Result<Vec<RawTurn>> {
        let db_path = self.db_path.clone();
        if !tokio::fs::try_exists(&db_path).await.unwrap_or(false) {
            debug!("OpenCode database {:?} does not exist", db_path);
            return Ok(Vec::new());
        }

        tokio::task::spawn_blocking(move || -> Result<Vec<RawTurn>> {
            let conn = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

            let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
            let tables: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            if !tables.contains(&"message".to_string()) && !tables.contains(&"messages".to_string()) {
                debug!("No message table found in OpenCode db");
                return Ok(Vec::new());
            }

            let msg_table = if tables.contains(&"message".to_string()) { "message" } else { "messages" };
            let has_session = tables.contains(&"session".to_string()) || tables.contains(&"sessions".to_string());
            let sess_table = if tables.contains(&"session".to_string()) { "session" } else { "sessions" };

            // Introspect columns in message table
            let pragma_query = format!("PRAGMA table_info({})");
            let mut pragma_stmt = conn.prepare(&pragma_query)?;
            let cols: Vec<String> = pragma_stmt
                .query_map([], |row| row.get(1))?
                .filter_map(|r| r.ok())
                .collect();

            let content_col = if cols.contains(&"content".to_string()) {
                "content"
            } else if cols.contains(&"data".to_string()) {
                "data"
            } else if cols.contains(&"body".to_string()) {
                "body"
            } else {
                "id"
            };

            let query = if has_session {
                format!(
                    "SELECT m.id, m.session_id, m.role, m.{}, s.title, s.cwd, s.model                      FROM {} m LEFT JOIN {} s ON s.id = m.session_id                      WHERE m.{} IS NOT NULL AND m.{} != '' LIMIT 20000",
                    content_col, msg_table, sess_table, content_col, content_col
                )
            } else {
                format!(
                    "SELECT m.id, m.session_id, m.role, m.{}, '', '', ''                      FROM {} m                      WHERE m.{} IS NOT NULL AND m.{} != '' LIMIT 20000",
                    content_col, msg_table, content_col, content_col
                )
            };

            let mut query_stmt = conn.prepare(&query)?;
            let rows = query_stmt.query_map([], |row| {
                let msg_id: String = row.get(0).unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
                let session_id: String = row.get(1).unwrap_or_else(|_| "opencode-default".to_string());
                let role: String = row.get(2).unwrap_or_else(|_| "user".to_string());
                let raw_content: String = row.get(3).unwrap_or_default();
                let title: String = row.get(4).unwrap_or_default();
                let cwd: String = row.get(5).unwrap_or_default();
                let model: String = row.get(6).unwrap_or_default();

                Ok((msg_id, session_id, role, raw_content, title, cwd, model))
            })?;

            let mut turns = Vec::new();
            for (idx, r) in rows.flatten().enumerate() {
                let (msg_id, session_id, role, raw_content, title, cwd, model) = r;
                let mut content = raw_content.clone();

                // If content is structured JSON, extract text parts
                if let Ok(val) = serde_json::from_str::<Value>(&raw_content) {
                    if let Some(parts) = val.get("parts").and_then(|p| p.as_array()) {
                        let mut extracted = String::new();
                        for part in parts {
                            if let Some(txt) = part.get("text").and_then(|t| t.as_str()) {
                                extracted.push_str(txt);
                                extracted.push('
');
                            }
                        }
                        if !extracted.trim().is_empty() {
                            content = extracted;
                        }
                    }
                }

                let project = if !cwd.is_empty() {
                    cwd
                } else if !title.is_empty() {
                    title
                } else {
                    session_id.clone()
                };

                let model_str = if !model.is_empty() { Some(model) } else { None };

                turns.push(RawTurn {
                    session_id: session_id.clone(),
                    turn_index: idx,
                    role,
                    content,
                    timestamp: None,
                    model: model_str,
                    file_paths: vec![],
                    meta: serde_json::json!({
                        "source_app": "opencode",
                        "message_id": msg_id,
                        "project": project,
                    }),
                });
            }

            Ok(turns)
        }).await?
    }

    /// Import all OpenCode sessions into the given MemoryStore.
    pub async fn import_all(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>> {
        let mut raw_turns = self.read_turns().await?;
        let report = self.sanitizer.sanitize_turns(&mut raw_turns);
        debug!("OpenCode sanitization report: {} kept, {} dropped", report.kept, report.dropped_boilerplate);

        let mut all_imported = Vec::new();
        let workspace_id = "agent:opencode".to_string();

        for turn in raw_turns {
            let record_path = format!("opencode://sessions/{}/{}", turn.session_id, turn.turn_index);
            let mut record = MemoryRecord {
                workspace_id: workspace_id.clone(),
                path: record_path.clone(),
                content: turn.content.clone(),
                metadata: serde_json::json!({
                    "source_app": "opencode",
                    "agent_id": "opencode",
                    "project": turn.meta.get("project").and_then(|p| p.as_str()).unwrap_or("unknown"),
                    "model": turn.model.unwrap_or_else(|| "opencode-unknown".to_string()),
                    "session_id": turn.session_id,
                    "turn_index": turn.turn_index,
                    "file_paths": turn.file_paths,
                    "importer_version": env!("CARGO_PKG_VERSION"),
                }),
                ..Default::default()
            };

            record.id = stable_key("memory", &[&workspace_id, &record_path]);

            if let Some(embedder) = &self.embedder {
                if let Ok(emb) = embedder.encode(&record.content).await {
                    record.embedding = emb;
                }
            }

            store.put(record.clone()).await.context("put opencode record")?;
            all_imported.push(record);
        }

        info!("✅ Successfully imported {} OpenCode turns", all_imported.len());
        Ok(all_imported)
    }
}
