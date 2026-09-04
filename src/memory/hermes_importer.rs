//! Hermes Sessions Importer
//!
//! Scans ~/.hermes/sessions/ for SQLite session databases and imports session history into MemoryStore.

use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::embedding::Embedder;
use crate::memory::schema::MemoryLevel;
use crate::memory::store::{stable_key, MemoryRecord, MemoryStore};
use std::sync::Arc;

pub struct HermesImporter {
    sessions_dir: PathBuf,
    embedder: Option<Arc<dyn Embedder>>,
}

impl HermesImporter {
    pub fn new() -> Self {
        let sessions_dir = Self::resolve_sessions_dir();
        Self {
            sessions_dir,
            embedder: None,
        }
    }

    pub fn with_dir<P: AsRef<Path>>(path: P) -> Self {
        Self {
            sessions_dir: path.as_ref().to_path_buf(),
            embedder: None,
        }
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    fn resolve_sessions_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("HERMES_SESSIONS_DIR") {
            return PathBuf::from(dir);
        }
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home).join(".hermes").join("sessions");
            if path.exists() {
                return path;
            }
        }
        PathBuf::from(".hermes/sessions")
    }

    /// Import all session SQLite files found in sessions_dir into MemoryStore.
    pub async fn import_all(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>> {
        info!(
            "🔍 HermesImporter scanning directory: {:?}",
            self.sessions_dir
        );
        let mut imported_records = Vec::new();

        if !self.sessions_dir.exists() {
            info!(
                "Hermes sessions dir {:?} does not exist. Skipping.",
                self.sessions_dir
            );
            return Ok(imported_records);
        }

        let mut read_dir = tokio::fs::read_dir(&self.sessions_dir).await?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                if ext == "db"
                    || ext == "sqlite"
                    || ext == "sqlite3"
                    || path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .contains("session")
                {
                    match self.import_db_file(&path, store).await {
                        Ok(mut records) => imported_records.append(&mut records),
                        Err(e) => warn!("Failed to import Hermes session db {:?}: {}", path, e),
                    }
                }
            }
        }

        info!(
            "✅ HermesImporter imported {} records",
            imported_records.len()
        );
        Ok(imported_records)
    }

    async fn import_db_file(
        &self,
        db_path: &Path,
        store: &dyn MemoryStore,
    ) -> Result<Vec<MemoryRecord>> {
        let db_path_buf = db_path.to_path_buf();
        let session_id = db_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let records = tokio::task::spawn_blocking(move || -> Result<Vec<(String, String, String)>> {
            let conn = Connection::open_with_flags(&db_path_buf, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

            // Query tables
            let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
            let tables: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            let mut items = Vec::new();

            if tables.contains(&"messages".to_string()) {
                let query = "SELECT id, role, content FROM messages WHERE content IS NOT NULL AND content != ''";
                if let Ok(mut msg_stmt) = conn.prepare(query) {
                    let rows = msg_stmt.query_map([], |row| {
                        let id: String = row.get(0).unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
                        let role: String = row.get(1).unwrap_or_else(|_| "user".to_string());
                        let content: String = row.get(2).unwrap_or_default();
                        Ok((id, role, content))
                    });
                    if let Ok(mapped) = rows {
                        for r in mapped.flatten() {
                            items.push(r);
                        }
                    }
                }
            } else if tables.contains(&"history".to_string()) {
                let query = "SELECT id, role, content FROM history WHERE content IS NOT NULL AND content != ''";
                if let Ok(mut msg_stmt) = conn.prepare(query) {
                    let rows = msg_stmt.query_map([], |row| {
                        let id: String = row.get(0).unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
                        let role: String = row.get(1).unwrap_or_else(|_| "user".to_string());
                        let content: String = row.get(2).unwrap_or_default();
                        Ok((id, role, content))
                    });
                    if let Ok(mapped) = rows {
                        for r in mapped.flatten() {
                            items.push(r);
                        }
                    }
                }
            } else {
                // Fallback inspect any table with text/content columns
                for table in tables {
                    if table.starts_with("sqlite_") { continue; }
                    let safe_table = table.replace('"', "\"\"");
                    let query = format!("SELECT rowid, content FROM \"{}\" WHERE content IS NOT NULL LIMIT 500", safe_table);
                    if let Ok(mut msg_stmt) = conn.prepare(&query) {
                        let rows = msg_stmt.query_map([], |row| {
                            let rowid: i64 = row.get(0).unwrap_or(0);
                            let content: String = row.get(1).unwrap_or_default();
                            Ok((rowid.to_string(), "unknown".to_string(), content))
                        });
                        if let Ok(mapped) = rows {
                            for r in mapped.flatten() {
                                items.push(r);
                            }
                        }
                    }
                }
            }

            Ok(items)
        }).await??;

        let mut final_records = Vec::new();
        for (item_id, role, content) in records {
            let record_path = format!("hermes/sessions/{}/{}", session_id, item_id);
            let workspace_id = format!("hermes:{}", session_id);

            let mut record = MemoryRecord {
                id: String::new(),
                workspace_id: workspace_id.clone(),
                path: record_path.clone(),
                content: content.clone(),
                metadata: json!({
                    "source": "hermes_sessions",
                    "session_id": session_id,
                    "role": role,
                    "item_id": item_id,
                }),
                embedding: vec![],
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                revision: 1,
                primary: true,
                parent_id: None,
                cluster_id: None,
                level: MemoryLevel::Raw,
                relation: None,
                score: 0.0,
                deleted_at: None,
                clearance: Default::default(),
                revisions: vec![],
                encrypted_dek: None,
                content_iv: None,
                metadata_iv: None,
                ..Default::default()
            };

            if let Some(embedder) = &self.embedder {
                if let Ok(emb) = embedder.encode(&content).await {
                    record.embedding = emb;
                }
            }

            record.id = stable_key("memory", &[&record.workspace_id, &record.path]);
            store.put(record.clone()).await?;
            final_records.push(record);
        }

        Ok(final_records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::InMemoryMemoryStore;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_hermes_importer() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("session1.db");

        // Create a test sqlite db with messages table
        {
            let conn = Connection::open(&db_path)?;
            conn.execute(
                "CREATE TABLE messages (id TEXT PRIMARY KEY, role TEXT, content TEXT)",
                [],
            )?;
            conn.execute(
                "INSERT INTO messages (id, role, content) VALUES ('msg1', 'user', 'Hello Hermes')",
                [],
            )?;
            conn.execute(
                "INSERT INTO messages (id, role, content) VALUES ('msg2', 'assistant', 'Hello User')",
                [],
            )?;
        }

        let store = InMemoryMemoryStore::new();
        let importer = HermesImporter::with_dir(dir.path());
        let records = importer.import_all(&store).await?;

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].path, "hermes/sessions/session1/msg1");
        assert_eq!(records[0].content, "Hello Hermes");

        Ok(())
    }
}
