//! SQLite vector store backend implementation
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::codebase::connection_manager::ConnectionManager;
use crate::memory::schema::MemoryQueryFilters;
use crate::memory::sqlite_store::TABLE_MEMORIES;
use crate::memory::store::{
    filter_records, GraphHopPath, GraphHopResult, HybridSearchMode, HybridSearchResult, MemoryStore,
};
use anyhow::{Context, Result};
use rusqlite::params;
use std::collections::{HashMap, HashSet};

use super::{graph, search, utils, FusionSource, VecSqliteMemoryStore};
use crate::memory::fts;

impl VecSqliteMemoryStore {
    #[allow(dead_code)]
    pub(crate) async fn upsert_vector(
        &self,
        memory_id: &str,
        workspace_id: &str,
        embedding: &[f32],
    ) -> Result<()> {
        let memory_id = memory_id.to_string();
        let workspace_id = workspace_id.to_string();
        let embedding_json =
            serde_json::to_string(embedding).context("failed to serialize embedding")?;

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO memory_embeddings(id, workspace_id, embedding) VALUES (?1, ?2, vec_f32(?3))",
                params![memory_id, workspace_id, embedding_json],
            )
            .context("failed to upsert memory_embeddings row")?;
            Ok(())
        }).await
    }

    pub(crate) async fn perform_hybrid_search(
        &self,
        workspace_id: &str,
        query: &str,
        mode: HybridSearchMode,
        filters: Option<&MemoryQueryFilters>,
        limit: usize,
        embedding: Option<Vec<f32>>,
    ) -> Result<Vec<HybridSearchResult>> {
        let trimmed_query = query.trim().to_string();
        let candidate_limit = Self::candidate_limit(limit);
        let include_vector = matches!(mode, HybridSearchMode::Vector | HybridSearchMode::Both);
        let include_text = matches!(mode, HybridSearchMode::Text | HybridSearchMode::Both);

        let workspace_id_c = workspace_id.to_string();
        let filters_c = filters.cloned();
        let trimmed_query_c = trimmed_query.clone();

        let scored = ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut internal_scored: HashMap<String, HybridSearchResult> = HashMap::new();

            let dataset_size = {
                conn.query_row(
                    &format!(
                        "SELECT COUNT(*) FROM {} WHERE workspace_id = ?",
                        TABLE_MEMORIES
                    ),
                    params![workspace_id_c],
                    |row| row.get::<_, i64>(0)
                ).unwrap_or(0).max(0) as usize
            };
            let rrf_k = Self::dynamic_rrf_k(dataset_size);

            if include_vector {
                if let Some(emb) = &embedding {
                    let embedding_json = serde_json::to_string(emb).unwrap_or_default();
                    let vector_sql = r#"
                        SELECT m.id, m.workspace_id, m.path, m.content, m.metadata, m.embedding,
                               m.created_at, m.updated_at, m.revision, m.primary_flag,
                               m.parent_id, m.cluster_id, m.level, m.relation, m.revisions,
                               CAST(vec_distance_cosine(e.embedding, vec_f32(?1)) AS REAL) AS distance
                        FROM memory_embeddings e
                        JOIN memory_records m ON m.id = e.id AND m.workspace_id = ?2
                        WHERE e.workspace_id = ?2
                        ORDER BY distance ASC
                        LIMIT ?3
                    "#;

                    let mut stmt = conn.prepare(vector_sql)?;
                    let mut rows = stmt
                        .query(params![
                            embedding_json,
                            workspace_id_c,
                            candidate_limit as i64
                        ])?;
                    let mut rank = 0usize;
                    while let Some(row) = rows.next()? {
                        let distance = match row.get::<_, rusqlite::types::Value>(15)? {
                            rusqlite::types::Value::Real(v) => v as f32,
                            rusqlite::types::Value::Integer(v) => v as f32,
                            _ => 0.0,
                        };
                        let similarity = 1.0 - distance;
                        let record = Self::deserialize_record(row)?;
                        if Self::row_matches_filters(&workspace_id_c, &record, filters_c.as_ref()) {
                            rank += 1;
                            search::merge_rrf_result(
                                &mut internal_scored,
                                FusionSource::Vector,
                                rrf_k,
                                rank,
                                Some(similarity),
                                record,
                            );
                        }
                    }
                }
            }

            if include_text && !trimmed_query_c.is_empty() {
                if let Some(fts_query) = fts::build_fts_query(&trimmed_query_c) {
                    let fts_sql = r#"
                        SELECT m.id, m.workspace_id, m.path, m.content, m.metadata, m.embedding,
                               m.created_at, m.updated_at, m.revision, m.primary_flag,
                               m.parent_id, m.cluster_id, m.level, m.relation, m.revisions, CAST(bm25(memory_fts, 1.0, 0.8) AS REAL) AS rank
                        FROM memory_fts f
                        JOIN memory_records m ON m.id = f.id AND m.workspace_id = ?
                        WHERE f.memory_fts MATCH ?
                        ORDER BY rank
                        LIMIT ?
                    "#;

                    let mut stmt = conn.prepare(fts_sql)?;
                    let mut rows = stmt
                        .query(params![workspace_id_c, fts_query, candidate_limit as i64])?;
                    let mut rank = 0usize;
                    while let Some(row) = rows.next()? {
                        let bm25_score = match row.get::<_, rusqlite::types::Value>(15)? {
                            rusqlite::types::Value::Real(v) => Some(v as f32),
                            rusqlite::types::Value::Integer(v) => Some(v as f32),
                            _ => None,
                        };
                        let record = Self::deserialize_record(row)?;
                        if Self::row_matches_filters(&workspace_id_c, &record, filters_c.as_ref()) {
                            rank += 1;
                            search::merge_rrf_result(
                                &mut internal_scored,
                                FusionSource::Fts,
                                rrf_k,
                                rank,
                                bm25_score,
                                record,
                            );
                        }
                    }
                }

                let entity_terms = utils::search_tokens(&trimmed_query_c);
                if !entity_terms.is_empty() {
                    let mut kg_rank = 0usize;
                    let mut seen_ids = HashSet::<String>::new();

                    // Seed from entities mentioned in the query
                    let mut entity_stmt = conn
                        .prepare("SELECT id FROM entities WHERE workspace_id = ? AND name LIKE ?")?;
                    for term in entity_terms {
                        let mut entity_rows =
                            entity_stmt.query(params![workspace_id_c, format!("%{term}%")])?;
                        while let Some(row) = entity_rows.next()? {
                            let entity_id: String = row.get(0)?;
                            // Find memories linked to this entity
                            let mut mem_stmt = conn.prepare("SELECT memory_id FROM memory_entities WHERE entity_id = ? AND workspace_id = ?")?;
                            let mut mem_rows =
                                mem_stmt.query(params![entity_id, workspace_id_c])?;
                            while let Some(mem_row) = mem_rows.next()? {
                                let memory_id: String =
                                    mem_row.get(0)?;
                                if seen_ids.insert(memory_id.clone()) {
                                    // load_record_by_id logic here but sync
                                    let mut stmt = conn.prepare("SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions FROM memory_records WHERE id = ? AND workspace_id = ?")?;
                                    let mut rows = stmt.query(params![memory_id, workspace_id_c])?;
                                    if let Some(row) = rows.next()? {
                                        let record = Self::deserialize_record(row)?;
                                        if Self::row_matches_filters(&workspace_id_c, &record, filters_c.as_ref())
                                        {
                                            kg_rank += 1;
                                            search::merge_rrf_result(
                                                &mut internal_scored,
                                                FusionSource::Kg,
                                                rrf_k,
                                                kg_rank,
                                                None,
                                                record,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(internal_scored)
        }).await?;

        let mut results: Vec<_> = scored.into_values().collect();
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if results.is_empty() && include_text && !trimmed_query.is_empty() {
            let memories = self.list(workspace_id).await?;
            return Ok(
                filter_records(memories, workspace_id, &trimmed_query, filters)?
                    .into_iter()
                    .take(limit)
                    .enumerate()
                    .map(|(idx, record)| HybridSearchResult {
                        record,
                        score: 1.0 / (Self::configured_rrf_k() as f32 + (idx + 1) as f32),
                        vector_score: 0.0,
                        lexical_score: 0.0,
                        kg_score: 0.0,
                        bm25: None,
                    })
                    .collect(),
            );
        }

        results.truncate(limit);
        Ok(results)
    }

    pub(crate) async fn perform_graph_hops(
        &self,
        workspace_id: &str,
        path_or_id: &str,
        max_hops: usize,
        query: &str,
    ) -> Result<GraphHopResult> {
        let source = self
            .get(workspace_id, path_or_id)
            .await?
            .with_context(|| format!("memory not found for graph traversal: {path_or_id}"))?;

        let workspace_id_c = workspace_id.to_string();
        let query_c = query.to_string();
        let source_c = source.clone();

        let paths = ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let seed_ids = graph::resolve_graph_seed_entities(conn, &workspace_id_c, &source_c, &query_c)?;

            if seed_ids.is_empty() {
                return Ok(Vec::new());
            }

            let sql_params = vec!["?"; seed_ids.len()].join(", ");
            let sql = format!(
                r#"
                WITH RECURSIVE graph_walk(root_id, current_id, current_name, depth, entity_path, relation_path) AS (
                    SELECT e.id, e.id, e.name, 0, e.name, ''
                    FROM entities e
                    WHERE e.id IN ({sql_params})
                      AND e.workspace_id = ?
                    UNION ALL
                    SELECT
                        graph_walk.root_id,
                        r.target_id,
                        target.name,
                        graph_walk.depth + 1,
                        graph_walk.entity_path || ' -> ' || target.name,
                        CASE
                            WHEN graph_walk.relation_path = '' THEN r.relation_type
                            ELSE graph_walk.relation_path || ' -> ' || r.relation_type
                        END
                    FROM graph_walk
                    JOIN relations r ON r.source_id = graph_walk.current_id
                                    AND r.workspace_id = ?
                    JOIN entities target ON target.id = r.target_id
                                        AND target.workspace_id = ?
                    WHERE graph_walk.depth < ?
                      AND instr(graph_walk.entity_path, target.name) = 0
                )
                SELECT current_id, current_name, depth, entity_path, relation_path
                FROM graph_walk
                WHERE depth > 0
                ORDER BY depth, entity_path
                "#
            );

            let mut stmt = conn.prepare(&sql)?;

            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            for id in seed_ids {
                params_vec.push(Box::new(id));
            }
            params_vec.push(Box::new(workspace_id_c.clone()));
            params_vec.push(Box::new(workspace_id_c.clone()));
            params_vec.push(Box::new(workspace_id_c.clone()));
            params_vec.push(Box::new(max_hops as i64));

            let mut rows = stmt.query(rusqlite::params_from_iter(params_vec))?;
            let mut paths = Vec::new();

            while let Some(row) = rows.next()? {
                let entity_id: String = row.get(0)?;
                let entity_name: String = row.get(1)?;
                let depth = row.get::<_, i64>(2)?.max(0) as usize;
                let entity_path: String = row.get(3)?;
                let relation_path: String = row.get(4)?;

                let mut hit_stmt = conn.prepare(&format!(
                    "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions
                     FROM {}
                     WHERE workspace_id = ?
                       AND content LIKE '%' || ? || '%'
                     ORDER BY updated_at DESC
                     LIMIT 3",
                    TABLE_MEMORIES
                ))?;
                let mut hit_rows = hit_stmt
                    .query(params![workspace_id_c, entity_name.clone()])?;
                let mut memory_hits = Vec::new();
                while let Some(hit_row) = hit_rows.next()? {
                    if let Ok(record) = Self::deserialize_record(hit_row) {
                        if record.id != source_c.id {
                            memory_hits.push(record);
                        }
                    }
                }

                paths.push(GraphHopPath {
                    entity_id,
                    entity_name,
                    depth,
                    entity_path,
                    relation_path,
                    memory_hits,
                });
            }
            Ok(paths)
        }).await?;

        Ok(GraphHopResult {
            source,
            hops: max_hops,
            query: query.to_string(),
            paths,
        })
    }
}
