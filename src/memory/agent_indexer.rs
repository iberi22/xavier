//! Agent memory indexer
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::memory::store::{MemoryRecord, MemoryStore};
use anyhow::Result;
use chrono::Utc;
use tracing::{debug, info};

use crate::memory::agent_scanner::{AgentScanner, AgentSession};
use crate::memory::file_indexer::{FileChunk, FileIndexer, IndexedFile};

/// Stable virtual path of an agent session: `agent_memory://<ide>/<source stem>`.
/// The same transcript always maps to the same path, so `add()` upserts it
/// instead of creating a new record on every scan.
pub fn virtual_path_for(session: &AgentSession) -> String {
    let source_stem = std::path::Path::new(&session.source_file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("x-{}", uuid::Uuid::new_v4()));
    format!("agent_memory://{}/{}", session.ide, source_stem)
}

/// AgentIndexer se encarga de formatear e indexar las sesiones de agentes (Cursor, Windsurf, etc.)
#[derive(Clone)]
pub struct AgentIndexer {
    scanner: AgentScanner,
    // Podríamos usar el FileIndexer subyacente para reusar la logica de chunking
    #[allow(dead_code)] // Held for underlying chunking logic reuse
    _file_indexer: FileIndexer,
}

impl AgentIndexer {
    /// Ingests a single `IndexedFile` for an agent session into the store, enforcing
    /// coherent forgetting semantics:
    /// - If a vigente record exists and source file is unchanged (same mtime and content): no-op.
    /// - If a vigente record exists and source file changed: updates vigente record in-place (same ID and path).
    /// - If the route was explicitly forgotten and source file is unchanged: skips reinsertion (no resurrection).
    /// - If brand new: inserts a new record.
    pub async fn ingest_file(
        &self,
        store: &dyn MemoryStore,
        workspace_id: &str,
        file: &IndexedFile,
    ) -> Result<Option<String>> {
        let all_records = store.list(workspace_id).await?;
        let matching: Vec<MemoryRecord> = all_records
            .into_iter()
            .filter(|r| r.path == file.path)
            .collect();

        let is_forgotten = |r: &MemoryRecord| -> bool {
            r.deleted_at.is_some()
                || r.metadata
                    .get("deleted_at")
                    .and_then(|v| v.as_str())
                    .is_some()
        };

        let mut active = Vec::new();
        let mut forgotten = Vec::new();

        for r in matching {
            if is_forgotten(&r) {
                forgotten.push(r);
            } else {
                active.push(r);
            }
        }

        if !active.is_empty() {
            active.sort_by(|a, b| {
                b.updated_at
                    .cmp(&a.updated_at)
                    .then_with(|| b.id.cmp(&a.id))
            });
            let mut vigente = active.remove(0);

            let last_mod_matches = vigente
                .metadata
                .get("last_modified")
                .and_then(|v| v.as_str())
                .is_some_and(|lm| lm == file.last_modified);

            if last_mod_matches && vigente.content == file.content {
                return Ok(Some(vigente.id));
            }

            vigente.content = file.content.clone();
            vigente.updated_at = Utc::now();
            vigente.deleted_at = None;
            if let Some(obj) = vigente.metadata.as_object_mut() {
                obj.insert(
                    "last_modified".to_string(),
                    serde_json::json!(file.last_modified),
                );
                obj.insert("size".to_string(), serde_json::json!(file.size));
                obj.remove("deleted_at");
            }
            store.update(vigente.clone()).await?;
            return Ok(Some(vigente.id));
        }

        if !forgotten.is_empty() {
            forgotten.sort_by(|a, b| {
                b.updated_at
                    .cmp(&a.updated_at)
                    .then_with(|| b.id.cmp(&a.id))
            });
            let latest_forgotten = &forgotten[0];
            let last_mod_matches = latest_forgotten
                .metadata
                .get("last_modified")
                .and_then(|v| v.as_str())
                .is_some_and(|lm| lm == file.last_modified);

            if last_mod_matches {
                debug!(path = %file.path, "Skipping reinsertion of explicitly forgotten agent session");
                return Ok(None);
            }
        }

        let record = MemoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            workspace_id: workspace_id.to_string(),
            path: file.path.clone(),
            content: file.content.clone(),
            metadata: serde_json::json!({
                "source": "agent_scanner",
                "last_modified": file.last_modified,
                "size": file.size,
            }),
            embedding: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            revision: 1,
            primary: true,
            deleted_at: None,
            ..Default::default()
        };
        let id = record.id.clone();
        store.put(record).await?;
        Ok(Some(id))
    }

    /// New.
    pub fn new(file_indexer: FileIndexer) -> Self {
        Self {
            scanner: AgentScanner::new(),
            _file_indexer: file_indexer,
        }
    }

    /// Scanner.
    pub fn scanner(&self) -> &AgentScanner {
        &self.scanner
    }

    /// Escanea, formatea a Markdown e indexa en la base de datos de memoria
    pub async fn index_agents(&self) -> Result<Vec<IndexedFile>> {
        info!("🤖 Starting Agentic IDE Conversation Indexing...");
        let sessions = self.scanner.scan_all().await?;

        let mut indexed_files = Vec::new();

        for session in sessions {
            let markdown_content = self.format_session_to_markdown(&session);

            // Re-use chunk generation logic from file_indexer
            // Usamos un FileIndexer mock o la instancia principal para el chunking
            let chunks = self.generate_chunks_for_session(&markdown_content);

            // Path estable derivado del archivo fuente: mismo transcript → mismo
            // path → add() hace UPSERT por path (sin duplicados entre pasadas).
            // El UUID aleatorio anterior (uuid::new_v4) duplicaba TODO el corpus
            // de Cursor en cada escaneo de 6h (42k+ registros de ~414 archivos).
            let virtual_path = virtual_path_for(&session);

            let indexed_file = IndexedFile {
                path: virtual_path.clone(),
                content: markdown_content,
                chunks,
                last_modified: session.updated_at,
                size: session.messages.len(), // Number of messages
            };

            indexed_files.push(indexed_file);
            debug!("Formatted and chunked session: {}", virtual_path);
        }

        info!(
            "✅ Agent Indexing complete. Formatted {} virtual documents.",
            indexed_files.len()
        );
        Ok(indexed_files)
    }

    fn format_session_to_markdown(&self, session: &AgentSession) -> String {
        let mut md = String::new();
        md.push_str("# Agent Conversation Session\n");
        md.push_str(&format!("- **IDE/Tool**: {}\n", session.ide));
        if let Some(ref proj) = session.project_path {
            md.push_str(&format!("- **Project**: {}\n", proj));
        }
        md.push_str(&format!("- **Last Updated**: {}\n", session.updated_at));
        md.push_str(&format!("- **Source DB**: {}\n\n", session.source_file));

        md.push_str("## Chat History\n\n");

        for msg in &session.messages {
            md.push_str(&format!("### {}\n", msg.role.to_uppercase()));
            md.push_str(&format!("{}\n\n", msg.content));
        }

        md
    }

    fn generate_chunks_for_session(&self, content: &str) -> Vec<FileChunk> {
        let mut chunks = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        let mut current_chunk = String::new();
        let mut start_line = 0;
        let mut line_count = 0;

        for (i, line) in lines.iter().enumerate() {
            current_chunk.push_str(line);
            current_chunk.push('\n');
            line_count += 1;

            if line_count >= 30 || line.is_empty() && current_chunk.len() > 300 {
                if !current_chunk.trim().is_empty() {
                    chunks.push(FileChunk {
                        index: chunks.len(),
                        content: current_chunk.clone(),
                        start_line,
                        end_line: i,
                    });
                }
                current_chunk.clear();
                start_line = i + 1;
                line_count = 0;
            }
        }

        if !current_chunk.trim().is_empty() {
            chunks.push(FileChunk {
                index: chunks.len(),
                content: current_chunk,
                start_line,
                end_line: lines.len(),
            });
        }

        chunks
    }
}

#[cfg(test)]
mod stable_path_tests {
    #[tokio::test]
    async fn test_ingest_rescan_unchanged_source_leaves_state_and_does_not_resurrect() {
        let store = std::sync::Arc::new(crate::memory::store::InMemoryMemoryStore::new());
        let indexer = AgentIndexer::new(FileIndexer::new(
            crate::memory::file_indexer::FileIndexerConfig::default(),
            None,
        ));
        let path = "agent_memory://cursor/chat-1";
        let mtime = "2026-10-13T10:00:00Z";

        // Prepopulate: 1 vigente and 2 forgotten rows
        let vigente = MemoryRecord {
            id: "vigente-1".to_string(),
            workspace_id: "test-ws".to_string(),
            path: path.to_string(),
            content: "Hello world".to_string(),
            metadata: serde_json::json!({ "source": "agent_scanner", "last_modified": mtime }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
            ..Default::default()
        };
        let forgotten1 = MemoryRecord {
            id: "forgotten-1".to_string(),
            workspace_id: "test-ws".to_string(),
            path: path.to_string(),
            content: "Old 1".to_string(),
            metadata: serde_json::json!({ "source": "agent_scanner", "last_modified": mtime, "deleted_at": "2026-10-13T11:00:00Z" }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: Some(Utc::now()),
            ..Default::default()
        };
        store.put(vigente).await.unwrap();
        store.put(forgotten1).await.unwrap();

        let indexed_file = IndexedFile {
            path: path.to_string(),
            content: "Hello world".to_string(),
            chunks: vec![],
            last_modified: mtime.to_string(),
            size: 1,
        };

        // Re-scan of unchanged source
        let res = indexer
            .ingest_file(&*store, "test-ws", &indexed_file)
            .await
            .unwrap();
        assert_eq!(res, Some("vigente-1".to_string()));

        let list = store.list("test-ws").await.unwrap();
        assert_eq!(list.len(), 2, "No new rows created on re-scan");

        let f1 = list.iter().find(|r| r.id == "forgotten-1").unwrap();
        assert!(f1.deleted_at.is_some(), "Forgotten row must stay forgotten");

        let v1 = list.iter().find(|r| r.id == "vigente-1").unwrap();
        assert!(v1.deleted_at.is_none(), "Vigente row must stay active");
    }

    #[tokio::test]
    async fn test_ingest_changed_source_updates_vigente_in_place() {
        let store = std::sync::Arc::new(crate::memory::store::InMemoryMemoryStore::new());
        let indexer = AgentIndexer::new(FileIndexer::new(
            crate::memory::file_indexer::FileIndexerConfig::default(),
            None,
        ));
        let path = "agent_memory://cursor/chat-1";
        let mtime_old = "2026-10-13T10:00:00Z";
        let mtime_new = "2026-10-13T12:00:00Z";

        let vigente = MemoryRecord {
            id: "vigente-1".to_string(),
            workspace_id: "test-ws".to_string(),
            path: path.to_string(),
            content: "Original content".to_string(),
            metadata: serde_json::json!({ "source": "agent_scanner", "last_modified": mtime_old }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
            ..Default::default()
        };
        store.put(vigente).await.unwrap();

        let indexed_file = IndexedFile {
            path: path.to_string(),
            content: "Updated content from changed file".to_string(),
            chunks: vec![],
            last_modified: mtime_new.to_string(),
            size: 2,
        };

        let res = indexer
            .ingest_file(&*store, "test-ws", &indexed_file)
            .await
            .unwrap();
        assert_eq!(
            res,
            Some("vigente-1".to_string()),
            "Must update same ID in place"
        );

        let list = store.list("test-ws").await.unwrap();
        assert_eq!(list.len(), 1, "Must not create a new row");
        assert_eq!(list[0].content, "Updated content from changed file");
        assert_eq!(
            list[0]
                .metadata
                .get("last_modified")
                .and_then(|v| v.as_str()),
            Some(mtime_new)
        );
    }

    #[tokio::test]
    async fn test_ingest_explicitly_forgotten_route_does_not_reappear() {
        let store = std::sync::Arc::new(crate::memory::store::InMemoryMemoryStore::new());
        let indexer = AgentIndexer::new(FileIndexer::new(
            crate::memory::file_indexer::FileIndexerConfig::default(),
            None,
        ));
        let path = "agent_memory://cursor/chat-1";
        let mtime = "2026-10-13T10:00:00Z";

        // The route was explicitly forgotten (deleted_at set)
        let forgotten = MemoryRecord {
            id: "forgotten-1".to_string(),
            workspace_id: "test-ws".to_string(),
            path: path.to_string(),
            content: "Forgotten content".to_string(),
            metadata: serde_json::json!({
                "source": "agent_scanner",
                "last_modified": mtime,
                "deleted_at": "2026-10-13T11:00:00Z"
            }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: Some(Utc::now()),
            ..Default::default()
        };
        store.put(forgotten).await.unwrap();

        let indexed_file = IndexedFile {
            path: path.to_string(),
            content: "Forgotten content".to_string(),
            chunks: vec![],
            last_modified: mtime.to_string(),
            size: 1,
        };

        // Re-scan of unchanged source file for explicitly forgotten session
        let res = indexer
            .ingest_file(&*store, "test-ws", &indexed_file)
            .await
            .unwrap();
        assert_eq!(res, None, "Forgotten session must not be resurrected");

        let list = store.list("test-ws").await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].deleted_at.is_some(), "Must remain forgotten");
    }
    use super::*;

    fn session(ide: &str, source: &str, updated_at: &str) -> AgentSession {
        AgentSession {
            ide: ide.to_string(),
            project_path: None,
            updated_at: updated_at.to_string(),
            messages: vec![],
            source_file: source.to_string(),
        }
    }

    #[test]
    fn two_scans_of_unchanged_source_keep_one_path() {
        let first = session(
            "cursor",
            "/home/u/.cursor/chat-1.json",
            "2026-10-13T10:00:00Z",
        );
        let second = session(
            "cursor",
            "/home/u/.cursor/chat-1.json",
            "2026-10-13T10:00:00Z",
        );
        assert_eq!(virtual_path_for(&first), virtual_path_for(&second));
    }

    #[test]
    fn changed_source_keeps_the_same_path_so_add_updates_in_place() {
        let before = session(
            "cursor",
            "/home/u/.cursor/chat-1.json",
            "2026-10-13T10:00:00Z",
        );
        let after = session(
            "cursor",
            "/home/u/.cursor/chat-1.json",
            "2026-10-13T12:30:00Z",
        );
        assert_eq!(virtual_path_for(&before), virtual_path_for(&after));
        assert_eq!(virtual_path_for(&after), "agent_memory://cursor/chat-1");
    }

    #[test]
    fn different_sources_get_different_paths() {
        let a = session(
            "cursor",
            "/home/u/.cursor/chat-1.json",
            "2026-10-13T10:00:00Z",
        );
        let b = session(
            "cursor",
            "/home/u/.cursor/chat-2.json",
            "2026-10-13T10:00:00Z",
        );
        assert_ne!(virtual_path_for(&a), virtual_path_for(&b));
    }
}
