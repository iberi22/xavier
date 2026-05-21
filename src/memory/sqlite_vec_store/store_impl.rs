use std::any::Any;
use anyhow::{Context, Result};
use async_trait::async_trait;
use libsql::params;
use sha2::{Digest, Sha256};

use crate::checkpoint::Checkpoint;
use crate::domain::memory::belief::BeliefEdge;
use crate::memory::schema::MemoryQueryFilters;
use crate::memory::store::{
    DurableWorkspaceState, GraphHopResult, HybridSearchMode,
    HybridSearchResult, MemoryBackend, MemoryRecord, MemoryStore, SessionTokenRecord,
};
use crate::memory::sqlite_store::{
    TABLE_CHECKPOINTS, TABLE_MEMORIES,
};

use super::{VecSqliteMemoryStore, vector, graph};

#[async_trait]
impl MemoryStore for VecSqliteMemoryStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Vec
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn health(&self) -> Result<String> {
        let conn = self.pool.get().await?;
        conn.query("SELECT 1", ()).await?;
        Ok(format!("vecsqlite {}", self.config.detail()))
    }

    async fn put(&self, record: MemoryRecord) -> Result<()> {
        let conn = self.pool.get().await?;

        // Compute content hash for tamper-evident hash chain
        let content_hash = format!("{:x}", Sha256::digest(record.content.as_bytes()));

        // Get the previous hash for chain linking
        let prev_hash: Option<String> = {
            let mut stmt = conn.prepare("SELECT content_hash FROM memory_chain ORDER BY created_at DESC LIMIT 1").await?;
            let mut rows = stmt.query(()).await?;
            if let Some(row) = rows.next().await? {
                row.get::<Option<String>>(0).ok().flatten()
            } else {
                None
            }
        };

        // Store in main table first
        {
            let embedding_blob = if !record.embedding.is_empty()
                && Self::qjl_enabled_for_workspace(&conn, &record.workspace_id).await
            {
                vector::serialize_embedding_qjl(&record.embedding)
            } else {
                vector::serialize_embedding(&record.embedding)
            };

            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    TABLE_MEMORIES
                ),
                params![
                    record.id.clone(),
                    record.workspace_id.clone(),
                    record.path.clone(),
                    record.content.clone(),
                    serde_json::to_string(&record.metadata).unwrap_or_default(),
                    embedding_blob,
                    record.created_at.to_rfc3339(),
                    record.updated_at.to_rfc3339(),
                    record.revision,
                    record.primary as i32,
                    record.parent_id.clone(),
                    record.cluster_id.clone(),
                    record.level.as_str(),
                    serde_json::to_string(&record.relation).unwrap_or_default(),
                    serde_json::to_string(&record.revisions).unwrap_or_default(),
                ],
            ).await?;

            // Sync to FTS5
            conn.execute("DELETE FROM memory_fts WHERE id = ?", params![record.id.clone()]).await?;
            let code_tokens =
                super::fts::code_tokens(&format!("{} {}", &record.path, &record.content)).join(" ");
            conn.execute(
                "INSERT INTO memory_fts(id, path, content, code_tokens) VALUES (?, ?, ?, ?)",
                params![record.id.clone(), record.path.clone(), record.content.clone(), code_tokens],
            ).await?;

            Self::sync_memory_entities(&conn, &record.workspace_id, &record).await?;

            // Add to hash chain
            let chain_id = ulid::Ulid::new().to_string();
            conn.execute(
                "INSERT INTO memory_chain (id, prev_hash, content_hash) VALUES (?, ?, ?)",
                params![chain_id, prev_hash, content_hash],
            ).await?;

            self.append_timeline_event(&conn, &record.workspace_id, &record).await?;
        }

        // Store vector in native vector search table
        if !record.embedding.is_empty() {
            self.upsert_vector(&record.id, &record.workspace_id, &record.embedding).await?;
        }

        Ok(())
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let conn = self.pool.get().await?;

        // Try by id first (O(1) lookup)
        let key = Self::row_key(workspace_id, id_or_path);
        let mut stmt = conn.prepare(&format!(
            "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE id = ? LIMIT 1",
            TABLE_MEMORIES
        )).await?;

        let mut rows = stmt.query([key.as_str()]).await?;
        if let Some(row) = rows.next().await? {
            return Ok(Some(Self::deserialize_record(&row)?));
        }

        // Fallback: try by path
        let mut stmt = conn.prepare(&format!(
            "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE workspace_id = ? AND path = ?",
            TABLE_MEMORIES
        )).await?;

        let mut rows = stmt.query(params![workspace_id, id_or_path]).await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(Self::deserialize_record(&row)?))
        } else {
            Ok(None)
        }
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
            let conn = self.pool.get().await?;
            let tx = conn.transaction().await?;

            // Remove dependent rows first to satisfy foreign keys.
            tx.execute(
                "DELETE FROM memory_entities WHERE workspace_id = ? AND memory_id = ?",
                params![workspace_id, record.id.clone()],
            )
            .await
            .context("delete memory_entities for parent record")?;

            tx.execute(
                "DELETE FROM memory_entities WHERE workspace_id = ? AND memory_id IN (
                    SELECT id FROM memory_records WHERE workspace_id = ? AND parent_id = ?
                )",
                params![workspace_id, workspace_id, record.id.clone()],
            )
            .await
            .context("delete memory_entities for child records")?;

            // Delete from vector table
            tx.execute(
                "DELETE FROM memory_embeddings WHERE id = ? AND workspace_id = ?",
                params![record.id.clone(), workspace_id],
            )
            .await
            .context("delete memory_embeddings for parent record")?;

            tx.execute(
                "DELETE FROM memory_embeddings WHERE workspace_id = ? AND id IN (
                    SELECT id FROM memory_records WHERE workspace_id = ? AND parent_id = ?
                )",
                params![workspace_id, workspace_id, record.id.clone()],
            )
            .await
            .context("delete memory_embeddings for child records")?;

            // Delete from FTS5
            tx.execute("DELETE FROM memory_fts WHERE id = ?", params![record.id.clone()])
                .await
                .context("delete memory_fts for parent record")?;

            tx.execute(
                "DELETE FROM memory_fts WHERE id IN (
                    SELECT id FROM memory_records WHERE workspace_id = ? AND parent_id = ?
                )",
                params![workspace_id, record.id.clone()],
            )
            .await
            .context("delete memory_fts for child records")?;

            let memory_node_id = graph::memory_node_id(workspace_id, &record.id);

            tx.execute(
                "DELETE FROM relations WHERE source_id = ?",
                params![memory_node_id.clone()],
            )
            .await
            .context("delete relations where memory node is source")?;

            tx.execute(
                "DELETE FROM relations WHERE target_id = ?",
                params![memory_node_id.clone()],
            )
            .await
            .context("delete relations where memory node is target")?;

            tx.execute(
                "DELETE FROM entities WHERE id = ?",
                params![memory_node_id.clone()],
            )
            .await
            .context("delete memory node entity")?;

            // Remove child memories before parent.
            tx.execute(
                &format!(
                    "DELETE FROM {} WHERE workspace_id = ? AND parent_id = ?",
                    TABLE_MEMORIES
                ),
                params![workspace_id, record.id.clone()],
            )
            .await
            .context("delete child memory records")?;

            tx.execute(
                &format!(
                    "DELETE FROM {} WHERE workspace_id = ? AND id = ?",
                    TABLE_MEMORIES
                ),
                params![workspace_id, record.id.clone()],
            )
            .await
            .context("delete parent memory record")?;

            tx.commit().await.context("commit memory delete transaction")?;
        }
        Ok(removed)
    }

    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        let conn = self.pool.get().await?;
        let mut stmt = conn.prepare(&format!(
            "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE workspace_id = ?",
            TABLE_MEMORIES
        )).await?;

        let mut rows = stmt.query([workspace_id]).await?;
        let mut records = Vec::new();
        while let Some(row) = rows.next().await? {
            records.push(Self::deserialize_record(&row)?);
        }
        Ok(records)
    }

    async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>> {
        let conn = self.pool.get().await?;
        let mut stmt = conn.prepare(&format!(
            "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE workspace_id = ?",
            TABLE_MEMORIES
        )).await?;

        let mut rows = stmt.query([workspace_id]).await?;
        let mut records = Vec::new();
        while let Some(row) = rows.next().await? {
            let record = Self::deserialize_record(&row)?;
            if Self::row_matches_filters(workspace_id, &record, filters) && record.matches_query(query) {
                records.push(record);
            }
        }
        Ok(records)
    }

    async fn hybrid_search(
        &self,
        workspace_id: &str,
        query: &str,
        mode: HybridSearchMode,
        filters: Option<&MemoryQueryFilters>,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>> {
        self.perform_hybrid_search(workspace_id, query, mode, filters, limit, None).await
    }

    async fn graph_hops(
        &self,
        workspace_id: &str,
        path_or_id: &str,
        hops: usize,
        query: &str,
    ) -> Result<GraphHopResult> {
        self.perform_graph_hops(workspace_id, path_or_id, hops, query).await
    }

    async fn load_workspace_state(&self, workspace_id: &str) -> Result<DurableWorkspaceState> {
        let memories = self.list(workspace_id).await?;
        let conn = self.pool.get().await?;

        let mut stmt = conn.prepare(&format!("SELECT id, source_id, target_id, relation_type, weight, confidence_score, provenance_id, contradicts_edge_id, created_at, updated_at FROM {} WHERE source_id LIKE ? OR target_id LIKE ?", "relations")).await?;
        let workspace_prefix = format!("entity:{}%", workspace_id);
        let mut rows = stmt.query(params![workspace_prefix.clone(), workspace_prefix.clone()]).await?;

        let mut beliefs = Vec::new();
        while let Some(row) = rows.next().await? {
            let weight = row.get::<f64>(4).map_err(anyhow::Error::msg)? as f32;
            let confidence_score = row.get::<f64>(5).map_err(anyhow::Error::msg)? as f32;
            let provenance_id = row.get::<String>(6).map_err(anyhow::Error::msg)?;
            let contradicts_edge_id = row.get::<Option<String>>(7).ok().flatten();
            beliefs.push(BeliefEdge {
                id: row.get(0).map_err(anyhow::Error::msg)?,
                source: row.get(1).map_err(anyhow::Error::msg)?,
                target: row.get(2).map_err(anyhow::Error::msg)?,
                relation_type: row.get(3).map_err(anyhow::Error::msg)?,
                weight,
                confidence_score,
                provenance_id,
                contradicts_edge_id,
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<String>(8).map_err(anyhow::Error::msg)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<String>(9).map_err(anyhow::Error::msg)?)
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
    }

    async fn save_beliefs(&self, _workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()> {
        let conn = self.pool.get().await?;
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
            ).await?;
        }
        Ok(())
    }

    async fn save_session_token(
        &self,
        workspace_id: &str,
        token: SessionTokenRecord,
    ) -> Result<()> {
        let conn = self.pool.get().await?;
        conn.execute(
            "INSERT OR REPLACE INTO session_tokens (token, workspace_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
            params![
                token.token,
                workspace_id,
                token.created_at.to_rfc3339(),
                token.expires_at.to_rfc3339(),
            ],
        ).await?;
        Ok(())
    }

    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool> {
        let conn = self.pool.get().await?;
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM session_tokens WHERE token = ? AND workspace_id = ? AND expires_at > ?").await?;
        let mut rows = stmt.query(params![token, workspace_id, chrono::Utc::now().to_rfc3339()]).await?;
        let valid = if let Some(row) = rows.next().await? {
            row.get::<i64>(0).unwrap_or(0) > 0
        } else {
            false
        };
        Ok(valid)
    }

    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()> {
        let conn = self.pool.get().await?;
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
        ).await?;
        Ok(())
    }

    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        let conn = self.pool.get().await?;
        let mut stmt = conn.prepare(&format!(
            "SELECT id, task_id, name, data, created_at FROM {} WHERE workspace_id = ? AND task_id = ? AND name = ?",
            TABLE_CHECKPOINTS
        )).await?;

        let mut rows = stmt.query(params![workspace_id, task_id, name]).await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(Checkpoint {
                task_id: row.get(1).map_err(anyhow::Error::msg)?,
                name: row.get(2).map_err(anyhow::Error::msg)?,
                data: serde_json::from_str(&row.get::<String>(3).map_err(anyhow::Error::msg)?).unwrap_or_default(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>> {
        let conn = self.pool.get().await?;
        let mut stmt = conn.prepare(&format!(
            "SELECT id, task_id, name, data, created_at FROM {} WHERE workspace_id = ? AND task_id = ?",
            TABLE_CHECKPOINTS
        )).await?;

        let mut rows = stmt.query(params![workspace_id, task_id]).await?;
        let mut result = Vec::new();
        while let Some(row) = rows.next().await? {
            result.push(Checkpoint {
                task_id: row.get(1).map_err(anyhow::Error::msg)?,
                name: row.get(2).map_err(anyhow::Error::msg)?,
                data: serde_json::from_str(&row.get::<String>(3).map_err(anyhow::Error::msg)?).unwrap_or_default(),
            });
        }
        Ok(result)
    }

    async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<()> {
        let conn = self.pool.get().await?;
        conn.execute(
            &format!("DELETE FROM {} WHERE workspace_id = ? AND task_id = ? AND name = ?", TABLE_CHECKPOINTS),
            params![workspace_id, task_id, name],
        ).await?;
        Ok(())
    }

    async fn list_timeline_events(
        &self,
        workspace_id: &str,
        since: &str,
    ) -> Result<Vec<crate::server::events::RealtimeEvent>> {
        self.perform_list_timeline_events(workspace_id, since).await
    }
}
