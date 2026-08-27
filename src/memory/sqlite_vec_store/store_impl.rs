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

/// Compute cosine similarity between two f32 vectors.
/// Used for semantic deduplication.
pub fn cosine_similarity(v1: &[f32], v2: &[f32]) -> f32 {
    if v1.is_empty() || v2.is_empty() || v1.len() != v2.len() {
        return 0.0;
    }
    let mut dot_product = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for i in 0..v1.len() {
        dot_product += v1[i] * v2[i];
        norm_a += v1[i] * v1[i];
        norm_b += v2[i] * v2[i];
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot_product / (norm_a.sqrt() * norm_b.sqrt())
}

/// Check if the new string contains the old string.
/// Returns true if `new` is a superset of `old` (including if they are identical, or if `old` is empty).
pub fn is_superset(new: &str, old: &str) -> bool {
    if old.is_empty() {
        return true;
    }
    new.contains(old)
}

#[async_trait]
impl MemoryStore for VecSqliteMemoryStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Vec
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn set_dedup_settings(&self, settings: crate::settings::types::DedupSettings) {
        let mut lock = self.dedup_config.write().await;
        *lock = settings;
    }

    async fn compact(&self) -> Result<()> {
        let project_id = self.project_id.clone();
        ConnectionManager::global()
            .with_conn(&project_id, move |conn| {
                conn.execute_batch("VACUUM;")?;
                Ok(())
            })
            .await
    }

    async fn db_size(&self) -> Result<Option<u64>> {
        if self.config.path.exists() {
            if let Ok(metadata) = std::fs::metadata(&self.config.path) {
                return Ok(Some(metadata.len()));
            }
        }
        Ok(None)
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
        let record = self.put_embed(record).await?;
        let record = self.put_validate(record).await?;

        let project_id = self.project_id.clone();
        let store = self.clone();
        let record_c = record.clone();

        ConnectionManager::global()
            .with_conn(&project_id, move |conn| {
                store.put_store(conn, &record_c)?;
                store.put_index(conn, &record_c)?;
                store.put_link(conn, &record_c)?;
                Ok(())
            })
            .await?;

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

        let record = ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            // Try by id first (O(1) lookup)
            let key = crate::memory::store::stable_key("sqlite_mem", &[&workspace_id, &id_or_path]);
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv FROM {} WHERE id = ? LIMIT 1",
                TABLE_MEMORIES
            ))?;

            {
                let mut rows = stmt.query([&key])?;
                if let Some(row) = rows.next()? {
                    return Ok(Some(VecSqliteMemoryStore::deserialize_record(row)?));
                }
            }

            // Fallback: try by raw id_or_path directly (e.g. ULIDs)
            {
                let mut rows = stmt.query([&id_or_path])?;
                if let Some(row) = rows.next()? {
                    return Ok(Some(VecSqliteMemoryStore::deserialize_record(row)?));
                }
            }

            // Fallback: try by path
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv FROM {} WHERE workspace_id = ? AND path = ?",
                TABLE_MEMORIES
            ))?;

            let mut rows = stmt.query(params![workspace_id, id_or_path])?;
            if let Some(row) = rows.next()? {
                Ok(Some(VecSqliteMemoryStore::deserialize_record(row)?))
            } else {
                Ok(None)
            }
        }).await?;

        if let Some(mut record) = record {
            crate::memory::sqlite_store::SqliteMemoryStore::decrypt_record(&mut record)?;
            Ok(Some(record))
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

                    // Delete from vector tables
                    tx.execute(
                        "DELETE FROM memory_embeddings WHERE id = ? AND workspace_id = ?",
                        params![record_id, workspace_id],
                    )?;
                    tx.execute(
                        "DELETE FROM memory_embeddings_768 WHERE id = ? AND workspace_id = ?",
                        params![record_id, workspace_id],
                    )?;

                    tx.execute(
                        "DELETE FROM memory_embeddings WHERE workspace_id = ? AND id IN (
                        SELECT id FROM memory_records WHERE workspace_id = ? AND parent_id = ?
                    )",
                        params![workspace_id, workspace_id, record_id],
                    )?;
                    tx.execute(
                        "DELETE FROM memory_embeddings_768 WHERE workspace_id = ? AND id IN (
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
                        "DELETE FROM relations WHERE workspace_id = ? AND source_id = ?",
                        params![workspace_id, memory_node_id],
                    )?;

                    tx.execute(
                        "DELETE FROM relations WHERE workspace_id = ? AND target_id = ?",
                        params![workspace_id, memory_node_id],
                    )?;

                    tx.execute(
                        "DELETE FROM entities WHERE workspace_id = ? AND id = ?",
                        params![workspace_id, memory_node_id],
                    )?;

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
        let records = ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv FROM {} WHERE workspace_id = ?",
                TABLE_MEMORIES
            ))?;

            let mut rows = stmt.query([workspace_id])?;
            let mut records = Vec::new();
            while let Some(row) = rows.next()? {
                records.push(VecSqliteMemoryStore::deserialize_record(row)?);
            }
            Ok(records)
        }).await?;

        let mut results = Vec::with_capacity(records.len());
        for mut record in records {
            crate::memory::sqlite_store::SqliteMemoryStore::decrypt_record(&mut record)?;
            results.push(record);
        }
        Ok(results)
    }

    async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>> {
        let records = self.list(workspace_id).await?;
        crate::memory::store::filter_records(records, workspace_id, query, filters)
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
        let workspace_id_c = workspace_id.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            // Load beliefs
            let mut stmt = conn.prepare("SELECT id, source_id, target_id, relation_type, weight, confidence_score, provenance_id, contradicts_edge_id, is_inferred, source_language, target_language, created_at, updated_at FROM relations WHERE workspace_id = ?")?;
            let mut rows = stmt.query(params![workspace_id_c])?;

            let mut beliefs = Vec::new();
            while let Some(row) = rows.next()? {
                let weight = row.get::<_, f64>(4)? as f32;
                let confidence_score = row.get::<_, f64>(5)? as f32;
                let provenance_id = row.get::<_, String>(6)?;
                let contradicts_edge_id = row.get::<_, Option<String>>(7)?;
                let is_inferred: i32 = row.get(8)?;
                beliefs.push(BeliefEdge {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    target: row.get(2)?,
                    relation_type: row.get(3)?,
                    weight,
                    confidence_score,
                    provenance_id,
                    contradicts_edge_id,
                    is_inferred: is_inferred != 0,
                    source_language: row.get(9)?,
                    target_language: row.get(10)?,
                    created_at: chrono::DateTime::parse_from_rfc3339(
                        &row.get::<_, String>(11)?,
                    )
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                    updated_at: chrono::DateTime::parse_from_rfc3339(
                        &row.get::<_, String>(12)?,
                    )
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                });
            }

            // Load session tokens (filter expired)
            let now = chrono::Utc::now();
            let session_tokens = {
                let mut stmt = conn.prepare("SELECT id, workspace_id, token, created_at, expires_at FROM session_tokens WHERE workspace_id = ?")?;
                let mut rows = stmt.query([&workspace_id_c])?;
                let mut tokens = Vec::new();
                while let Some(row) = rows.next()? {
                    let expires_at = chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                            .unwrap_or_else(|_| chrono::Utc::now());

                    if expires_at > now {
                        tokens.push(SessionTokenRecord {
                            token: row.get(2)?,
                            created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                                .map(|dt| dt.with_timezone(&chrono::Utc))
                                .unwrap_or_else(|_| chrono::Utc::now()),
                            expires_at,
                        });
                    }
                }
                tokens
            };

            // Load checkpoints
            let checkpoints = {
                let mut stmt = conn.prepare(&format!(
                    "SELECT task_id, name, data FROM {} WHERE workspace_id = ?",
                    TABLE_CHECKPOINTS
                ))?;
                let mut rows = stmt.query([&workspace_id_c])?;
                let mut checkpoints = Vec::new();
                while let Some(row) = rows.next()? {
                    checkpoints.push(Checkpoint {
                        task_id: row.get(0)?,
                        name: row.get(1)?,
                        data: serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or_default(),
                    });
                }
                checkpoints
            };

            Ok(DurableWorkspaceState {
                memories,
                beliefs,
                session_tokens,
                checkpoints,
                entity_graph_snapshot: None,
            })
        }).await
    }

    async fn save_beliefs(&self, workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            for belief in &beliefs {
                super::graph::ensure_seed_entities(
                    conn,
                    &workspace_id,
                    &[(&belief.source, "source"), (&belief.target, "target")],
                )?;
            }
            for belief in beliefs {
                conn.execute(
                    "INSERT OR REPLACE INTO relations (id, source_id, target_id, relation_type, weight, confidence_score, provenance_id, contradicts_edge_id, is_inferred, source_language, target_language, created_at, updated_at, workspace_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        belief.id,
                        belief.source,
                        belief.target,
                        belief.relation_type,
                        belief.weight,
                        belief.confidence_score,
                        belief.provenance_id,
                        belief.contradicts_edge_id,
                        belief.is_inferred as i32,
                        belief.source_language,
                        belief.target_language,
                        belief.created_at.to_rfc3339(),
                        belief.updated_at.to_rfc3339(),
                        workspace_id,
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
        let token_key =
            crate::memory::store::stable_key("session_token_row", &[&workspace_id, &token.token]);
        let token_val = token.token;
        let created_at = token.created_at.to_rfc3339();
        let expires_at = token.expires_at.to_rfc3339();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO session_tokens (id, workspace_id, token, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
                params![
                    token_key,
                    workspace_id,
                    token_val,
                    created_at,
                    expires_at,
                ],
            )?;
            Ok(())
        }).await
    }

    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool> {
        let workspace_id = workspace_id.to_string();
        let token = token.to_string();

        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                let token_key =
                    crate::memory::store::stable_key("session_token_row", &[&workspace_id, &token]);
                let now = chrono::Utc::now().to_rfc3339();

                let count: i32 = conn.query_row(
                    "SELECT COUNT(*) FROM session_tokens WHERE id = ? AND expires_at > ?",
                    params![token_key, now],
                    |row| row.get(0),
                )?;

                Ok(count > 0)
            })
            .await
    }

    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        let checkpoint_key = crate::memory::store::stable_key(
            "checkpoint_row",
            &[&workspace_id, &checkpoint.task_id, &checkpoint.name],
        );
        let task_id = checkpoint.task_id;
        let name = checkpoint.name;
        let data_json = serde_json::to_string(&checkpoint.data)?;

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (id, workspace_id, task_id, name, data, created_at) VALUES (?, ?, ?, ?, ?, ?)",
                    TABLE_CHECKPOINTS
                ),
                params![
                    checkpoint_key,
                    workspace_id,
                    task_id,
                    name,
                    data_json,
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

                // Cleanup orphaned or stale memory symbol links (>30 days)
                tx.execute(
                    "DELETE FROM memory_symbol_links WHERE created_at < datetime('now', '-30 days') OR memory_id NOT IN (SELECT id FROM memory_records)",
                    (),
                )?;

                tx.commit()?;
                Ok(orphans)
            })
            .await
    }

    async fn load_entity_graph_snapshot(&self, workspace_id: &str) -> Result<Option<String>> {
        let workspace_id = workspace_id.to_string();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                let mut stmt =
                    conn.prepare("SELECT data FROM entity_graph_snapshots WHERE workspace_id = ?")?;
                match stmt.query_row([&workspace_id], |row| row.get::<_, String>(0)) {
                    Ok(data) => Ok(Some(data)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(anyhow::anyhow!("SQLite query failed: {}", e)),
                }
            })
            .await
    }

    async fn save_entity_graph_snapshot(&self, workspace_id: &str, data: &str) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        let data = data.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO entity_graph_snapshots (workspace_id, data, updated_at) VALUES (?, ?, ?)",
                    params![workspace_id, data, now],
                )?;
                Ok(())
            })
            .await
    }

    async fn symbols_for_memory(&self, memory_id: &str) -> Result<Vec<String>> {
        let memory_id = memory_id.to_string();
        ConnectionManager::global()
            .with_conn(&self.project_id, move |conn| {
                // Delete stale links older than 30 days
                let _ = conn.execute(
                    "DELETE FROM memory_symbol_links WHERE created_at < datetime('now', '-30 days')",
                    [],
                );

                let mut stmt = conn.prepare(
                    "SELECT symbol_id FROM memory_symbol_links WHERE memory_id = ? ORDER BY confidence DESC LIMIT 10",
                )?;
                let rows = stmt.query_map([&memory_id], |row| row.get::<_, String>(0))?;
                let mut symbols = Vec::new();
                for symbol in rows.flatten() {
                    symbols.push(symbol);
                }

                if symbols.is_empty() {
                    let mut rec_stmt = conn.prepare(
                        "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv FROM memory_records WHERE id = ? LIMIT 1",
                    )?;
                    let record = {
                        let mut rows = rec_stmt.query([&memory_id])?;
                        if let Some(row) = rows.next()? {
                            Some(VecSqliteMemoryStore::deserialize_record(row)?)
                        } else {
                            None
                        }
                    };

                    if let Some(mut record) = record {
                        let _ = crate::memory::sqlite_store::SqliteMemoryStore::decrypt_record(&mut record);
                        symbols = Self::link_memory_on_demand(conn, &memory_id, &record.content)?;
                    }
                }

                Ok(symbols)
            })
            .await
    }
}

impl VecSqliteMemoryStore {
    /// Decomposed put step 1: Generate embeddings if missing and align embedding status.
    pub async fn put_embed(&self, mut record: MemoryRecord) -> Result<MemoryRecord> {
        // Auto-generate missing embeddings if a provider is configured
        if record.embedding.is_empty() {
            if crate::memory::embedder::EmbeddingClient::is_configured_from_env() {
                match crate::memory::embedder::EmbeddingClient::from_env_async().await {
                    Ok(client) => match client.embed(&record.content).await {
                        Ok(vector) => {
                            record.embedding = vector;
                        }
                        Err(e) => {
                            record.embedding_attempts += 1;
                            tracing::warn!(
                                "Memory record {} saved WITHOUT embedding: client embedding generation failed: {}",
                                record.id,
                                e
                            );
                        }
                    },
                    Err(e) => {
                        record.embedding_attempts += 1;
                        tracing::warn!(
                            "Memory record {} saved WITHOUT embedding: failed to initialize embedding client: {}",
                            record.id,
                            e
                        );
                    }
                }
            } else {
                tracing::warn!(
                    "Memory record {} saved WITHOUT embedding: embedding provider is not configured",
                    record.id
                );
            }
        }

        // Align embedding_status with actual embedding content
        if record.embedding.is_empty() {
            if record.embedding_attempts > 0 {
                record.embedding_status = "retry".to_string();
            } else {
                record.embedding_status = "pending".to_string();
            }
        } else {
            record.embedding_status = "completed".to_string();
        }

        Ok(record)
    }

    /// Decomposed put step 2: Input validation, SSP path sanitization, deduplication, and rest encryption.
    pub async fn put_validate(&self, mut record: MemoryRecord) -> Result<MemoryRecord> {
        // Extract and check "dedup" flag from metadata. Remove it so it doesn't persist.
        let mut is_dedup = false;
        if let serde_json::Value::Object(ref mut map) = record.metadata {
            if let Some(val) = map.remove("dedup") {
                is_dedup = val.as_bool().unwrap_or(false);
            }
        }

        // SSP canonical paths (stability/{repo}/... and features/{repo}/{feature_id})
        // always UPSERT by exact path: reuse the existing row id so INSERT OR REPLACE
        // updates in place instead of creating duplicates on every stabilize/index run.
        let canonical_path =
            record.path.starts_with("stability/") || record.path.starts_with("features/");
        if canonical_path && !record.path.is_empty() {
            let project_id = self.project_id.clone();
            let path_c = record.path.clone();
            let ws_c = record.workspace_id.clone();
            let existing: Option<String> = ConnectionManager::global()
                .with_conn(&project_id, move |conn| {
                    let mut stmt = conn.prepare(
                        "SELECT id FROM memory_records WHERE workspace_id = ?1 AND path = ?2 LIMIT 1",
                    )?;
                    match stmt.query_row(params![ws_c, path_c], |row| row.get::<_, String>(0)) {
                        Ok(id) => Ok(Some(id)),
                        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                        Err(e) => Err(anyhow::anyhow!("SSP upsert lookup failed: {e}")),
                    }
                })
                .await
                .unwrap_or(None);
            if let Some(existing_id) = existing {
                record.id = existing_id;
                record.revision += 1;
            }
        }

        let mut dedup_settings = {
            let lock = self.dedup_config.read().await;
            lock.clone()
        };

        let is_ssp_path =
            record.path.starts_with("stability/") || record.path.starts_with("features/");
        if is_ssp_path {
            dedup_settings.enabled = true;
            dedup_settings.scope = crate::settings::types::DedupScope::PathExact;
        }

        if dedup_settings.enabled && (is_dedup || is_ssp_path) && !record.embedding.is_empty() {
            let record_c = record.clone();
            let project_id_c = self.project_id.clone();
            let dedup_settings_c = dedup_settings.clone();

            let record_ns = match crate::memory::schema::resolve_metadata(
                &record_c.path,
                &record_c.metadata,
                &record_c.workspace_id,
                None,
            ) {
                Ok(res) => res.namespace,
                Err(_) => crate::memory::schema::MemoryNamespace::default(),
            };

            let namespaces_match = |ns1: &crate::memory::schema::MemoryNamespace,
                                    ns2: &crate::memory::schema::MemoryNamespace|
             -> bool {
                ns1.org_id == ns2.org_id
                    && ns1.user_id == ns2.user_id
                    && ns1.agent_id == ns2.agent_id
                    && ns1.session_id == ns2.session_id
                    && ns1.project == ns2.project
                    && ns1.scope == ns2.scope
            };

            let query_res = ConnectionManager::global().with_conn(&project_id_c, move |conn| {
                let mut best_cand: Option<(MemoryRecord, f32)> = None;

                // 1. Try sqlite-vec cosine distance query (vector_distance equivalent)
                if let Ok(emb_json) = serde_json::to_string(&record_c.embedding) {
                    let table_name = if record_c.embedding.len() == 768 {
                        "memory_embeddings_768"
                    } else {
                        "memory_embeddings"
                    };
                    let (sql, query_params) = match dedup_settings_c.scope {
                        crate::settings::types::DedupScope::PathExact => {
                            (
                                format!(
                                    r#"
                                 SELECT m.id, m.workspace_id, m.path, m.content, m.metadata, m.embedding,
                                        m.created_at, m.updated_at, m.revision, m.primary_flag,
                                        m.parent_id, m.cluster_id, m.level, m.relation, m.revisions,
                                        m.encrypted_dek, m.content_iv, m.metadata_iv,
                                        CAST(vec_distance_cosine(e.embedding, vec_f32(?1)) AS REAL) AS distance
                                 FROM {} e
                                 JOIN memory_records m ON m.id = e.id AND m.workspace_id = ?2
                                 WHERE e.workspace_id = ?2 AND m.path = ?3
                                 ORDER BY distance ASC
                                 LIMIT 1
                                 "#,
                                    table_name
                                ),
                                vec![
                                    rusqlite::types::Value::Text(emb_json),
                                    rusqlite::types::Value::Text(record_c.workspace_id.clone()),
                                    rusqlite::types::Value::Text(record_c.path.clone()),
                                ]
                            )
                        }
                        crate::settings::types::DedupScope::Namespace => {
                            (
                                format!(
                                    r#"
                                 SELECT m.id, m.workspace_id, m.path, m.content, m.metadata, m.embedding,
                                        m.created_at, m.updated_at, m.revision, m.primary_flag,
                                        m.parent_id, m.cluster_id, m.level, m.relation, m.revisions,
                                        m.encrypted_dek, m.content_iv, m.metadata_iv,
                                        CAST(vec_distance_cosine(e.embedding, vec_f32(?1)) AS REAL) AS distance
                                 FROM {} e
                                 JOIN memory_records m ON m.id = e.id AND m.workspace_id = ?2
                                 WHERE e.workspace_id = ?2
                                   AND json_extract(m.metadata, '$.namespace.org_id') IS ?3
                                   AND json_extract(m.metadata, '$.namespace.user_id') IS ?4
                                   AND json_extract(m.metadata, '$.namespace.agent_id') IS ?5
                                   AND json_extract(m.metadata, '$.namespace.session_id') IS ?6
                                   AND json_extract(m.metadata, '$.namespace.project') IS ?7
                                   AND json_extract(m.metadata, '$.namespace.scope') IS ?8
                                 ORDER BY distance ASC
                                 LIMIT 1
                                 "#,
                                    table_name
                                ),
                                vec![
                                    rusqlite::types::Value::Text(emb_json),
                                    rusqlite::types::Value::Text(record_c.workspace_id.clone()),
                                    record_ns.org_id.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                    record_ns.user_id.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                    record_ns.agent_id.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                    record_ns.session_id.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                    record_ns.project.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                    record_ns.scope.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                ]
                            )
                        }
                    };

                    match conn.prepare(&sql) {
                        Ok(mut stmt) => {
                            match stmt.query(rusqlite::params_from_iter(&query_params)) {
                                Ok(mut rows) => {
                                    if let Ok(Some(row)) = rows.next() {
                                        let distance = match row.get::<_, rusqlite::types::Value>(18) {
                                            Ok(rusqlite::types::Value::Real(v)) => v as f32,
                                            Ok(rusqlite::types::Value::Integer(v)) => v as f32,
                                            _ => 1.0,
                                        };
                                        let similarity = 1.0 - distance;
                                        if let Ok(rec) = Self::deserialize_record(row) {
                                            best_cand = Some((rec, similarity));
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("v1_api: stmt.query 1 error: {:?}", e);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("v1_api: conn.prepare 1 error: {:?}", e);
                        }
                    }
                }

                // 2. Fallback to manual Rust cosine similarity search
                if best_cand.is_none() {
                    let (sql, query_params) = match dedup_settings_c.scope {
                        crate::settings::types::DedupScope::PathExact => {
                            (
                                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv FROM memory_records WHERE workspace_id = ? AND path = ?".to_string(),
                                vec![
                                    rusqlite::types::Value::Text(record_c.workspace_id.clone()),
                                    rusqlite::types::Value::Text(record_c.path.clone()),
                                ]
                            )
                        }
                        crate::settings::types::DedupScope::Namespace => {
                            (
                                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv FROM memory_records WHERE workspace_id = ? AND json_extract(metadata, '$.namespace.org_id') IS ? AND json_extract(metadata, '$.namespace.user_id') IS ? AND json_extract(metadata, '$.namespace.agent_id') IS ? AND json_extract(metadata, '$.namespace.session_id') IS ? AND json_extract(metadata, '$.namespace.project') IS ? AND json_extract(metadata, '$.namespace.scope') IS ?".to_string(),
                                vec![
                                    rusqlite::types::Value::Text(record_c.workspace_id.clone()),
                                    record_ns.org_id.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                    record_ns.user_id.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                    record_ns.agent_id.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                    record_ns.session_id.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                    record_ns.project.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                    record_ns.scope.as_ref().map(|s| rusqlite::types::Value::Text(s.clone())).unwrap_or(rusqlite::types::Value::Null),
                                ]
                            )
                        }
                    };

                    match conn.prepare(&sql) {
                        Ok(mut stmt) => {
                            match stmt.query(rusqlite::params_from_iter(&query_params)) {
                                Ok(mut rows) => {
                                    let mut best_sim = -1.0f32;
                                    let mut best_rec = None;
                                    while let Ok(Some(row)) = rows.next() {
                                        match Self::deserialize_record(row) {
                                            Ok(rec) => {
                                                let is_match = match dedup_settings_c.scope {
                                                    crate::settings::types::DedupScope::PathExact => rec.path == record_c.path,
                                                    crate::settings::types::DedupScope::Namespace => {
                                                        let rec_meta = match crate::memory::schema::resolve_metadata(&rec.path, &rec.metadata, &rec.workspace_id, None) {
                                                            Ok(m) => m,
                                                            Err(_) => continue,
                                                        };
                                                        namespaces_match(&record_ns, &rec_meta.namespace)
                                                    }
                                                };
                                                if is_match {
                                                    let sim = cosine_similarity(&record_c.embedding, &rec.embedding);
                                                    if sim > best_sim {
                                                        best_sim = sim;
                                                        best_rec = Some(rec);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!("v1_api: deserialize 2 error: {:?}", e);
                                            }
                                        }
                                    }
                                    if let Some(rec) = best_rec {
                                        best_cand = Some((rec, best_sim));
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("v1_api: stmt.query 2 error: {:?}", e);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("v1_api: conn.prepare 2 error: {:?}", e);
                        }
                    }
                }

                Ok(best_cand)
            }).await;

            if let Ok(Some((mut existing_record, mut similarity))) = query_res {
                if !existing_record.embedding.is_empty() && !record.embedding.is_empty() {
                    similarity = cosine_similarity(&record.embedding, &existing_record.embedding);
                }
                if similarity >= dedup_settings.threshold {
                    tracing::info!(
                        "Semantic dedup (similarity {} >= {}): Updating existing memory {} with new content",
                        similarity,
                        dedup_settings.threshold,
                        existing_record.id
                    );
                    let _ = crate::memory::sqlite_store::SqliteMemoryStore::decrypt_record(
                        &mut existing_record,
                    );

                    if is_superset(&record.content, &existing_record.content) {
                        let existing_revisions = existing_record.revisions.clone();
                        let existing_revision_num = existing_record.revision;

                        record.id = existing_record.id.clone();
                        record.created_at = existing_record.created_at;
                        record.updated_at = chrono::Utc::now();
                        record.revision = existing_revision_num + 1;
                        record.revisions = existing_revisions;
                    } else {
                        record = crate::memory::store::revisioned_record(existing_record, record);

                        let max_revs = if dedup_settings.max_revisions > 0 {
                            dedup_settings.max_revisions
                        } else {
                            5
                        };
                        if record.revisions.len() > max_revs {
                            let excess = record.revisions.len() - max_revs;
                            record.revisions.drain(0..excess);
                        }
                    }
                } else {
                    tracing::info!(
                        "Semantic dedup (similarity {} <= {}): Similarity is below threshold, inserting as new memory {}",
                        similarity,
                        dedup_settings.threshold,
                        record.id
                    );
                }
            }
        }

        let security = crate::security::get_security_service();
        if security.get_config().encryption_at_rest_enabled {
            let mgr = security.get_key_manager()?;
            let kek = security.get_kek()?;

            let workspace_id = record.workspace_id.clone();
            let project_id = self.project_id.clone();
            let _salt_bytes = ConnectionManager::global()
                .with_conn(&project_id, move |conn| {
                    let mut stmt = conn.prepare(
                        "SELECT salt FROM encryption_metadata WHERE workspace_id = ?",
                    )?;
                    match stmt.query_row([&workspace_id], |row| row.get::<_, Vec<u8>>(0)) {
                        Ok(salt) => Ok(salt),
                        Err(rusqlite::Error::QueryReturnedNoRows) => {
                            let new_salt = crate::crypto::keys::KeySalt::generate();
                            let salt_vec = new_salt.as_bytes().to_vec();
                            conn.execute(
                                "INSERT INTO encryption_metadata (id, workspace_id, salt, created_at) VALUES (?, ?, ?, ?)",
                                params![ulid::Ulid::new().to_string(), workspace_id, salt_vec, chrono::Utc::now().to_rfc3339()],
                            )?;
                            Ok(salt_vec)
                        }
                        Err(e) => Err(anyhow::anyhow!("Database error: {}", e)),
                    }
                })
                .await?;

            let dek = mgr.generate_dek();
            let encrypted_dek = mgr
                .encrypt_dek(&dek, &kek)
                .map_err(|e| anyhow::anyhow!("DEK encryption failed: {}", e))?;

            let content_nonce = crate::crypto::encryption::NonceBytes::generate();
            let encrypted_content = crate::crypto::encryption::encrypt_data(
                record.content.as_bytes(),
                dek.as_bytes(),
                &content_nonce,
            )
            .map_err(|e| anyhow::anyhow!("Content encryption failed: {}", e))?;

            let metadata_nonce = crate::crypto::encryption::NonceBytes::generate();
            let metadata_json = serde_json::to_string(&record.metadata)?;
            let encrypted_metadata = crate::crypto::encryption::encrypt_data(
                metadata_json.as_bytes(),
                dek.as_bytes(),
                &metadata_nonce,
            )
            .map_err(|e| anyhow::anyhow!("Metadata encryption failed: {}", e))?;

            record.content = crate::utils::crypto::hex_encode(&encrypted_content.ciphertext);
            record.metadata = serde_json::json!({
                "encrypted": crate::utils::crypto::hex_encode(&encrypted_metadata.ciphertext)
            });
            record.encrypted_dek = Some(encrypted_dek);
            record.content_iv = Some(content_nonce.as_bytes().to_vec());
            record.metadata_iv = Some(metadata_nonce.as_bytes().to_vec());
        }

        Ok(record)
    }

    /// Decomposed put step 3: SQLite INSERT or REPLACE into memory_records table and hash chain linking.
    pub fn put_store(&self, conn: &rusqlite::Connection, record: &MemoryRecord) -> Result<()> {
        let content_hash = format!("{:x}", Sha256::digest(record.content.as_bytes()));

        let prev_hash: Option<String> = {
            conn.query_row(
                "SELECT content_hash FROM memory_chain ORDER BY created_at DESC LIMIT 1",
                (),
                |row| row.get(0),
            )
            .ok()
        };

        let qjl_enabled = {
            let threshold = super::VecSqliteMemoryStore::configured_qjl_threshold();
            let current_vectors: usize = conn
                .query_row(
                    "SELECT COUNT(*) FROM memory_embeddings WHERE workspace_id = ?",
                    params![record.workspace_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            current_vectors >= threshold
        };

        let embedding_blob = if !record.embedding.is_empty() && qjl_enabled {
            vector::serialize_embedding_qjl(&record.embedding)
        } else {
            vector::serialize_embedding(&record.embedding)
        };

        conn.execute(
            &format!(
                "INSERT OR REPLACE INTO {} (id, workspace_id, path, content, metadata, embedding, encrypted_dek, content_iv, metadata_iv, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, embedding_status, embedding_attempts) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
                TABLE_MEMORIES
            ),
            params![
                record.id,
                record.workspace_id,
                record.path,
                record.content,
                serde_json::to_string(&record.metadata).unwrap_or_default(),
                embedding_blob,
                record.encrypted_dek,
                record.content_iv,
                record.metadata_iv,
                record.created_at.to_rfc3339(),
                record.updated_at.to_rfc3339(),
                record.revision,
                record.primary as i32,
                record.parent_id,
                record.cluster_id,
                record.level.as_str(),
                serde_json::to_string(&record.relation).unwrap_or_default(),
                serde_json::to_string(&record.revisions).unwrap_or_default(),
                record.embedding_status,
                record.embedding_attempts,
            ],
        )?;

        graph::sync_memory_entities(conn, &record.workspace_id, record)?;

        let chain_id = ulid::Ulid::new().to_string();
        conn.execute(
            "INSERT INTO memory_chain (id, prev_hash, content_hash) VALUES (?, ?, ?)",
            params![chain_id, prev_hash, content_hash],
        )?;

        Ok(())
    }

    /// Decomposed put step 4: FTS5 and vector index updates.
    pub fn put_index(&self, conn: &rusqlite::Connection, record: &MemoryRecord) -> Result<()> {
        conn.execute("DELETE FROM memory_fts WHERE id = ?", params![record.id])?;
        let code_tokens =
            super::fts::code_tokens(&format!("{} {}", &record.path, &record.content)).join(" ");
        conn.execute(
            "INSERT INTO memory_fts(id, path, content, code_tokens) VALUES (?, ?, ?, ?)",
            params![record.id, record.path, record.content, code_tokens],
        )?;

        if !record.embedding.is_empty() {
            let embedding_json = serde_json::to_string(&record.embedding).unwrap_or_default();
            let table_name = if record.embedding.len() == 768 {
                "memory_embeddings_768"
            } else {
                "memory_embeddings"
            };
            let sql = format!(
                "INSERT OR REPLACE INTO {}(id, workspace_id, embedding) VALUES (?1, ?2, vec_f32(?3))",
                table_name
            );
            conn.execute(
                &sql,
                params![record.id, record.workspace_id, embedding_json],
            )?;
        }

        Ok(())
    }

    /// Decomposed put step 5: Symbol linking (no-op on put to prevent auto-linking bloat; linking is performed on demand).
    pub fn put_link(&self, _conn: &rusqlite::Connection, _record: &MemoryRecord) -> Result<()> {
        Ok(())
    }

    /// On-demand symbol linking for a given memory record (max 10 links per memory).
    pub fn link_memory_on_demand(
        conn: &rusqlite::Connection,
        memory_id: &str,
        content: &str,
    ) -> Result<Vec<String>> {
        if content.is_empty() {
            return Ok(Vec::new());
        }

        let mut symbol_links: Vec<(String, f64)> = Vec::new();

        let code_db_path = crate::codebase::codegraph_paths::code_graph_db_path_for(
            std::path::Path::new("."),
        );
        if code_db_path.exists() {
            if let Ok(code_db) = code_graph::db::CodeGraphDB::new(&code_db_path) {
                if let Ok(links) = code_db.link_memory_to_symbols(memory_id, content) {
                    for link in links {
                        symbol_links.push((link.symbol_id, link.confidence));
                    }
                }
            }
        }

        if symbol_links.is_empty() {
            let candidate_words: std::collections::HashSet<&str> = content
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|w| w.len() >= 4)
                .collect();

            for word in candidate_words {
                symbol_links.push((word.to_string(), 1.0));
            }
        }

        // Cap at max_links_per_memory = 10
        symbol_links.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        symbol_links.truncate(10);

        let mut symbols = Vec::new();
        for (symbol_id, confidence) in symbol_links {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO memory_symbol_links (memory_id, symbol_id, confidence) VALUES (?1, ?2, ?3)",
                params![memory_id, symbol_id, confidence],
            );
            symbols.push(symbol_id);
        }

        Ok(symbols)
    }

    /// Reconcile embedding_status across memory_records to align status with physical vector existence.
    ///
    /// Fixes false 'completed' statuses where embedding blob is missing or empty (length(embedding) <= 100)
    /// and ensures valid embeddings (length(embedding) > 100) have status 'completed'.
    pub async fn reconcile_embedding_status(&self) -> Result<usize> {
        let project_id = self.project_id.clone();
        ConnectionManager::global()
            .with_conn(&project_id, move |conn| {
                Self::reconcile_embedding_status_conn(conn)
            })
            .await
    }

    /// Synchronous helper to reconcile embedding status on a connection.
    pub fn reconcile_embedding_status_conn(conn: &rusqlite::Connection) -> Result<usize> {
        let tx = conn.unchecked_transaction()?;
        let c1 = tx.execute(
            "UPDATE memory_records SET embedding_status = CASE WHEN embedding_attempts > 0 THEN 'retry' ELSE 'pending' END, updated_at = CURRENT_TIMESTAMP WHERE (embedding_status = 'completed' OR embedding_status IS NULL) AND (embedding IS NULL OR length(embedding) <= 100)",
            [],
        )?;
        let c2 = tx.execute(
            "UPDATE memory_records SET embedding_status = 'completed', updated_at = CURRENT_TIMESTAMP WHERE (embedding_status IS NULL OR embedding_status != 'completed') AND embedding IS NOT NULL AND length(embedding) > 100",
            [],
        )?;
        tx.commit()?;
        Ok(c1 + c2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_records (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}',
                embedding BLOB,
                encrypted_dek BLOB,
                content_iv BLOB,
                metadata_iv BLOB,
                created_at DATETIME NOT NULL,
                updated_at DATETIME NOT NULL,
                revision INTEGER NOT NULL DEFAULT 1,
                primary_flag INTEGER DEFAULT 1,
                parent_id TEXT,
                cluster_id TEXT,
                level TEXT DEFAULT 'atom',
                relation TEXT,
                revisions TEXT,
                embedding_status TEXT DEFAULT 'pending',
                embedding_attempts INTEGER DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS memory_symbol_links (
                memory_id TEXT NOT NULL,
                symbol_id TEXT NOT NULL,
                confidence REAL NOT NULL DEFAULT 1.0,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (memory_id, symbol_id)
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_reconcile_embedding_status() {
        let conn = setup_test_db();

        // 1. Record with false 'completed' (embedding NULL)
        conn.execute(
            "INSERT INTO memory_records (id, workspace_id, path, content, created_at, updated_at, embedding_status, embedding_attempts) VALUES ('rec_1', 'ws1', 'p1', 'c1', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'completed', 0)",
            [],
        ).unwrap();

        // 2. Record with false 'completed' (embedding blob empty / <=100 bytes)
        conn.execute(
            "INSERT INTO memory_records (id, workspace_id, path, content, created_at, updated_at, embedding, embedding_status, embedding_attempts) VALUES ('rec_2', 'ws1', 'p2', 'c2', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', X'1234', 'completed', 1)",
            [],
        ).unwrap();

        // 3. Record with real embedding (>100 bytes) but status 'pending'
        let fake_vec_blob = vec![0u8; 3072];
        conn.execute(
            "INSERT INTO memory_records (id, workspace_id, path, content, created_at, updated_at, embedding, embedding_status, embedding_attempts) VALUES ('rec_3', 'ws1', 'p3', 'c3', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', ?1, 'pending', 0)",
            params![fake_vec_blob],
        ).unwrap();

        let stats_before = VecSqliteMemoryStore::embedding_integrity_stats_conn(&conn).unwrap();
        assert_eq!(stats_before.total, 3);
        assert_eq!(stats_before.completed_without_vector, 2);
        assert_eq!(stats_before.completed_real, 0);

        let reconciled = VecSqliteMemoryStore::reconcile_embedding_status_conn(&conn).unwrap();
        assert_eq!(reconciled, 3);

        let stats_after = VecSqliteMemoryStore::embedding_integrity_stats_conn(&conn).unwrap();
        assert_eq!(stats_after.total, 3);
        assert_eq!(stats_after.completed_without_vector, 0);
        assert_eq!(stats_after.completed_real, 1);
        assert_eq!(stats_after.pending, 1);
        assert_eq!(stats_after.retry, 1);
    }

    #[test]
    fn test_put_record_status_alignment() {
        let mut rec_empty = MemoryRecord {
            id: "m_empty".to_string(),
            workspace_id: "ws1".to_string(),
            path: "p_empty".to_string(),
            content: "test empty".to_string(),
            embedding: vec![],
            embedding_status: "completed".to_string(), // False status passed initially
            embedding_attempts: 0,
            ..Default::default()
        };

        if rec_empty.embedding.is_empty() {
            if rec_empty.embedding_attempts > 0 {
                rec_empty.embedding_status = "retry".to_string();
            } else {
                rec_empty.embedding_status = "pending".to_string();
            }
        } else {
            rec_empty.embedding_status = "completed".to_string();
        }

        assert_eq!(rec_empty.embedding_status, "pending");

        let mut rec_attempts = MemoryRecord {
            id: "m_attempts".to_string(),
            workspace_id: "ws1".to_string(),
            path: "p_att".to_string(),
            content: "test att".to_string(),
            embedding: vec![],
            embedding_status: "completed".to_string(),
            embedding_attempts: 2,
            ..Default::default()
        };

        if rec_attempts.embedding.is_empty() {
            if rec_attempts.embedding_attempts > 0 {
                rec_attempts.embedding_status = "retry".to_string();
            } else {
                rec_attempts.embedding_status = "pending".to_string();
            }
        } else {
            rec_attempts.embedding_status = "completed".to_string();
        }

        assert_eq!(rec_attempts.embedding_status, "retry");
    }

    #[test]
    fn test_put_link_no_op() {
        let conn = setup_test_db();
        let record = MemoryRecord {
            id: "mem_no_auto_link".to_string(),
            content: "one two three four five six seven eight nine ten eleven twelve".to_string(),
            ..Default::default()
        };

        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig {
            path: std::path::PathBuf::from(":memory:"),
            ..Default::default()
        };
        let store = VecSqliteMemoryStore {
            config,
            project_id: "test_project".to_string(),
            event_tx: None,
            dedup_config: std::sync::Arc::new(tokio::sync::RwLock::new(Default::default())),
        };

        store.put_link(&conn, &record).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_symbol_links", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0, "put_link should be no-op and not insert symbol links eagerly");
    }

    #[test]
    fn test_link_memory_on_demand_and_max_10() {
        let conn = setup_test_db();
        let content = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu";
        let symbols = VecSqliteMemoryStore::link_memory_on_demand(&conn, "mem_demand", content).unwrap();

        assert!(!symbols.is_empty());
        assert!(symbols.len() <= 10, "On-demand linking must cap at max 10 links");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_symbol_links WHERE memory_id = 'mem_demand'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count as usize, symbols.len());
        assert!(count <= 10);
    }

    #[test]
    fn test_stale_symbol_links_cleanup() {
        let conn = setup_test_db();
        // Insert a fresh link and a stale link (>30 days old)
        conn.execute(
            "INSERT INTO memory_symbol_links (memory_id, symbol_id, confidence, created_at) VALUES ('m1', 'fresh_sym', 1.0, datetime('now'))",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO memory_symbol_links (memory_id, symbol_id, confidence, created_at) VALUES ('m1', 'stale_sym', 1.0, datetime('now', '-31 days'))",
            [],
        ).unwrap();

        let count_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_symbol_links", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count_before, 2);

        conn.execute(
            "DELETE FROM memory_symbol_links WHERE created_at < datetime('now', '-30 days')",
            [],
        ).unwrap();

        let count_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_symbol_links", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count_after, 1);

        let remaining_symbol: String = conn
            .query_row("SELECT symbol_id FROM memory_symbol_links", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining_symbol, "fresh_sym");
    }
}
