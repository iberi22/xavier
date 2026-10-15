//! Agent memory indexer
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::Result;
use tracing::{debug, info};

use crate::memory::agent_scanner::{AgentScanner, AgentSession};
use crate::memory::file_indexer::{FileChunk, FileIndexer, IndexedFile};

/// Stable virtual path of an agent session: `agent_memory://<ide>/<source stem>`.
/// The same transcript maps to the same path. The QMD document writer treats
/// it as canonical: unchanged documents are skipped and edits reuse the record ID.
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

            // Formatting alone does not persist or deduplicate records. Callers
            // ingest this stable path through the QMD document writer.
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

#[cfg(test)]
mod scan_persistence_tests {
    use super::*;
    use crate::memory::file_indexer::FileIndexerConfig;
    use crate::memory::qmd_memory::QmdMemory;
    use crate::memory::store::{InMemoryMemoryStore, MemoryRecord, MemoryStore};
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, UNIX_EPOCH};

    const WORKSPACE: &str = "agent-scanner-regression";

    fn write_source(path: &Path, changed: bool) {
        let content = if changed {
            "updated fixture message"
        } else {
            "original fixture message"
        };
        let value = serde_json::json!({"role": "user", "content": content}).to_string();
        if path.extension().and_then(|ext| ext.to_str()) == Some("vscdb") {
            let conn = rusqlite::Connection::open(path).expect("open fixture database");
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS ItemTable (key TEXT PRIMARY KEY, value TEXT)",
            )
            .expect("create fixture table");
            conn.execute(
                "INSERT OR REPLACE INTO ItemTable VALUES ('chat.fixture', ?1)",
                [value],
            )
            .expect("write fixture session");
        } else {
            std::fs::write(path, value).expect("write fixture session");
        }
        let seconds = if changed {
            1_700_000_100
        } else {
            1_700_000_000
        };
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open source")
            .set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
            .expect("set source mtime");
    }

    fn indexer(root: &Path) -> AgentIndexer {
        AgentIndexer {
            scanner: AgentScanner::with_search_paths(vec![root.to_path_buf()]),
            _file_indexer: FileIndexer::new(
                FileIndexerConfig {
                    root_path: root.to_path_buf(),
                    ..Default::default()
                },
                None,
            ),
        }
    }

    async fn persist_scan(indexer: &AgentIndexer, memory: &QmdMemory) -> IndexedFile {
        let mut files = indexer.index_agents().await.expect("scan fixture");
        assert_eq!(files.len(), 1);
        let file = files.remove(0);
        // Match the scanner's caller metadata; supply an embedding to avoid a model/backend.
        memory.add_document_typed_with_embedding(
            file.path.clone(), file.content.clone(),
            serde_json::json!({"source": "agent_scanner", "last_modified": file.last_modified, "size": file.size}),
            None, Some(vec![]),
        ).await.expect("persist scan");
        file
    }

    async fn records(store: &InMemoryMemoryStore) -> Vec<MemoryRecord> {
        store.list(WORKSPACE).await.expect("list persisted records")
    }

    #[tokio::test]
    async fn two_scans_of_unchanged_source_keep_one_record() {
        // The library test binary already invokes isolate_test_process!(). Explicit
        // search roots also keep these fixtures independent of HOME and other tests.
        for extension in ["json", "vscdb"] {
            let temp = tempfile::tempdir().expect("temporary scan root");
            let source = temp.path().join(format!("session.{extension}"));
            write_source(&source, false);
            let indexer = indexer(temp.path());
            let store = Arc::new(InMemoryMemoryStore::new());
            let memory = QmdMemory::new_lazy(WORKSPACE);
            memory.set_store(store.clone()).await;
            let first = persist_scan(&indexer, &memory).await;
            let before = records(&store).await.remove(0);
            persist_scan(&indexer, &memory).await;
            assert_eq!(records(&store).await[0].updated_at, before.updated_at);
            // Exercise store lookup with an empty cache, as after a restart.
            let reloaded = QmdMemory::new_lazy(WORKSPACE);
            reloaded.set_store(store.clone()).await;
            let second = persist_scan(&indexer, &reloaded).await;
            assert_eq!(first.last_modified, "2023-11-14T22:13:20+00:00");
            assert_eq!(first.content, second.content);
            assert_eq!(first.last_modified, second.last_modified);
            let after = records(&store).await;
            assert_eq!(after.len(), 1);
            assert_eq!(after[0].id, before.id);
            assert_eq!(after[0].content, before.content);
            assert_eq!(
                after[0].updated_at, before.updated_at,
                "identical scans must skip writing"
            );
            assert_eq!(reloaded.all_documents().await.len(), 1);
        }
    }

    #[tokio::test]
    async fn changed_source_updates_in_place() {
        for extension in ["json", "vscdb"] {
            let temp = tempfile::tempdir().expect("temporary scan root");
            let source = temp.path().join(format!("session.{extension}"));
            write_source(&source, false);
            let indexer = indexer(temp.path());
            let store = Arc::new(InMemoryMemoryStore::new());
            let memory = QmdMemory::new_lazy(WORKSPACE);
            memory.set_store(store.clone()).await;
            let first = persist_scan(&indexer, &memory).await;
            let before = records(&store).await.remove(0);
            write_source(&source, true);
            let reloaded = QmdMemory::new_lazy(WORKSPACE);
            reloaded.set_store(store.clone()).await;
            let second = persist_scan(&indexer, &reloaded).await;
            let after = records(&store).await;
            assert_eq!(after.len(), 1);
            assert_eq!(after[0].id, before.id);
            assert_eq!(first.path, second.path);
            assert_ne!(first.content, second.content);
            assert_eq!(second.last_modified, "2023-11-14T22:15:00+00:00");
            assert_eq!(after[0].content, second.content);
            assert_eq!(after[0].metadata["last_modified"], second.last_modified);
            assert_eq!(reloaded.all_documents().await.len(), 1);
        }
    }

    #[tokio::test]
    async fn scanning_preserves_existing_duplicate_records() {
        let temp = tempfile::tempdir().expect("temporary scan root");
        let source = temp.path().join("session.json");
        write_source(&source, false);
        let indexer = indexer(temp.path());
        let store = Arc::new(InMemoryMemoryStore::new());
        let memory = QmdMemory::new_lazy(WORKSPACE);
        memory.set_store(store.clone()).await;
        persist_scan(&indexer, &memory).await;
        let mut legacy = records(&store).await.remove(0);
        legacy.id = "legacy-duplicate".to_string();
        legacy.content = "Historical duplicate retained for a separate cleanup".to_string();
        store
            .put(legacy.clone())
            .await
            .expect("seed legacy duplicate");
        write_source(&source, true);
        persist_scan(&indexer, &memory).await;
        let after = records(&store).await;
        assert_eq!(after.len(), 2);
        let retained = after
            .iter()
            .find(|record| record.id == legacy.id)
            .expect("legacy duplicate remains present");
        assert_eq!(retained.content, legacy.content);
        assert_eq!(retained.metadata, legacy.metadata);
        assert_eq!(retained.deleted_at, legacy.deleted_at);
    }
}
