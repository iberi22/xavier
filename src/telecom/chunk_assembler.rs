//! Concurrent chunk assembler & stream rebuilder for file transfer (WAVE-27.22 / Issue #2400).
//!
//! Handles out-of-order received file chunks, tracks missing chunk sequences,
//! writes buffers safely to disk or memory, and verifies complete SHA-256 integrity.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Errors occurring during chunk assembly or file reconstruction.
#[derive(Debug, Error)]
pub enum ChunkAssemblerError {
    #[error("Chunk index {index} out of bounds (total chunks: {total})")]
    ChunkOutOfBounds { index: usize, total: usize },

    #[error("Chunk integrity verification failed: expected {expected}, actual {actual}")]
    ChunkChecksumMismatch { expected: String, actual: String },

    #[error("File integrity verification failed: expected {expected}, actual {actual}")]
    FileChecksumMismatch { expected: String, actual: String },

    #[error("File I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Incomplete assembly: missing chunks {0:?}")]
    IncompleteAssembly(Vec<usize>),
}

/// Incoming chunk payload representation.
#[derive(Debug, Clone)]
pub struct AssembledChunk {
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub data: Vec<u8>,
    pub chunk_sha256: Option<String>,
}

/// Out-of-order chunk assembly and buffered stream rebuilder.
#[derive(Debug, Clone)]
pub struct ChunkAssembler {
    pub file_id: String,
    pub expected_file_sha256: Option<String>,
    pub total_chunks: usize,
    pub total_bytes: usize,
    received_chunks: BTreeMap<usize, Vec<u8>>,
}

impl ChunkAssembler {
    /// Initialize a new assembler session for a file transfer.
    pub fn new(
        file_id: impl Into<String>,
        total_chunks: usize,
        total_bytes: usize,
        expected_file_sha256: Option<String>,
    ) -> Self {
        Self {
            file_id: file_id.into(),
            expected_file_sha256,
            total_chunks,
            total_bytes,
            received_chunks: BTreeMap::new(),
        }
    }

    /// Ingest a single chunk (supports completely out-of-order receipt).
    pub fn ingest_chunk(&mut self, chunk: AssembledChunk) -> Result<bool, ChunkAssemblerError> {
        if chunk.chunk_index >= self.total_chunks {
            return Err(ChunkAssemblerError::ChunkOutOfBounds {
                index: chunk.chunk_index,
                total: self.total_chunks,
            });
        }

        // Verify per-chunk checksum if provided
        if let Some(ref expected) = chunk.chunk_sha256 {
            let mut hasher = Sha256::new();
            hasher.update(&chunk.data);
            let actual = to_hex(&hasher.finalize());
            if actual != *expected {
                return Err(ChunkAssemblerError::ChunkChecksumMismatch {
                    expected: expected.clone(),
                    actual,
                });
            }
        }

        self.received_chunks.insert(chunk.chunk_index, chunk.data);
        Ok(self.is_complete())
    }

    /// Check if all chunks have been received.
    pub fn is_complete(&self) -> bool {
        self.received_chunks.len() == self.total_chunks
    }

    /// Calculate the progress ratio [0.0..1.0].
    pub fn progress(&self) -> f64 {
        if self.total_chunks == 0 {
            1.0
        } else {
            self.received_chunks.len() as f64 / self.total_chunks as f64
        }
    }

    /// Return list of missing chunk indices.
    pub fn missing_chunks(&self) -> Vec<usize> {
        (0..self.total_chunks)
            .filter(|idx| !self.received_chunks.contains_key(idx))
            .collect()
    }

    /// Reassemble the contiguous byte buffer and verify full-file checksum.
    pub fn assemble_bytes(&self) -> Result<Vec<u8>, ChunkAssemblerError> {
        let missing = self.missing_chunks();
        if !missing.is_empty() {
            return Err(ChunkAssemblerError::IncompleteAssembly(missing));
        }

        let mut output = Vec::with_capacity(self.total_bytes);
        for idx in 0..self.total_chunks {
            if let Some(bytes) = self.received_chunks.get(&idx) {
                output.extend_from_slice(bytes);
            }
        }

        if let Some(ref expected) = self.expected_file_sha256 {
            let mut hasher = Sha256::new();
            hasher.update(&output);
            let actual = to_hex(&hasher.finalize());
            if actual != *expected {
                return Err(ChunkAssemblerError::FileChecksumMismatch {
                    expected: expected.clone(),
                    actual,
                });
            }
        }

        Ok(output)
    }

    /// Write the assembled stream directly to the target file path.
    pub fn assemble_to_disk<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<PathBuf, ChunkAssemblerError> {
        let bytes = self.assemble_bytes()?;
        std::fs::write(path.as_ref(), bytes)?;
        Ok(path.as_ref().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_out_of_order_chunk_assembly_and_sha256() {
        let payload = b"Hello Xavier Distributed File Transfer Protocol 2026!";
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let full_sha = to_hex(&hasher.finalize());

        let chunk_size = 10;
        let chunks_data: Vec<&[u8]> = payload.chunks(chunk_size).collect();
        let total_chunks = chunks_data.len();

        let mut assembler = ChunkAssembler::new(
            "transfer-test-1",
            total_chunks,
            payload.len(),
            Some(full_sha),
        );

        // Receive chunks in reverse/mixed order
        let mut indices: Vec<usize> = (0..total_chunks).collect();
        indices.reverse();

        for idx in indices {
            let data = chunks_data[idx].to_vec();
            let mut ch = Sha256::new();
            ch.update(&data);
            let ch_sha = to_hex(&ch.finalize());

            let is_done = assembler
                .ingest_chunk(AssembledChunk {
                    chunk_index: idx,
                    total_chunks,
                    data,
                    chunk_sha256: Some(ch_sha),
                })
                .unwrap();

            if idx == 0 {
                // last ingested since reversed
                assert!(is_done);
            }
        }

        assert!(assembler.is_complete());
        assert_eq!(assembler.progress(), 1.0);
        assert!(assembler.missing_chunks().is_empty());

        let assembled = assembler.assemble_bytes().unwrap();
        assert_eq!(assembled, payload);
    }

    #[test]
    fn test_missing_chunks_detection() {
        let mut assembler = ChunkAssembler::new("test-missing", 5, 50, None);
        assert_eq!(assembler.missing_chunks(), vec![0, 1, 2, 3, 4]);

        assembler
            .ingest_chunk(AssembledChunk {
                chunk_index: 1,
                total_chunks: 5,
                data: vec![1; 10],
                chunk_sha256: None,
            })
            .unwrap();

        assembler
            .ingest_chunk(AssembledChunk {
                chunk_index: 3,
                total_chunks: 5,
                data: vec![3; 10],
                chunk_sha256: None,
            })
            .unwrap();

        assert_eq!(assembler.missing_chunks(), vec![0, 2, 4]);
        assert!(!assembler.is_complete());
        assert!(matches!(
            assembler.assemble_bytes(),
            Err(ChunkAssemblerError::IncompleteAssembly(_))
        ));
    }
}
