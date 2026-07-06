//! VRAM Active Cache for GPU-accelerated cosine similarity search.
//!
//! Part of the VRAM Cortex (Phase 2) implementation.

use std::collections::HashMap;
use anyhow::Result;

#[cfg(feature = "gpu-search")]
use candle_core::{Device, Tensor, DType};

/// Active VRAM Cache for fast vector similarity search.
pub struct ActiveVramCache {
    #[cfg(feature = "gpu-search")]
    /// Massively parallel K-matrix of document embeddings.
    #[allow(dead_code)]
    pub(crate) matrix: Option<Tensor>,

    /// Map from tensor row index to document ID (or SQLite rowid).
    #[allow(dead_code)]
    pub(crate) index_to_id: HashMap<usize, String>,

    #[cfg(feature = "gpu-search")]
    /// Target compute device (CPU/CUDA/Metal).
    #[allow(dead_code)]
    pub(crate) device: Device,
}

impl ActiveVramCache {
    #[cfg(feature = "gpu-search")]
    pub fn new(device: Device) -> Self {
        Self {
            matrix: None,
            index_to_id: HashMap::new(),
            device,
        }
    }

    #[cfg(not(feature = "gpu-search"))]
    pub fn new() -> Self {
        Self {
            index_to_id: HashMap::new(),
        }
    }

    /// Warmed up the cache by uploading embeddings to VRAM.
    #[cfg(feature = "gpu-search")]
    pub async fn warmup(&mut self, embeddings: Vec<(String, Vec<f32>)>) -> Result<()> {
        if embeddings.is_empty() {
            return Ok(());
        }

        let num_docs = embeddings.len();
        let dims = embeddings[0].1.len();

        tracing::debug!(
            "Warming up VRAM cache with {} documents (dims={})",
            num_docs,
            dims
        );

        let mut flat_embeddings = Vec::with_capacity(num_docs * dims);
        let mut index_to_id = HashMap::with_capacity(num_docs);

        for (i, (id, vector)) in embeddings.into_iter().enumerate() {
            if vector.len() != dims {
                // Skip inconsistent dimensions or handle error
                continue;
            }
            flat_embeddings.extend(vector);
            index_to_id.insert(i, id);
        }

        if flat_embeddings.is_empty() {
            return Ok(());
        }

        let actual_num_docs = index_to_id.len();
        let tensor = Tensor::from_vec(flat_embeddings, (actual_num_docs, dims), &self.device)?;

        // Ensure the tensor is in the right DType and on the right device
        let tensor = tensor.to_dtype(DType::F32)?;

        self.matrix = Some(tensor);
        self.index_to_id = index_to_id;

        tracing::info!(
            "🚀 VRAM Active Cache warmed up: {} vectors uploaded to {:?}",
            actual_num_docs,
            self.device
        );

        Ok(())
    }

    #[cfg(not(feature = "gpu-search"))]
    pub async fn warmup(&mut self, _embeddings: Vec<(String, Vec<f32>)>) -> Result<()> {
        tracing::warn!("VRAM cache warmup skipped: gpu-search feature not enabled");
        Ok(())
    }

    /// Perform similarity search using the VRAM-resident matrix.
    pub async fn search(&self, _query_vector: &[f32], _limit: usize) -> Result<Vec<(String, f32)>> {
        // Implementation of Phase 3 logic (Top-K extraction) will go here.
        // For Phase 2, we just ensure the structure and warmup are functional.
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vram_cache_warmup_cpu() {
        #[cfg(feature = "gpu-search")]
        {
            let device = Device::Cpu;
            let mut cache = ActiveVramCache::new(device);

            let embeddings = vec![
                ("doc1".to_string(), vec![1.0, 0.0, 0.0]),
                ("doc2".to_string(), vec![0.0, 1.0, 0.0]),
            ];

            cache.warmup(embeddings).await.unwrap();

            assert!(cache.matrix.is_some());
            assert_eq!(cache.index_to_id.len(), 2);
            assert_eq!(cache.index_to_id.get(&0).unwrap(), "doc1");
            assert_eq!(cache.index_to_id.get(&1).unwrap(), "doc2");
        }
    }

    #[tokio::test]
    async fn test_vram_cache_warmup_empty() {
        #[cfg(feature = "gpu-search")]
        {
            let device = Device::Cpu;
            let mut cache = ActiveVramCache::new(device);
            cache.warmup(vec![]).await.unwrap();
            assert!(cache.matrix.is_none());
        }
    }
}
