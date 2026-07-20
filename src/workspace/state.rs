//! Workspace state management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{broadcast, Mutex, RwLock};

use super::config::WorkspaceConfig;
use super::ops::*;
use super::usage::{
    EmbeddingProviderSnapshot, OptimizationMetrics, OptimizationUsageSnapshot, SyncPolicySnapshot,
    UsageCountersSnapshot, UsageEvent, UsageMetrics, WorkspaceLimitsSnapshot,
    WorkspaceUsageSnapshot,
};
use crate::agents::{router::RouteCategory, AgentRuntime, RuntimeConfig};
use crate::checkpoint::CheckpointManager;
use crate::codebase::conversations_db::ConversationsDb;
use crate::memory::{
    belief_graph::{BeliefGraph, SharedBeliefGraph},
    entity_graph::{EntityGraph, SharedEntityGraph},
    postgres_store::PostgresMemoryStore,
    qmd_memory::{estimate_document_bytes, MemoryUsage, QmdMemory},
    schema::MemoryQueryFilters,
    semantic::SemanticMemory,
    sqlite_store::SqliteMemoryStore,
    sqlite_vec_store::VecSqliteMemoryStore,
    store::{MemoryBackend, MemoryRecord, MemoryStore, SessionTokenRecord},
    supabase_store::SupabaseMemoryStore,
    working::{MemoryItem, WorkingMemory, WorkingMemoryConfig},
};
use crate::retrieval::LayerWeights;
use crate::settings::XavierSettings;
use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToken {
    pub token: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

async fn detect_cloud_backend() -> MemoryBackend {
    let settings = XavierSettings::current();

    // 1. Postgres / Neon detection (connection test)
    let pg_url = std::env::var("XAVIER_POSTGRES_URL")
        .ok()
        .or_else(|| settings.memory.postgres_url.clone());

    if let Some(url) = pg_url {
        tracing::debug!("testing postgres connection for auto-backend...");
        let options = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(2));

        match options.connect(&url).await {
            Ok(_) => {
                tracing::info!("auto-detected postgres backend");
                return MemoryBackend::Postgres;
            }
            Err(e) => {
                tracing::debug!("postgres auto-detection failed: {}", e);
            }
        }
    }

    // 2. Supabase detection (env vars check)
    let supabase_url = std::env::var("XAVIER_SUPABASE_URL")
        .ok()
        .or_else(|| settings.memory.supabase_url.clone());
    let supabase_key = std::env::var("XAVIER_SUPABASE_KEY")
        .ok()
        .or_else(|| settings.memory.supabase_key.clone());

    if supabase_url.is_some() && supabase_key.is_some() {
        tracing::info!("auto-detected supabase backend");
        return MemoryBackend::Supabase;
    }

    // 3. Fallback to SQLite (vec)
    tracing::info!("falling back to local sqlite (vec) backend");
    MemoryBackend::Vec
}

pub struct WorkspaceState {
    pub(super) config: WorkspaceConfig,
    pub memory: Arc<QmdMemory>,
    pub runtime: Arc<AgentRuntime>,
    pub belief_graph: SharedBeliefGraph,
    pub entity_graph: SharedEntityGraph,
    pub semantic_memory: Arc<SemanticMemory>,
    pub memory_manager: Arc<crate::memory::manager::MemoryManager>,
    pub checkpoint_manager: Arc<CheckpointManager>,
    pub conversations_db: Arc<ConversationsDb>,
    pub(super) store: Arc<dyn MemoryStore>,
    pub(super) store_migrated_from_file: bool,
    pub(super) store_migration_detail: String,
    pub usage_state_path: PathBuf,
    pub(super) persist_lock: Mutex<()>,
    pub(super) requests_used: AtomicUsize,
    pub(super) usage_metrics: UsageMetrics,
    pub(super) optimization_metrics: OptimizationMetrics,
    pub hormer: Arc<crate::agents::hormer::Hormer>,
    pub zone_booster: Arc<crate::retrieval::gating::AdaptiveZoneBooster>,
    pub working_memory: Arc<RwLock<WorkingMemory>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedUsageState {
    pub requests_used: usize,
    pub total_units: u64,
    pub counters: Vec<UsageCountersSnapshot>,
    #[serde(default)]
    pub optimization: OptimizationUsageSnapshot,
}

impl WorkspaceState {
    pub async fn new(
        mut config: WorkspaceConfig,
        runtime_config: RuntimeConfig,
        workspace_root: impl Into<PathBuf>,
    ) -> Result<Self> {
        let workspace_root = workspace_root.into();
        fs::create_dir_all(&workspace_root).await?;
        let usage_state_path = workspace_root.join("usage.json");
        let file_store_path = resolve_file_store_path(&workspace_root);
        let migration_marker_path = durable_migration_marker_path(&file_store_path);

        if config.memory_backend == MemoryBackend::Auto {
            config.memory_backend = detect_cloud_backend().await;
        }

        let (store, store_migrated_from_file, store_migration_detail): (
            Arc<dyn MemoryStore>,
            bool,
            String,
        ) = match config.memory_backend {
            MemoryBackend::Auto => unreachable!("auto backend should have been resolved"),
            MemoryBackend::File => (
                Arc::new(FileMemoryStore::new(file_store_path.clone()).await?),
                false,
                format!("file backend using {}", file_store_path.display()),
            ),
            MemoryBackend::Memory => (
                Arc::new(InMemoryMemoryStore::new()),
                false,
                "ephemeral in-memory backend".to_string(),
            ),
            MemoryBackend::Sqlite => {
                let store: Arc<dyn MemoryStore> = Arc::new(SqliteMemoryStore::from_env().await?);
                let migration = migrate_file_store_if_needed(
                    &config.id,
                    &file_store_path,
                    &migration_marker_path,
                    Arc::clone(&store),
                )
                .await?;
                (store, migration.migrated, migration.detail)
            }
            MemoryBackend::Vec => {
                let store: Arc<dyn MemoryStore> = Arc::new(VecSqliteMemoryStore::from_env().await?);
                let migration = migrate_file_store_if_needed(
                    &config.id,
                    &file_store_path,
                    &migration_marker_path,
                    Arc::clone(&store),
                )
                .await?;
                (store, migration.migrated, migration.detail)
            }
            MemoryBackend::Postgres => {
                let store: Arc<dyn MemoryStore> = Arc::new(PostgresMemoryStore::from_env().await?);
                (store, false, "postgres backend".to_string())
            }
            MemoryBackend::Supabase => {
                let store: Arc<dyn MemoryStore> = Arc::new(SupabaseMemoryStore::from_env().await?);
                (store, false, "supabase backend".to_string())
            }
        };
        let durable_state = store.load_workspace_state(&config.id).await?;
        let docs = Arc::new(RwLock::new(
            durable_state
                .memories
                .iter()
                .map(MemoryRecord::to_document)
                .collect(),
        ));
        let mut memory = QmdMemory::new_with_workspace(Arc::clone(&docs), config.id.clone());
        memory.set_store(Arc::clone(&store)).await;
        memory.init().await?;

        let working_memory = Arc::new(RwLock::new(WorkingMemory::with_config(
            WorkingMemoryConfig::from_env(),
        )));
        {
            let wm = Arc::clone(&working_memory);
            let docs_clone = Arc::clone(&docs);
            tokio::spawn(async move {
                let docs_guard = docs_clone.read().await;
                let mut wm_guard = wm.write().await;
                // Just take the last capacity items as initial working set
                let capacity = wm_guard.capacity();
                let start = docs_guard.len().saturating_sub(capacity);
                for doc in &docs_guard[start..] {
                    let item =
                        MemoryItem::new(doc.id.as_deref().unwrap_or(&doc.path), &doc.content)
                            .with_metadata(doc.metadata.clone());
                    wm_guard.push(item);
                }
                tracing::info!("working memory initialized with {} items", wm_guard.len());
            });
        }

        let belief_graph = Arc::new(RwLock::new(BeliefGraph::new()));
        belief_graph
            .read()
            .await
            .replace_relations(durable_state.beliefs.clone());
        memory.set_belief_graph(Arc::clone(&belief_graph));
        let memory = Arc::new(memory);
        let entity_graph = Arc::new(EntityGraph::new());
        {
            let eg = Arc::clone(&entity_graph);
            let store_clone = Arc::clone(&store);
            let workspace_id = config.id.clone();
            let docs = memory.all_documents().await;
            tokio::spawn(async move {
                let mut restored = false;
                match store_clone.load_entity_graph_snapshot(&workspace_id).await {
                    Ok(Some(snapshot)) => match eg.import_json(&snapshot).await {
                        Ok(()) => {
                            tracing::info!(
                                "entity graph restored from snapshot for workspace {}",
                                workspace_id
                            );
                            restored = true;
                        }
                        Err(error) => {
                            tracing::warn!(%error, "failed to import entity graph snapshot; falling back to full reindex");
                        }
                    },
                    Ok(None) => {
                        tracing::debug!("no entity graph snapshot found; starting full reindex");
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to load entity graph snapshot; falling back to full reindex");
                    }
                }

                if !restored {
                    for document in &docs {
                        let memory_id = document
                            .id
                            .as_deref()
                            .unwrap_or(document.path.as_str())
                            .to_string();
                        if let Err(error) = eg
                            .upsert_memory(&memory_id, &document.content, Some(&document.metadata))
                            .await
                        {
                            tracing::warn!(%error, memory_id = %memory_id, "failed to index entity graph from existing memory");
                        }
                    }
                    tracing::info!(
                        "entity graph indexing complete for {} documents",
                        docs.len()
                    );
                    // Save initial snapshot after reindexing so it doesn't have to reindex next time!
                    if let Ok(json) = eg.export_json().await {
                        if let Err(error) = store_clone
                            .save_entity_graph_snapshot(&workspace_id, &json)
                            .await
                        {
                            tracing::warn!(%error, "failed to save initial entity graph snapshot after reindexing");
                        }
                    }
                }
            });
        }
        let semantic_memory = Arc::new(SemanticMemory::new());
        let memory_manager = Arc::new(crate::memory::manager::MemoryManager::new(
            Arc::clone(&memory),
            Some(Arc::clone(&belief_graph)),
        ));
        let checkpoint_manager = Arc::new(CheckpointManager::with_store(
            config.id.clone(),
            Arc::clone(&store),
        ));
        #[cfg(test)]
        let conversations_db = Arc::new(ConversationsDb::open_in_memory(&config.id).await?);
        #[cfg(not(test))]
        let conversations_db = Arc::new(ConversationsDb::open(&config.id).await?);
        conversations_db.create_schema().await?;

        let settings = XavierSettings::current();
        let navigation_policy = Arc::new(RwLock::new(crate::retrieval::NavigationPolicy::new(
            LayerWeights::new(
                settings.retrieval.learned_policy.working_weight,
                settings.retrieval.learned_policy.episodic_weight,
                settings.retrieval.learned_policy.semantic_weight,
            ),
            crate::retrieval::policy::TraversalWeights {
                semantic_similarity: settings.retrieval.learned_policy.semantic_similarity_weight,
                confidence: settings.retrieval.learned_policy.confidence_weight,
                edge_weight: settings.retrieval.learned_policy.edge_weight,
                recency: settings.retrieval.learned_policy.recency_weight,
                cross_layer: settings.retrieval.learned_policy.cross_layer_weight,
                cross_dir: settings.retrieval.learned_policy.cross_dir_weight,
                peripheral_hub: settings.retrieval.learned_policy.peripheral_hub_weight,
            },
            settings.retrieval.learned_policy.learning_rate,
        )));

        let zone_booster = Arc::new(crate::retrieval::gating::AdaptiveZoneBooster::new());
        let hormer = Arc::new(
            crate::agents::hormer::Hormer::new(Arc::clone(&navigation_policy))
                .with_booster(Arc::clone(&zone_booster)),
        );

        let state = Self {
            runtime: Arc::new(
                AgentRuntime::new(
                    Arc::clone(&memory),
                    Some(Arc::clone(&belief_graph)),
                    runtime_config,
                )?
                .with_checkpoint_manager(Arc::clone(&checkpoint_manager)),
            ),
            belief_graph,
            entity_graph,
            semantic_memory,
            memory_manager,
            checkpoint_manager,
            conversations_db,
            store,
            store_migrated_from_file,
            store_migration_detail,
            usage_state_path,
            persist_lock: Mutex::new(()),
            config,
            memory,
            requests_used: AtomicUsize::new(0),
            usage_metrics: UsageMetrics::new(),
            optimization_metrics: OptimizationMetrics::new(),
            hormer,
            zone_booster,
            working_memory,
        };

        crate::scheduler::daemon::MemoryDaemon::new(Arc::clone(&state.memory_manager)).spawn();

        state.load_usage_state().await?;
        Ok(state)
    }

    pub fn config(&self) -> &WorkspaceConfig {
        &self.config
    }
    pub fn durable_store_backend(&self) -> &'static str {
        self.store.backend().as_str()
    }
    pub fn durable_store(&self) -> Arc<dyn MemoryStore> {
        Arc::clone(&self.store)
    }
    pub fn durable_store_migrated_from_file(&self) -> bool {
        self.store_migrated_from_file
    }
    pub fn durable_store_migration_detail(&self) -> &str {
        &self.store_migration_detail
    }
    pub async fn durable_store_health(&self) -> Result<String> {
        self.store.health().await
    }

    pub fn event_tx_channel(
        &self,
    ) -> Option<&broadcast::Sender<crate::server::events::RealtimeEvent>> {
        let store = self
            .store
            .as_ref()
            .as_any()
            .downcast_ref::<VecSqliteMemoryStore>()?;
        store.event_tx_ref()
    }

    pub async fn record_request(&self, event: UsageEvent) -> Result<()> {
        self.requests_used.fetch_add(1, Ordering::Relaxed);
        self.usage_metrics.record(event);
        self.persist_usage_state().await
    }

    pub async fn usage_snapshot(&self) -> WorkspaceUsageSnapshot {
        let usage = self.memory.usage().await;
        WorkspaceUsageSnapshot {
            workspace_id: self.config.id.clone(),
            plan: self.config.plan,
            document_count: usage.document_count,
            storage_bytes_used: usage.storage_bytes,
            storage_bytes_limit: self.config.storage_limit_bytes,
            storage_bytes_remaining: self
                .config
                .storage_limit_bytes
                .map(|limit| limit.saturating_sub(usage.storage_bytes)),
            requests_used: self.requests_used.load(Ordering::Relaxed),
            request_limit: self.config.request_limit,
            request_units_used: self.usage_metrics.total_units(),
            request_unit_limit: self.config.request_unit_limit,
            sync_policy: self.config.sync_policy,
            counters: self.usage_metrics.snapshots(),
            optimization: self.optimization_metrics.snapshot().await,
        }
    }

    pub async fn export_sync(&self) -> Result<String> {
        let sync_dir = self
            .usage_state_path
            .parent()
            .ok_or_else(|| anyhow!("usage_state_path has no parent directory"))?
            .join("sync");
        let mut manifest = crate::sync::chunks::load_manifest(&sync_dir)?;
        let docs = self.memory.all_documents().await;
        crate::sync::chunks::export_to_chunk(&sync_dir, &docs, &mut manifest)
    }

    pub async fn import_sync(&self) -> Result<usize> {
        let sync_dir = self
            .usage_state_path
            .parent()
            .ok_or_else(|| anyhow!("usage_state_path has no parent directory"))?
            .join("sync");
        let manifest = crate::sync::chunks::load_manifest(&sync_dir)?;
        let mut total_imported = 0;
        for hash in manifest.chunks.keys() {
            let docs = crate::sync::chunks::import_from_chunk(&sync_dir, hash)?;
            for doc in docs {
                if self
                    .memory
                    .get(doc.id.as_deref().unwrap_or(&doc.path))
                    .await?
                    .is_none()
                {
                    self.memory.add(doc).await?;
                    total_imported += 1;
                }
            }
        }
        Ok(total_imported)
    }

    pub async fn record_optimization(
        &self,
        route_category: RouteCategory,
        semantic_cache_hit: bool,
        llm_used: bool,
        model: Option<&str>,
    ) -> Result<()> {
        self.optimization_metrics
            .record(route_category, semantic_cache_hit, llm_used, model)
            .await;
        self.persist_usage_state().await
    }

    pub fn limits_snapshot(&self) -> WorkspaceLimitsSnapshot {
        WorkspaceLimitsSnapshot {
            workspace_id: self.config.id.clone(),
            plan: self.config.plan,
            storage_limit_bytes: self.config.storage_limit_bytes,
            request_limit: self.config.request_limit,
            request_unit_limit: self.config.request_unit_limit.unwrap_or(0),
            embedding_provider_mode: self.config.embedding_provider_mode,
            managed_google_embeddings: self.config.managed_google_embeddings,
            sync_policy: self.config.sync_policy,
        }
    }

    pub fn sync_policy_snapshot(&self) -> SyncPolicySnapshot {
        SyncPolicySnapshot {
            workspace_id: self.config.id.clone(),
            current: self.config.sync_policy,
            supported: crate::workspace::config::SyncPolicy::supported().to_vec(),
        }
    }

    pub async fn embedding_provider_snapshot(&self) -> EmbeddingProviderSnapshot {
        use crate::memory::embedder::EmbeddingClient;
        use crate::settings::XavierSettings;
        let settings = XavierSettings::current();
        let configured_url = if settings.models.embedding_url.is_empty() {
            if settings.embedding.endpoint.is_empty() {
                None
            } else {
                Some(settings.embedding.endpoint.clone())
            }
        } else {
            Some(settings.models.embedding_url.clone())
        };
        let configured_model = if settings.models.embedding_model.is_empty() {
            None
        } else {
            Some(settings.models.embedding_model.clone())
        };
        let configured = EmbeddingClient::is_configured_from_env();
        let (available, last_error) = if configured {
            match EmbeddingClient::from_env() {
                Ok(client) => match client.health().await {
                    Ok(true) => (true, None),
                    Ok(false) => (
                        false,
                        Some("embedding service returned empty vectors".to_string()),
                    ),
                    Err(error) => (false, Some(error.to_string())),
                },
                Err(error) => (false, Some(error.to_string())),
            }
        } else {
            (false, None)
        };
        EmbeddingProviderSnapshot {
            workspace_id: self.config.id.clone(),
            mode: self.config.embedding_provider_mode,
            managed_google_embeddings: self.config.managed_google_embeddings,
            configured_model,
            configured_url,
            configured,
            available,
            last_error,
        }
    }

    pub async fn ensure_within_request_limit(&self) -> Result<()> {
        if let Some(limit) = self.config.request_limit {
            let current = self.requests_used.load(Ordering::Relaxed);
            if current > limit {
                return Err(anyhow!(
                    "request quota exceeded for workspace {}: {} > {}",
                    self.config.id,
                    current,
                    limit
                ));
            }
        }
        if let Some(limit) = self.config.request_unit_limit {
            let current = self.usage_metrics.total_units();
            if current > limit {
                return Err(anyhow!(
                    "request unit quota exceeded for workspace {}: {} > {}",
                    self.config.id,
                    current,
                    limit
                ));
            }
        }
        Ok(())
    }

    pub async fn ensure_within_storage_limit(
        &self,
        path: &str,
        content: &str,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        let Some(limit) = self.config.storage_limit_bytes else {
            return Ok(());
        };
        let MemoryUsage { storage_bytes, .. } = self.memory.usage().await;
        let projected = storage_bytes + estimate_document_bytes(path, content, metadata);
        if projected > limit {
            return Err(anyhow!("storage quota exceeded for workspace {}: projected {} bytes exceeds limit {} bytes", self.config.id, projected, limit));
        }
        Ok(())
    }

    pub async fn ingest(
        &self,
        path: String,
        content: String,
        metadata: serde_json::Value,
        auto_curate: bool,
    ) -> Result<String> {
        self.ingest_typed(path, content, metadata, None, None, auto_curate)
            .await
    }

    pub async fn ingest_typed(
        &self,
        path: String,
        content: String,
        metadata: serde_json::Value,
        typed: Option<crate::memory::schema::TypedMemoryPayload>,
        content_vector: Option<Vec<f32>>,
        auto_curate: bool,
    ) -> Result<String> {
        self.ensure_within_storage_limit(&path, &content, &metadata)
            .await?;
        let doc_id = if let Some(content_vector) = content_vector {
            self.memory
                .add_document_typed_with_embedding(
                    path,
                    content.clone(),
                    metadata.clone(),
                    typed,
                    Some(content_vector),
                )
                .await?
        } else {
            self.memory
                .add_document_typed(path, content.clone(), metadata.clone(), typed)
                .await?
        };
        self.index_memory_layers(&doc_id, &content, &metadata).await;
        if auto_curate {
            let action = crate::memory::manager::MemoryAction::Curate {
                doc_id: doc_id.clone(),
            };
            let _ = self.memory_manager.execute_actions(vec![action]).await;
        }
        Ok(doc_id)
    }

    pub async fn persist_beliefs(&self) -> Result<()> {
        let beliefs = self.belief_graph.read().await.get_relations();
        self.store.save_beliefs(&self.config.id, beliefs).await
    }

    pub async fn index_memory_entities(
        &self,
        memory_id: &str,
        content: &str,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        let result = self
            .entity_graph
            .upsert_memory(memory_id, content, Some(metadata))
            .await
            .map(|_| ());

        if result.is_ok() {
            if let Ok(json) = self.entity_graph.export_json().await {
                if let Err(error) = self
                    .store
                    .save_entity_graph_snapshot(&self.config.id, &json)
                    .await
                {
                    tracing::warn!(%error, "failed to save entity graph snapshot after indexing");
                }
            }

            let _ = crate::notifications::NOTIFICATIONS
                .notify(
                    crate::notifications::IslandId::Memory,
                    "Memory Indexed",
                    &format!("New memory indexed: {}", memory_id),
                    "success",
                )
                .await;
        } else if let Err(ref e) = result {
            let _ = crate::notifications::NOTIFICATIONS
                .notify(
                    crate::notifications::IslandId::Errors,
                    "Entity Indexing Failed",
                    &format!("Failed to index entities for {}: {}", memory_id, e),
                    "error",
                )
                .await;
        }

        result
    }

    pub async fn index_memory_layers(
        &self,
        memory_id: &str,
        content: &str,
        metadata: &serde_json::Value,
    ) {
        if let Err(error) = self
            .index_memory_entities(memory_id, content, metadata)
            .await
        {
            tracing::warn!(%error, memory_id = %memory_id, "failed to index memory entities");
        }
        if let Err(error) = self.semantic_memory.index_memory(memory_id, content).await {
            tracing::warn!(%error, memory_id = %memory_id, "failed to index semantic memory");
        }

        // Upsert to working memory (hot set)
        let item = MemoryItem::new(memory_id, content).with_metadata(metadata.clone());
        self.working_memory.write().await.push(item);
    }

    pub async fn remove_memory_entities(&self, memory_id: &str) -> Result<()> {
        let result = self.entity_graph.remove_memory(memory_id).await;
        if result.is_ok() {
            if let Ok(json) = self.entity_graph.export_json().await {
                if let Err(error) = self
                    .store
                    .save_entity_graph_snapshot(&self.config.id, &json)
                    .await
                {
                    tracing::warn!(%error, "failed to save entity graph snapshot after removal");
                }
            }
        }
        result
    }
    pub async fn list_memory_records(&self) -> Result<Vec<MemoryRecord>> {
        self.store.list(&self.config.id).await
    }
    pub async fn list_memory_records_filtered(
        &self,
        filters: MemoryQueryFilters,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        self.store
            .list_filtered(&self.config.id, &filters, limit)
            .await
    }
    pub async fn get_memory_record(&self, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        self.store.get(&self.config.id, id_or_path).await
    }
    pub async fn delete_memory_record(&self, id: &str) -> Result<Option<MemoryRecord>> {
        self.store.delete(&self.config.id, id).await
    }

    pub async fn update_primary_memory(
        &self,
        id: &str,
        path: String,
        content: String,
        metadata: serde_json::Value,
        typed: Option<crate::memory::schema::TypedMemoryPayload>,
    ) -> Result<Option<String>> {
        let Some(existing) = self.memory.get(id).await? else {
            return Ok(None);
        };
        let normalized = crate::memory::schema::normalize_metadata(
            &path,
            metadata,
            &self.config.id,
            typed.as_ref(),
        )?;
        let mut document = crate::memory::qmd_memory::MemoryDocument {
            id: existing.id.clone(),
            path,
            content,
            metadata: normalized,
            content_vector: Some(existing.embedding.clone()),
            embedding: existing.embedding.clone(),
            cluster_id: typed.as_ref().and_then(|t| t.cluster_id.clone()),
            parent_id: None,
            level: typed
                .as_ref()
                .and_then(|t| t.level)
                .unwrap_or(existing.level),
            relation: typed.as_ref().and_then(|t| t.relation.clone()),
            clearance: typed
                .as_ref()
                .and_then(|t| t.clearance)
                .unwrap_or(existing.clearance),
            minhash: None,
            score: 0.0,
            ..Default::default()
        };
        if let Some(object) = document.metadata.as_object_mut() {
            let revision = existing
                .metadata
                .get("revision")
                .and_then(|value| value.as_u64())
                .unwrap_or(1)
                + 1;
            let created_at = existing
                .metadata
                .get("created_at")
                .cloned()
                .unwrap_or_else(|| serde_json::json!(Utc::now().to_rfc3339()));
            object.insert("revision".to_string(), serde_json::json!(revision));
            object.insert("created_at".to_string(), created_at);
            object.insert(
                "updated_at".to_string(),
                serde_json::json!(Utc::now().to_rfc3339()),
            );
        }
        self.memory.update(document.clone()).await?;
        let memory_id = document.id.clone().unwrap_or_else(|| document.path.clone());
        self.index_memory_layers(&memory_id, &document.content, &document.metadata)
            .await;
        Ok(document.id)
    }

    pub async fn record_session_exchange(
        &self,
        session_id: &str,
        source_app: &str,
        user_message: &str,
        assistant_message: &str,
    ) -> Result<String> {
        let timestamp = Utc::now();
        let path = format!(
            "sessions/{}/{}",
            session_id,
            timestamp.format("%Y%m%dT%H%M%S%.3fZ")
        );
        let content = format!("User: {user_message}\nAssistant: {assistant_message}");
        let metadata =
            serde_json::json!({ "session_time": timestamp.to_rfc3339(), "source": source_app, });
        let doc_id = self
            .memory
            .add_document_typed(
                path,
                content.clone(),
                metadata.clone(),
                Some(crate::memory::schema::TypedMemoryPayload {
                    kind: Some(crate::memory::schema::MemoryKind::Episodic),
                    evidence_kind: Some(crate::memory::schema::EvidenceKind::SessionSummary),
                    namespace: Some(crate::memory::schema::MemoryNamespace {
                        session_id: Some(session_id.to_string()),
                        ..crate::memory::schema::MemoryNamespace::default()
                    }),
                    provenance: Some(crate::memory::schema::MemoryProvenance {
                        source_app: Some(source_app.to_string()),
                        source_type: Some("session_exchange".to_string()),
                        recorded_at: Some(timestamp.to_rfc3339()),
                        ..crate::memory::schema::MemoryProvenance::default()
                    }),
                    ..Default::default()
                }),
            )
            .await?;

        // Session exchanges are hot, push to working memory
        let item = MemoryItem::new(&doc_id, &content).with_metadata(metadata);
        self.working_memory.write().await.push(item);

        Ok(doc_id)
    }

    async fn load_usage_state(&self) -> Result<()> {
        if !fs::try_exists(&self.usage_state_path)
            .await
            .unwrap_or(false)
        {
            return Ok(());
        }
        let payload = fs::read_to_string(&self.usage_state_path).await?;
        let persisted: PersistedUsageState = serde_json::from_str(&payload)?;
        self.requests_used
            .store(persisted.requests_used, Ordering::Relaxed);
        self.usage_metrics
            .hydrate(persisted.total_units, &persisted.counters);
        self.optimization_metrics
            .hydrate(&persisted.optimization)
            .await;
        Ok(())
    }

    async fn persist_usage_state(&self) -> Result<()> {
        let _guard = self.persist_lock.lock().await;
        let snapshot = PersistedUsageState {
            requests_used: self.requests_used.load(Ordering::Relaxed),
            total_units: self.usage_metrics.total_units(),
            counters: self.usage_metrics.snapshots(),
            optimization: self.optimization_metrics.snapshot().await,
        };
        let payload = serde_json::to_vec_pretty(&snapshot)?;
        fs::write(&self.usage_state_path, payload).await?;
        Ok(())
    }

    pub async fn generate_session_token(&self) -> Result<String> {
        let token = ulid::Ulid::new().to_string();
        let now = Utc::now();
        let expires_at = now + Duration::hours(12);
        self.store
            .save_session_token(
                &self.config.id,
                SessionTokenRecord {
                    token: token.clone(),
                    created_at: now,
                    expires_at,
                },
            )
            .await?;
        Ok(token)
    }

    pub async fn is_session_token_valid(&self, token_str: &str) -> bool {
        self.store
            .is_session_token_valid(&self.config.id, token_str)
            .await
            .unwrap_or(false)
    }

    /// Get current documents in working memory (hot set) converted to MemoryDocument
    pub async fn working_documents(&self) -> Vec<crate::memory::qmd_memory::MemoryDocument> {
        let wm = self.working_memory.read().await;
        wm.items()
            .iter()
            .map(|item| crate::memory::qmd_memory::MemoryDocument {
                id: Some(item.id.clone()),
                content: item.content.clone(),
                metadata: item
                    .metadata
                    .clone()
                    .unwrap_or_else(|| serde_json::json!({})),
                ..Default::default()
            })
            .collect()
    }

    /// Creates a minimal workspace state for internal background tasks.
    pub async fn new_minimal(
        workspace_id: String,
        workspace_root: PathBuf,
        memory: Arc<QmdMemory>,
        store: Arc<dyn MemoryStore>,
    ) -> Self {
        use crate::checkpoint::CheckpointManager;
        use crate::memory::belief_graph::BeliefGraph;
        use crate::memory::entity_graph::EntityGraph;
        use crate::memory::semantic::SemanticMemory;
        use crate::workspace::config::{
            EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig,
        };

        let belief_graph = Arc::new(RwLock::new(BeliefGraph::new()));
        let entity_graph = Arc::new(EntityGraph::new());
        let semantic_memory = Arc::new(SemanticMemory::new());
        let memory_manager = Arc::new(crate::memory::manager::MemoryManager::new(
            Arc::clone(&memory),
            Some(Arc::clone(&belief_graph)),
        ));
        let checkpoint_manager = Arc::new(CheckpointManager::with_store(
            workspace_id.clone(),
            Arc::clone(&store),
        ));
        let working_memory = Arc::new(RwLock::new(WorkingMemory::new()));

        // Mocking conversations_db for minimal state
        #[cfg(any(test, feature = "test-utils"))]
        let conversations_db = Arc::new(ConversationsDb::new_test());
        #[cfg(not(any(test, feature = "test-utils")))]
        let conversations_db = Arc::new(
            ConversationsDb::open("minimal")
                .await
                .expect("failed to open minimal conversations_db"),
        );

        let navigation_policy = Arc::new(RwLock::new(
            crate::retrieval::NavigationPolicy::with_defaults(),
        ));
        let zone_booster = Arc::new(crate::retrieval::gating::AdaptiveZoneBooster::new());
        let hormer = Arc::new(
            crate::agents::hormer::Hormer::new(navigation_policy)
                .with_booster(Arc::clone(&zone_booster)),
        );

        #[cfg(any(test, feature = "test-utils"))]
        let runtime = Arc::new(AgentRuntime::new_test());
        #[cfg(not(any(test, feature = "test-utils")))]
        let runtime = Arc::new(
            AgentRuntime::new(memory.clone(), None, RuntimeConfig::default())
                .expect("failed to create minimal runtime"),
        );

        Self {
            config: WorkspaceConfig {
                id: workspace_id,
                token: "minimal-token".to_string(),
                plan: PlanTier::Community,
                memory_backend: store.backend(),
                storage_limit_bytes: None,
                request_limit: None,
                request_unit_limit: None,
                embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
                managed_google_embeddings: false,
                sync_policy: SyncPolicy::LocalOnly,
            },
            memory,
            runtime,
            belief_graph,
            entity_graph,
            semantic_memory,
            memory_manager,
            checkpoint_manager,
            conversations_db,
            store,
            store_migrated_from_file: false,
            store_migration_detail: "minimal".to_string(),
            usage_state_path: workspace_root.join("usage.json"),
            persist_lock: Mutex::new(()),
            requests_used: AtomicUsize::new(0),
            usage_metrics: UsageMetrics::new(),
            optimization_metrics: OptimizationMetrics::new(),
            hormer,
            zone_booster,
            working_memory,
        }
    }
}

use crate::memory::store::{FileMemoryStore, InMemoryMemoryStore};

#[cfg(test)]
mod auto_detect_tests {
    use super::*;
    use crate::memory::store::MemoryBackend;
    use std::sync::LazyLock;
    use tokio::sync::Mutex;

    static SERIAL_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[tokio::test]
    async fn test_detect_cloud_backend_fallback() {
        let _guard = SERIAL_LOCK.lock().await;
        let old_pg = std::env::var("XAVIER_POSTGRES_URL").ok();
        let old_sb_url = std::env::var("XAVIER_SUPABASE_URL").ok();
        let old_sb_key = std::env::var("XAVIER_SUPABASE_KEY").ok();

        std::env::remove_var("XAVIER_POSTGRES_URL");
        std::env::remove_var("XAVIER_SUPABASE_URL");
        std::env::remove_var("XAVIER_SUPABASE_KEY");

        let backend = detect_cloud_backend().await;
        assert_eq!(backend, MemoryBackend::Vec);

        if let Some(v) = old_pg {
            std::env::set_var("XAVIER_POSTGRES_URL", v);
        }
        if let Some(v) = old_sb_url {
            std::env::set_var("XAVIER_SUPABASE_URL", v);
        }
        if let Some(v) = old_sb_key {
            std::env::set_var("XAVIER_SUPABASE_KEY", v);
        }
    }

    #[tokio::test]
    async fn test_detect_cloud_backend_supabase() {
        let _guard = SERIAL_LOCK.lock().await;
        let old_pg = std::env::var("XAVIER_POSTGRES_URL").ok();
        let old_sb_url = std::env::var("XAVIER_SUPABASE_URL").ok();
        let old_sb_key = std::env::var("XAVIER_SUPABASE_KEY").ok();

        std::env::remove_var("XAVIER_POSTGRES_URL");
        std::env::set_var("XAVIER_SUPABASE_URL", "https://example.supabase.co");
        std::env::set_var("XAVIER_SUPABASE_KEY", "some-key");

        let backend = detect_cloud_backend().await;
        assert_eq!(backend, MemoryBackend::Supabase);

        std::env::remove_var("XAVIER_SUPABASE_URL");
        std::env::remove_var("XAVIER_SUPABASE_KEY");

        if let Some(v) = old_pg {
            std::env::set_var("XAVIER_POSTGRES_URL", v);
        }
        if let Some(v) = old_sb_url {
            std::env::set_var("XAVIER_SUPABASE_URL", v);
        }
        if let Some(v) = old_sb_key {
            std::env::set_var("XAVIER_SUPABASE_KEY", v);
        }
    }

    #[tokio::test]
    async fn test_detect_cloud_backend_postgres_fail_fallback() {
        let _guard = SERIAL_LOCK.lock().await;
        let old_pg = std::env::var("XAVIER_POSTGRES_URL").ok();
        let old_sb_url = std::env::var("XAVIER_SUPABASE_URL").ok();
        let old_sb_key = std::env::var("XAVIER_SUPABASE_KEY").ok();

        // Bogus URL that will fail connection test
        std::env::set_var("XAVIER_POSTGRES_URL", "postgres://nonexistent:5432/db");
        std::env::remove_var("XAVIER_SUPABASE_URL");
        std::env::remove_var("XAVIER_SUPABASE_KEY");

        let backend = detect_cloud_backend().await;
        // Should fallback to Vec since postgres connection fails
        assert_eq!(backend, MemoryBackend::Vec);

        std::env::remove_var("XAVIER_POSTGRES_URL");

        if let Some(v) = old_pg {
            std::env::set_var("XAVIER_POSTGRES_URL", v);
        }
        if let Some(v) = old_sb_url {
            std::env::set_var("XAVIER_SUPABASE_URL", v);
        }
        if let Some(v) = old_sb_key {
            std::env::set_var("XAVIER_SUPABASE_KEY", v);
        }
    }
}
