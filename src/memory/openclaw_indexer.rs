//! OpenClaw Agent Indexer
//!
//! Indexes OpenClaw agent sessions into the Xavier memory store.

use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tokio::fs;
use sha2::{Sha256, Digest};

use crate::memory::openclaw_scanner::{OpenClawAgentScanner, AgentScanResult};
use crate::memory::store::{MemoryStore, MemoryRecord};
use crate::memory::schema::MemoryLevel;
use crate::embedding::Embedder;

pub struct OpenClawAgentIndexer {
    scanner: OpenClawAgentScanner,
    store: Arc<dyn MemoryStore>,
    embedder: Option<Arc<dyn Embedder>>,
}

pub struct IndexReport {
    pub total_files: usize,
    pub total_chunks: usize,
    pub records_created: usize,
}

impl OpenClawAgentIndexer {
    pub fn new(scanner: OpenClawAgentScanner, store: Arc<dyn MemoryStore>) -> Self {
        Self { scanner, store, embedder: None }
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    pub async fn index_all(&self) -> Result<IndexReport> {
        let agents = self.scanner.scan_all_agents().await?;
        let mut report = IndexReport {
            total_files: 0,
            total_chunks: 0,
            records_created: 0,
        };

        for agent in agents {
            let agent_report = self.index_agent_result(agent).await?;
            report.total_files += agent_report.total_files;
            report.total_chunks += agent_report.total_chunks;
            report.records_created += agent_report.records_created;
        }

        Ok(report)
    }

    pub async fn index_agent(&self, agent_id: &str) -> Result<IndexReport> {
        let agent = self.scanner.scan_agent(agent_id).await?;
        self.index_agent_result(agent).await
    }

    async fn index_agent_result(&self, agent: AgentScanResult) -> Result<IndexReport> {
        let mut report = IndexReport {
            total_files: agent.files.len(),
            total_chunks: 0,
            records_created: 0,
        };

        let workspace_id = format!("agent:{}", agent.agent_id);

        for file in agent.files {
            let content = match fs::read_to_string(&file.path).await {
                Ok(c) => c,
                Err(_) => continue, // Skip binary or unreadable files
            };

            if content.trim().is_empty() {
                continue;
            }

            // Simple chunking for now (one chunk per file if small, or split by lines)
            // In a real scenario, we'd use a more sophisticated chunker.
            let chunks = self.chunk_content(&content);
            report.total_chunks += chunks.len();

            for (i, chunk) in chunks.into_iter().enumerate() {
                let mut hasher = Sha256::new();
                hasher.update(workspace_id.as_bytes());
                hasher.update(file.path.to_string_lossy().as_bytes());
                hasher.update(i.to_string().as_bytes());
                let record_id = format!("{:x}", hasher.finalize());

                let embedding = if let Some(ref embedder) = self.embedder {
                    match embedder.encode(&chunk).await {
                        Ok(e) => e,
                        Err(_) => vec![],
                    }
                } else {
                    vec![]
                };

                let record = MemoryRecord {
                    id: record_id,
                    workspace_id: workspace_id.clone(),
                    path: format!("{}/{}", file.path.display(), i),
                    content: chunk,
                    metadata: serde_json::json!({
                        "agent_id": agent.agent_id,
                        "source_file": file.path.to_string_lossy(),
                        "chunk_index": i,
                        "last_modified": file.last_modified.to_rfc3339(),
                    }),
                    embedding,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    revision: 1,
                    primary: true,
                    parent_id: None,
                    cluster_id: None,
                    level: MemoryLevel::Raw,
                    relation: None,
                    clearance: Default::default(),
                    revisions: vec![],
                    encrypted_dek: None,
                    content_iv: None,
                    metadata_iv: None,
                };

                self.store.put(record).await?;
                report.records_created += 1;
            }
        }

        Ok(report)
    }

    fn chunk_content(&self, content: &str) -> Vec<String> {
        // Very basic line-based chunking
        let mut chunks = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        let chunk_size = 50; // lines

        for chunk_lines in lines.chunks(chunk_size) {
            let chunk = chunk_lines.join("\n");
            if !chunk.trim().is_empty() {
                chunks.push(chunk);
            }
        }

        if chunks.is_empty() && !content.trim().is_empty() {
            chunks.push(content.to_string());
        }

        chunks
    }
}
