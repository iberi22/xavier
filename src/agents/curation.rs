// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Content curation agent
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::agents::provider::ModelProviderClient;
use crate::memory::belief_graph::Belief;
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::schema::{ContextZone, MemoryLevel, TypedMemoryPayload};
use crate::memory::store::MemoryRecord;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurationResult {
    pub domain: String,
    pub topic: String,
    pub subtopic: Option<String>,
    pub importance: f32,
    pub beliefs: Vec<Belief>,
}

pub struct CurationAgent {
    client: ModelProviderClient,
    memory: Option<Arc<QmdMemory>>,
}

impl CurationAgent {
    pub fn new() -> Self {
        Self {
            client: ModelProviderClient::from_env(),
            memory: None,
        }
    }

    pub fn with_memory(mut self, memory: Arc<QmdMemory>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub async fn curate(&self, content: &str) -> Result<CurationResult> {
        info!(
            "🧠 Curating content: {}...",
            content.chars().take(50).collect::<String>()
        );

        let prompt = format!(
            "Analyze the following content and categorize it into a hierarchical structure (Domain > Topic > Fact).\n\
             Extract key facts as SPO triples (Subject-Predicate-Object).\n\
             Return ONLY a JSON object with fields:\n\
             - domain (string): High-level area (e.g., Technology, Health, Business)\n\
             - topic (string): Specific subject within the domain (e.g., Rust Programming, AI Agents)\n\
             - subtopic (optional string): Granular detail (e.g., Memory Management, RAG Pipeline)\n\
             - importance (float 0.0-1.0): How critical this information is\n\
             - beliefs (array of objects): Extract specific facts as SPO triples.\n\
               Fields for each belief: 'subject', 'predicate', 'object', and 'confidence' [High/Medium/Low].\n\n\
             Content:\n\"\"\"\n{}\n\"\"\"",
            content
        );

        // We use an empty context for raw classification
        let response = self.client.generate_response(&prompt, &[]).await?;
        let text = response.text;

        // Extract JSON from response (handling potential markdown blocks)
        let json_str = if let Some(start) = text.find('{') {
            if let Some(end) = text.rfind('}') {
                &text[start..=end]
            } else {
                &text[start..]
            }
        } else {
            &text
        };

        let result: CurationResult = serde_json::from_str(json_str)?;
        Ok(result)
    }

    /// Implement recursive summarization for memory clusters (RAPTOR style)
    pub async fn summarize_cluster(&self, cluster_id: &str) -> Result<Option<String>> {
        let Some(memory) = &self.memory else {
            return Ok(None);
        };

        let filters = crate::memory::schema::MemoryQueryFilters {
            cluster_ids: Some(vec![cluster_id.to_string()]),
            ..Default::default()
        };

        let docs = memory.search_filtered("", 100, Some(&filters)).await?;
        if docs.is_empty() {
            return Ok(None);
        }

        let combined_content = docs
            .iter()
            .map(|d| d.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");

        info!(
            "🌳 Summarizing cluster {} ({} docs)",
            cluster_id,
            docs.len()
        );

        let prompt = format!(
            "Sintetiza la siguiente colección de fragmentos de memoria en un resumen ejecutivo coherente.\n\
             Este resumen actuará como el nodo padre en una jerarquía de información.\n\n\
             Fragmentos:\n\"\"\"\n{}\n\"\"\"",
            combined_content
        );

        let response = self.client.generate_response(&prompt, &[]).await?;
        let summary = response.text;

        let parent_id = ulid::Ulid::new().to_string();
        let path = format!("clusters/{}/summary", cluster_id);

        memory
            .add_document_typed(
                path,
                summary.clone(),
                serde_json::json!({
                    "cluster_id": cluster_id,
                    "is_parent": true,
                    "child_count": docs.len()
                }),
                Some(TypedMemoryPayload {
                    level: Some(MemoryLevel::Extracted),
                    zone: Some(ContextZone::Cluster),
                    cluster_id: Some(cluster_id.to_string()),
                    ..Default::default()
                }),
            )
            .await?;

        // Assign parent_id to children
        for mut doc in docs {
            doc.parent_id = Some(parent_id.clone());
            memory.update(doc).await?;
        }

        Ok(Some(parent_id))
    }

    /// Aggregates multiple detailed (Raw) memories into a summarized "Zone" memory.
    /// The generated memory will have `MemoryLevel::Extracted`, linking back to its
    /// children via its cluster ID, enabling traversal up/down the hierarchy tree.
    pub async fn group_into_zone(
        &self,
        cluster_id: &str,
        workspace_id: &str,
        memories: &[MemoryRecord],
    ) -> Result<MemoryRecord> {
        info!(
            "🧠 Grouping {} memories into zone/cluster {}...",
            memories.len(),
            cluster_id
        );

        let contents = memories
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");

        let prompt = format!(
            "You are aggregating a cluster of related information from a knowledge graph.\n\
             Read the following memory snippets and generate a comprehensive but concise summary \n\
             that represents the overarching 'Zone' or parent concept they all belong to.\n\n\
             Memories:\n\"\"\"\n{}\n\"\"\"\n\n\
             Return ONLY the summary text.",
            contents
        );

        let response = self.client.generate_response(&prompt, &[]).await?;
        let summary = response.text;

        let now = Utc::now();
        Ok(MemoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            workspace_id: workspace_id.to_string(),
            path: format!("zone_summary/{}", cluster_id),
            content: summary.trim().to_string(),
            metadata: serde_json::json!({
                "type": "zone_summary",
                "aggregated_count": memories.len(),
            }),
            embedding: Vec::new(),
            created_at: now,
            updated_at: now,
            revision: 1,
            primary: true,
            parent_id: None,
            cluster_id: Some(cluster_id.to_string()),
            level: MemoryLevel::Extracted,
            relation: None,
            clearance: crate::memory::schema::ClearanceLevel::Unclassified,
            revisions: vec![],
            ..Default::default()
        })
    }
}

impl Default for CurationAgent {
    fn default() -> Self {
        Self::new()
    }
}
