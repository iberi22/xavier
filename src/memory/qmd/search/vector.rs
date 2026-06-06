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

    let docs = memory.docs.read().await;

    let mut similarities: Vec<(f32, MemoryDocument)> = docs
        .iter()
        .filter_map(|doc| {
            let score = cosine_similarity(&query_vector, &doc.embedding);
            (score > 0.0).then(|| (score, doc.clone()))
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
        .map(|(_, doc)| doc)
        .take(limit)
        .collect())
}
