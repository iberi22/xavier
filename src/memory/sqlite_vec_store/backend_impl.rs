use std::collections::{HashMap, HashSet};
use anyhow::{Context, Result};
use libsql::params;
use crate::memory::schema::MemoryQueryFilters;
use crate::memory::store::{
    GraphHopResult, HybridSearchMode,
    HybridSearchResult, GraphHopPath, filter_records, MemoryStore,
};
use crate::memory::sqlite_store::TABLE_MEMORIES;

use super::{VecSqliteMemoryStore, search, fts, utils, FusionSource};

impl VecSqliteMemoryStore {
    pub(crate) async fn upsert_vector(&self, memory_id: &str, workspace_id: &str, embedding: &[f32]) -> Result<()> {
        let conn = self.pool.get().await?;
        let embedding_json = serde_json::to_string(embedding).context("failed to serialize embedding")?;

        conn.execute(
            "INSERT OR REPLACE INTO memory_embeddings(id, workspace_id, embedding) VALUES (?1, ?2, vector32(?3))",
            params![memory_id, workspace_id, embedding_json],
        )
        .await
        .context("failed to upsert memory_embeddings row")?;
        Ok(())
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
        let trimmed_query = query.trim();
        let candidate_limit = Self::candidate_limit(limit);
        let include_vector = matches!(mode, HybridSearchMode::Vector | HybridSearchMode::Both);
        let include_text = matches!(mode, HybridSearchMode::Text | HybridSearchMode::Both);
        let mut scored: HashMap<String, HybridSearchResult> = HashMap::new();

        {
            let conn = self.pool.get().await?;
            let dataset_size = {
                let mut stmt = conn.prepare(&format!(
                    "SELECT COUNT(*) FROM {} WHERE workspace_id = ?",
                    TABLE_MEMORIES
                )).await?;
                let mut rows = stmt.query(params![workspace_id]).await?;
                if let Some(row) = rows.next().await? {
                    row.get::<i64>(0).unwrap_or(0).max(0) as usize
                } else {
                    0
                }
            };
            let rrf_k = Self::dynamic_rrf_k(dataset_size);

            if include_vector {
                if let Some(emb) = &embedding {
                    let embedding_json = serde_json::to_string(emb).unwrap_or_default();
                    let vector_sql = r#"
                        SELECT m.id, m.workspace_id, m.path, m.content, m.metadata, m.embedding,
                               m.created_at, m.updated_at, m.revision, m.primary_flag,
                               m.parent_id, m.cluster_id, m.level, m.relation, m.revisions,
                               vector_distance_cos(e.embedding, vector32(?1)) AS distance
                        FROM memory_embeddings e
                        JOIN memory_records m ON m.id = e.id AND m.workspace_id = ?2
                        WHERE e.workspace_id = ?2
                        ORDER BY distance ASC
                        LIMIT ?3
                    "#;

                    let mut stmt = conn.prepare(vector_sql).await?;
                    let mut rows = stmt.query(params![embedding_json, workspace_id, candidate_limit as i64]).await?;
                    let mut rank = 0usize;
                    while let Some(row) = rows.next().await? {
                        let distance = row.get::<f64>(15).map_err(anyhow::Error::msg)? as f32;
                        let similarity = 1.0 - distance;
                        let record = Self::deserialize_record(&row)?;
                        if Self::row_matches_filters(workspace_id, &record, filters) {
                            rank += 1;
                            search::merge_rrf_result(
                                &mut scored,
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

            if include_text && !trimmed_query.is_empty() {
                if let Some(fts_query) = fts::build_fts_query(trimmed_query) {
                    let fts_sql = r#"
                        SELECT m.id, m.workspace_id, m.path, m.content, m.metadata, m.embedding,
                               m.created_at, m.updated_at, m.revision, m.primary_flag,
                               m.parent_id, m.cluster_id, m.level, m.relation, m.revisions, bm25(f, 1.0, 0.8) AS rank
                        FROM memory_fts f
                        JOIN memory_records m ON m.id = f.id AND m.workspace_id = ?
                        WHERE f.memory_fts MATCH ?
                        ORDER BY rank
                        LIMIT ?
                    "#;

                    let mut stmt = conn.prepare(fts_sql).await?;
                    let mut rows = stmt.query(params![workspace_id, fts_query, candidate_limit as i64]).await?;
                    let mut rank = 0usize;
                    while let Some(row) = rows.next().await? {
                        let bm25_score = row.get::<f64>(15).ok().map(|v| v as f32);
                        let record = Self::deserialize_record(&row)?;
                        if Self::row_matches_filters(workspace_id, &record, filters) {
                            rank += 1;
                            search::merge_rrf_result(
                                &mut scored,
                                FusionSource::Fts,
                                rrf_k,
                                rank,
                                bm25_score,
                                record,
                            );
                        }
                    }
                }

                let entity_terms = utils::search_tokens(trimmed_query);
                if !entity_terms.is_empty() {
                    let mut kg_rank = 0usize;
                    let mut seen_ids = HashSet::<String>::new();

                    // Seed from entities mentioned in the query
                    let mut entity_stmt = conn.prepare("SELECT id FROM entities WHERE name LIKE ?").await?;
                    for term in entity_terms {
                        let mut entity_rows = entity_stmt.query(params![format!("%{term}%")]).await?;
                        while let Some(row) = entity_rows.next().await? {
                            let entity_id: String = row.get(0).map_err(anyhow::Error::msg)?;
                            // Find memories linked to this entity
                            let mut mem_stmt = conn.prepare("SELECT memory_id FROM memory_entities WHERE entity_id = ? AND workspace_id = ?").await?;
                            let mut mem_rows = mem_stmt.query(params![entity_id, workspace_id]).await?;
                            while let Some(mem_row) = mem_rows.next().await? {
                                let memory_id: String = mem_row.get(0).map_err(anyhow::Error::msg)?;
                                if seen_ids.insert(memory_id.clone()) {
                                    if let Some(record) = Self::load_record_by_id(&conn, workspace_id, &memory_id).await? {
                                        if Self::row_matches_filters(workspace_id, &record, filters) {
                                            kg_rank += 1;
                                            search::merge_rrf_result(
                                                &mut scored,
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
        }

        let mut results: Vec<_> = scored.into_values().collect();
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if results.is_empty() && include_text && !trimmed_query.is_empty() {
            let memories = self.list(workspace_id).await?;
            return Ok(filter_records(
                memories,
                workspace_id,
                trimmed_query,
                filters,
            )?
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
            .collect());
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
        let conn = self.pool.get().await?;
        let seed_ids = self.resolve_graph_seed_entities(&conn, workspace_id, &source, query).await?;

        if seed_ids.is_empty() {
            return Ok(GraphHopResult {
                source,
                hops: max_hops,
                query: query.to_string(),
                paths: Vec::new(),
            });
        }

        let sql_params = std::iter::repeat_n("?", seed_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            WITH RECURSIVE graph_walk(root_id, current_id, current_name, depth, entity_path, relation_path) AS (
                SELECT e.id, e.id, e.name, 0, e.name, ''
                FROM entities e
                WHERE e.id IN ({sql_params})
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
                JOIN entities target ON target.id = r.target_id
                WHERE graph_walk.depth < ?
                  AND instr(graph_walk.entity_path, target.name) = 0
            )
            SELECT current_id, current_name, depth, entity_path, relation_path
            FROM graph_walk
            WHERE depth > 0
            ORDER BY depth, entity_path
            "#
        );

        let mut params_vec: Vec<libsql::Value> = seed_ids
            .into_iter()
            .map(libsql::Value::Text)
            .collect();
        params_vec.push(libsql::Value::Integer(max_hops as i64));

        let mut stmt = conn.prepare(&sql).await.context("graph_hops prepare failed")?;
        let mut rows = stmt.query(params_vec).await?;
        let mut paths = Vec::new();

        while let Some(row) = rows.next().await? {
            let entity_id: String = row.get(0).map_err(anyhow::Error::msg)?;
            let entity_name: String = row.get(1).map_err(anyhow::Error::msg)?;
            let depth = row.get::<i64>(2).map_err(anyhow::Error::msg)?.max(0) as usize;
            let entity_path: String = row.get(3).map_err(anyhow::Error::msg)?;
            let relation_path: String = row.get(4).map_err(anyhow::Error::msg)?;

            let mut hit_stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions
                 FROM {}
                 WHERE workspace_id = ?
                   AND content LIKE '%' || ? || '%'
                 ORDER BY updated_at DESC
                 LIMIT 3",
                TABLE_MEMORIES
            )).await?;
            let mut hit_rows = hit_stmt.query(params![workspace_id, entity_name.clone()]).await?;
            let mut memory_hits = Vec::new();
            while let Some(hit_row) = hit_rows.next().await? {
                if let Ok(record) = Self::deserialize_record(&hit_row) {
                    if record.id != source.id {
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

        Ok(GraphHopResult {
            source,
            hops: max_hops,
            query: query.to_string(),
            paths,
        })
    }
}
