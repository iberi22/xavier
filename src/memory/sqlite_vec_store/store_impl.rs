//! Store trait implementation for SQLite backend
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::Result;
use async_trait::async_trait;
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::any::Any;

use crate::checkpoint::Checkpoint;
use crate::codebase::connection_manager::ConnectionManager;
use crate::domain::memory::belief::BeliefEdge;
use crate::memory::schema::MemoryQueryFilters;
use crate::memory::sqlite_store::{TABLE_CHECKPOINTS, TABLE_MEMORIES};
use crate::memory::store::{
    DurableWorkspaceState, GraphHopResult, HybridSearchMode, HybridSearchResult, MemoryBackend,
    MemoryRecord, MemoryStore, SessionTokenRecord,
};

use super::{graph, vector, VecSqliteMemoryStore};

#[async_trait]
impl MemoryStore for VecSqliteMemoryStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Vec
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn health(&self) -> Result<String> {
        let detail = self.config.detail();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                conn.query_row("SELECT 1", [], |_row| Ok(()))?;
                Ok(format!("vecsqlite {}", detail))
            })
            .await
    }

    async fn put(&self, record: MemoryRecord) -> Result<()> {
        let project_id = self.project_id.clone();
        let record_c = record.clone();

        ConnectionManager::global().with_conn(&project_id, move |conn| {
            // Compute content hash for tamper-evident hash chain
            let content_hash = format!("{:x}", Sha256::digest(record_c.content.as_bytes()));

            // Get the previous hash for chain linking
            let prev_hash: Option<String> = {
                conn.query_row(
                    "SELECT content_hash FROM memory_chain ORDER BY created_at DESC LIMIT 1",
                    (),
                    |row| row.get(0)
                ).ok()
            };

            // Store in main table first
            {
                // qjl_enabled_for_workspace logic here
                let qjl_enabled = {
                    let threshold = 1000; // Mock or get from config
                    let current_vectors: usize = conn.query_row(
                        "SELECT COUNT(*) FROM memory_embeddings WHERE workspace_id = ?",
                        params![record_c.workspace_id],
                        |row| row.get(0)
                    ).unwrap_or(0);
                    current_vectors >= threshold
                };

                let embedding_blob = if !record_c.embedding.is_empty() && qjl_enabled
                {
                    vector::serialize_embedding_qjl(&record_c.embedding)
                } else {
                    vector::serialize_embedding(&record_c.embedding)
                };

                conn.execute(
                    &format!(
                        "INSERT OR REPLACE INTO {} (id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                        TABLE_MEMORIES
                    ),
                    params![
                        record_c.id,
                        record_c.workspace_id,
                        record_c.path,
                        record_c.content,
                        serde_json::to_string(&record_c.metadata).unwrap_or_default(),
                        embedding_blob,
                        record_c.created_at.to_rfc3339(),
                        record_c.updated_at.to_rfc3339(),
                        record_c.revision,
                        record_c.primary as i32,
                        record_c.parent_id,
                        record_c.cluster_id,
                        record_c.level.as_str(),
                        serde_json::to_string(&record_c.relation).unwrap_or_default(),
                        serde_json::to_string(&record_c.revisions).unwrap_or_default(),
                    ],
                )?;

                // Sync to FTS5
                conn.execute(
                    "DELETE FROM memory_fts WHERE id = ?",
                    params![record_c.id],
                )?;
                let code_tokens =
                    super::fts::code_tokens(&format!("{} {}", &record_c.path, &record_c.content)).join(" ");
                conn.execute(
                    "INSERT INTO memory_fts(id, path, content, code_tokens) VALUES (?, ?, ?, ?)",
                    params![
                        record_c.id,
                        record_c.path,
                        record_c.content,
                        code_tokens
                    ],
                )?;

                graph::sync_memory_entities(conn, &record_c.workspace_id, &record_c)?;

                // Add to hash chain
                let chain_id = ulid::Ulid::new().to_string();
                conn.execute(
                    "INSERT INTO memory_chain (id, prev_hash, content_hash) VALUES (?, ?, ?)",
                    params![chain_id, prev_hash, content_hash],
                )?;

                // Call refined append_timeline_event (now sync inside with_conn)
                // Need a reference to Self, but we're inside closure.
                // Wait, append_timeline_event can be a static-like helper or we can pass store state.
                // For now, let's keep the logic inline or make it a method that takes &Connection.
            }

            // Store vector in native vector search table
            if !record_c.embedding.is_empty() {
                let embedding_json = serde_json::to_string(&record_c.embedding).unwrap_or_default();
                conn.execute(
                    "INSERT OR REPLACE INTO memory_embeddings(id, workspace_id, embedding) VALUES (?1, ?2, vector32(?3))",
                    params![record_c.id, record_c.workspace_id, embedding_json],
                )?;
            }

            Ok(())
        }).await?;

        // Re-run timeline event outside to handle broadcast if needed, or refine append_timeline_event
        let store_clone = self.clone();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                store_clone.append_timeline_event(conn, &record.workspace_id, &record)
            })
            .await
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let workspace_id = workspace_id.to_string();
        let id_or_path = id_or_path.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            // Try by id first (O(1) lookup)
            let key = crate::memory::store::stable_key("sqlite_mem", &[&workspace_id, &id_or_path]);
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE id = ? LIMIT 1",
                TABLE_MEMORIES
            ))?;

            let mut rows = stmt.query([&key])?;
            if let Some(row) = rows.next()? {
                return Ok(Some(VecSqliteMemoryStore::deserialize_record(row)?));
            }

            // Fallback: try by path
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE workspace_id = ? AND path = ?",
                TABLE_MEMORIES
            ))?;

            let mut rows = stmt.query(params![workspace_id, id_or_path])?;
            if let Some(row) = rows.next()? {
                Ok(Some(VecSqliteMemoryStore::deserialize_record(row)?))
            } else {
                Ok(None)
            }
        }).await
    }

    async fn update(&self, record: MemoryRecord) -> Result<()> {
        let record = if let Some(existing) = self.get(&record.workspace_id, &record.id).await? {
            crate::memory::store::revisioned_record(existing, record)
        } else if let Some(existing) = self.get(&record.workspace_id, &record.path).await? {
            crate::memory::store::revisioned_record(existing, record)
        } else {
            record
        };
        self.put(record).await
    }

    async fn delete(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let removed = self.get(workspace_id, id_or_path).await?;
        if let Some(record) = &removed {
            let workspace_id = workspace_id.to_string();
            let record_id = record.id.clone();

            ConnectionManager::global()
                .with_conn(&self.project_id, move |conn| {
                    let tx = conn.unchecked_transaction()?;

                    // Remove dependent rows first to satisfy foreign keys.
                    tx.execute(
                        "DELETE FROM memory_entities WHERE workspace_id = ? AND memory_id = ?",
                        params![workspace_id, record_id],
                    )?;

                    tx.execute(
                        "DELETE FROM memory_entities WHERE workspace_id = ? AND memory_id IN (
                        SELECT id FROM memory_records WHERE workspace_id = ? AND parent_id = ?
                    )",
                        params![workspace_id, workspace_id, record_id],
                    )?;

                    // Delete from vector table
                    tx.execute(
                        "DELETE FROM memory_embeddings WHERE id = ? AND workspace_id = ?",
                        params![record_id, workspace_id],
                    )?;

                    tx.execute(
                        "DELETE FROM memory_embeddings WHERE workspace_id = ? AND id IN (
                        SELECT id FROM memory_records WHERE workspace_id = ? AND parent_id = ?
                    )",
                        params![workspace_id, workspace_id, record_id],
                    )?;

                    // Delete from FTS5
                    tx.execute("DELETE FROM memory_fts WHERE id = ?", params![record_id])?;

                    tx.execute(
                        "DELETE FROM memory_fts WHERE id IN (
                        SELECT id FROM memory_records WHERE workspace_id = ? AND parent_id = ?
                    )",
                        params![workspace_id, record_id],
                    )?;

                    let memory_node_id = graph::memory_node_id(&workspace_id, &record_id);

                    tx.execute(
                        "DELETE FROM relations WHERE source_id = ?",
                        params![memory_node_id],
                    )?;

                    tx.execute(
                        "DELETE FROM relations WHERE target_id = ?",
                        params![memory_node_id],
                    )?;

                    tx.execute("DELETE FROM entities WHERE id = ?", params![memory_node_id])?;

                    // Remove child memories before parent.
                    tx.execute(
                        &format!(
                            "DELETE FROM {} WHERE workspace_id = ? AND parent_id = ?",
                            TABLE_MEMORIES
                        ),
                        params![workspace_id, record_id],
                    )?;

                    tx.execute(
                        &format!(
                            "DELETE FROM {} WHERE workspace_id = ? AND id = ?",
                            TABLE_MEMORIES
                        ),
                        params![workspace_id, record_id],
                    )?;
                    tx.commit()?;
                    Ok(())
                })
                .await?;
        }
        Ok(removed)
    }

    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        let workspace_id = workspace_id.to_string();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE workspace_id = ?",
                TABLE_MEMORIES
            ))?;

            let mut rows = stmt.query([workspace_id])?;
            let mut records = Vec::new();
            while let Some(row) = rows.next()? {
                records.push(VecSqliteMemoryStore::deserialize_record(row)?);
            }
            Ok(records)
        }).await
    }

    async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>> {
        let workspace_id_c = workspace_id.to_string();
        let query = query.to_string();
        let filters = filters.cloned();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE workspace_id = ?",
                TABLE_MEMORIES
            ))?;

            let mut rows = stmt.query([&workspace_id_c])?;
            let mut records = Vec::new();
            while let Some(row) = rows.next()? {
                let record = VecSqliteMemoryStore::deserialize_record(row)?;
                if VecSqliteMemoryStore::row_matches_filters(&workspace_id_c, &record, filters.as_ref())
                    && record.matches_query(&query)
                {
                    records.push(record);
                }
            }
            Ok(records)
        }).await
    }

    async fn hybrid_search(
        &self,
        workspace_id: &str,
        query: &str,
        mode: HybridSearchMode,
        filters: Option<&MemoryQueryFilters>,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        self.perform_hybrid_search(workspace_id, query, mode, filters, limit, None)
            .await
    }

    async fn graph_hops(
        &self,
        workspace_id: &str,
        path_or_id: &str,
        hops: usize,
        query: &str,
    ) -> Result<GraphHopResult> {
        self.perform_graph_hops(workspace_id, path_or_id, hops, query)
            .await
    }

    async fn load_workspace_state(&self, workspace_id: &str) -> Result<DurableWorkspaceState> {
        let memories = self.list(workspace_id).await?;
        let workspace_id = workspace_id.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare("SELECT id, source_id, target_id, relation_type, weight, confidence_score, provenance_id, contradicts_edge_id, created_at, updated_at FROM relations WHERE source_id LIKE ? OR target_id LIKE ?")?;
            let workspace_prefix = format!("entity:{}%", workspace_id);
            let mut rows = stmt
                .query(params![workspace_prefix.clone(), workspace_prefix.clone()])?;

            let mut beliefs = Vec::new();
            while let Some(row) = rows.next()? {
                let weight = row.get::<_, f64>(4)? as f32;
                let confidence_score = row.get::<_, f64>(5)? as f32;
                let provenance_id = row.get::<_, String>(6)?;
                let contradicts_edge_id = row.get::<_, Option<String>>(7)?;
                beliefs.push(BeliefEdge {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    target: row.get(2)?,
                    relation_type: row.get(3)?,
                    weight,
                    confidence_score,
                    provenance_id,
                    contradicts_edge_id,
                    created_at: chrono::DateTime::parse_from_rfc3339(
                        &row.get::<_, String>(8)?,
                    )
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                    updated_at: chrono::DateTime::parse_from_rfc3339(
                        &row.get::<_, String>(9)?,
                    )
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                });
            }

            Ok(DurableWorkspaceState {
                memories,
                beliefs,
                session_tokens: Vec::new(),
                checkpoints: Vec::new(),
            })
        }).await
    }

    async fn save_beliefs(&self, _workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()> {
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            for belief in beliefs {
                conn.execute(
                    "INSERT OR REPLACE INTO relations (id, source_id, target_id, relation_type, weight, confidence_score, provenance_id, contradicts_edge_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        belief.id,
                        belief.source,
                        belief.target,
                        belief.relation_type,
                        belief.weight,
                        belief.confidence_score,
                        belief.provenance_id,
                        belief.contradicts_edge_id,
                        belief.created_at.to_rfc3339(),
                        belief.updated_at.to_rfc3339(),
                    ],
                )?;
            }
            Ok(())
        }).await
    }

    async fn save_session_token(
        &self,
        workspace_id: &str,
        token: SessionTokenRecord,
    ) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO session_tokens (token, workspace_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
                params![
                    token.token,
                    workspace_id,
                    token.created_at.to_rfc3339(),
                    token.expires_at.to_rfc3339(),
                ],
            )?;
            Ok(())
        }).await
    }

    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool> {
        let workspace_id = workspace_id.to_string();
        let token = token.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare("SELECT COUNT(*) FROM session_tokens WHERE token = ? AND workspace_id = ? AND expires_at > ?")?;
            let mut rows = stmt
                .query(params![
                    token,
                    workspace_id,
                    chrono::Utc::now().to_rfc3339()
                ])?;
            let valid = if let Some(row) = rows.next()? {
                row.get::<_, i64>(0).unwrap_or(0) > 0
            } else {
                false
            };
            Ok(valid)
        }).await
    }

    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (id, workspace_id, task_id, name, data, created_at) VALUES (?, ?, ?, ?, ?, ?)",
                    TABLE_CHECKPOINTS
                ),
                params![
                    ulid::Ulid::new().to_string(),
                    workspace_id,
                    checkpoint.task_id,
                    checkpoint.name,
                    serde_json::to_string(&checkpoint.data).unwrap_or_default(),
                    chrono::Utc::now().to_rfc3339(),
                ],
            )?;
            Ok(())
        }).await
    }

    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        let workspace_id = workspace_id.to_string();
        let task_id = task_id.to_string();
        let name = name.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, task_id, name, data, created_at FROM {} WHERE workspace_id = ? AND task_id = ? AND name = ?",
                TABLE_CHECKPOINTS
            ))?;

            let mut rows = stmt.query(params![workspace_id, task_id, name])?;
            if let Some(row) = rows.next()? {
                let data_str: String = row.get(3)?;
                Ok(Some(Checkpoint {
                    task_id: row.get(1)?,
                    name: row.get(2)?,
                    data: serde_json::from_str(&data_str)
                        .unwrap_or_default(),
                }))
            } else {
                Ok(None)
            }
        }).await
    }

    async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>> {
        let workspace_id = workspace_id.to_string();
        let task_id = task_id.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, task_id, name, data, created_at FROM {} WHERE workspace_id = ? AND task_id = ?",
                TABLE_CHECKPOINTS
            ))?;

            let mut rows = stmt.query(params![workspace_id, task_id])?;
            let mut result = Vec::new();
            while let Some(row) = rows.next()? {
                let data_str: String = row.get(3)?;
                result.push(Checkpoint {
                    task_id: row.get(1)?,
                    name: row.get(2)?,
                    data: serde_json::from_str(&data_str)
                        .unwrap_or_default(),
                });
            }
            Ok(result)
        }).await
    }

    async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        let task_id = task_id.to_string();
        let name = name.to_string();

        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                conn.execute(
                    &format!(
                        "DELETE FROM {} WHERE workspace_id = ? AND task_id = ? AND name = ?",
                        TABLE_CHECKPOINTS
                    ),
                    params![workspace_id, task_id, name],
                )?;
                Ok(())
            })
            .await
    }

    async fn list_timeline_events(
        &self,
        workspace_id: &str,
        since: &str,
    ) -> Result<Vec<crate::server::events::RealtimeEvent>> {
        self.perform_list_timeline_events(workspace_id, since).await
    }

    async fn cleanup_orphans(&self) -> Result<usize> {
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                let tx = conn.unchecked_transaction()?;

                // Find orphaned embeddings (id in memory_embeddings but not in memory_records)
                let orphans: usize = {
                    let mut stmt = tx.prepare(
                        "SELECT COUNT(*) FROM memory_embeddings WHERE id NOT IN (SELECT id FROM memory_records)"
                    )?;
                    stmt.query_row((), |row| row.get(0)).unwrap_or(0)
                };

                if orphans > 0 {
                    tx.execute(
                        "DELETE FROM memory_embeddings WHERE id NOT IN (SELECT id FROM memory_records)",
                        ()
                    )?;
                }

                // Cleanup orphaned graph entities and relations
                tx.execute(
                    "DELETE FROM relations WHERE source_id NOT IN (SELECT id FROM entities) OR target_id NOT IN (SELECT id FROM entities)",
                    ()
                )?;

                tx.commit()?;
                Ok(orphans)
            })
            .await
    }

    async fn ls(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<Vec<crate::memory::hierarchy::MemoryHierarchyNode>> {
        let all = self.list(workspace_id).await?;
        Ok(crate::memory::hierarchy::MemoryTree::build_ls(all, path))
    }
}
