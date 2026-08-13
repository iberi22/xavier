//! SQLite backend with libSQL vector search for Xavier memory store.
//!
//! Uses native approximate nearest neighbor search via libSQL
//! for semantic similarity matching on memory embeddings.

use std::collections::HashSet;

use anyhow::Result;
use rusqlite::params;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::codebase::connection_manager::ConnectionManager;
use crate::memory::schema::{MemoryLevel, MemoryQueryFilters};
use crate::memory::sqlite_store::TABLE_MEMORIES;
use crate::memory::store::{stable_key, HybridSearchMode, HybridSearchResult, MemoryRecord};

pub mod audit;
pub mod backend_impl;
pub mod config;
pub mod db;
pub mod fts;
pub mod graph;
pub mod schema_impl;
pub mod search;
pub mod store_impl;
pub mod types;
pub mod utils;
pub mod vector;

pub use config::*;
pub use types::*;

/// Vector-enabled SQLite memory store using libSQL for HNSW-like similarity search.
#[derive(Clone)]
pub struct VecSqliteMemoryStore {
    pub(crate) project_id: String,
    pub(crate) config: VecSqliteStoreConfig,
    pub(crate) event_tx: Option<broadcast::Sender<crate::server::events::RealtimeEvent>>,
    pub(crate) dedup_config:
        std::sync::Arc<tokio::sync::RwLock<crate::settings::types::DedupSettings>>,
}

/// ConnectionManager project_id for a vec-store file path.
///
/// Must stay in sync with [`VecSqliteMemoryStore::new`] so panel / vacuum /
/// other callers hit the same pool (and migrations) as memory operations.
pub fn project_id_for_path(path: &std::path::Path) -> String {
    let digest = crate::crypto::hex_encode(Sha256::digest(path.to_string_lossy().as_bytes()));
    format!("vec_store_{}", &digest[..12])
}

impl VecSqliteMemoryStore {
    /// From env.
    pub async fn from_env() -> Result<Self> {
        let store = Self::new(VecSqliteStoreConfig::from_env()).await?;
        let settings = crate::settings::XavierSettings::current();
        *store.dedup_config.write().await = settings.memory.dedup.clone();
        Ok(store)
    }

    /// Set event tx.
    pub fn set_event_tx(&mut self, tx: broadcast::Sender<crate::server::events::RealtimeEvent>) {
        self.event_tx = Some(tx);
    }

    /// Get a reference to the event broadcast sender if available
    pub fn event_tx_ref(&self) -> Option<&broadcast::Sender<crate::server::events::RealtimeEvent>> {
        self.event_tx.as_ref()
    }

    /// ConnectionManager pool id for this store instance.
    pub fn connection_project_id(&self) -> &str {
        &self.project_id
    }

    /// New.
    pub async fn new(config: VecSqliteStoreConfig) -> Result<Self> {
        db::ensure_dir(&config.path).await?;
        vector::register_sqlite_vec_extension()?;

        let project_id = project_id_for_path(&config.path);
        ConnectionManager::global().connect_with_path(&project_id, config.path.clone())?;

        let store = Self {
            project_id,
            config,
            event_tx: None,
            dedup_config: std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::settings::types::DedupSettings::default(),
            )),
        };

        // Initialize schema
        store.init_schema_async().await?;

        Ok(store)
    }

    /// Register sqlite vec extension.
    pub fn register_sqlite_vec_extension() -> Result<()> {
        vector::register_sqlite_vec_extension()
    }

    /// Configured qjl threshold.
    pub(crate) fn configured_qjl_threshold() -> usize {
        utils::configured_qjl_threshold()
    }

    #[expect(dead_code, reason = "Helper para construir row key usado desde traits")]
    /// Row key.
    pub(crate) fn row_key(workspace_id: &str, memory_id: &str) -> String {
        stable_key("sqlite_mem", &[workspace_id, memory_id])
    }

    /// Deserialize record.
    pub(crate) fn deserialize_record(row: &rusqlite::Row) -> Result<MemoryRecord> {
        let metadata_str: String = row.get(4).unwrap_or_else(|_| "{}".to_string());
        // Null embeddings are valid (e.g. after model change invalidation / pending reindex).
        let embedding_blob: Vec<u8> = row.get::<_, Option<Vec<u8>>>(5)?.unwrap_or_default();

        Ok(MemoryRecord {
            id: row.get(0)?,
            workspace_id: row.get(1)?,
            path: row.get(2)?,
            content: row.get(3)?,
            metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
            embedding: if embedding_blob.is_empty() {
                Vec::new()
            } else {
                vector::deserialize_embedding(&embedding_blob)
            },
            created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            revision: row.get(8)?,
            primary: row.get::<_, i32>(9)? != 0,
            parent_id: row.get::<_, Option<String>>(10)?,
            cluster_id: row.get::<_, Option<String>>(11)?,
            level: MemoryLevel::parse(&row.get::<_, String>(12)?),
            relation: row
                .get::<_, Option<String>>(13)?
                .and_then(|s| serde_json::from_str(&s).ok()),
            clearance: serde_json::from_str::<serde_json::Value>(&metadata_str)
                .ok()
                .and_then(|v| v.get("clearance").cloned())
                .and_then(|v| {
                    v.as_str()
                        .map(|s| crate::security::clearance::ClearanceLevel::from(s))
                })
                .unwrap_or_default(),
            revisions: row
                .get::<_, Option<String>>(14)?
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            encrypted_dek: row.get(15).ok(),
            content_iv: row.get(16).ok(),
            metadata_iv: row.get(17).ok(),
            score: 0.0,
            deleted_at: None,
        })
    }

    /// Candidate limit.
    pub(crate) fn candidate_limit(limit: usize) -> usize {
        limit.max(1).saturating_mul(5)
    }

    /// Configured rrf k.
    pub(crate) fn configured_rrf_k() -> usize {
        utils::configured_rrf_k()
    }

    /// Dynamic rrf k.
    pub(crate) fn dynamic_rrf_k(dataset_size: usize) -> usize {
        utils::dynamic_rrf_k(dataset_size)
    }

    /// Entity extraction enabled.
    pub(crate) fn entity_extraction_enabled() -> bool {
        utils::entity_extraction_enabled()
    }

    /// Audit chain enabled.
    pub(crate) fn audit_chain_enabled() -> bool {
        utils::audit_chain_enabled()
    }

    /// Row matches filters.
    pub(crate) fn row_matches_filters(
        workspace_id: &str,
        record: &MemoryRecord,
        filters: Option<&MemoryQueryFilters>,
    ) -> bool {
        filters.is_none_or(|filters| {
            crate::memory::schema::resolve_metadata(
                &record.path,
                &record.metadata,
                workspace_id,
                None,
            )
            .map(|resolved| {
                filters
                    .workspace_id
                    .as_deref()
                    .is_none_or(|value| resolved.namespace.workspace_id.as_deref() == Some(value))
                    && filters
                        .project
                        .as_deref()
                        .is_none_or(|value| resolved.namespace.project.as_deref() == Some(value))
                    && filters
                        .scope
                        .as_deref()
                        .is_none_or(|value| resolved.namespace.scope.as_deref() == Some(value))
                    && filters
                        .session_id
                        .as_deref()
                        .is_none_or(|value| resolved.namespace.session_id.as_deref() == Some(value))
            })
            .unwrap_or(false)
        })
    }

    #[expect(dead_code, reason = "Metodo de store usado via MemoryStore trait")]
    /// Load record by id.
    pub(crate) async fn load_record_by_id(
        &self,
        workspace_id: &str,
        memory_id: &str,
    ) -> Result<Option<MemoryRecord>> {
        let workspace_id = workspace_id.to_string();
        let memory_id = memory_id.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE id = ? AND workspace_id = ?",
                TABLE_MEMORIES
            ))?;

            let mut rows = stmt.query(params![memory_id, workspace_id])?;
            if let Some(row) = rows.next()? {
                let record = Self::deserialize_record(row)?;
                Ok(Some(record))
            } else {
                Ok(None)
            }
        }).await
    }

    #[expect(dead_code, reason = "Metodo de store usado via MemoryStore trait")]
    /// Sync memory entities.
    pub(crate) async fn sync_memory_entities(
        &self,
        workspace_id: &str,
        record: &MemoryRecord,
    ) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        let record = record.clone();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                graph::sync_memory_entities(conn, &workspace_id, &record)
            })
            .await
    }

    #[expect(dead_code, reason = "Metodo de store usado via MemoryStore trait")]
    /// Resolve graph seed entities.
    pub(crate) async fn resolve_graph_seed_entities(
        &self,
        workspace_id: &str,
        source: &MemoryRecord,
        query: &str,
    ) -> Result<HashSet<String>> {
        let workspace_id = workspace_id.to_string();
        let source = source.clone();
        let query = query.to_string();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                graph::resolve_graph_seed_entities(conn, &workspace_id, &source, &query)
            })
            .await
    }

    /// Hybrid search with embedding.
    pub async fn hybrid_search_with_embedding(
        &self,
        workspace_id: &str,
        query: &str,
        embedding: Vec<f32>,
        filters: Option<&MemoryQueryFilters>,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        self.perform_hybrid_search(
            workspace_id,
            query,
            HybridSearchMode::Both,
            filters,
            limit,
            Some(embedding),
        )
        .await
    }
}
