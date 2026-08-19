//! Search implementation for SQLite vector store
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::memory::sqlite_vec_store::types::FusionSource;
use crate::memory::store::{HybridSearchResult, MemoryRecord};
use std::collections::HashMap;

/// Merge rrf result.
pub fn merge_rrf_result(
    scored: &mut HashMap<String, HybridSearchResult>,
    source: FusionSource,
    rrf_k: usize,
    rank: usize,
    bm25: Option<f32>,
    record: MemoryRecord,
) {
    let rrf_score = 1.0 / (rrf_k as f32 + rank as f32);
    let key = record.id.clone();

    if let Some(existing) = scored.get_mut(&key) {
        existing.score += rrf_score * source_weight(source);
        match source {
            FusionSource::Vector => existing.vector_score = rrf_score,
            FusionSource::Fts => {
                existing.lexical_score = rrf_score;
                existing.bm25 = bm25;
            }
            FusionSource::Kg => existing.kg_score = rrf_score,
        }
    } else {
        let mut result = HybridSearchResult {
            record,
            score: rrf_score * source_weight(source),
            vector_score: 0.0,
            lexical_score: 0.0,
            kg_score: 0.0,
            bm25: None,
        };
        match source {
            FusionSource::Vector => result.vector_score = rrf_score,
            FusionSource::Fts => {
                result.lexical_score = rrf_score;
                result.bm25 = bm25;
            }
            FusionSource::Kg => result.kg_score = rrf_score,
        }
        scored.insert(key, result);
    }
}

fn source_weight(source: FusionSource) -> f32 {
    use crate::memory::sqlite_vec_store::config::*;
    match source {
        FusionSource::Vector => DEFAULT_VECTOR_WEIGHT,
        FusionSource::Fts => DEFAULT_FTS_WEIGHT,
        FusionSource::Kg => DEFAULT_KG_WEIGHT,
    }
}
