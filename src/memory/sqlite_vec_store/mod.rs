//! SQLite backend with libSQL vector search for Xavier memory store.
//!
//! Uses native approximate nearest neighbor search via libSQL
//! for semantic similarity matching on memory embeddings.

use std::{collections::HashSet, sync::Arc};

use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};
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

/// Vector-enabled SQLite memory store using r2d2-sqlite and sqlite-vec for HNSW-like similarity search.
#[derive(Clone)]
pub struct VecSqliteMemoryStore {
    pub(crate) pool: Arc<Pool<SqliteConnectionManager>>,
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

    pub fn get_pool(&self) -> Arc<Pool<SqliteConnectionManager>> {
        self.pool.clone()
    }

    /// Get a reference to the event broadcast sender if available
    pub fn event_tx_ref(&self) -> Option<&broadcast::Sender<crate::server::events::RealtimeEvent>> {
        self.event_tx.as_ref()
    }

    pub async fn new(config: VecSqliteStoreConfig) -> Result<Self> {
        let manager = ConnectionManager::global();
        let root = config.path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());
        manager.connect("vec_store", &root)?;
        let pool = manager.get_pool("vec_store")?;

        let store = Self {
            pool,
            config,
            event_tx: None,
        };

        // Initialize schema
        store.init_schema()?;

        Ok(store)
    }

    pub fn new_with_pool(
        pool: Arc<Pool<SqliteConnectionManager>>,
        config: VecSqliteStoreConfig,
    ) -> Self {
        Self {
            pool,
            config,
            event_tx: None,
        }
    }

    pub fn register_sqlite_vec_extension() -> Result<()> {
        Ok(())
    }

    pub(crate) fn configured_qjl_threshold() -> usize {
        utils::configured_qjl_threshold()
    }

    pub(crate) fn row_key(workspace_id: &str, memory_id: &str) -> String {
        stable_key("sqlite_mem", &[workspace_id, memory_id])
    }

    pub(crate) fn deserialize_record(row: &rusqlite::Row) -> Result<MemoryRecord> {
        let id: String = row.get(0).map_err(|e| anyhow::anyhow!(e))?;
        let workspace_id: String = row.get(1).map_err(|e| anyhow::anyhow!(e))?;
        let path: String = row.get(2).map_err(|e| anyhow::anyhow!(e))?;
        let content: String = row.get(3).map_err(|e| anyhow::anyhow!(e))?;
        let metadata_str: String = row.get(4).map_err(|e| anyhow::anyhow!(e))?;
        let embedding_blob: Vec<u8> = row.get(5).map_err(|e| anyhow::anyhow!(e))?;
        let created_at_str: String = row.get(6).map_err(|e| anyhow::anyhow!(e))?;
        let updated_at_str: String = row.get(7).map_err(|e| anyhow::anyhow!(e))?;
        let revision: i32 = row.get(8).map_err(|e| anyhow::anyhow!(e))?;
        let primary_int: i32 = row.get(9).map_err(|e| anyhow::anyhow!(e))?;
        let parent_id: Option<String> = row.get(10).map_err(|e| anyhow::anyhow!(e))?;
        let cluster_id: Option<String> = row.get(11).map_err(|e| anyhow::anyhow!(e))?;
        let level_str: String = row.get(12).map_err(|e| anyhow::anyhow!(e))?;
        let relation_str: Option<String> = row.get(13).map_err(|e| anyhow::anyhow!(e))?;
        let revisions_str: Option<String> = row.get(14).map_err(|e| anyhow::anyhow!(e))?;

        Ok(MemoryRecord {
            id,
            workspace_id,
            path,
            content,
            metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
            embedding: vector::deserialize_embedding(&embedding_blob),
            created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            revision,
            primary: primary_int != 0,
            parent_id,
            cluster_id,
            level: MemoryLevel::parse(&level_str),
            relation: relation_str.and_then(|s| serde_json::from_str(&s).ok()),
            revisions: revisions_str.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
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

    pub(crate) async fn qjl_enabled_for_workspace(conn: &Connection, workspace_id: &str) -> bool {
        let threshold = Self::configured_qjl_threshold();
        let mut stmt = match conn
            .prepare("SELECT COUNT(*) FROM memory_embeddings WHERE workspace_id = ?")
        {
            Ok(stmt) => stmt,
            Err(_) => return false,
        };
        let current_vectors: usize = stmt.query_row(params![workspace_id], |row| row.get(0)).unwrap_or(0);
        current_vectors >= threshold
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

    pub(crate) async fn load_record_by_id(
        conn: &Connection,
        workspace_id: &str,
        memory_id: &str,
    ) -> Result<Option<MemoryRecord>> {
        let mut stmt = conn.prepare(&format!(
            "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE id = ? AND workspace_id = ?",
            TABLE_MEMORIES
        ))?;

        match stmt.query_row(params![memory_id, workspace_id], |row| {
            Self::deserialize_record(row).map_err(|e| rusqlite::Error::Other(e.into()))
        }) {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("SQLite error: {}", e)),
        }
    }

    pub(crate) async fn sync_memory_entities(
        conn: &Connection,
        workspace_id: &str,
        record: &MemoryRecord,
    ) -> Result<()> {
        graph::sync_memory_entities(conn, workspace_id, record).await
    }

    pub(crate) async fn resolve_graph_seed_entities(
        &self,
        conn: &Connection,
        workspace_id: &str,
        source: &MemoryRecord,
        query: &str,
    ) -> Result<HashSet<String>> {
        graph::resolve_graph_seed_entities(conn, workspace_id, source, query).await
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
