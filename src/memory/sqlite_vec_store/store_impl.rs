use std::any::Any;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rusqlite::params;
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

use super::{VecSqliteMemoryStore, vector, fts, graph};

#[async_trait]
impl MemoryStore for VecSqliteMemoryStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Vec
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn health(&self) -> Result<String> {
        let conn = self.conn.lock();
        conn.query_row("SELECT 1", [], |_row| Ok(()))?;
        Ok(format!("vecsqlite {}", self.config.detail()))
    }

    async fn put(&self, record: MemoryRecord) -> Result<()> {
        // Compute content hash for tamper-evident hash chain
        let content_hash = format!("{:x}", Sha256::digest(record.content.as_bytes()));

        // Get the previous hash for chain linking
        let prev_hash: Option<String> = {
            let conn = self.conn.lock();
            conn.query_row(
                "SELECT content_hash FROM memory_chain ORDER BY created_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok()
        };

        // Store in main table first
        {
            let conn = self.conn.lock();
            let embedding_blob = if !record.embedding.is_empty()
                && Self::qjl_enabled_for_workspace(&conn, &record.workspace_id)
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
                    record.id,
                    record.workspace_id,
                    record.path,
                    record.content,
                    serde_json::to_string(&record.metadata).unwrap_or_default(),
                    embedding_blob,
                    record.created_at.to_rfc3339(),
                    record.updated_at.to_rfc3339(),
                    record.revision,
                    record.primary as i32,
                    record.parent_id,
                    record.cluster_id,
                    record.level.as_str(),
                    serde_json::to_string(&record.relation).unwrap_or_default(),
                    serde_json::to_string(&record.revisions).unwrap_or_default(),
                ],
            )?;

            // Sync to FTS5
            conn.execute("DELETE FROM memory_fts WHERE id = ?", params![&record.id])?;
            let code_tokens =
                fts::code_tokens(&format!("{} {}", &record.path, &record.content)).join(" ");
            conn.execute(
                "INSERT INTO memory_fts(id, path, content, code_tokens) VALUES (?, ?, ?, ?)",
                params![&record.id, &record.path, &record.content, code_tokens],
            )?;

            Self::sync_memory_entities(&conn, &record.workspace_id, &record)?;

            // Add to hash chain
            let chain_id = ulid::Ulid::new().to_string();
            conn.execute(
                "INSERT INTO memory_chain (id, prev_hash, content_hash) VALUES (?, ?, ?)",
                params![chain_id, prev_hash, content_hash],
            )?;
            self.append_timeline_event(&conn, &record.workspace_id, &record)?;
        }

        // Store vector in sqlite-vec virtual table
        if !record.embedding.is_empty() {
            self.upsert_vector(&record.id, &record.workspace_id, &record.embedding)?;
        }

        Ok(())
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let conn = self.conn.lock();

        // Try by id first (O(1) lookup)
        let key = Self::row_key(workspace_id, id_or_path);
        let mut stmt = conn.prepare(&format!(
            "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE id = ? LIMIT 1",
            TABLE_MEMORIES
        ))?;

        let mut rows = stmt.query([&key])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(Self::deserialize_record(row)?));
        }
        drop(rows);
        drop(stmt);

        // Fallback: try by path
        let mut stmt = conn.prepare(&format!(
            "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE workspace_id = ? AND path = ?",
            TABLE_MEMORIES
        ))?;

        let mut rows = stmt.query(params![workspace_id, id_or_path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::deserialize_record(row)?))
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
            let conn = self.conn.lock();
            let tx = conn.unchecked_transaction()?;

            // Remove dependent rows first to satisfy foreign keys.
            tx.execute(
                "DELETE FROM memory_entities WHERE workspace_id = ? AND memory_id = ?",
                params![workspace_id, &record.id],
            )
            .context("delete memory_entities for parent record")?;
            tx.execute(
                "DELETE FROM memory_entities WHERE workspace_id = ? AND memory_id IN (
                    SELECT id FROM memory_records WHERE workspace_id = ? AND parent_id = ?
                )",
                params![workspace_id, workspace_id, &record.id],
            )
            .context("delete memory_entities for child records")?;

            // Delete from vector table
            tx.execute(
                "DELETE FROM memory_embeddings WHERE id = ? AND workspace_id = ?",
                params![&record.id, workspace_id],
            )
            .context("delete memory_embeddings for parent record")?;
            tx.execute(
                "DELETE FROM memory_embeddings WHERE workspace_id = ? AND id IN (
                    SELECT id FROM memory_records WHERE workspace_id = ? AND parent_id = ?
                )",
                params![workspace_id, workspace_id, &record.id],
            )
            .context("delete memory_embeddings for child records")?;

            // Delete from FTS5
            tx.execute("DELETE FROM memory_fts WHERE id = ?", params![&record.id])
                .context("delete memory_fts for parent record")?;
            tx.execute(
                "DELETE FROM memory_fts WHERE id IN (
                    SELECT id FROM memory_records WHERE workspace_id = ? AND parent_id = ?
                )",
                params![workspace_id, &record.id],
            )
            .context("delete memory_fts for child records")?;

            let memory_node_id = graph::memory_node_id(workspace_id, &record.id);
            tx.execute(
                "DELETE FROM relations WHERE source_id = ?",
                params![&memory_node_id],
            )
            .context("delete relations where memory node is source")?;
            tx.execute(
                "DELETE FROM relations WHERE target_id = ?",
                params![&memory_node_id],
            )
            .context("delete relations where memory node is target")?;
            tx.execute(
                "DELETE FROM entities WHERE id = ?",
                params![&memory_node_id],
            )
            .context("delete memory node entity")?;

            // Remove child memories before parent.
            tx.execute(
                &format!(
                    "DELETE FROM {} WHERE workspace_id = ? AND parent_id = ?",
                    TABLE_MEMORIES
                ),
                params![workspace_id, &record.id],
            )
            .context("delete child memory records")?;
            tx.execute(
                &format!(
                    "DELETE FROM {} WHERE workspace_id = ? AND id = ?",
                    TABLE_MEMORIES
                ),
                params![workspace_id, &record.id],
            )
            .context("delete parent memory record")?;
            tx.commit().context("commit memory delete transaction")?;
        }
        Ok(removed)
    }

    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE workspace_id = ?",
            TABLE_MEMORIES
        ))?;

        let rows = stmt.query_map([workspace_id], Self::deserialize_record)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM {} WHERE workspace_id = ?",
            TABLE_MEMORIES
        ))?;

        let rows = stmt.query_map([workspace_id], Self::deserialize_record)?;
        let mut records = Vec::new();
        for row in rows {
            let record = row?;
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
        self.perform_hybrid_search(workspace_id, query, mode, filters, limit).await
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
        let conn = self.conn.lock();

        let mut stmt = conn.prepare(&format!("SELECT id, source_id, target_id, relation_type, weight, confidence_score, provenance_id, contradicts_edge_id, created_at, updated_at FROM {} WHERE source_id LIKE ? OR target_id LIKE ?", "relations"))?;
        let workspace_prefix = format!("entity:{}%", workspace_id);
        let belief_rows = stmt.query_map(params![workspace_prefix, workspace_prefix], |row| {
            Ok(BeliefEdge {
                id: row.get(0)?,
                source: row.get(1)?,
                target: row.get(2)?,
                relation_type: row.get(3)?,
                weight: row.get(4)?,
                confidence_score: row.get(5)?,
                provenance_id: row.get(6)?,
                contradicts_edge_id: row.get(7)?,
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
        })?;

        let mut beliefs = Vec::new();
        for b in belief_rows {
            beliefs.push(b?);
        }

        Ok(DurableWorkspaceState {
            memories,
            beliefs,
            session_tokens: Vec::new(),
            checkpoints: Vec::new(),
        })
    }

    async fn save_beliefs(&self, _workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()> {
        let conn = self.conn.lock();
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
    }

    async fn save_session_token(
        &self,
        workspace_id: &str,
        token: SessionTokenRecord,
    ) -> Result<()> {
        let conn = self.conn.lock();
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
    }

    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool> {
        let conn = self.conn.lock();
        let valid: bool = conn.query_row(
            "SELECT COUNT(*) FROM session_tokens WHERE token = ? AND workspace_id = ? AND expires_at > ?",
            params![token, workspace_id, chrono::Utc::now().to_rfc3339()],
            |row| row.get::<_, i64>(0).map(|count| count > 0),
        )?;
        Ok(valid)
    }

    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            &format!(
                "INSERT OR REPLACE INTO {} (workspace_id, task_id, name, data) VALUES (?, ?, ?, ?)",
                TABLE_CHECKPOINTS
            ),
            params![
                workspace_id,
                checkpoint.task_id,
                checkpoint.name,
                serde_json::to_string(&checkpoint.data).unwrap_or_default(),
            ],
        )?;
        Ok(())
    }

    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT id, task_id, name, data, created_at FROM {} WHERE workspace_id = ? AND task_id = ? AND name = ?",
            TABLE_CHECKPOINTS
        ))?;

        match stmt.query_row(params![workspace_id, task_id, name], |row| {
            Ok(Checkpoint {
                task_id: row.get(1)?,
                name: row.get(2)?,
                data: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
            })
        }) {
            Ok(cp) => Ok(Some(cp)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT id, task_id, name, data, created_at FROM {} WHERE workspace_id = ? AND task_id = ?",
            TABLE_CHECKPOINTS
        ))?;

        let rows = stmt.query_map(params![workspace_id, task_id], |row| {
            Ok(Checkpoint {
                task_id: row.get(1)?,
                name: row.get(2)?,
                data: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
            })
        })?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            &format!("DELETE FROM {} WHERE workspace_id = ? AND task_id = ? AND name = ?", TABLE_CHECKPOINTS),
            params![workspace_id, task_id, name],
        )?;
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
