use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::embedding::Embedder;
use crate::memory::schema::ClearanceLevel;
use crate::memory::store::MemoryStore;
use crate::settings::XavierSettings;

/// Local embeddings pipeline for authorized memories.
pub struct LocalEmbeddingPipeline {
    embedder: Arc<dyn Embedder>,
    store: Arc<dyn MemoryStore>,
    max_clearance: ClearanceLevel,
    consent_given: bool,
}

impl LocalEmbeddingPipeline {
    pub fn new(
        embedder: Arc<dyn Embedder>,
        store: Arc<dyn MemoryStore>,
        max_clearance: ClearanceLevel,
    ) -> Self {
        let consent_given = XavierSettings::current().data_commons.consent_given;
        Self::with_consent(embedder, store, max_clearance, consent_given)
    }

    pub fn with_consent(
        embedder: Arc<dyn Embedder>,
        store: Arc<dyn MemoryStore>,
        max_clearance: ClearanceLevel,
        consent_given: bool,
    ) -> Self {
        Self {
            embedder,
            store,
            max_clearance,
            consent_given,
        }
    }

    pub fn from_env(embedder: Arc<dyn Embedder>, store: Arc<dyn MemoryStore>) -> Self {
        // Default to Secret for local embeddings, can be more restrictive
        let max_clearance = ClearanceLevel::Secret;

        Self::new(embedder, store, max_clearance)
    }

    /// Process all memories in a workspace and generate missing embeddings for authorized ones.
    pub async fn process_workspace(&self, workspace_id: &str) -> Result<usize> {
        if !self.consent_given {
            warn!(workspace_id = %workspace_id, "Skipping embedding pipeline: user consent not given");
            return Ok(0);
        }

        debug!(workspace_id = %workspace_id, "Starting local embedding pipeline");

        let records = self.store.list(workspace_id).await?;
        let mut processed_count = 0;

        for record in records {
            if !record.is_authorized_for_embedding(self.consent_given, self.max_clearance) {
                debug!(id = %record.id, "Skipping record: not authorized for embedding");
                continue;
            }

            if !record.embedding.is_empty() {
                continue;
            }

            match self.embedder.encode(&record.content).await {
                Ok(vector) => {
                    let mut updated_record = record.clone();
                    updated_record.embedding = vector;
                    self.store.update(updated_record).await?;
                    processed_count += 1;
                }
                Err(e) => {
                    warn!(id = %record.id, error = %e, "Failed to generate embedding for record");
                }
            }
        }

        info!(workspace_id = %workspace_id, processed = processed_count, "Local embedding pipeline completed");
        Ok(processed_count)
    }
}
