//! SQLite backend with libSQL vector search for Xavier memory store.
//!
//! Uses native approximate nearest neighbor search via libSQL
//! for semantic similarity matching on memory embeddings.

use std::collections::HashSet;

use anyhow::Result;
use rusqlite::params;
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
}

impl VecSqliteMemoryStore {
    pub async fn from_env() -> Result<Self> {
        Self::new(VecSqliteStoreConfig::from_env()).await
    }

    pub fn set_event_tx(&mut self, tx: broadcast::Sender<crate::server::events::RealtimeEvent>) {
        self.event_tx = Some(tx);
    }

    /// Get a reference to the event broadcast sender if available
    pub fn event_tx_ref(&self) -> Option<&broadcast::Sender<crate::server::events::RealtimeEvent>> {
        self.event_tx.as_ref()
    }

    pub async fn new(config: VecSqliteStoreConfig) -> Result<Self> {
        db::ensure_dir(&config.path).await?;
        vector::register_sqlite_vec_extension()?;

        let project_id = "vec_store";
        ConnectionManager::global().connect(project_id, ".")?;

        let store = Self {
            project_id: project_id.to_string(),
            config,
            event_tx: None,
        };

        // Initialize schema
        store.init_schema_async().await?;

        Ok(store)
    }

    pub fn register_sqlite_vec_extension() -> Result<()> {
        vector::register_sqlite_vec_extension()
    }

    pub(crate) fn configured_qjl_threshold() -> usize {
        utils::configured_qjl_threshold()
    }

    #[allow(dead_code)]
    pub(crate) fn row_key(workspace_id: &str, memory_id: &str) -> String {
        stable_key("sqlite_mem", &[workspace_id, memory_id])
    }

    pub(crate) fn deserialize_record(row: &rusqlite::Row) -> Result<MemoryRecord> {
        let metadata_str: String = row.get(4)?;
        let embedding_blob: Vec<u8> = row.get(5)?;

        Ok(MemoryRecord {
            id: row.get(0)?,
            workspace_id: row.get(1)?,
            path: row.get(2)?,
            content: row.get(3)?,
            metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
            embedding: vector::deserialize_embedding(&embedding_blob),
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
            clearance: Default::default(),
            revisions: row
                .get::<_, Option<String>>(14)?
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            encrypted_dek: row.get(15)?,
            content_iv: row.get(16)?,
            metadata_iv: row.get(17)?,
        })
    }

    pub(crate) fn candidate_limit(limit: usize) -> usize {
        limit.max(1).saturating_mul(5)
    }

    pub(crate) fn configured_rrf_k() -> usize {
        utils::configured_rrf_k()
    }

    pub(crate) fn dynamic_rrf_k(dataset_size: usize) -> usize {
        utils::dynamic_rrf_k(dataset_size)
    }

    pub(crate) fn entity_extraction_enabled() -> bool {
        utils::entity_extraction_enabled()
    }

    pub(crate) fn audit_chain_enabled() -> bool {
        utils::audit_chain_enabled()
    }

    #[allow(dead_code)]
    pub(crate) async fn qjl_enabled_for_workspace(workspace_id: &str) -> bool {
        let threshold = Self::configured_qjl_threshold();
        let workspace_id = workspace_id.to_string();

        let result = ConnectionManager::global()
            .with_conn("vec_store", move |conn| {
                let mut stmt =
                    conn.prepare("SELECT COUNT(*) FROM memory_embeddings WHERE workspace_id = ?")?;
                let current_vectors: usize =
                    stmt.query_row(params![workspace_id], |row| row.get(0))?;
                Ok(current_vectors >= threshold)
            })
            .await;

        result.unwrap_or(false)
    }

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

    #[allow(dead_code)]
    pub(crate) async fn load_record_by_id(
        workspace_id: &str,
        memory_id: &str,
    ) -> Result<Option<MemoryRecord>> {
        let workspace_id = workspace_id.to_string();
        let memory_id = memory_id.to_string();

        ConnectionManager::global().with_conn("vec_store", move |conn| {
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

    #[allow(dead_code)]
    pub(crate) async fn sync_memory_entities(
        workspace_id: &str,
        record: &MemoryRecord,
    ) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        let record = record.clone();
        ConnectionManager::global()
            .with_conn("vec_store", move |conn| {
                graph::sync_memory_entities(conn, &workspace_id, &record)
            })
            .await
    }

    #[allow(dead_code)]
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
            .with_conn("vec_store", move |conn| {
                graph::resolve_graph_seed_entities(conn, &workspace_id, &source, &query)
            })
            .await
    }

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
