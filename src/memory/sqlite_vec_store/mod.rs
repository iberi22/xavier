//! SQLite backend with libSQL vector search for Xavier memory store.
//!
//! Uses native approximate nearest neighbor search via libSQL
//! for semantic similarity matching on memory embeddings.

use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
use libsql::{params, Connection};
use tokio::sync::broadcast;

use crate::memory::schema::{MemoryLevel, MemoryQueryFilters};
use crate::memory::sqlite_store::TABLE_MEMORIES;
use crate::memory::store::{stable_key, HybridSearchMode, HybridSearchResult, MemoryRecord};
use crate::ports::outbound::schema_init::SchemaInitializer;

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
    pub(crate) conn: Arc<Connection>,
    pub(crate) pool: crate::utils::connection_pool::LibsqlConnectionPool,
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

    pub fn get_conn(&self) -> Arc<Connection> {
        self.conn.clone()
    }

    pub fn get_pool(&self) -> crate::utils::connection_pool::LibsqlConnectionPool {
        self.pool.clone()
    }

    /// Get a reference to the event broadcast sender if available
    pub fn event_tx_ref(&self) -> Option<&broadcast::Sender<crate::server::events::RealtimeEvent>> {
        self.event_tx.as_ref()
    }

    pub async fn new(config: VecSqliteStoreConfig) -> Result<Self> {
        db::ensure_dir(&config.path).await?;
        vector::register_sqlite_vec_extension()?;

        let pool = db::open_pool(&config.path).await?;
        let conn = db::open_connection(&config.path).await?;

        let store = Self {
            conn: Arc::new(conn),
            pool,
            config,
            event_tx: None,
        };

        // Initialize schema
        store.init_schema()?;

        Ok(store)
    }

    pub fn new_with_pool(
        conn: Arc<Connection>,
        pool: crate::utils::connection_pool::LibsqlConnectionPool,
        config: VecSqliteStoreConfig,
    ) -> Self {
        let _ = vector::register_sqlite_vec_extension();
        Self {
            conn,
            pool,
            config,
            event_tx: None,
        }
    }

    pub fn register_sqlite_vec_extension() -> Result<()> {
        vector::register_sqlite_vec_extension()
    }

    pub(crate) fn configured_qjl_threshold() -> usize {
        utils::configured_qjl_threshold()
    }

    pub(crate) fn row_key(workspace_id: &str, memory_id: &str) -> String {
        stable_key("sqlite_mem", &[workspace_id, memory_id])
    }

    pub(crate) fn deserialize_record(row: &libsql::Row) -> Result<MemoryRecord> {
        let metadata_str: String = row.get(4).map_err(anyhow::Error::msg)?;
        let embedding_blob: Vec<u8> = row.get(5).map_err(anyhow::Error::msg)?;

        Ok(MemoryRecord {
            id: row.get(0).map_err(anyhow::Error::msg)?,
            workspace_id: row.get(1).map_err(anyhow::Error::msg)?,
            path: row.get(2).map_err(anyhow::Error::msg)?,
            content: row.get(3).map_err(anyhow::Error::msg)?,
            metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
            embedding: vector::deserialize_embedding(&embedding_blob),
            created_at: chrono::DateTime::parse_from_rfc3339(
                &row.get::<String>(6).map_err(anyhow::Error::msg)?,
            )
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
            updated_at: chrono::DateTime::parse_from_rfc3339(
                &row.get::<String>(7).map_err(anyhow::Error::msg)?,
            )
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
            revision: row.get(8).map_err(anyhow::Error::msg)?,
            primary: row.get::<i32>(9).map_err(anyhow::Error::msg)? != 0,
            parent_id: row.get::<Option<String>>(10).ok().flatten(),
            cluster_id: row.get::<Option<String>>(11).ok().flatten(),
            level: MemoryLevel::parse(&row.get::<String>(12).map_err(anyhow::Error::msg)?),
            relation: row
                .get::<Option<String>>(13)
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str(&s).ok()),
            revisions: row
                .get::<Option<String>>(14)
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
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
        let stmt = match conn
            .prepare("SELECT COUNT(*) FROM memory_embeddings WHERE workspace_id = ?")
            .await
        {
            Ok(stmt) => stmt,
            Err(_) => return false,
        };
        let mut rows = match stmt.query(params![workspace_id]).await {
            Ok(rows) => rows,
            Err(_) => return false,
        };
        let current_vectors = match rows.next().await {
            Ok(Some(row)) => row.get::<i64>(0).unwrap_or_default().max(0) as usize,
            _ => 0,
        };
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
        let stmt = conn.prepare(&format!(
            "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE id = ? AND workspace_id = ?",
            TABLE_MEMORIES
        )).await?;

        let mut rows = stmt.query(params![memory_id, workspace_id]).await?;
        if let Some(row) = rows.next().await? {
            let record = Self::deserialize_record(&row)?;
            Ok(Some(record))
        } else {
            Ok(None)
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
