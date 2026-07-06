//! Vector search ÔÇö cosine similarity search over document embeddings.

use anyhow::Result;

use crate::memory::qmd_memory::types::MemoryDocument;
use crate::memory::qmd_memory::utils::cosine_similarity;
use crate::memory::qmd_memory::QmdMemory;

/// Perform vector similarity search against all documents.
pub async fn vsearch(
    memory: &QmdMemory,
    query_vector: Vec<f32>,
    limit: usize,
) -> Result<Vec<MemoryDocument>> {
    if query_vector.is_empty() {
        return Ok(Vec::new());
    }

    #[cfg(feature = "gpu-search")]
    if let Some(vram_cache) = &memory.vram_cache {
        if vram_cache.count().await > 0 {
            let results = vram_cache.search(&query_vector, limit).await?;
            if !results.is_empty() {
                if let Some(store) = memory.store().await {
                    let ids: Vec<String> = results.iter().map(|(id, _)| id.clone()).collect();
                    let docs = store.get_batch(&memory.workspace_id, &ids).await?;

                    let mut final_results = Vec::new();
                    for (id, score) in results {
                        if let Some(record) = docs.get(&id) {
                            let mut doc = record.to_document();
                            doc.score = score;
                            final_results.push(doc);
                        }
                    }

                    // Normalize scores
                    if let Some(max_sim) = final_results.iter().map(|d| d.score).reduce(f32::max) {
                        if max_sim > 0.0 {
                            for doc in final_results.iter_mut() {
                                doc.score = 0.5 + 0.5 * (doc.score / max_sim);
                            }
                        }
                    }

                    return Ok(final_results);
                }
            }
        }
    }

    let docs = memory.docs.read().await;

    let mut similarities: Vec<(f32, MemoryDocument)> = docs
        .iter()
        .filter_map(|doc| {
            let score = cosine_similarity(&query_vector, &doc.embedding);
            if score > 0.0 {
                let mut d = doc.clone();
                d.score = score;
                Some((score, d))
            } else {
                None
            }
        })
        .collect();

    if let Some(max_sim) = similarities.iter().map(|(s, _)| *s).reduce(f32::max) {
        if max_sim > 0.0 {
            for (score, _) in similarities.iter_mut() {
                *score = 0.5 + 0.5 * (*score / max_sim);
            }
        }
    }

    similarities.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.path.cmp(&right.1.path))
    });

    Ok(similarities
        .into_iter()
        .map(|(score, mut doc)| {
            doc.score = score;
            doc
        })
        .take(limit)
        .collect())
}
