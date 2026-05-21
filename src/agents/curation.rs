use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::agents::provider::ModelProviderClient;
use crate::memory::belief_graph::Belief;
use crate::memory::schema::MemoryLevel;
use crate::memory::store::MemoryRecord;

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
}

impl CurationAgent {
    pub fn new() -> Self {
        Self {
            client: ModelProviderClient::from_env(),
        }
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

        // Extract JSON from response (handling potential markdown blocks)
        let json_str = if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                &response[start..=end]
            } else {
                &response[start..]
            }
        } else {
            &response
        };

        let result: CurationResult = serde_json::from_str(json_str)?;
        Ok(result)
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
        info!("🧠 Grouping {} memories into zone/cluster {}...", memories.len(), cluster_id);

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

        let summary = self.client.generate_response(&prompt, &[]).await?;

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
            embedding: Vec::new(), // To be embedded later by the indexing pipeline
            created_at: now,
            updated_at: now,
            revision: 1,
            primary: true,
            parent_id: None, // This is the parent for the cluster
            cluster_id: Some(cluster_id.to_string()),
            level: MemoryLevel::Extracted,
            relation: None,
            revisions: Vec::new(),
        })
    }
}

impl Default for CurationAgent {
    fn default() -> Self {
        Self::new()
    }
}
