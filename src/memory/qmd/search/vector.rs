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
    vsearch_filtered(memory, query_vector, limit, None).await
}

/// Perform vector similarity search against documents matching filters before truncation.
pub async fn vsearch_filtered(
    memory: &QmdMemory,
    query_vector: Vec<f32>,
    limit: usize,
    filters: Option<&crate::memory::schema::MemoryQueryFilters>,
) -> Result<Vec<MemoryDocument>> {
    if query_vector.is_empty() {
        return Ok(Vec::new());
    }

    let include_activity = filters.and_then(|f| f.include_activity).unwrap_or(false);
    let docs = memory.docs.clone().read_owned().await;
    let workspace_id = memory.workspace_id.clone();
    let owned_filters = filters.cloned();
    let cancelled = tokio_util::sync::CancellationToken::new();
    let _cancel_on_drop = cancelled.clone().drop_guard();
    tokio::task::spawn_blocking(move || {
        let mut similarities: Vec<(f32, usize)> = docs
            .iter()
            .enumerate()
            .take_while(|_| !cancelled.is_cancelled())
            .filter(|(_, doc)| {
                crate::memory::schema::matches_filters(
                    &doc.path,
                    &doc.metadata,
                    &workspace_id,
                    owned_filters.as_ref(),
                )
            })
            .filter(|(_, doc)| {
                include_activity || !crate::memory::schema::is_noise_content(&doc.content)
            })
            .filter_map(|(index, doc)| {
                let score = cosine_similarity(&query_vector, &doc.embedding);
                (score > 0.0).then_some((score, index))
            })
            .collect();

        if let Some(max_sim) = similarities.iter().map(|(s, _)| *s).reduce(f32::max) {
            if max_sim > 0.0 {
                for (score, _) in &mut similarities {
                    *score = 0.5 + 0.5 * (*score / max_sim);
                }
            }
        }
        let compare = |left: &(f32, usize), right: &(f32, usize)| {
            right
                .0
                .partial_cmp(&left.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| docs[left.1].path.cmp(&docs[right.1].path))
                .then_with(|| left.1.cmp(&right.1))
        };
        if similarities.len() > limit {
            similarities.select_nth_unstable_by(limit, compare);
            similarities.truncate(limit);
        }
        similarities.sort_unstable_by(compare);
        similarities
            .into_iter()
            .map(|(score, index)| {
                let mut doc = docs[index].clone();
                doc.score = score;
                doc
            })
            .collect()
    })
    .await
    .map_err(Into::into)
}

#[cfg(test)]
mod latency_regression {
    use super::*;
    use std::future::Future;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn vector_scan_yields_and_preserves_normalized_top_hits() {
        let docs = (0..2000)
            .map(|i| MemoryDocument {
                id: Some(i.to_string()),
                path: format!("docs/{i:04}"),
                content: "runtime memoria rust ".repeat(100),
                embedding: vec![0.5; 768],
                ..Default::default()
            })
            .collect();
        let memory = QmdMemory::new(Arc::new(RwLock::new(docs)));
        let started = std::time::Instant::now();
        let search = vsearch_filtered(&memory, vec![0.5; 768], 5, None);
        tokio::pin!(search);
        let yielded = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(search.as_mut().poll(cx).is_pending())
        })
        .await;
        eprintln!("vector first poll: {:?}", started.elapsed());
        assert!(
            yielded,
            "vector scoring blocked the Tokio worker until completion"
        );
        let result = search.await.unwrap();
        eprintln!("vector scan: {:?}", started.elapsed());
        assert_eq!(result.len(), 5);
        assert_eq!(result[0].path, "docs/0000");
        assert!((result[0].score - 1.0).abs() < 1e-6);
    }
}
