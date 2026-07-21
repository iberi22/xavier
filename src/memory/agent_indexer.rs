//! Agent memory indexer
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::Result;
use tracing::{debug, info};

use crate::memory::agent_scanner::{AgentScanner, AgentSession};
use crate::memory::file_indexer::{FileChunk, FileIndexer, IndexedFile};

/// AgentIndexer se encarga de formatear e indexar las sesiones de agentes (Cursor, Windsurf, etc.)
#[derive(Clone)]
pub struct AgentIndexer {
    scanner: AgentScanner,
    // Podríamos usar el FileIndexer subyacente para reusar la logica de chunking
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

            let virtual_path = format!("agent_memory://{}/{}", session.ide, uuid::Uuid::new_v4());

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
