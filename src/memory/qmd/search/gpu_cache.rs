//! GPU-accelerated VRAM Active Cache for cosine similarity search.
//!
//! This module provides a prototype for performing massive parallel vector
//! similarity searches using the HuggingFace `candle` library.

use candle_core::{Device, Tensor, Result};

/// A cache that stores embeddings in VRAM and performs cosine similarity
/// searches using matrix multiplication.
pub struct GpuCosineCache {
    device: Device,
    /// Tensor of shape [N, D] where N is number of documents and D is embedding dimension.
    /// Embeddings are expected to be L2-normalized.
    embeddings: Option<Tensor>,
}

impl GpuCosineCache {
    /// Create a new GPU cache on the specified device.
    pub fn new(device: Device) -> Self {
        Self {
            device,
            embeddings: None,
        }
    }

    /// Update the cache with a new set of embeddings.
    /// Performs L2 normalization before storing in VRAM.
    pub fn set_embeddings(&mut self, embeddings: &[Vec<f32>]) -> Result<()> {
        if embeddings.is_empty() {
            self.embeddings = None;
            return Ok(());
        }

        let rows = embeddings.len();
        let cols = embeddings[0].len();
        let flat: Vec<f32> = embeddings.iter().flatten().cloned().collect();

        let t = Tensor::from_vec(flat, (rows, cols), &self.device)?;

        // L2 Normalization: t / sqrt(sum(t^2))
        // We use a small epsilon to avoid division by zero.
        let sq = t.sqr()?;
        let sum_sq = sq.sum_keepdim(1)?;
        let norm = sum_sq.affine(1.0, 1e-8)?.sqrt()?;

        self.embeddings = Some(t.broadcast_div(&norm)?);

        Ok(())
    }

    /// Perform a cosine similarity search for a query vector.
    /// Returns the Top-K indices and their similarity scores.
    pub fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(usize, f32)>> {
        let Some(ref db) = self.embeddings else {
            return Ok(vec![]);
        };

        let dim = query.len();
        let q = Tensor::from_vec(query.to_vec(), (1, dim), &self.device)?;

        // L2 Normalize query
        let q_sq = q.sqr()?;
        let q_sum_sq = q_sq.sum_keepdim(1)?;
        let q_norm = q_sum_sq.affine(1.0, 1e-8)?.sqrt()?;
        let q = q.broadcast_div(&q_norm)?;

        // Cosine similarity via MatMul: Q * DB^T
        // q: [1, D], db: [N, D] -> db^T: [D, N]
        // result: [1, N]
        let similarities = q.matmul(&db.t()?)?;

        // Extract to CPU for Top-K (Prototype Workaround)
        let similarities_vec: Vec<f32> = similarities.flatten_all()?.to_vec1()?;

        let mut indexed_scores: Vec<(usize, f32)> = similarities_vec
            .into_iter()
            .enumerate()
            .collect();

        // Sort descending by score
        indexed_scores.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        indexed_scores.truncate(top_k);

        Ok(indexed_scores)
    }

    /// Returns the number of embeddings currently in the cache.
    pub fn len(&self) -> usize {
        self.embeddings.as_ref().map(|t| t.dim(0).unwrap_or(0)).unwrap_or(0)
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_cosine_cache_cpu() -> Result<()> {
        let device = Device::Cpu;
        let mut cache = GpuCosineCache::new(device);

        let embeddings = vec![
            vec![1.0, 0.0, 0.0], // Doc 0
            vec![0.0, 1.0, 0.0], // Doc 1
            vec![0.707, 0.707, 0.0], // Doc 2 (Similar to both 0 and 1)
        ];

        cache.set_embeddings(&embeddings)?;
        assert_eq!(cache.len(), 3);

        // Query similar to Doc 0
        let query = vec![1.0, 0.1, 0.0];
        let results = cache.search(&query, 2)?;

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 0); // Doc 0 should be first
        assert!(results[0].1 > 0.9);
        assert_eq!(results[1].0, 2); // Doc 2 should be second

        Ok(())
    }

    #[test]
    fn test_normalization() -> Result<()> {
        let device = Device::Cpu;
        let mut cache = GpuCosineCache::new(device);

        // Non-normalized vectors
        let embeddings = vec![
            vec![10.0, 0.0, 0.0],
            vec![0.0, 5.0, 0.0],
        ];

        cache.set_embeddings(&embeddings)?;

        // After normalization, they should be [1, 0, 0] and [0, 1, 0]
        // Query [1, 0, 0] should give score 1.0 for Doc 0
        let results = cache.search(&vec![1.0, 0.0, 0.0], 1)?;
        assert!((results[0].1 - 1.0).abs() < 1e-6);

        Ok(())
    }
}
