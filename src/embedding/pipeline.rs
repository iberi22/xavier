use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::agents::registry::AgentRegistry;
use crate::domain::cycle_breaks::w30_12::MemoryConsumerStore;
use crate::embedding::Embedder;
use crate::settings::XavierSettings;
use xavier_core_logic::ClearanceLevel;

/// Local embeddings pipeline for authorized memories.
pub struct LocalEmbeddingPipeline {
    embedder: Arc<dyn Embedder>,
    store: Arc<dyn MemoryConsumerStore>,
    max_clearance: ClearanceLevel,
    consent_given: bool,
    registry: Option<AgentRegistry>,
}

impl LocalEmbeddingPipeline {
    /// New.
    pub fn new<S: MemoryConsumerStore + ?Sized + 'static>(
        embedder: Arc<dyn Embedder>,
        store: Arc<S>,
        max_clearance: ClearanceLevel,
    ) -> Self {
        let consent_given = XavierSettings::current().data_commons.consent_given;
        Self::with_consent(embedder, store, max_clearance, consent_given)
    }

    /// With consent.
    pub fn with_consent<S: MemoryConsumerStore + ?Sized + 'static>(
        embedder: Arc<dyn Embedder>,
        store: Arc<S>,
        max_clearance: ClearanceLevel,
        consent_given: bool,
    ) -> Self {
        let registry = AgentRegistry::resolve_and_load().ok();
        Self {
            embedder,
            store: store.into_consumer_store(),
            max_clearance,
            consent_given,
            registry,
        }
    }

    /// With agent registry explicitly attached.
    pub fn with_registry(mut self, registry: AgentRegistry) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Returns reference to the attached agent registry, if loaded.
    pub fn registry(&self) -> Option<&AgentRegistry> {
        self.registry.as_ref()
    }

    /// From env.
    pub fn from_env<S: MemoryConsumerStore + ?Sized + 'static>(
        embedder: Arc<dyn Embedder>,
        store: Arc<S>,
    ) -> Self {
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

        let records = self.store.list_records(workspace_id).await?;
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
                    self.store.update_record(updated_record).await?;
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
