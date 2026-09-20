//! Zstandard (zstd) payload compressor for telecom frames over 1KB (WAVE-27.25 / Issue #2403).
//!
//! Provides bandwidth-conserving adaptive compression for large telemetry frames,
//! knowledge embeddings, and binary streams across peer-to-peer mesh links.

use std::io::{Read, Write};
use thiserror::Error;

/// Threshold above which payloads are evaluated for compression (1024 bytes).
pub const DEFAULT_COMPRESSION_THRESHOLD_BYTES: usize = 1024;

/// Default zstd compression level (balanced performance/ratio).
pub const DEFAULT_COMPRESSION_LEVEL: i32 = 3;

/// Errors arising during compression or decompression.
#[derive(Debug, Error)]
pub enum CompressionError {
    #[error("Compression I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Decompression failed: {0}")]
    DecompressError(String),
}

/// Adaptive compression outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompressionResult {
    /// Payload was compressed; contains compressed bytes.
    Compressed(Vec<u8>),
    /// Payload was below threshold or did not compress smaller; contains original bytes.
    Uncompressed(Vec<u8>),
}

/// Payload compression engine using Zstandard.
pub struct PayloadCompressor {
    threshold_bytes: usize,
    compression_level: i32,
}

impl Default for PayloadCompressor {
    fn default() -> Self {
        Self {
            threshold_bytes: DEFAULT_COMPRESSION_THRESHOLD_BYTES,
            compression_level: DEFAULT_COMPRESSION_LEVEL,
        }
    }
}

impl PayloadCompressor {
    /// Create a new compressor with custom threshold and compression level.
    pub fn new(threshold_bytes: usize, compression_level: i32) -> Self {
        Self {
            threshold_bytes,
            compression_level,
        }
    }

    /// Compresses payload if size exceeds threshold AND compressed size is strictly smaller.
    pub fn compress_if_beneficial(
        &self,
        raw: &[u8],
    ) -> Result<CompressionResult, CompressionError> {
        if raw.len() < self.threshold_bytes {
            return Ok(CompressionResult::Uncompressed(raw.to_vec()));
        }

        let compressed = self.compress_raw(raw)?;
        if compressed.len() < raw.len() {
            Ok(CompressionResult::Compressed(compressed))
        } else {
            Ok(CompressionResult::Uncompressed(raw.to_vec()))
        }
    }

    /// Compress arbitrary slice using configured zstd level.
    pub fn compress_raw(&self, raw: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let mut encoder = zstd::Encoder::new(Vec::new(), self.compression_level)?;
        encoder.write_all(raw)?;
        let compressed = encoder.finish()?;
        Ok(compressed)
    }

    /// Decompress zstd-encoded frame.
    pub fn decompress_raw(compressed: &[u8]) -> Result<Vec<u8>, CompressionError> {
        let mut decoder = zstd::Decoder::new(compressed)
            .map_err(|e| CompressionError::DecompressError(e.to_string()))?;
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| CompressionError::DecompressError(e.to_string()))?;
        Ok(decompressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_skips_small_payloads() {
        let compressor = PayloadCompressor::default();
        let small_data = b"small payload under 1KB";
        let res = compressor.compress_if_beneficial(small_data).unwrap();

        match res {
            CompressionResult::Uncompressed(bytes) => assert_eq!(bytes, small_data),
            CompressionResult::Compressed(_) => panic!("Small data should not be compressed"),
        }
    }

    #[test]
    fn test_compression_compresses_large_redundant_payload() {
        let compressor = PayloadCompressor::default();
        // 4KB of repeated text
        let large_data = "XAVIER_TELECOM_NODE_DATA_2026_MESH_TELEMETRY_"
            .repeat(100)
            .into_bytes();
        assert!(large_data.len() > 1024);

        let res = compressor.compress_if_beneficial(&large_data).unwrap();
        match res {
            CompressionResult::Compressed(compressed) => {
                assert!(compressed.len() < large_data.len());
                let recovered = PayloadCompressor::decompress_raw(&compressed).unwrap();
                assert_eq!(recovered, large_data);
            }
            CompressionResult::Uncompressed(_) => {
                panic!("Expected repetitive data to be compressed")
            }
        }
    }

    #[test]
    fn test_raw_compress_decompress_roundtrip() {
        let compressor = PayloadCompressor::default();
        let original = b"Direct raw compression test buffer for telecom streaming";
        let compressed = compressor.compress_raw(original).unwrap();
        let recovered = PayloadCompressor::decompress_raw(&compressed).unwrap();
        assert_eq!(recovered, original);
    }
}
