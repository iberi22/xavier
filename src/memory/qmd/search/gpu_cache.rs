//! Active VRAM Cache for GPU-accelerated vector search using Candle.
//!
//! Provides a high-performance cache for document embeddings in VRAM,
//! enabling fast cosine similarity search via matrix multiplication.

use anyhow::Result;
use candle_core::{Device, Tensor};
use tokio::sync::RwLock;

/// Active VRAM Cache for cosine similarity search.
pub struct ActiveVramCache {
    /// Device to perform operations on (CPU, CUDA, or Metal).
    device: Device,
    /// Tensor containing all document embeddings [N, D].
    embeddings: RwLock<Option<Tensor>>,
    /// Mapping from tensor index to document row ID.
    index_to_id: RwLock<Vec<String>>,
    /// Current number of documents in the cache.
    count: RwLock<usize>,
}

impl ActiveVramCache {
    /// Create a new ActiveVramCache on the best available device.
    pub fn new() -> Result<Self> {
        let device = if cfg!(feature = "gpu-search-cuda") {
            Device::new_cuda(0).unwrap_or(Device::Cpu)
        } else if cfg!(feature = "gpu-search-metal") {
            Device::new_metal(0).unwrap_or(Device::Cpu)
        } else {
            Device::Cpu
        };

        tracing::info!("Initializing ActiveVramCache on device: {:?}", device);

        Ok(Self {
            device,
            embeddings: RwLock::new(None),
            index_to_id: RwLock::new(Vec::new()),
            count: RwLock::new(0),
        })
    }

    /// Load embeddings into the VRAM cache.
    pub async fn load_embeddings(&self, ids: Vec<String>, vectors: Vec<Vec<f32>>) -> Result<()> {
        if ids.is_empty() || vectors.is_empty() {
            return Ok(());
        }

        let n = ids.len();
        let d = vectors[0].len();

        // Flatten vectors for tensor creation
        let flat_vectors: Vec<f32> = vectors.into_iter().flatten().collect();
        let tensor = Tensor::from_vec(flat_vectors, (n, d), &self.device)?;

        // Normalize embeddings for cosine similarity (so matmul gives cosine similarity)
        let l2_norm = tensor.sqr()?.sum_keepdim(1)?.sqrt()?;
        let normalized_tensor = tensor.broadcast_div(&l2_norm)?;

        let mut embeddings_guard = self.embeddings.write().await;
        let mut index_to_id_guard = self.index_to_id.write().await;
        let mut count_guard = self.count.write().await;

        *embeddings_guard = Some(normalized_tensor);
        *index_to_id_guard = ids;
        *count_guard = n;

        Ok(())
    }

    /// Perform vector similarity search against cached embeddings.
    pub async fn search(&self, query_vector: &[f32], limit: usize) -> Result<Vec<(String, f32)>> {
        let embeddings_guard = self.embeddings.read().await;
        let index_to_id_guard = self.index_to_id.read().await;

        let embeddings = match &*embeddings_guard {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };

        if query_vector.is_empty() {
            return Ok(Vec::new());
        }

        let d = query_vector.len();
        let query_tensor = Tensor::from_vec(query_vector.to_vec(), (1, d), &self.device)?;

        // Normalize query vector
        let query_l2_norm = query_tensor.sqr()?.sum_keepdim(1)?.sqrt()?;
        let normalized_query = query_tensor.broadcast_div(&query_l2_norm)?;

        // matmul [1, D] x [D, N] -> [1, N]
        // embeddings is [N, D], so we transpose it
        let similarities = normalized_query.matmul(&embeddings.t()?)?;

        // Extract to CPU for Top-K extraction (Candle workaround)
        let sim_vec = similarities.flatten_all()?.to_vec1::<f32>()?;

        let mut results: Vec<(usize, f32)> = sim_vec
            .into_iter()
            .enumerate()
            .filter(|(_, score)| *score > 0.0)
            .collect();

        // Sort by similarity score descending
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let final_results: Vec<(String, f32)> = results
            .into_iter()
            .take(limit)
            .filter_map(|(idx, score)| {
                index_to_id_guard.get(idx).map(|id| (id.clone(), score))
            })
            .collect();

        Ok(final_results)
    }

    /// Returns the number of items in the cache.
    pub async fn count(&self) -> usize {
        *self.count.read().await
    }

    /// Clear the cache.
    pub async fn clear(&self) -> Result<()> {
        *self.embeddings.write().await = None;
        self.index_to_id.write().await.clear();
        *self.count.write().await = 0;
        Ok(())
    }
}
