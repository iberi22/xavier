//! OpenClaw Agent Memory Indexer
//!
//! Concurrently chunks, embeds, and stores agent memories from OpenClaw into the MemoryStore.

use std::sync::Arc;
use anyhow::Result;
use serde_json::json;
use tracing::{info, warn};
use crate::memory::openclaw_scanner::{AgentMemory, OpenClawAgentScanner};
use crate::memory::store::{MemoryRecord, MemoryStore, stable_key};
use crate::embedding::Embedder;

pub struct OpenClawAgentIndexer {
    embedder: Arc<dyn Embedder>,
}

impl OpenClawAgentIndexer {
    pub fn new(embedder: Arc<dyn Embedder>) -> Self {
        Self { embedder }
    }

    /// Indexa un agente completo a MemoryStore
    pub async fn index_agent(
        &self,
        memory: &AgentMemory,
        store: &dyn MemoryStore,
    ) -> Result<Vec<MemoryRecord>> {
        info!("Indexing agent: {}", memory.agent_id);
        let mut records = Vec::new();

        // 1. Process MEMORY.md
        let memory_chunks = self.chunk_memory_md(&memory.agent_id, &memory.memory_md);
        records.extend(memory_chunks);

        // 2. Process daily logs
        for log in &memory.daily_logs {
            let log_chunks = self.chunk_daily_log(&memory.agent_id, log);
            records.extend(log_chunks);
        }

        // 3. Process optional files
        if let Some(content) = &memory.soul_md {
            records.push(self.create_single_chunk_record(&memory.agent_id, "SOUL.md", "SOUL", content));
        }
        if let Some(content) = &memory.user_md {
            records.push(self.create_single_chunk_record(&memory.agent_id, "USER.md", "USER", content));
        }
        if let Some(content) = &memory.tools_md {
            records.push(self.create_single_chunk_record(&memory.agent_id, "TOOLS.md", "TOOLS", content));
        }

        let mut final_records = Vec::new();

        for mut record in records {
            // Extraction of tags
            record.metadata["tags"] = json!(self.extract_tags(&record.content));

            // Embeddings - using encode as per the Embedder trait in crate::embedding
            match self.embedder.encode(&record.content).await {
                Ok(embedding) => {
                    record.embedding = embedding;
                }
                Err(e) => {
                    warn!("Failed to generate embedding for {}: {}. Continuing...", record.path, e);
                }
            }

            // Ensure stable ID for upsert behavior
            record.id = stable_key("memory", &[&record.workspace_id, &record.path]);

            // Store record
            store.put(record.clone()).await?;
            final_records.push(record);
        }

        Ok(final_records)
    }

    /// Indexa todos los agentes del scanner
    pub async fn index_all_agents(
        &self,
        scanner: &OpenClawAgentScanner,
        store: &dyn MemoryStore,
    ) -> Result<Vec<MemoryRecord>> {
        let agents = scanner.scan_all_agents().await?;
        let mut all_records = Vec::new();

        for agent in agents {
            match self.index_agent(&agent, store).await {
                Ok(records) => all_records.extend(records),
                Err(e) => warn!("Failed to index agent {}: {}", agent.agent_id, e),
            }
        }

        Ok(all_records)
    }

    fn chunk_memory_md(&self, agent_id: &str, content: &str) -> Vec<MemoryRecord> {
        let mut records = Vec::new();
        let mut current_section_title = "Introduction".to_string();
        let mut current_section_content = String::new();
        let mut sections = Vec::new();

        for line in content.lines() {
            if line.starts_with("## ") {
                if !current_section_content.trim().is_empty() {
                    sections.push((current_section_title.clone(), current_section_content.clone()));
                }
                current_section_title = line["## ".len()..].trim().to_string();
                current_section_content = line.to_string();
                current_section_content.push('\n');
            } else {
                current_section_content.push_str(line);
                current_section_content.push('\n');
            }
        }
        if !current_section_content.trim().is_empty() {
            sections.push((current_section_title, current_section_content));
        }

        let total_chunks = sections.len();
        for (i, (title, content)) in sections.into_iter().enumerate() {
            let mut record = MemoryRecord::default();
            record.workspace_id = format!("agent:{}", agent_id);

            let title_slug = title.to_lowercase()
                .replace(' ', "-")
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-')
                .collect::<String>();

            record.path = format!("{}/MEMORY.md#{}", agent_id, title_slug);
            record.content = content;
            record.metadata = json!({
                "agent_id": agent_id,
                "file_type": "MEMORY",
                "chunk_index": i,
                "total_chunks": total_chunks,
                "section_title": title
            });
            records.push(record);
        }

        records
    }

    fn chunk_daily_log(&self, agent_id: &str, log: &crate::memory::openclaw_scanner::DailyLog) -> Vec<MemoryRecord> {
        let mut records = Vec::new();

        // 1 chunk per file if < 5KB
        if log.content.len() < 5120 {
            let mut record = MemoryRecord::default();
            record.workspace_id = format!("agent:{}", agent_id);
            record.path = format!("{}/logs/{}", agent_id, log.date);
            record.content = log.content.clone();
            record.metadata = json!({
                "agent_id": agent_id,
                "file_type": "daily_log",
                "date": log.date
            });
            records.push(record);
        } else {
            // chunk por sección si es grande
            let mut current_section_content = String::new();
            let mut sections = Vec::new();

            for line in log.content.lines() {
                if line.starts_with("## ") {
                    if !current_section_content.trim().is_empty() {
                        sections.push(current_section_content.clone());
                    }
                    current_section_content = line.to_string();
                    current_section_content.push('\n');
                } else {
                    current_section_content.push_str(line);
                    current_section_content.push('\n');
                }
            }
            if !current_section_content.trim().is_empty() {
                sections.push(current_section_content);
            }

            for (i, content) in sections.into_iter().enumerate() {
                let mut record = MemoryRecord::default();
                record.workspace_id = format!("agent:{}", agent_id);
                record.path = format!("{}/logs/{}#section-{}", agent_id, log.date, i);
                record.content = content;
                record.metadata = json!({
                    "agent_id": agent_id,
                    "file_type": "daily_log",
                    "date": log.date,
                    "chunk_index": i
                });
                records.push(record);
            }
        }

        records
    }

    fn create_single_chunk_record(&self, agent_id: &str, file_name: &str, file_type: &str, content: &str) -> MemoryRecord {
        let mut record = MemoryRecord::default();
        record.workspace_id = format!("agent:{}", agent_id);
        record.path = format!("{}/{}", agent_id, file_name);
        record.content = content.to_string();
        record.metadata = json!({
            "agent_id": agent_id,
            "file_type": file_type
        });
        record
    }

    fn extract_tags(&self, content: &str) -> Vec<String> {
        let mut tags = Vec::new();

        // URLs
        let url_regex = regex::Regex::new(r"https?://[^\s/$.?#].[^\s]*").unwrap();
        for mat in url_regex.find_iter(content) {
            tags.push(mat.as_str().to_string());
        }

        // Hashtags
        let hashtag_regex = regex::Regex::new(r"#[a-zA-Z0-9_]+").unwrap();
        for mat in hashtag_regex.find_iter(content) {
            tags.push(mat.as_str().to_string());
        }

        // Palabras que empiezan con mayúscula repetidas
        let mut counts = std::collections::HashMap::new();
        let word_regex = regex::Regex::new(r"\b[A-Z][a-zA-Z]{2,}\b").unwrap();
        for mat in word_regex.find_iter(content) {
            *counts.entry(mat.as_str()).or_insert(0) += 1;
        }
        for (word, count) in counts {
            if count > 1 {
                tags.push(word.to_string());
            }
        }

        tags.sort();
        tags.dedup();
        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::openclaw_scanner::DailyLog;
    use crate::memory::store::InMemoryMemoryStore;
    use crate::embedding::NoopEmbedder;

    #[test]
    fn test_chunk_memory_md() {
        let indexer = OpenClawAgentIndexer::new(Arc::new(NoopEmbedder));
        let content = "Initial info\n## Section 1\nContent of section 1\n## Section 2\nContent of section 2";
        let records = indexer.chunk_memory_md("test_agent", content);

        assert_eq!(records.len(), 3);
        assert_eq!(records[0].metadata["section_title"], "Introduction");
        assert_eq!(records[1].metadata["section_title"], "Section 1");
        assert_eq!(records[2].metadata["section_title"], "Section 2");
        assert!(records[1].path.contains("section-1"));
    }

    #[test]
    fn test_chunk_daily_log_small() {
        let indexer = OpenClawAgentIndexer::new(Arc::new(NoopEmbedder));
        let log = DailyLog {
            date: "2024-01-01".to_string(),
            content: "Small content".to_string(),
        };
        let records = indexer.chunk_daily_log("test_agent", &log);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, "test_agent/logs/2024-01-01");
    }

    #[test]
    fn test_chunk_daily_log_large() {
        let indexer = OpenClawAgentIndexer::new(Arc::new(NoopEmbedder));
        let mut large_content = String::new();
        for _ in 0..600 {
            large_content.push_str("Some repeated text to make it large enough. ");
        }
        large_content.push_str("\n## Section 1\nLarge section 1\n## Section 2\nLarge section 2");

        let log = DailyLog {
            date: "2024-01-01".to_string(),
            content: large_content,
        };
        let records = indexer.chunk_daily_log("test_agent", &log);

        assert!(records.len() >= 3);
    }

    #[test]
    fn test_extract_tags() {
        let indexer = OpenClawAgentIndexer::new(Arc::new(NoopEmbedder));
        let content = "Check out https://xavier.swal.dev and #ai. Also, Xavier is great, Xavier is memory.";
        let tags = indexer.extract_tags(content);

        assert!(tags.contains(&"https://xavier.swal.dev".to_string()));
        assert!(tags.contains(&"#ai".to_string()));
        assert!(tags.contains(&"Xavier".to_string()));
    }

    #[tokio::test]
    async fn test_index_agent() {
        let indexer = OpenClawAgentIndexer::new(Arc::new(NoopEmbedder));
        let store = InMemoryMemoryStore::new();
        let memory = AgentMemory {
            agent_id: "lasantacruz".to_string(),
            memory_md: "## Memory\nSome memory content".to_string(),
            soul_md: Some("Soul content".to_string()),
            user_md: None,
            tools_md: None,
            daily_logs: vec![DailyLog {
                date: "2024-01-01".to_string(),
                content: "Daily log content".to_string(),
            }],
        };

        let records = indexer.index_agent(&memory, &store).await.unwrap();
        assert_eq!(records.len(), 3); // 1 MEMORY, 1 daily log, 1 SOUL

        let stored_records = store.list("agent:lasantacruz").await.unwrap();
        assert_eq!(stored_records.len(), 3);
    }
}
