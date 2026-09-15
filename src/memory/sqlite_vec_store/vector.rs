#![allow(unsafe_code)]
//! Vector operations for SQLite vector store
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::memory::sqlite_vec_store::config::QJL_MAGIC;
use anyhow::Result;

/// Register sqlite vec extension.
pub fn register_sqlite_vec_extension() -> Result<()> {
    unsafe {
        #[cfg(target_os = "android")]
        type CharPtr = u8;
        #[cfg(not(target_os = "android"))]
        type CharPtr = i8;

        let init = std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut CharPtr,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(sqlite_vec::sqlite3_vec_init as *const ());
        rusqlite::ffi::sqlite3_auto_extension(Some(init));
    }
    Ok(())
}

/// Serialize embedding.
pub fn serialize_embedding(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Serialize embedding qjl.
pub fn serialize_embedding_qjl(embedding: &[f32]) -> Vec<u8> {
    let dims = embedding.len() as u32;
    let max_abs = embedding
        .iter()
        .fold(0.0_f32, |acc, value| acc.max(value.abs()));
    let scale_1 = if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 };
    let coarse: Vec<i8> = embedding
        .iter()
        .map(|value| ((value / scale_1).round().clamp(-127.0, 127.0)) as i8)
        .collect();
    let residuals: Vec<f32> = embedding
        .iter()
        .zip(coarse.iter())
        .map(|(value, quantized)| value - (*quantized as f32 * scale_1))
        .collect();
    let residual_max = residuals
        .iter()
        .fold(0.0_f32, |acc, value| acc.max(value.abs()));
    let scale_2 = if residual_max > 0.0 {
        residual_max / 127.0
    } else {
        1.0
    };
    let residual_quantized: Vec<i8> = residuals
        .iter()
        .map(|value| ((value / scale_2).round().clamp(-127.0, 127.0)) as i8)
        .collect();

    let mut bytes = Vec::with_capacity(16 + (embedding.len() * 2));
    bytes.extend_from_slice(QJL_MAGIC);
    bytes.extend_from_slice(&dims.to_le_bytes());
    bytes.extend_from_slice(&scale_1.to_le_bytes());
    bytes.extend_from_slice(&scale_2.to_le_bytes());
    bytes.extend(coarse.into_iter().map(|value| value as u8));
    bytes.extend(residual_quantized.into_iter().map(|value| value as u8));
    bytes
}

/// Deserialize embedding.
pub fn deserialize_embedding(data: &[u8]) -> Vec<f32> {
    if data.len() >= 16 && &data[..4] == QJL_MAGIC {
        let dims = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let scale_1 = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let scale_2 = f32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        let expected_len = 16 + (dims * 2);
        if data.len() >= expected_len {
            let coarse = &data[16..16 + dims];
            let residual = &data[16 + dims..expected_len];
            return coarse
                .iter()
                .zip(residual.iter())
                .map(|(coarse, residual)| {
                    let coarse = *coarse as i8 as f32;
                    let residual = *residual as i8 as f32;
                    (coarse * scale_1) + (residual * scale_2)
                })
                .collect();
        }
    }

    data.as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

/// Quantizes a 32-bit floating-point embedding vector to 8-bit signed integers (i8)
/// using dynamic uniform scaling: scale = max(|v_i|) / 127.0.
/// Returns the quantized `Vec<i8>` and the scaling factor `scale`.
pub fn quantize_scalar_i8(embedding: &[f32]) -> (Vec<i8>, f32) {
    let max_abs = embedding
        .iter()
        .fold(0.0_f32, |acc, val| acc.max(val.abs()));
    let scale = if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 };
    let quantized = embedding
        .iter()
        .map(|&val| ((val / scale).round().clamp(-127.0, 127.0)) as i8)
        .collect();
    (quantized, scale)
}

/// Dequantizes an 8-bit signed integer vector back to 32-bit floating point vector
/// given its scaling factor.
pub fn dequantize_scalar_i8(quantized: &[i8], scale: f32) -> Vec<f32> {
    quantized.iter().map(|&q| (q as f32) * scale).collect()
}

/// Computes exact f32 cosine similarity between two vectors.
pub fn cosine_similarity_f32(v1: &[f32], v2: &[f32]) -> f32 {
    if v1.len() != v2.len() || v1.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut norm1 = 0.0_f32;
    let mut norm2 = 0.0_f32;
    for (&x, &y) in v1.iter().zip(v2.iter()) {
        dot += x * y;
        norm1 += x * x;
        norm2 += y * y;
    }
    if norm1 <= 0.0 || norm2 <= 0.0 {
        0.0
    } else {
        dot / (norm1.sqrt() * norm2.sqrt())
    }
}

/// Computes exact f32 cosine distance (1.0 - cosine_similarity).
pub fn cosine_distance_f32(v1: &[f32], v2: &[f32]) -> f32 {
    1.0 - cosine_similarity_f32(v1, v2)
}

/// Computes fast cosine similarity directly on scalar 8-bit quantized vectors
/// using integer dot product and norms scaled by their respective float scale factors.
pub fn cosine_similarity_i8(q1: &[i8], scale1: f32, q2: &[i8], scale2: f32) -> f32 {
    if q1.len() != q2.len() || q1.is_empty() {
        return 0.0;
    }
    let mut dot_i32: i64 = 0;
    let mut norm1_i32: i64 = 0;
    let mut norm2_i32: i64 = 0;
    for (&x, &y) in q1.iter().zip(q2.iter()) {
        let x64 = x as i64;
        let y64 = y as i64;
        dot_i32 += x64 * y64;
        norm1_i32 += x64 * x64;
        norm2_i32 += y64 * y64;
    }
    if norm1_i32 <= 0 || norm2_i32 <= 0 {
        return 0.0;
    }
    let dot = (dot_i32 as f64) * (scale1 as f64) * (scale2 as f64);
    let norm1 = (norm1_i32 as f64).sqrt() * (scale1 as f64);
    let norm2 = (norm2_i32 as f64).sqrt() * (scale2 as f64);
    if norm1 <= 0.0 || norm2 <= 0.0 {
        0.0
    } else {
        (dot / (norm1 * norm2)) as f32
    }
}

/// Computes cosine distance on scalar 8-bit quantized vectors.
pub fn cosine_distance_i8(q1: &[i8], scale1: f32, q2: &[i8], scale2: f32) -> f32 {
    1.0 - cosine_similarity_i8(q1, scale1, q2, scale2)
}

/// Computes cosine distance directly on QJL quantized byte buffers.
pub fn cosine_distance_qjl(qjl1: &[u8], qjl2: &[u8]) -> f32 {
    let v1 = deserialize_embedding(qjl1);
    let v2 = deserialize_embedding(qjl2);
    cosine_distance_f32(&v1, &v2)
}

/// Evaluation metrics report comparing vector memory footprint, cosine errors,
/// and top-k recall trade-offs across quantization strategies (f32 vs scalar i8 vs QJL).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QuantizationMetricsReport {
    /// Dimension of candidate vector embeddings.
    pub dimensions: usize,
    /// Total candidate vector count evaluated.
    pub dataset_size: usize,
    /// Memory per vector in bytes for standard 32-bit floats (f32).
    pub f32_bytes_per_vector: usize,
    /// Memory per vector in bytes for scalar 8-bit integers (i8 + scale f32).
    pub i8_bytes_per_vector: usize,
    /// Memory per vector in bytes for two-stage QJL quantized format.
    pub qjl_bytes_per_vector: usize,
    /// Percentage memory footprint reduction of scalar i8 vs f32 (e.g., ~75%).
    pub i8_memory_reduction_pct: f32,
    /// Percentage memory footprint reduction of QJL vs f32 (e.g., ~50%).
    pub qjl_memory_reduction_pct: f32,
    /// Mean absolute cosine distance error of scalar i8 vs f32.
    pub mean_f32_vs_i8_cosine_error: f32,
    /// Mean absolute cosine distance error of QJL vs f32.
    pub mean_f32_vs_qjl_cosine_error: f32,
    /// Recall@k for scalar 8-bit quantized nearest-neighbor retrieval.
    pub i8_recall_at_k: f32,
    /// Recall@k for QJL quantized nearest-neighbor retrieval.
    pub qjl_recall_at_k: f32,
}

/// Evaluates vector search quantization trade-offs over a dataset of vectors and query set.
pub fn evaluate_quantization_metrics(
    embeddings: &[Vec<f32>],
    queries: &[Vec<f32>],
    top_k: usize,
) -> QuantizationMetricsReport {
    let dataset_size = embeddings.len();
    let dims = embeddings.first().map_or(0, |v| v.len());
    let f32_bytes = dims * 4;
    let i8_bytes = dims + 4; // 1 byte per dim + 4 byte scale
    let qjl_bytes = 16 + (dims * 2); // 16 bytes header + 2 bytes per dim (coarse + residual)

    let i8_memory_reduction_pct = if f32_bytes > 0 {
        ((f32_bytes - i8_bytes) as f32 / f32_bytes as f32) * 100.0
    } else {
        0.0
    };

    let qjl_memory_reduction_pct = if f32_bytes > 0 {
        ((f32_bytes.saturating_sub(qjl_bytes)) as f32 / f32_bytes as f32) * 100.0
    } else {
        0.0
    };

    let i8_dataset: Vec<(Vec<i8>, f32)> =
        embeddings.iter().map(|e| quantize_scalar_i8(e)).collect();
    let qjl_dataset: Vec<Vec<u8>> = embeddings
        .iter()
        .map(|e| serialize_embedding_qjl(e))
        .collect();

    let mut total_i8_cosine_err = 0.0_f32;
    let mut total_qjl_cosine_err = 0.0_f32;
    let mut pair_count = 0usize;

    let mut i8_recall_sum = 0.0_f32;
    let mut qjl_recall_sum = 0.0_f32;

    for query in queries {
        let query_qjl = serialize_embedding_qjl(query);
        let (query_i8, query_scale) = quantize_scalar_i8(query);

        let mut f32_dists: Vec<(usize, f32)> = embeddings
            .iter()
            .enumerate()
            .map(|(idx, e)| (idx, cosine_distance_f32(query, e)))
            .collect();
        f32_dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut i8_dists: Vec<(usize, f32)> = i8_dataset
            .iter()
            .enumerate()
            .map(|(idx, (q, scale))| (idx, cosine_distance_i8(&query_i8, query_scale, q, *scale)))
            .collect();
        i8_dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut qjl_dists: Vec<(usize, f32)> = qjl_dataset
            .iter()
            .enumerate()
            .map(|(idx, bytes)| (idx, cosine_distance_qjl(&query_qjl, bytes)))
            .collect();
        qjl_dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let k = top_k.min(dataset_size).max(1);
        let f32_top_k: std::collections::HashSet<usize> =
            f32_dists.iter().take(k).map(|&(idx, _)| idx).collect();

        let i8_hits = i8_dists
            .iter()
            .take(k)
            .filter(|&(idx, _)| f32_top_k.contains(idx))
            .count();
        let qjl_hits = qjl_dists
            .iter()
            .take(k)
            .filter(|&(idx, _)| f32_top_k.contains(idx))
            .count();

        i8_recall_sum += i8_hits as f32 / k as f32;
        qjl_recall_sum += qjl_hits as f32 / k as f32;

        for idx in 0..dataset_size.min(100) {
            let f32_d = f32_dists[idx].1;
            let i8_d = i8_dists[idx].1;
            let qjl_d = qjl_dists[idx].1;

            total_i8_cosine_err += (f32_d - i8_d).abs();
            total_qjl_cosine_err += (f32_d - qjl_d).abs();
            pair_count += 1;
        }
    }

    let num_queries = queries.len().max(1);
    let pair_count_denom = pair_count.max(1) as f32;

    QuantizationMetricsReport {
        dimensions: dims,
        dataset_size,
        f32_bytes_per_vector: f32_bytes,
        i8_bytes_per_vector: i8_bytes,
        qjl_bytes_per_vector: qjl_bytes,
        i8_memory_reduction_pct,
        qjl_memory_reduction_pct,
        mean_f32_vs_i8_cosine_error: total_i8_cosine_err / pair_count_denom,
        mean_f32_vs_qjl_cosine_error: total_qjl_cosine_err / pair_count_denom,
        i8_recall_at_k: i8_recall_sum / num_queries as f32,
        qjl_recall_at_k: qjl_recall_sum / num_queries as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scalar_i8_quantization_roundtrip() {
        let original = vec![0.5, -0.25, 0.8, -0.9, 0.0];
        let (quantized, scale) = quantize_scalar_i8(&original);
        let reconstructed = dequantize_scalar_i8(&quantized, scale);

        assert_eq!(original.len(), reconstructed.len());
        for (orig, rec) in original.iter().zip(reconstructed.iter()) {
            assert!(
                (orig - rec).abs() < 0.02,
                "expected close fit: orig={}, rec={}",
                orig,
                rec
            );
        }
    }

    #[test]
    fn test_cosine_similarity_f32_and_i8() {
        let v1 = vec![1.0, 0.0, 0.0, 0.5];
        let v2 = vec![0.9, 0.1, 0.0, 0.4];

        let sim_f32 = cosine_similarity_f32(&v1, &v2);
        let dist_f32 = cosine_distance_f32(&v1, &v2);
        assert!((sim_f32 + dist_f32 - 1.0).abs() < 1e-5);

        let (q1, s1) = quantize_scalar_i8(&v1);
        let (q2, s2) = quantize_scalar_i8(&v2);
        let sim_i8 = cosine_similarity_i8(&q1, s1, &q2, s2);
        let dist_i8 = cosine_distance_i8(&q1, s1, &q2, s2);

        assert!((sim_f32 - sim_i8).abs() < 0.05);
        assert!((dist_f32 - dist_i8).abs() < 0.05);
    }

    #[test]
    fn test_quantization_evaluation_report() {
        let embeddings = vec![
            vec![1.0, 0.0, 0.0, 0.5],
            vec![0.0, 1.0, 0.0, 0.2],
            vec![0.0, 0.0, 1.0, -0.5],
            vec![0.7, 0.7, 0.0, 0.1],
        ];
        let queries = vec![vec![0.9, 0.1, 0.0, 0.4]];

        let report = evaluate_quantization_metrics(&embeddings, &queries, 2);
        assert_eq!(report.dimensions, 4);
        assert_eq!(report.dataset_size, 4);
        assert_eq!(report.f32_bytes_per_vector, 16);
        assert_eq!(report.i8_bytes_per_vector, 8);
        assert!(report.i8_memory_reduction_pct > 40.0);
        assert!(report.i8_recall_at_k >= 0.5);
    }
}
