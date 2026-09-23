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
                if ext == "json" {
                    match self.import_json_file(&path, store).await {
                        Ok(mut records) => imported_records.append(&mut records),
                        Err(e) => warn!("Failed to import Hermes session json {:?}: {}", path, e),
                    }
                } else if ext == "db"
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
            let record_id = stable_key("memory", &[&workspace_id, &record_path]);

            // Incremental skip (stability): identical content already stored
            // reuses the existing record without re-embedding. Fail-open:
            // on store error fall through to the normal embed+put path.
            if let Ok(Some(existing)) = store.get(&workspace_id, &record_id).await {
                if existing.content == content {
                    final_records.push(existing);
                    continue;
                }
            }

            let mut record = MemoryRecord {
                id: record_id,
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

            store.put(record.clone()).await?;
            final_records.push(record);
        }

        Ok(final_records)
    }

    async fn import_json_file(
        &self,
        json_path: &Path,
        store: &dyn MemoryStore,
    ) -> Result<Vec<MemoryRecord>> {
        let content = tokio::fs::read_to_string(json_path).await?;
        let val: serde_json::Value = serde_json::from_str(&content)?;

        let session_id = val
            .get("session_id")
            .and_then(|s| s.as_str())
            .unwrap_or_else(|| {
                json_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
            })
            .to_string();

        let model = val
            .pointer("/request/body/model")
            .and_then(|m| m.as_str())
            .unwrap_or("hermes-unknown");

        let mut final_records = Vec::new();

        if let Some(messages) = val
            .pointer("/request/body/messages")
            .and_then(|m| m.as_array())
        {
            for (idx, msg) in messages.iter().enumerate() {
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                let msg_content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                if msg_content.trim().is_empty() {
                    continue;
                }

                let record_path = format!("hermes/sessions/{}/{}", session_id, idx);
                let workspace_id = "agent:hermes".to_string();
                let record_id = stable_key("memory", &[&workspace_id, &record_path]);

                // Incremental skip (stability): identical content already
                // stored reuses the existing record without re-embedding.
                if let Ok(Some(existing)) = store.get(&workspace_id, &record_id).await {
                    if existing.content == msg_content {
                        final_records.push(existing);
                        continue;
                    }
                }

                let mut record = MemoryRecord {
                    id: record_id,
                    workspace_id: workspace_id.clone(),
                    path: record_path.clone(),
                    content: msg_content.to_string(),
                    metadata: json!({
                        "source_app": "hermes",
                        "agent_id": "hermes",
                        "session_id": session_id,
                        "role": role,
                        "model": model,
                        "turn_index": idx,
                        "importer_version": env!("CARGO_PKG_VERSION"),
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
                    if let Ok(emb) = embedder.encode(msg_content).await {
                        record.embedding = emb;
                    }
                }

                store.put(record.clone()).await?;
                final_records.push(record);
            }
        }

        Ok(final_records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::{Embedder, EmbeddingError};
    use crate::memory::store::InMemoryMemoryStore;
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    /// Counting embedder: records every encode call for the incremental-skip test.
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

    /// Stability regression: a second identical import must not re-encode.
    #[tokio::test]
    async fn test_hermes_importer_second_pass_no_reencode() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("session1.db");

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
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = HermesImporter::with_dir(dir.path()).with_embedder(embedder.clone());

        let first = importer.import_all(&store).await?;
        assert_eq!(first.len(), 2);
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 2);

        // Second identical pass: all records hit the incremental skip.
        let second = importer.import_all(&store).await?;
        assert_eq!(second.len(), 2);
        assert_eq!(
            embedder.calls.load(Ordering::SeqCst),
            2,
            "second identical import must not call encode again"
        );

        Ok(())
    }

    fn write_json_fixture(dir: &std::path::Path, user_text: &str) -> Result<()> {
        let val = serde_json::json!({
            "session_id": "json-session-1",
            "request": {
                "body": {
                    "model": "hermes-test",
                    "messages": [
                        {"role": "user", "content": user_text},
                        {"role": "assistant", "content": "Acknowledged."}
                    ]
                }
            }
        });
        std::fs::write(
            dir.join("json-session-1.json"),
            serde_json::to_string(&val)?,
        )?;
        Ok(())
    }

    /// Stability S1.01: the JSON import path must also skip identical content.
    #[tokio::test]
    async fn test_hermes_json_second_pass_no_reencode() -> Result<()> {
        let dir = tempdir()?;
        write_json_fixture(dir.path(), "Hello JSON Hermes")?;

        let store = InMemoryMemoryStore::new();
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = HermesImporter::with_dir(dir.path()).with_embedder(embedder.clone());

        let first = importer.import_all(&store).await?;
        assert_eq!(first.len(), 2);
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 2);

        let second = importer.import_all(&store).await?;
        assert_eq!(second.len(), 2);
        assert_eq!(
            embedder.calls.load(Ordering::SeqCst),
            2,
            "second identical JSON import must not call encode again"
        );

        Ok(())
    }

    /// Stability S1.01: changed content must re-embed exactly once and update the store.
    #[tokio::test]
    async fn test_hermes_changed_content_reembeds() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("session1.db");

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
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let importer = HermesImporter::with_dir(dir.path()).with_embedder(embedder.clone());

        let first = importer.import_all(&store).await?;
        assert_eq!(first.len(), 2);
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 2);

        // Mutate one message, re-import: exactly one re-encode expected.
        {
            let conn = Connection::open(&db_path)?;
            conn.execute(
                "UPDATE messages SET content = 'Hello Hermes EDITED' WHERE id = 'msg1'",
                [],
            )?;
        }
        let second = importer.import_all(&store).await?;
        assert_eq!(second.len(), 2);
        assert_eq!(
            embedder.calls.load(Ordering::SeqCst),
            3,
            "exactly one changed record must re-encode"
        );

        let stored = store
            .get("hermes:session1", &second[0].id)
            .await?
            .expect("record must exist");
        assert_eq!(stored.content, "Hello Hermes EDITED");

        Ok(())
    }
}
